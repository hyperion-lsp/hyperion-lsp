// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JLS §14.14.1: the init clause of a basic `for` is a LocalVariableDeclaration,
//! which allows `final` and annotations. jackson-databind uses
//! `for (final int len = props.length; i < len; ++i)`, which produced twenty
//! parse errors on valid code.

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

#[test]
fn for_init_accepts_final_modifier() {
    let (root, errors) = parse(
        "class A { void m(int[] props) { int i = 0; for (final int len = props.length; i < len; ++i) { } } }",
    );
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::FOR_STMT));
}

#[test]
fn for_init_accepts_annotations_and_final_together() {
    let (_, errors) = parse(
        "class A { void m() { for (@SuppressWarnings(\"x\") final long a = 0, b = 1; a < b; a++) { } } }",
    );
    assert!(errors.is_empty(), "parse errors: {errors:?}");
}

#[test]
fn for_init_without_modifiers_still_parses() {
    let (_, errors) =
        parse("class A { void m() { for (int i = 0, j = 1; i < j; i++) { } for (;;) { } } }");
    assert!(errors.is_empty(), "parse errors: {errors:?}");
}
