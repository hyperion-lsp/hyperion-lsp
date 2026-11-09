// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for interface inheritance indexing.
//!
//! The workspace indexer must correctly model interface multi-extends:
//!
//!   interface C extends A, B { ... }
//!
//! The Java semantics here are "C is a subtype of both A and B" — i.e. C
//! has TWO parents, both of which are interfaces. Source symbols retain the
//! written names (`A`, `B`) so late same-package/import changes can relink an
//! edge without reindexing the declaring file. Canonical hierarchy queries
//! must nevertheless expose `p.C` beneath both `p.A` and `p.B`.
//!
//! Pre-fix bug: `extract_parent_class` accumulated every identifier between
//! `extends` and `{` into a single `parent_name_parts` vector and then
//! `qualify_type_name` joined them with dots. So `C extends A, B` became
//! `parent_class = Some("A.B")` (a non-existent FQN) and `interfaces = []`.
//! Every feature that walked interface inheritance — find-implementations,
//! cross-file method resolution, hover-on-method, goto-def for inherited
//! methods — silently lost half its information graph as a result.

use std::sync::Arc;

use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

fn index_files(files: &[(&str, &str)]) -> Arc<WorkspaceIndex> {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().to_path_buf();
    let pkg = root.join("src/main/java/p");
    std::fs::create_dir_all(&pkg).unwrap();
    let idx = Arc::new(WorkspaceIndex::new(root.clone()));
    for (name, src) in files {
        let path = pkg.join(name);
        std::fs::write(&path, src).unwrap();
        idx.index_file_with_source(&path, src).expect("index");
    }
    // Leak the tempdir so file paths stay valid for the duration of the
    // test — index reads file content lazily in some paths.
    std::mem::forget(temp);
    idx
}

#[test]
fn interface_with_two_parents_populates_interfaces_field() {
    let idx = index_files(&[
        ("A.java", "package p; public interface A { void m(); }"),
        ("B.java", "package p; public interface B { void m(); }"),
        ("C.java", "package p; public interface C extends A, B { }"),
    ]);

    let c = idx.get_symbol("p.C").expect("p.C should be indexed");

    let interfaces: Vec<&str> = c.interfaces.iter().map(|i| i.as_ref()).collect();
    assert!(interfaces.contains(&"A"), "got {interfaces:?}");
    assert!(interfaces.contains(&"B"), "got {interfaces:?}");
    assert_eq!(
        interfaces.len(),
        2,
        "interface C extends A, B — interfaces field should have exactly 2 entries, got {:?}",
        interfaces
    );

    // For an interface, there is no class to extend — `parent_class` must
    // be None. The pre-fix bug stored `Some("A.B")` here.
    assert!(
        c.parent_class.is_none(),
        "interface C must not have a parent_class (Java has no class-to-interface inheritance), \
         got Some({:?})",
        c.parent_class.as_deref()
    );
    for parent in ["p.A", "p.B"] {
        assert!(
            idx.find_direct_subtypes(parent)
                .iter()
                .any(|symbol| symbol.fqn.as_ref() == "p.C"),
            "canonical hierarchy should resolve p.C beneath {parent}"
        );
    }
}

/// Single-parent interface extends — the simpler case. Should be a no-op
/// after the fix; this guards against accidentally moving single-parent
/// behavior to the wrong field.
#[test]
fn interface_with_single_parent_populates_interfaces_field() {
    let idx = index_files(&[
        (
            "Base.java",
            "package p; public interface Base { void m(); }",
        ),
        (
            "Derived.java",
            "package p; public interface Derived extends Base { }",
        ),
    ]);

    let derived = idx
        .get_symbol("p.Derived")
        .expect("p.Derived should be indexed");

    let interfaces: Vec<&str> = derived.interfaces.iter().map(|i| i.as_ref()).collect();
    assert_eq!(
        interfaces,
        vec!["Base"],
        "source symbol should retain the contextual spelling"
    );
    assert!(
        derived.parent_class.is_none(),
        "interface Derived must not have a parent_class"
    );
    assert!(idx
        .find_direct_subtypes("p.Base")
        .iter()
        .any(|symbol| symbol.fqn.as_ref() == "p.Derived"));
}

/// Class with extends + implements: the existing happy path. Guards
/// against regressing how classes are handled while we change interface
/// handling.
#[test]
fn class_extends_and_implements_unchanged() {
    let idx = index_files(&[
        ("A.java", "package p; public interface A { void m(); }"),
        ("B.java", "package p; public interface B { void m(); }"),
        ("Base.java", "package p; public class Base { }"),
        (
            "Derived.java",
            "package p; public class Derived extends Base implements A, B { }",
        ),
    ]);

    let d = idx
        .get_symbol("p.Derived")
        .expect("p.Derived should be indexed");

    assert_eq!(
        d.parent_class.as_deref(),
        Some("Base"),
        "source symbol should retain the contextual superclass spelling"
    );

    let interfaces: Vec<&str> = d.interfaces.iter().map(|i| i.as_ref()).collect();
    assert!(
        interfaces.contains(&"A") && interfaces.contains(&"B"),
        "class Derived implements A, B — interfaces should retain A and B, got {:?}",
        interfaces
    );
    assert_eq!(interfaces.len(), 2);
    for parent in ["p.Base", "p.A", "p.B"] {
        assert!(
            idx.find_direct_subtypes(parent)
                .iter()
                .any(|symbol| symbol.fqn.as_ref() == "p.Derived"),
            "canonical hierarchy should resolve p.Derived beneath {parent}"
        );
    }
}

/// Class with no inheritance — both fields should be empty/None. Guards
/// against the fix accidentally introducing phantom relationships.
#[test]
fn class_with_no_inheritance_has_no_relationships() {
    let idx = index_files(&[("Standalone.java", "package p; public class Standalone { }")]);
    let s = idx.get_symbol("p.Standalone").expect("indexed");
    assert!(s.parent_class.is_none());
    assert!(s.interfaces.is_empty());
}
