// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;
use tempfile::TempDir;

fn index_source(index: &WorkspaceIndex, temp: &TempDir, source: &str) {
    index
        .index_file_with_source(&temp.path().join("p/Functions.java"), source)
        .unwrap();
}

#[test]
fn object_methods_are_fallback_members_but_not_function_contracts() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/Object.java"),
            concat!(
                "package java.lang; public class Object { ",
                "public boolean equals(Object other) { return false; } ",
                "public String toString() { return null; } }"
            ),
        )
        .unwrap();
    index_source(
        &index,
        &temp,
        concat!(
            "package p; interface Root { boolean equals(Object other); } ",
            "interface Child extends Root {} interface Plain {}"
        ),
    );

    let equals = index.find_effective_methods_by_name("p.Child", "equals");
    assert_eq!(equals.len(), 1);
    assert_eq!(equals[0].container.as_deref(), Some("p.Root"));
    let to_string = index.find_effective_methods_by_name("p.Plain", "toString");
    assert_eq!(to_string.len(), 1);
    assert_eq!(to_string[0].container.as_deref(), Some("java.lang.Object"));
    assert!(index.find_functional_interface_method("p.Child").is_none());
}

#[test]
fn function_method_uses_effective_inherited_contracts() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; interface Parent { String apply(int value); } ",
            "interface Child extends Parent {} ",
            "interface WithObject extends Child { boolean equals(Object other); }"
        ),
    );

    for interface in ["p.Parent", "p.Child", "p.WithObject"] {
        let method = index
            .find_functional_interface_method(interface)
            .unwrap_or_else(|| panic!("{interface} should have one function method"));
        assert_eq!(method.fqn.as_ref(), "p.Parent.apply(int)");
    }
}

#[test]
fn defaults_object_only_overloads_incompatible_returns_and_sealed_are_not_sams() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; interface Parent { void run(); } ",
            "interface Satisfied extends Parent { default void run() {} } ",
            "interface ObjectOnly { int hashCode(); boolean equals(Object other); ",
            "String toString(); } interface Left { int value(); } ",
            "interface Right { String value(); } interface BadReturn extends Left, Right {} ",
            "interface Overloaded { void accept(String value); void accept(int value); } ",
            "sealed interface SealedAction { void run(); }"
        ),
    );

    for interface in [
        "p.Satisfied",
        "p.ObjectOnly",
        "p.BadReturn",
        "p.Overloaded",
        "p.SealedAction",
    ] {
        assert!(
            index.find_functional_interface_method(interface).is_none(),
            "{interface} must not have a Java function type"
        );
    }
}

#[test]
fn direct_and_inherited_generic_signatures_are_substituted() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; interface Mapper<Input, Output> { Output map(Input value); } ",
            "interface Parent<T> { T apply(T value); } ",
            "interface Middle<U> extends Parent<java.util.List<U>> {} ",
            "interface Child extends Middle<String> {}"
        ),
    );

    let signature = index
        .find_functional_interface_signature(
            "p.Mapper",
            &["p.Request".to_string(), "p.Response".to_string()],
        )
        .expect("direct generic function signature");
    assert_eq!(signature.parameter_types, ["p.Request"]);
    assert_eq!(signature.return_type, "p.Response");
    let inherited = index
        .find_functional_interface_signature("p.Child", &[])
        .expect("inherited generic function signature");
    assert_eq!(inherited.parameter_types, ["java.util.List<String>"]);
    assert_eq!(inherited.return_type, "java.util.List<String>");
    assert_eq!(
        java_parameter_type_from_declaration("java.util.Map<K, V> values"),
        "java.util.Map<K, V>"
    );
    assert_eq!(
        java_parameter_type_from_declaration("java.util.Map<K, V>"),
        "java.util.Map<K, V>"
    );
}

#[test]
fn unrelated_default_and_abstract_conflict_is_not_a_sam() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; interface DefaultSide { default void run() {} } ",
            "interface AbstractSide { void run(); } ",
            "interface Conflict extends DefaultSide, AbstractSide {}"
        ),
    );

    assert_eq!(
        index
            .find_effective_methods_by_name("p.Conflict", "run")
            .len(),
        2
    );
    assert!(index
        .find_functional_interface_method("p.Conflict")
        .is_none());
}

#[test]
fn generic_projection_controls_sam_election_and_default_conflicts() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/String.java"),
            "package java.lang; public final class String {}",
        )
        .unwrap();
    index_source(
        &index,
        &temp,
        concat!(
            "package p; interface Generic<T> { void accept(T value); } ",
            "interface Fixed { void accept(String value); } ",
            "interface Combined extends Generic<String>, Fixed {} ",
            "interface GenericDefault<T> { default void accept(T value) {} } ",
            "interface Conflict extends GenericDefault<String>, Fixed {}"
        ),
    );

    let combined = index
        .find_functional_interface_method("p.Combined")
        .expect("projected override-equivalent abstracts define one function method");
    assert_eq!(combined.simple_name.as_ref(), "accept");
    let signature = index
        .find_functional_interface_signature("p.Combined", &[])
        .expect("the elected generic function signature must project through Combined");
    assert_eq!(signature.parameter_types, ["java.lang.String"]);

    assert_eq!(
        index
            .find_effective_methods_by_name("p.Conflict", "accept")
            .len(),
        2
    );
    assert!(index
        .find_functional_interface_method("p.Conflict")
        .is_none());
}

#[test]
fn duplicate_abstracts_collapse_and_covariant_return_elects_narrower_contract() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; class Base {} class Narrow extends Base {} ",
            "interface Left { void run(); } interface Right { void run(); } ",
            "interface Duplicate extends Left, Right {} ",
            "interface Wide { Base value(); } interface Narrowed { Narrow value(); } ",
            "interface Covariant extends Wide, Narrowed {}"
        ),
    );

    let duplicate = index
        .find_functional_interface_method("p.Duplicate")
        .expect("override-equivalent abstracts define one function method");
    assert_eq!(duplicate.simple_name.as_ref(), "run");
    let covariant = index
        .find_functional_interface_method("p.Covariant")
        .expect("covariant contracts define one function method");
    assert_eq!(covariant.container.as_deref(), Some("p.Narrowed"));
    assert_eq!(covariant.return_type.as_deref(), Some("p.Narrow"));
}

#[test]
fn method_generic_sam_is_elected_but_lambda_signature_is_unavailable() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        "package p; interface GenericMethod { <T> T identity(T value); }",
    );

    let method = index
        .find_functional_interface_method("p.GenericMethod")
        .expect("a generic abstract method still defines the interface function method");
    assert_eq!(method.type_parameters.len(), 1);
    assert!(matches!(
        index.query_functional_interface("p.GenericMethod", &[]),
        FunctionalInterfaceQuery::Unavailable
    ));
}

#[test]
fn raw_function_signature_uses_canonical_erasure_and_declared_bounds() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; class Value {} ",
            "interface Plain<T> { T apply(T value); } ",
            "interface Bounded<T extends Value> { java.util.List<String> apply(T value); } ",
            "interface Child<U> extends Bounded<Value> {}"
        ),
    );
    let plain = index
        .find_functional_interface_signature("p.Plain", &[])
        .unwrap();
    assert_eq!(plain.parameter_types, ["java.lang.Object"]);
    assert_eq!(plain.return_type, "java.lang.Object");
    for target in ["p.Bounded", "p.Child"] {
        let signature = index
            .find_functional_interface_signature(target, &[])
            .unwrap();
        assert_eq!(signature.parameter_types, ["p.Value"], "{target}");
        assert_eq!(signature.return_type, "java.util.List", "{target}");
    }
    assert!(matches!(
        index.query_functional_interface("p.Plain", &["p.Value".into(), "p.Value".into()]),
        FunctionalInterfaceQuery::Unavailable
    ));
}

#[test]
fn wildcard_function_target_projects_ground_parameter_and_return_types() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        &temp,
        concat!(
            "package p; class Value {} ",
            "interface Mapper<T, R> { R apply(T value); } ",
            "interface Bounded<T extends Value> { T apply(T value); } ",
            "interface Recursive<T extends Comparable<T>> { T apply(T value); }"
        ),
    );
    let signature = index
        .find_functional_interface_signature(
            "p.Mapper",
            &[
                "? super\tp.Value".into(),
                "? extends\njava.lang.String".into(),
            ],
        )
        .unwrap();
    assert_eq!(signature.parameter_types, ["p.Value"]);
    assert_eq!(signature.return_type, "java.lang.String");
    let unbounded = index
        .find_functional_interface_signature("p.Mapper", &["?".into(), "?".into()])
        .unwrap();
    assert_eq!(unbounded.parameter_types, ["java.lang.Object"]);
    assert_eq!(unbounded.return_type, "java.lang.Object");
    let bounded = index
        .find_functional_interface_signature("p.Bounded", &["?".into()])
        .unwrap();
    assert_eq!(bounded.parameter_types, ["p.Value"]);
    assert_eq!(bounded.return_type, "p.Value");
    assert!(matches!(
        index.query_functional_interface("p.Recursive", &["?".into()]),
        FunctionalInterfaceQuery::Unavailable
    ));
}
