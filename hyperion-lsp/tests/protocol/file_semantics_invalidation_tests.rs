// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression test for over-invalidation of `file_semantics` Salsa caches in
//! `run_transitive_invalidation`.
//!
//! Background: when a file's symbols change and other files depend on them,
//! `run_transitive_invalidation` (server/diagnostics.rs) walks the dependency
//! graph and updates the workspace index + Salsa inputs for the affected files.
//! Commit 49b66dd2 added a final `db.bump_workspace_version()` after the batch,
//! intended to force `file_semantics` for dependent open files to refresh.
//!
//! The bug: `file_semantics(file)` reads `workspace_version` at the top of its
//! body to establish a Salsa dependency, so bumping that input invalidates the
//! cached `Arc<FileSemantics>` for *every* file in the database — not just the
//! files that actually depend on the changed symbols. On a large workspace
//! (thousands of Java files), recomputing `file_semantics` on the next
//! interactive request dominates cold-cache latency and pushes
//! `textDocument/references` past the editor's 10s timeout.
//!
//! This test pins the property:
//! "running `run_transitive_invalidation` for a change to file A must not
//! invalidate `file_semantics` for an open file C that does not depend on A,
//! even when A has other dependents that DO need to be re-indexed."
//!
//! Setup: A is the changed file, B imports A (forcing the invalidation batch
//! to reach the `bump_workspace_version` line), and C is unrelated. We assert
//! C's `Arc<FileSemantics>` survives.
//!
//! The test is deterministic — no wall-clock timing — and exercises the real
//! `run_transitive_invalidation` function so future refactors of the
//! invalidation strategy will keep it honest.

use std::sync::Arc;

use dashmap::DashMap;
use hyperion_lsp::file_registry::FileIdRegistry;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_lsp::server::diagnostics::run_transitive_invalidation;
use hyperion_lsp::DocumentState;
use hyperion_semantics::db::{FileId, Language, SemanticDatabase};
use hyperion_semantics::ResolvedName;
use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

#[test]
fn file_semantics_survives_transitive_invalidation_of_unrelated_file() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace_root = temp_dir.path().to_path_buf();

    // A: the file we'll "edit"; B: imports A (so it's a dependent); C: unrelated.
    let pkg_a_dir = workspace_root.join("a");
    let pkg_b_dir = workspace_root.join("b");
    let pkg_c_dir = workspace_root.join("c");
    std::fs::create_dir_all(&pkg_a_dir).unwrap();
    std::fs::create_dir_all(&pkg_b_dir).unwrap();
    std::fs::create_dir_all(&pkg_c_dir).unwrap();

    let path_a = pkg_a_dir.join("A.java");
    let path_b = pkg_b_dir.join("B.java");
    let path_c = pkg_c_dir.join("C.java");

    let src_a = "package a; public class A { public void foo() {} }";
    let src_b = "package b; import a.A; public class B { A a; public void use() { a.foo(); } }";
    let src_c = "package c; public class C { public void bar() {} }";

    std::fs::write(&path_a, src_a).unwrap();
    std::fs::write(&path_b, src_b).unwrap();
    std::fs::write(&path_c, src_c).unwrap();

    let workspace_index = Arc::new(WorkspaceIndex::new(workspace_root.clone()));

    // Use explicit shared IDs for the editor-state portions of this fixture.
    // Production invalidation resolves closed-file paths from WorkspaceIndex,
    // so it also works for low-domain IDs allocated by a cold workspace scan.
    let file_registry = Arc::new(FileIdRegistry::new());
    let file_id_a = file_registry.get_or_create_from_path(&path_a);
    let file_id_b = file_registry.get_or_create_from_path(&path_b);
    let file_id_c = file_registry.get_or_create_from_path(&path_c);

    workspace_index
        .index_file_with_source_and_id(&path_a, src_a, file_id_a)
        .expect("index A");
    workspace_index
        .index_file_with_source_and_id(&path_b, src_b, file_id_b)
        .expect("index B");
    workspace_index
        .index_file_with_source_and_id(&path_c, src_c, file_id_c)
        .expect("index C");

    let host = SemanticHost::new(Arc::clone(&workspace_index));
    host.with_db(|g| {
        g.register_file(file_id_a, Arc::new(src_a.to_string()), Language::Java);
        g.register_file(file_id_b, Arc::new(src_b.to_string()), Language::Java);
        g.register_file(file_id_c, Arc::new(src_c.to_string()), Language::Java);
    });

    // Sanity: B must actually be a dependent of A. Otherwise the batch update
    // is empty and the buggy `bump_workspace_version` line is skipped, and the
    // test wouldn't exercise the bug. Fail fast if this premise breaks (e.g.
    // because workspace_index dependency tracking changed).
    let a_fqns = workspace_index.get_file_symbols(file_id_a);
    let dependents: Vec<_> = a_fqns
        .iter()
        .flat_map(|fqn| workspace_index.get_dependent_files(fqn))
        .collect();
    assert!(
        dependents.contains(&file_id_b),
        "Test premise broken: B does not appear as a dependent of A in workspace_index. \
         A's FQNs: {:?}; dependents found: {:?}. Adjust the test sources so B has a \
         resolvable import on A.",
        a_fqns,
        dependents
    );

    // Warm C's file_semantics cache.
    let arc_c1 = host.read_now().file_semantics(file_id_c);
    let arc_c_again = host.read_now().file_semantics(file_id_c);
    assert!(
        Arc::ptr_eq(&arc_c1, &arc_c_again),
        "Sanity: file_semantics(C) recomputed without any change. \
         Cache is broken in a more fundamental way than the bug under test."
    );

    // Empty open_files / document_map: B is treated as "closed" so the BFS
    // re-indexes it into the Salsa DB (db_updates non-empty), which is what
    // forces execution into the buggy `bump_workspace_version` line.
    let document_map: DashMap<String, DocumentState> = DashMap::new();
    let open_files: Arc<DashMap<FileId, String>> = Arc::new(DashMap::new());

    let _ =
        run_transitive_invalidation(file_id_a, &mut host.write_now(), &document_map, &open_files);

    let arc_c2 = host.read_now().file_semantics(file_id_c);

    assert!(
        Arc::ptr_eq(&arc_c1, &arc_c2),
        "REGRESSION: file_semantics(C) was invalidated after run_transitive_invalidation \
         for an unrelated file A. C does not import A, so its cached semantics should \
         survive. Forces recomputation of every open file's semantics on each edit, \
         which is the dominant cost of cold-cache `textDocument/references` on large \
         workspaces. See hyperion-lsp/src/server/diagnostics.rs:run_transitive_invalidation."
    );
}

#[test]
fn open_dependent_semantics_are_invalidated_when_a_declaration_changes() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace_root = temp_dir.path().to_path_buf();
    let path_a = workspace_root.join("A.java");
    let path_b = workspace_root.join("B.java");
    let src_a = "package a; public class A { public int field; }";
    let changed_a = "package a; public class A {}";
    let src_b = "package b; import a.A; public class B { A value; int copy = value.field; }";
    std::fs::write(&path_a, src_a).unwrap();
    std::fs::write(&path_b, src_b).unwrap();

    let workspace_index = Arc::new(WorkspaceIndex::new(workspace_root));
    let file_registry = Arc::new(FileIdRegistry::new());
    let file_id_a = file_registry.get_or_create_from_path(&path_a);
    let file_id_b = file_registry.get_or_create_from_path(&path_b);
    workspace_index
        .index_file_with_source_and_id(&path_a, src_a, file_id_a)
        .expect("index A");
    workspace_index
        .index_file_with_source_and_id(&path_b, src_b, file_id_b)
        .expect("index B");

    let editor_text = Arc::new(src_b.to_owned());
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    host.with_db(|db| {
        db.register_file(file_id_a, Arc::new(src_a.to_owned()), Language::Java);
        db.register_file(file_id_b, Arc::clone(&editor_text), Language::Java);
    });
    let before = host.read_now().file_semantics(file_id_b);
    assert!(
        before.resolved_names().values().any(|resolved| matches!(
            resolved,
            ResolvedName::Field {
                owner_fqn,
                name,
                ..
            } if owner_fqn == "a.A" && name == "field"
        )),
        "qualified selector should carry A.field's exact identity before the edit"
    );

    workspace_index
        .index_file_with_source_and_id(&path_a, changed_a, file_id_a)
        .expect("remove A.field");

    let uri = tower_lsp::lsp_types::Url::from_file_path(&path_b)
        .expect("file URI")
        .to_string();
    let document_map = DashMap::new();
    document_map.insert(
        uri.clone(),
        DocumentState {
            uri: uri.clone(),
            language_id: "java".to_owned(),
            language: Language::Java,
            version: 1,
            desynchronized: false,
            text: editor_text,
            file_id: file_id_b,
            change_version: 1,
        },
    );
    let open_files = Arc::new(DashMap::new());
    open_files.insert(file_id_b, uri.clone());

    let refreshed =
        run_transitive_invalidation(file_id_a, &mut host.write_now(), &document_map, &open_files);

    assert_eq!(refreshed, vec![uri]);
    let after = host.read_now().file_semantics(file_id_b);
    assert!(
        !Arc::ptr_eq(&before, &after),
        "an open dependent retained semantics computed against the old declaration"
    );
    assert_ne!(
        before.as_ref(),
        after.as_ref(),
        "removing A.field should change B's pre-computed name resolution"
    );
    assert!(
        !after.resolved_names().values().any(|resolved| matches!(
            resolved,
            ResolvedName::Field {
                owner_fqn,
                name,
                ..
            } if owner_fqn == "a.A" && name == "field"
        )),
        "a removed qualified field must not survive dependent recomputation"
    );
}

#[test]
fn same_package_child_semantics_recompute_after_parent_edit_without_global_epoch() {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace_root = temp_dir.path().to_path_buf();
    let parent_path = workspace_root.join("p/Parent.java");
    let child_path = workspace_root.join("p/Child.java");
    std::fs::create_dir_all(parent_path.parent().unwrap()).unwrap();
    let parent_source = "package p; public class Parent { protected int inherited = 1; }";
    let changed_parent_source = "package p; public class Parent {}";
    let child_source = concat!(
        "package p; public class Child extends Parent { ",
        "int copy = inherited; }"
    );
    std::fs::write(&parent_path, parent_source).unwrap();
    std::fs::write(&child_path, child_source).unwrap();

    let workspace_index = Arc::new(WorkspaceIndex::new(workspace_root));
    let file_registry = Arc::new(FileIdRegistry::new());
    let parent_id = file_registry.get_or_create_from_path(&parent_path);
    let child_id = file_registry.get_or_create_from_path(&child_path);
    workspace_index
        .index_file_with_source_and_id(&parent_path, parent_source, parent_id)
        .expect("index parent");
    workspace_index
        .index_file_with_source_and_id(&child_path, child_source, child_id)
        .expect("index child");

    let child_text = Arc::new(child_source.to_owned());
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    host.with_db(|db| {
        db.register_file(
            parent_id,
            Arc::new(parent_source.to_owned()),
            Language::Java,
        );
        db.register_file(child_id, Arc::clone(&child_text), Language::Java);
    });
    let workspace_version = host.read_now().workspace_version();
    let before = host.read_now().file_semantics(child_id);
    assert!(Arc::ptr_eq(
        &before,
        &host.read_now().file_semantics(child_id)
    ));

    let stats = workspace_index
        .index_file_with_source_and_id(&parent_path, changed_parent_source, parent_id)
        .expect("edit same-package parent");
    assert_eq!(
        stats.dependent_files,
        vec![child_id],
        "a same-package extends edge must participate in targeted invalidation"
    );
    host.with_db(|db| {
        db.set_file_text_tracked(parent_id, Arc::new(changed_parent_source.to_owned()));
    });

    let uri = tower_lsp::lsp_types::Url::from_file_path(&child_path)
        .expect("file URI")
        .to_string();
    let document_map = DashMap::new();
    document_map.insert(
        uri.clone(),
        DocumentState {
            uri: uri.clone(),
            language_id: "java".to_owned(),
            language: Language::Java,
            version: 1,
            desynchronized: false,
            text: child_text,
            file_id: child_id,
            change_version: 1,
        },
    );
    let open_files = Arc::new(DashMap::new());
    open_files.insert(child_id, uri.clone());

    let refreshed =
        run_transitive_invalidation(parent_id, &mut host.write_now(), &document_map, &open_files);

    assert_eq!(refreshed, vec![uri]);
    assert_eq!(
        host.read_now().workspace_version(),
        workspace_version,
        "targeted dependent invalidation must not bump the global Salsa epoch"
    );
    let after = host.read_now().file_semantics(child_id);
    assert!(
        !Arc::ptr_eq(&before, &after),
        "the primed child semantics cache survived a same-package parent edit"
    );
    assert_ne!(
        before.as_ref(),
        after.as_ref(),
        "removing the inherited field should change the child's resolved semantics"
    );
}
