// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for generic type-argument extraction:
//! - wildcard type args (`? extends T`, `? super T`, bare `?`) must not be
//!   dropped (which previously also mis-flagged the type as a diamond `<>`),
//! - array-valued type args (`List<String[]>`) must not abort extraction,
//! - nested generic names (`Outer<String>.Inner`) must keep the inner name.

use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use hyperion_types::extract_type_structure;

fn first_class_type(code: &str) -> SyntaxNode {
    let tokens = Lexer::new(code).collect();
    let (green, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green);
    root.descendants()
        .find(|c| c.kind() == SyntaxKind::CLASS_TYPE)
        .expect("should find a CLASS_TYPE")
}

/// `List<? extends Number>` and `List<Number>` are different types (JLS 4.5.1:
/// a wildcard type argument denotes a *set* of types, not the bound itself), so
/// extraction must keep the variance as well as the bound. `TypeStructure`
/// encodes that in its existing fields as the marker base `? extends` carrying
/// the bound as its single type argument -- see `extract_wildcard_type` and
/// `TypeStructure::wildcard_bound`. This test previously asserted the flattened
/// shape (`type_args[0].base == "Number"`), which was the behaviour before
/// the variance-preserving marker was deliberately introduced; it
/// had been asserting a contract that discards variance ever since.
#[test]
fn bounded_wildcard_keeps_bound_and_is_not_diamond() {
    let node = first_class_type("class T { java.util.List<? extends Number> x; }");
    let ts = extract_type_structure(&node).expect("should extract");
    assert_eq!(ts.base, "java.util.List");
    assert!(!ts.is_diamond, "wildcard generic must not be a diamond");
    assert_eq!(ts.type_args.len(), 1, "wildcard arg must be present");

    let wildcard = &ts.type_args[0];
    assert_eq!(wildcard.base, "? extends", "variance must be preserved");
    assert_eq!(
        wildcard.type_args.len(),
        1,
        "the bound must be preserved, not dropped"
    );
    assert_eq!(wildcard.type_args[0].base, "Number");
    assert_eq!(ts.to_simple_string(), "List<? extends Number>");
}

/// The contravariant twin of the above: `? super` must be distinguishable from
/// `? extends`, otherwise `List<? super Number>` and `List<? extends Number>`
/// would extract identically.
#[test]
fn contravariant_wildcard_keeps_bound_and_variance() {
    let node = first_class_type("class T { java.util.List<? super Number> x; }");
    let ts = extract_type_structure(&node).expect("should extract");
    assert!(!ts.is_diamond);
    assert_eq!(ts.type_args.len(), 1);

    let wildcard = &ts.type_args[0];
    assert_eq!(wildcard.base, "? super");
    assert_eq!(wildcard.type_args.len(), 1);
    assert_eq!(wildcard.type_args[0].base, "Number");
    assert_eq!(ts.to_simple_string(), "List<? super Number>");
}

#[test]
fn unbounded_wildcard_is_placeholder_not_diamond() {
    let node = first_class_type("class T { java.util.List<?> x; }");
    let ts = extract_type_structure(&node).expect("should extract");
    assert!(!ts.is_diamond, "List<?> is not a diamond");
    assert_eq!(ts.type_args.len(), 1);
    assert_eq!(ts.type_args[0].base, "?");
}

#[test]
fn wildcard_preserves_positional_alignment() {
    // Map<?, String>: wildcard is the KEY (index 0), String is the VALUE (index 1).
    let node = first_class_type("class T { java.util.Map<?, String> x; }");
    let ts = extract_type_structure(&node).expect("should extract");
    assert_eq!(ts.type_args.len(), 2, "both args must be present");
    assert_eq!(ts.type_args[0].base, "?", "key slot is the wildcard");
    assert_eq!(ts.type_args[1].base, "String", "value slot is String");
}

#[test]
fn double_wildcard_map_is_not_diamond() {
    let node = first_class_type("class T { java.util.Map<?, ?> x; }");
    let ts = extract_type_structure(&node).expect("should extract");
    assert!(!ts.is_diamond);
    assert_eq!(ts.type_args.len(), 2);
}

#[test]
fn array_valued_type_arg_extracts_with_dimensions() {
    let node = first_class_type("class T { java.util.List<String[]> x; }");
    let ts = extract_type_structure(&node).expect("array-valued arg must not abort");
    assert_eq!(ts.base, "java.util.List");
    assert_eq!(ts.type_args.len(), 1);
    assert_eq!(ts.type_args[0].base, "String");
    assert_eq!(ts.type_args[0].dimensions, 1, "String[] has one dimension");
}

#[test]
fn primitive_array_valued_type_arg_extracts() {
    let node = first_class_type("class T { java.util.Comparator<int[]> x; }");
    let ts = extract_type_structure(&node).expect("primitive array arg must not abort");
    assert_eq!(ts.type_args.len(), 1);
    assert_eq!(ts.type_args[0].dimensions, 1);
}

#[test]
fn nested_generic_name_keeps_inner_segment() {
    let node = first_class_type("class T { Outer<String>.Inner x; }");
    let ts = extract_type_structure(&node).expect("should extract");
    assert_eq!(ts.base, "Outer.Inner", "inner name must not be dropped");
    assert_eq!(ts.type_args.len(), 1);
    assert_eq!(ts.type_args[0].base, "String");
}

#[test]
fn plain_diamond_still_detected() {
    // Sanity: a genuine diamond `<>` still flags is_diamond.
    let node = first_class_type(
        "class T { java.util.List<Integer> f() { return new java.util.ArrayList<>(); } }",
    );
    // Find the ArrayList<> node specifically (the diamond one).
    let tokens =
        Lexer::new("class T { void m() { var x = new java.util.ArrayList<>(); } }").collect();
    let (green, _e) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green);
    let diamond = root
        .descendants()
        .filter(|c| c.kind() == SyntaxKind::CLASS_TYPE)
        .find(|c| c.text().to_string().contains("<>"))
        .expect("should find diamond CLASS_TYPE");
    let ts = extract_type_structure(&diamond).expect("should extract");
    assert!(ts.is_diamond, "genuine <> must still be a diamond");
    let _ = node;
}
