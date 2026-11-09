// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin type hierarchy, exercised through the real LSP request path.
//!
//! `prepareTypeHierarchy` / `supertypes` / `subtypes` on a live `Backend`, so
//! per-language dispatch is part of what is under test. Supertypes and subtypes
//! dispatch on the *item's* URI rather than an open document: a hierarchy can be
//! walked far away from where it was prepared.
//!
//! Java behavior is also pinned at the bottom of the file: dispatch must agree
//! with the direct Java provider, and a top-level class must retain its exact
//! indexed identity when traversing both directions.

use crate::kotlin_hierarchy_harness::{type_fqns, Workspace};
use tower_lsp::lsp_types::SymbolKind;

// ---------------------------------------------------------------------------
// prepare
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prepare_on_a_class() {
    let workspace = Workspace::open(&[(
        "Animal.kt",
        r"package zoo

open class Ani<|>mal
",
    )])
    .await;

    let item = workspace.type_hierarchy_item("Animal.kt").await;

    assert_eq!(item.name, "Animal");
    assert_eq!(item.kind, SymbolKind::CLASS);
    assert_eq!(item.detail.as_deref(), Some("zoo.Animal"));
}

#[tokio::test]
async fn prepare_on_an_interface() {
    let workspace = Workspace::open(&[(
        "Speaker.kt",
        r"package zoo

interface Spea<|>ker {
    fun speak()
}
",
    )])
    .await;

    let item = workspace.type_hierarchy_item("Speaker.kt").await;

    assert_eq!(item.name, "Speaker");
    assert_eq!(item.kind, SymbolKind::INTERFACE);
}

#[tokio::test]
async fn prepare_on_an_object() {
    let workspace = Workspace::open(&[(
        "Registry.kt",
        r"package zoo

object Regi<|>stry
",
    )])
    .await;

    let item = workspace.type_hierarchy_item("Registry.kt").await;

    assert_eq!(item.name, "Registry");
    assert_eq!(item.kind, SymbolKind::OBJECT);
    assert_eq!(item.detail.as_deref(), Some("zoo.Registry"));
}

#[tokio::test]
async fn prepare_on_a_keyword_returns_nothing() {
    let workspace = Workspace::open(&[(
        "Animal.kt",
        r"package zoo

cla<|>ss Animal
",
    )])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("Animal.kt")
            .await
            .is_none(),
        "a cursor on `class` is not naming a type"
    );
}

#[tokio::test]
async fn prepare_on_a_class_body_identifier_returns_nothing() {
    let workspace = Workspace::open(&[(
        "Animal.kt",
        r#"package zoo

class Animal {
    val na<|>me = "unknown"
}
"#,
    )])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("Animal.kt")
            .await
            .is_none(),
        "an identifier inside a class body is not the class declaration"
    );
}

// ---------------------------------------------------------------------------
// supertypes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn supertypes_of_a_kotlin_class() {
    let workspace = Workspace::open(&[
        (
            "zoo/Animal.kt",
            r"package zoo

open class Animal
",
        ),
        (
            "zoo/Speaker.kt",
            r"package zoo

interface Speaker {
    fun speak()
}
",
        ),
        (
            "zoo/Dog.kt",
            r"package zoo

class D<|>og : Animal(), Speaker {
    override fun speak() {
    }
}
",
        ),
    ])
    .await;

    let item = workspace.type_hierarchy_item("zoo/Dog.kt").await;
    let supertypes = workspace.supertypes(item).await;

    assert_eq!(type_fqns(&supertypes), vec!["zoo.Animal", "zoo.Speaker"]);
}

#[tokio::test]
async fn supertypes_resolve_through_imports_across_packages() {
    let workspace = Workspace::open(&[
        (
            "base/Animal.kt",
            r"package base

open class Animal
",
        ),
        (
            "pets/Dog.kt",
            r"package pets

import base.Animal

class D<|>og : Animal()
",
        ),
    ])
    .await;

    let item = workspace.type_hierarchy_item("pets/Dog.kt").await;
    let supertypes = workspace.supertypes(item).await;

    assert_eq!(
        type_fqns(&supertypes),
        vec!["base.Animal"],
        "an imported supertype must not be package-qualified with the subtype's package"
    );
}

#[tokio::test]
async fn imported_kotlin_supertype_does_not_append_a_coincidental_package_guess() {
    let workspace = Workspace::open(&[
        ("base/Animal.kt", "package base\nopen class Animal\n"),
        ("pets/Animal.kt", "package pets\nopen class Animal\n"),
        (
            "pets/Dog.kt",
            "package pets\nimport base.Animal\nclass D<|>og : Animal()\n",
        ),
    ])
    .await;

    let dog = workspace.type_hierarchy_item("pets/Dog.kt").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(dog).await),
        vec!["base.Animal"],
        "the extractor's `pets.Animal` guess is not a second declared parent"
    );
}

#[tokio::test]
async fn imported_kotlin_supertype_creates_only_the_true_reverse_edge() {
    let workspace = Workspace::open(&[
        ("base/Animal.kt", "package base\nopen class Ani<|>mal\n"),
        ("pets/Animal.kt", "package pets\nopen class Ani<|>mal\n"),
        (
            "pets/Dog.kt",
            "package pets\nimport base.Animal\nclass Dog : Animal()\n",
        ),
    ])
    .await;

    let imported = workspace.type_hierarchy_item("base/Animal.kt").await;
    assert_eq!(
        type_fqns(&workspace.subtypes(imported).await),
        vec!["pets.Dog"]
    );

    let coincidental = workspace.type_hierarchy_item("pets/Animal.kt").await;
    assert!(
        workspace.subtypes(coincidental).await.is_empty(),
        "the extractor's package guess must not create a second reverse edge"
    );
}

#[tokio::test]
async fn same_package_type_shadows_star_import_in_kotlin_hierarchy() {
    let workspace = Workspace::open(&[
        ("other/Base.kt", "package other\nopen class Base\n"),
        ("pets/Base.kt", "package pets\nopen class Base\n"),
        (
            "pets/Child.kt",
            "package pets\nimport other.*\nclass Ch<|>ild : Base()\n",
        ),
    ])
    .await;

    let child = workspace.type_hierarchy_item("pets/Child.kt").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(child).await),
        vec!["pets.Base"],
        "same-package declarations outrank star imports"
    );
}

#[tokio::test]
async fn kotlin_explicit_import_resolves_the_first_segment_of_a_nested_type() {
    let workspace = Workspace::open(&[
        ("q/Outer.kt", "package q\nclass Outer { open class Base }\n"),
        (
            "p/Child.kt",
            "package p\nimport q.Outer\nclass Ch<|>ild : Outer.Base()\n",
        ),
    ])
    .await;

    let child = workspace.type_hierarchy_item("p/Child.kt").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(child).await),
        vec!["q.Outer.Base"]
    );
}

#[tokio::test]
async fn kotlin_nested_type_resolves_in_its_lexical_owner_before_imports() {
    let workspace = Workspace::open(&[
        (
            "other/Base.kt",
            "package other\nopen class Base\n",
        ),
        (
            "p/Outer.kt",
            "package p\nimport other.*\nclass Outer {\n    open class Base\n    class Ch<|>ild : Base()\n}\n",
        ),
    ])
    .await;

    let child = workspace.type_hierarchy_item("p/Outer.kt").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(child).await),
        vec!["p.Outer.Base"],
        "a lexically enclosing member type shadows an on-demand import"
    );
}

#[tokio::test]
async fn kotlin_prepare_rejects_ambiguous_star_imports() {
    let workspace = Workspace::open(&[
        ("a/Base.kt", "package a\nopen class Base\n"),
        ("b/Base.kt", "package b\nopen class Base\n"),
        (
            "use/Child.kt",
            "package use\nimport a.*\nimport b.*\nclass Child : Ba<|>se()\n",
        ),
    ])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("use/Child.kt")
            .await
            .is_none(),
        "two on-demand imports that contain the same type are ambiguous"
    );
}

#[tokio::test]
async fn kotlin_type_reference_in_a_body_sees_its_own_nested_type() {
    let workspace = Workspace::open(&[(
        "p/Outer.kt",
        "package p\nclass Outer {\n    class Base\n    val value: Ba<|>se? = null\n}\n",
    )])
    .await;

    let base = workspace.type_hierarchy_item("p/Outer.kt").await;
    assert_eq!(base.detail.as_deref(), Some("p.Outer.Base"));
}

#[tokio::test]
async fn kotlin_supertype_header_does_not_see_the_declarations_own_nested_type() {
    let workspace = Workspace::open(&[(
        "p/Child.kt",
        "package p\nclass Child : Ba<|>se() { class Base }\n",
    )])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("p/Child.kt")
            .await
            .is_none(),
        "Child.Base is not in scope in Child's own supertype header"
    );
}

#[tokio::test]
async fn kotlin_star_import_resolves_the_first_segment_of_a_nested_type() {
    let workspace = Workspace::open(&[
        ("Outer.kt", "class Outer { open class Base }\n"),
        ("a/Outer.kt", "package a\nclass Outer { open class Base }\n"),
        (
            "p/Child.kt",
            "package p\nimport a.*\nclass Ch<|>ild : Outer.Base()\n",
        ),
    ])
    .await;

    let child = workspace.type_hierarchy_item("p/Child.kt").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(child).await),
        vec!["a.Outer.Base"]
    );
}

#[tokio::test]
async fn kotlin_explicit_first_segment_binding_stops_after_a_missing_suffix() {
    let workspace = Workspace::open(&[
        ("q/Outer.kt", "package q\nclass Outer\n"),
        (
            "r/Outer.kt",
            "package r\nclass Outer { class Missing }\n",
        ),
        (
            "p/Use.kt",
            "package p\nimport q.Outer\nimport r.*\nclass Use { val value: Outer.Mis<|>sing? = null }\n",
        ),
    ])
    .await;

    assert!(
        workspace.prepare_type_hierarchy("p/Use.kt").await.is_none(),
        "an explicit Outer binding with no Missing member cannot fall through to r.Outer"
    );
}

#[tokio::test]
async fn kotlin_lexical_first_segment_binding_stops_after_a_missing_suffix() {
    let workspace = Workspace::open(&[
        (
            "q/Base.kt",
            "package q\nclass Base { class Missing }\n",
        ),
        (
            "p/Outer.kt",
            "package p\nimport q.*\nclass Outer {\n    class Base\n    val value: Base.Mis<|>sing? = null\n}\n",
        ),
    ])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("p/Outer.kt")
            .await
            .is_none(),
        "lexical p.Outer.Base binds Base even though its Missing suffix is absent"
    );
}

#[tokio::test]
async fn kotlin_current_package_binding_stops_after_a_missing_suffix() {
    let workspace = Workspace::open(&[
        ("p/Outer.kt", "package p\nclass Outer\n"),
        ("q/Outer.kt", "package q\nclass Outer { class Missing }\n"),
        (
            "p/Use.kt",
            "package p\nimport q.*\nclass Use { val value: Outer.Mis<|>sing? = null }\n",
        ),
    ])
    .await;

    assert!(
        workspace.prepare_type_hierarchy("p/Use.kt").await.is_none(),
        "same-package p.Outer binds Outer even though its Missing suffix is absent"
    );
}

#[tokio::test]
async fn kotlin_prepare_does_not_make_a_unique_unimported_type_visible() {
    let workspace = Workspace::open(&[
        ("a/Hidden.kt", "package a\nclass Hidden\n"),
        (
            "b/Use.kt",
            "package b\nclass Use { val value: Hid<|>den? = null }\n",
        ),
    ])
    .await;

    assert!(
        workspace.prepare_type_hierarchy("b/Use.kt").await.is_none(),
        "workspace uniqueness is not a Kotlin import"
    );
}

#[tokio::test]
async fn kotlin_default_imports_are_explicit_resolution_sources() {
    let workspace = Workspace::open(&[
        (
            "kotlin/DefaultBase.kt",
            "package kotlin\nopen class DefaultBase\n",
        ),
        ("p/Child.kt", "package p\nclass Ch<|>ild : DefaultBase()\n"),
    ])
    .await;

    let child = workspace.type_hierarchy_item("p/Child.kt").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(child).await),
        vec!["kotlin.DefaultBase"]
    );
}

#[tokio::test]
async fn kotlin_legacy_item_without_revision_identity_is_rejected() {
    let workspace = Workspace::open(&[
        ("alpha/Parent.kt", "package alpha\nopen class Parent\n"),
        (
            "alpha/Base.kt",
            "package alpha\nopen class Base : Parent()\n",
        ),
        ("beta/Parent.kt", "package beta\nopen class Parent\n"),
        (
            "beta/Base.kt",
            "package beta\nopen class Ba<|>se : Parent()\n",
        ),
    ])
    .await;

    let mut legacy = workspace.type_hierarchy_item("beta/Base.kt").await;
    legacy.data = None;
    legacy.detail = None;
    assert!(
        workspace.supertypes(legacy).await.is_empty(),
        "an item without opaque revision provenance must not be rebound to current bytes"
    );
}

#[tokio::test]
async fn kotlin_stale_encoded_item_does_not_retarget_by_simple_name() {
    let workspace = Workspace::open(&[
        ("p/Parent.kt", "package p\nopen class Parent\n"),
        ("p/Base.kt", "package p\nopen class Ba<|>se : Parent()\n"),
    ])
    .await;

    let mut stale = workspace.type_hierarchy_item("p/Base.kt").await;
    stale.data = Some(serde_json::json!({ "fqn": "removed.Base" }));
    stale.detail = None;
    stale.uri = tower_lsp::lsp_types::Url::parse("file:///removed/Base.kt").unwrap();
    assert!(
        workspace.supertypes(stale).await.is_empty(),
        "a stale encoded identity must not retarget to the unique indexed Base"
    );
}

#[tokio::test]
async fn supertypes_of_a_kotlin_class_extending_a_java_class() {
    let workspace = Workspace::open(&[
        (
            "base/Animal.java",
            r"package base;

public class Animal {
}
",
        ),
        (
            "pets/Dog.kt",
            r"package pets

import base.Animal

class D<|>og : Animal()
",
        ),
    ])
    .await;

    let item = workspace.type_hierarchy_item("pets/Dog.kt").await;
    let supertypes = workspace.supertypes(item).await;

    assert_eq!(type_fqns(&supertypes), vec!["base.Animal"]);
    assert!(
        supertypes[0].uri.as_str().ends_with("Animal.java"),
        "the supertype item points into the Java file: {:?}",
        supertypes[0].uri
    );
}

#[tokio::test]
async fn supertypes_of_an_object() {
    let workspace = Workspace::open(&[
        (
            "zoo/Speaker.kt",
            r"package zoo

interface Speaker {
    fun speak()
}
",
        ),
        (
            "zoo/Silence.kt",
            r"package zoo

object Sile<|>nce : Speaker {
    override fun speak() {
    }
}
",
        ),
    ])
    .await;

    let item = workspace.type_hierarchy_item("zoo/Silence.kt").await;
    let supertypes = workspace.supertypes(item).await;

    assert_eq!(
        type_fqns(&supertypes),
        vec!["zoo.Speaker"],
        "the Kotlin extractor records no supertypes for objects; the CST does"
    );
}

// ---------------------------------------------------------------------------
// subtypes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn subtypes_of_a_kotlin_interface() {
    let workspace = Workspace::open(&[
        (
            "zoo/Speaker.kt",
            r"package zoo

interface Spea<|>ker {
    fun speak()
}
",
        ),
        (
            "zoo/Dog.kt",
            r"package zoo

class Dog : Speaker {
    override fun speak() {
    }
}
",
        ),
        (
            "zoo/Cat.kt",
            r"package zoo

class Cat : Speaker {
    override fun speak() {
    }
}
",
        ),
    ])
    .await;

    let item = workspace.type_hierarchy_item("zoo/Speaker.kt").await;
    let subtypes = workspace.subtypes(item).await;

    assert_eq!(type_fqns(&subtypes), vec!["zoo.Cat", "zoo.Dog"]);
}

#[tokio::test]
async fn subtypes_of_a_sealed_class() {
    let workspace = Workspace::open(&[(
        "Shape.kt",
        r"package geometry

sealed class Sha<|>pe

class Circle : Shape()

class Square : Shape()
",
    )])
    .await;

    let item = workspace.type_hierarchy_item("Shape.kt").await;
    let subtypes = workspace.subtypes(item).await;

    assert_eq!(
        type_fqns(&subtypes),
        vec!["geometry.Circle", "geometry.Square"]
    );
}

#[tokio::test]
async fn subtypes_include_a_java_class_implementing_a_kotlin_interface() {
    let workspace = Workspace::open(&[
        (
            "zoo/Speaker.kt",
            r"package zoo

interface Spea<|>ker {
    fun speak()
}
",
        ),
        (
            "zoo/Parrot.java",
            r"package zoo;

public class Parrot implements Speaker {
    public void speak() {
    }
}
",
        ),
    ])
    .await;

    let item = workspace.type_hierarchy_item("zoo/Speaker.kt").await;
    let subtypes = workspace.subtypes(item).await;

    assert_eq!(type_fqns(&subtypes), vec!["zoo.Parrot"]);
    assert!(
        subtypes[0].uri.as_str().ends_with("Parrot.java"),
        "the subtype item points into the Java file: {:?}",
        subtypes[0].uri
    );
}

#[tokio::test]
async fn subtypes_are_empty_for_a_leaf_type() {
    let workspace = Workspace::open(&[(
        "Leaf.kt",
        r"package zoo

class Le<|>af
",
    )])
    .await;

    let item = workspace.type_hierarchy_item("Leaf.kt").await;

    assert!(
        workspace.subtypes(item).await.is_empty(),
        "a type nothing extends has no subtypes"
    );
}

#[tokio::test]
async fn java_hierarchy_keeps_same_named_packages_separate() {
    let workspace = Workspace::open(&[
        ("alpha/Base.java", "package alpha; public class Base {}"),
        (
            "alpha/Derived.java",
            "package alpha; public class Deri<|>ved extends Base {}",
        ),
        ("beta/Base.java", "package beta; public class Ba<|>se {}"),
        (
            "beta/Derived.java",
            "package beta; public class Derived extends Base {}",
        ),
        (
            "alpha/Use.java",
            "package alpha; public class Use { Ba<|>se value; }",
        ),
    ])
    .await;

    let alpha_derived = workspace.type_hierarchy_item("alpha/Derived.java").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(alpha_derived).await),
        vec!["alpha.Base"]
    );

    let beta_base = workspace.type_hierarchy_item("beta/Base.java").await;
    assert_eq!(
        type_fqns(&workspace.subtypes(beta_base).await),
        vec!["beta.Derived"]
    );

    let alpha_base_reference = workspace.type_hierarchy_item("alpha/Use.java").await;
    assert_eq!(alpha_base_reference.detail.as_deref(), Some("alpha.Base"));
    assert_eq!(
        type_fqns(&workspace.subtypes(alpha_base_reference).await),
        vec!["alpha.Derived"]
    );
}

#[tokio::test]
async fn java_prepare_rejects_non_type_identifier_inside_class() {
    let workspace = Workspace::open(&[("Body.java", "public class Body { int va<|>lue; }")]).await;

    assert!(
        workspace
            .prepare_type_hierarchy("Body.java")
            .await
            .is_none(),
        "a field name is not a type-hierarchy target"
    );
}

#[tokio::test]
async fn java_prepare_rejects_non_type_identifier_that_matches_a_type_name() {
    let workspace = Workspace::open(&[
        ("same/Widget.java", "package same; public class Widget {}"),
        (
            "same/Use.java",
            "package same; public class Use { int Wid<|>get; }",
        ),
    ])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("same/Use.java")
            .await
            .is_none(),
        "a variable declarator is not a type reference even when its spelling matches a type"
    );
}

#[tokio::test]
async fn java_prepare_rejects_ambiguous_wildcard_imports() {
    let workspace = Workspace::open(&[
        ("a/Base.java", "package a; public class Base {}"),
        ("b/Base.java", "package b; public class Base {}"),
        (
            "use/Use.java",
            "package use; import a.*; import b.*; public class Use { Ba<|>se value; }",
        ),
    ])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("use/Use.java")
            .await
            .is_none(),
        "hierarchy preparation must not choose the first indexed wildcard match"
    );
}

#[tokio::test]
async fn java_named_package_cannot_see_a_default_package_type() {
    let workspace = Workspace::open(&[
        ("Hidden.java", "public class Hidden {}"),
        (
            "p/Use.java",
            "package p; public class Use { Hid<|>den value; }",
        ),
    ])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("p/Use.java")
            .await
            .is_none(),
        "named-package Java code cannot access the unnamed package"
    );
}

#[tokio::test]
async fn java_extends_header_does_not_see_the_declarations_own_member_type() {
    let workspace = Workspace::open(&[(
        "p/Child.java",
        "package p; public class Child extends Ba<|>se { public static class Base {} }",
    )])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("p/Child.java")
            .await
            .is_none(),
        "Child.Base is not in scope in Child's own extends header"
    );
}

#[tokio::test]
async fn java_header_annotation_braces_do_not_mark_the_class_body_as_started() {
    let workspace = Workspace::open(&[(
        "p/Child.java",
        "package p; @interface A { int[] value(); } public class Child extends @A({1}) Ba<|>se { public static class Base {} }",
    )])
    .await;

    assert!(
        workspace
            .prepare_type_hierarchy("p/Child.java")
            .await
            .is_none(),
        "an annotation array initializer in the header is not the declaration body brace"
    );
}

#[tokio::test]
async fn java_record_header_sees_the_records_own_member_type() {
    let workspace = Workspace::open(&[(
        "p/Entry.java",
        "package p; public record Entry(Ba<|>se value) { public static class Base {} }",
    )])
    .await;

    let base = workspace.type_hierarchy_item("p/Entry.java").await;
    assert_eq!(base.detail.as_deref(), Some("p.Entry.Base"));
}

#[tokio::test]
async fn java_nested_header_sees_an_inherited_member_type() {
    let workspace = Workspace::open(&[
        (
            "p/Parent.java",
            "package p; public class Parent { public static class Base {} }",
        ),
        (
            "p/Child.java",
            "package p; public class Child extends Parent { class Grandchild extends Ba<|>se {} }",
        ),
    ])
    .await;

    let base = workspace.type_hierarchy_item("p/Child.java").await;
    assert_eq!(base.detail.as_deref(), Some("p.Parent.Base"));
}

#[tokio::test]
async fn java_static_single_import_can_import_a_member_type() {
    let workspace = Workspace::open(&[
        (
            "a/Outer.java",
            "package a; public class Outer { public static class Base {} }",
        ),
        (
            "use/Use.java",
            "package use; import static a.Outer.Base; public class Use { Ba<|>se value; }",
        ),
    ])
    .await;

    let base = workspace.type_hierarchy_item("use/Use.java").await;
    assert_eq!(base.detail.as_deref(), Some("a.Outer.Base"));
}

#[tokio::test]
async fn java_static_wildcard_import_can_import_a_member_type() {
    let workspace = Workspace::open(&[
        (
            "a/Outer.java",
            "package a; public class Outer { public static class Base {} }",
        ),
        (
            "use/Use.java",
            "package use; import static a.Outer.*; public class Use { Ba<|>se value; }",
        ),
    ])
    .await;

    let base = workspace.type_hierarchy_item("use/Use.java").await;
    assert_eq!(base.detail.as_deref(), Some("a.Outer.Base"));
}

#[tokio::test]
async fn java_imported_first_segment_beats_an_unrelated_exact_fqn() {
    let workspace = Workspace::open(&[
        (
            "Outer.java",
            "public class Outer { public static class Base {} }",
        ),
        (
            "q/Outer.java",
            "package q; public class Outer { public static class Base {} }",
        ),
        (
            "p/Child.java",
            "package p; import q.Outer; public class Child extends Outer.Ba<|>se {}",
        ),
    ])
    .await;

    let base = workspace.type_hierarchy_item("p/Child.java").await;
    assert_eq!(base.detail.as_deref(), Some("q.Outer.Base"));
}

#[tokio::test]
async fn java_supertypes_canonically_resolve_an_imported_nested_parent() {
    let workspace = Workspace::open(&[
        (
            "Outer.java",
            "public class Outer { public static class Base {} }",
        ),
        (
            "q/Outer.java",
            "package q; public class Outer { public static class Base {} }",
        ),
        (
            "p/Child.java",
            "package p; import q.Outer; public class Ch<|>ild extends Outer.Base {}",
        ),
    ])
    .await;

    let child = workspace.type_hierarchy_item("p/Child.java").await;
    assert_eq!(
        type_fqns(&workspace.supertypes(child).await),
        vec!["q.Outer.Base"],
        "raw `Outer.Base` must pass through the same exact/ambiguous resolver as indexing"
    );
}

#[tokio::test]
async fn java_stale_encoded_item_does_not_retarget_by_simple_name() {
    let workspace = Workspace::open(&[
        ("p/Parent.java", "package p; public class Parent {}"),
        (
            "p/Base.java",
            "package p; public class Ba<|>se extends Parent {}",
        ),
    ])
    .await;

    let mut stale = workspace.type_hierarchy_item("p/Base.java").await;
    stale.data = Some(serde_json::json!({ "fqn": "removed.Base" }));
    stale.detail = None;
    stale.uri = tower_lsp::lsp_types::Url::parse("file:///removed/Base.java").unwrap();
    assert!(
        workspace.supertypes(stale).await.is_empty(),
        "a stale encoded identity must not retarget to the unique indexed Base"
    );
}

#[tokio::test]
async fn java_subtype_items_preserve_enum_and_record_kinds() {
    let workspace = Workspace::open(&[
        (
            "model/Tagged.java",
            "package model; public interface Tag<|>ged {}",
        ),
        (
            "model/Choice.java",
            "package model; public enum Choice implements Tagged { ONE }",
        ),
        (
            "model/Entry.java",
            "package model; public record Entry(int value) implements Tagged {}",
        ),
    ])
    .await;

    let tagged = workspace.type_hierarchy_item("model/Tagged.java").await;
    let subtypes = workspace.subtypes(tagged).await;
    let kind = |fqn: &str| {
        subtypes
            .iter()
            .find(|item| item.detail.as_deref() == Some(fqn))
            .map(|item| item.kind)
    };
    assert_eq!(kind("model.Choice"), Some(SymbolKind::ENUM));
    assert_eq!(kind("model.Entry"), Some(SymbolKind::STRUCT));
}

#[tokio::test]
async fn kotlin_subtype_items_preserve_a_java_record_kind() {
    let workspace = Workspace::open(&[
        ("model/Tagged.kt", "package model\ninterface Tag<|>ged\n"),
        (
            "model/Entry.java",
            "package model; public record Entry(int value) implements Tagged {}",
        ),
    ])
    .await;

    let tagged = workspace.type_hierarchy_item("model/Tagged.kt").await;
    let subtypes = workspace.subtypes(tagged).await;
    assert_eq!(subtypes.len(), 1);
    assert_eq!(subtypes[0].detail.as_deref(), Some("model.Entry"));
    assert_eq!(subtypes[0].kind, SymbolKind::STRUCT);
}

#[tokio::test]
async fn kotlin_goto_implementation_uses_kotlin_hierarchy_dispatch() {
    use tower_lsp::lsp_types::GotoDefinitionResponse;

    let workspace = Workspace::open(&[
        (
            "api/Service.kt",
            "package api\ninterface Serv<|>ice { fun run() }\n",
        ),
        (
            "impl/ServiceImpl.kt",
            "package impl\nimport api.Service\nclass ServiceImpl : Service { override fun run() {} }\n",
        ),
    ])
    .await;

    let response = workspace
        .goto_implementation("api/Service.kt")
        .await
        .expect("Kotlin interface has an implementation");
    let locations = match response {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| tower_lsp::lsp_types::Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    };
    assert_eq!(locations.len(), 1, "expected one concrete implementation");
    assert!(locations[0].uri.as_str().ends_with("ServiceImpl.kt"));
}

#[tokio::test]
async fn kotlin_goto_implementation_excludes_a_sealed_intermediate_type() {
    use tower_lsp::lsp_types::GotoDefinitionResponse;

    let workspace = Workspace::open(&[
        ("p/Root.kt", "package p\ninterface Ro<|>ot\n"),
        ("p/Middle.kt", "package p\nsealed class Middle : Root\n"),
        ("p/Leaf.kt", "package p\nclass Leaf : Middle()\n"),
    ])
    .await;

    let response = workspace
        .goto_implementation("p/Root.kt")
        .await
        .expect("Root has a concrete leaf implementation");
    let locations = match response {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| tower_lsp::lsp_types::Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    };
    assert_eq!(locations.len(), 1);
    assert!(locations[0].uri.as_str().ends_with("Leaf.kt"));
}

#[tokio::test]
async fn kotlin_type_without_implementations_does_not_fall_back_to_its_definition() {
    let workspace = Workspace::open(&[("p/Root.kt", "package p\ninterface Ro<|>ot\n")]).await;

    assert!(
        workspace.goto_implementation("p/Root.kt").await.is_none(),
        "a recognized type with no implementations has no goto-implementation target"
    );
}

#[tokio::test]
async fn kotlin_non_type_cursor_uses_definition_fallback() {
    use tower_lsp::lsp_types::GotoDefinitionResponse;

    let workspace = Workspace::open(&[
        ("api/Greeting.kt", "package api\nfun greet() {}\n"),
        (
            "app/Main.kt",
            "package app\nimport api.greet\nfun main() { gre<|>et() }\n",
        ),
    ])
    .await;

    let response = workspace
        .goto_implementation("app/Main.kt")
        .await
        .expect("a callable cursor falls back to goto-definition");
    let locations = match response {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| tower_lsp::lsp_types::Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    };
    assert_eq!(locations.len(), 1);
    assert!(locations[0].uri.as_str().ends_with("Greeting.kt"));
}

// ---------------------------------------------------------------------------
// Java behavior through the dispatch layer
// ---------------------------------------------------------------------------

/// A Java-only workspace must produce exactly what the Java provider produces on
/// its own. The opt-in `LanguageHandler` convention means Java never reaches a
/// Kotlin code path, and this pins that.
#[tokio::test]
async fn java_type_hierarchy_round_trips_through_dispatch() {
    let base = r"package app;

public interface Base {
}
";
    let derived = r"package app;

public class Derived implements Base {
}
";

    let workspace =
        Workspace::open(&[("app/Base.java", base), ("app/Derived.java", derived)]).await;

    let reference = java_reference(&workspace);

    let through_server = workspace
        .prepare_type_hierarchy_at("app/Derived.java", 2, 15)
        .await
        .expect("java prepare");
    let direct = reference
        .provider
        .prepare_type_hierarchy(
            reference.derived_file_id,
            tower_lsp::lsp_types::Position::new(2, 15),
            &workspace.file("app/Derived.java").text,
        )
        .expect("java prepare reference")
        .expect("java prepare reference is non-empty");

    assert_eq!(
        type_items_without_revision_tokens(through_server.clone()),
        type_items_without_revision_tokens(direct.clone()),
        "Java prepareTypeHierarchy must preserve the Java provider's public result"
    );

    let server_supertypes = workspace.supertypes(through_server[0].clone()).await;
    assert_eq!(type_fqns(&server_supertypes), vec!["app.Base"]);
    let direct_supertypes = reference
        .provider
        .supertypes(&direct[0])
        .expect("java supertypes reference")
        .expect("top-level Java class has a supertype");
    assert_eq!(
        type_items_without_revision_tokens(server_supertypes),
        type_items_without_revision_tokens(direct_supertypes),
        "Java supertypes must preserve the Java provider's public result"
    );

    let base_item = workspace
        .prepare_type_hierarchy_at("app/Base.java", 2, 18)
        .await
        .expect("java prepare on the interface");
    let server_subtypes = workspace.subtypes(base_item[0].clone()).await;
    assert_eq!(type_fqns(&server_subtypes), vec!["app.Derived"]);
    let direct_base = reference
        .provider
        .prepare_type_hierarchy(
            reference.base_file_id,
            tower_lsp::lsp_types::Position::new(2, 18),
            &workspace.file("app/Base.java").text,
        )
        .expect("java prepare reference")
        .expect("java prepare reference is non-empty");
    let direct_subtypes = reference
        .provider
        .subtypes(&direct_base[0])
        .expect("java subtypes reference")
        .expect("java subtypes reference is non-empty");

    assert_eq!(
        type_items_without_revision_tokens(server_subtypes),
        type_items_without_revision_tokens(direct_subtypes),
        "Java subtypes must preserve the Java provider's public result"
    );
}

#[tokio::test]
async fn java_type_hierarchy_returns_only_direct_subtypes() {
    let workspace = Workspace::open(&[
        (
            "graph/Root.java",
            "package graph; public interface Ro<|>ot {}",
        ),
        (
            "graph/Middle.java",
            "package graph; public abstract class Middle implements Root {}",
        ),
        (
            "graph/Leaf.java",
            "package graph; public final class Leaf extends Middle {}",
        ),
    ])
    .await;

    let root = workspace.type_hierarchy_item("graph/Root.java").await;
    assert_eq!(
        type_fqns(&workspace.subtypes(root).await),
        vec!["graph.Middle"],
        "typeHierarchy/subtypes is a one-edge expansion, not goto-implementation"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct JavaReference {
    provider: hyperion_lsp::type_hierarchy::TypeHierarchyProvider,
    base_file_id: hyperion_semantics::db::FileId,
    derived_file_id: hyperion_semantics::db::FileId,
}

fn type_items_without_revision_tokens(
    mut items: Vec<tower_lsp::lsp_types::TypeHierarchyItem>,
) -> Vec<tower_lsp::lsp_types::TypeHierarchyItem> {
    for item in &mut items {
        let data = item
            .data
            .as_mut()
            .and_then(serde_json::Value::as_object_mut)
            .expect("type hierarchy items must retain opaque object data");
        assert!(data.get("fqn").is_some_and(serde_json::Value::is_string));
        assert!(data.get("w").is_some_and(serde_json::Value::is_string));
        match (data.get("i"), data.get("r"), data.get("x")) {
            (Some(file_id), Some(file_revision), None) => {
                assert!(file_id.is_string());
                assert!(file_revision.is_string());
                data.remove("i");
                data.remove("r");
            }
            (None, None, Some(external_source)) => assert!(external_source.is_string()),
            _ => panic!("type hierarchy item has malformed declaration provenance"),
        }
        data.remove("w");
    }
    items
}

/// The Java answer, computed straight from the Java provider over the same files
/// on disk, with no dispatch layer in between.
fn java_reference(workspace: &Workspace) -> JavaReference {
    use hyperion_workspace::WorkspaceIndex;

    use std::sync::Arc;

    let base_path = workspace
        .file("app/Base.java")
        .uri
        .to_file_path()
        .expect("base path");
    let derived_path = workspace
        .file("app/Derived.java")
        .uri
        .to_file_path()
        .expect("derived path");
    let root = base_path
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf();

    let index = WorkspaceIndex::new(root);
    for (path, source) in [
        (&base_path, &workspace.file("app/Base.java").text),
        (&derived_path, &workspace.file("app/Derived.java").text),
    ] {
        index
            .index_file_with_source(path, source)
            .expect("index reference file");
    }

    let base_file_id = index.get_file_id(&base_path).expect("base file id");
    let derived_file_id = index.get_file_id(&derived_path).expect("derived file id");

    let index = Arc::new(index);

    JavaReference {
        provider: hyperion_lsp::type_hierarchy::TypeHierarchyProvider::new(
            index,
            Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
        ),
        base_file_id,
        derived_file_id,
    }
}
