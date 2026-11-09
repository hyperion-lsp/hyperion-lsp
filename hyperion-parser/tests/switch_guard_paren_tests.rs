// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: Java 21 switch pattern guards with a parenthesized
//! condition (`case Integer i when (i > 5) -> ...`).
//!
//! `when` remains a contextual-keyword token in the lossless tree. The parser
//! must distinguish a guard from a Mockito-style `when(...)` call by grammar
//! position, including when the guard condition itself is parenthesized.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{Lexer, Parser};

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green), errors)
}

fn has_when_clause(root: &SyntaxNode) -> bool {
    root.descendants()
        .any(|n| n.kind() == SyntaxKind::WHEN_CLAUSE)
}

#[test]
fn parenthesized_guard_parses_without_errors() {
    let src = "class C { int m(Object o) { return switch (o) { \
               case Integer i when (i > 5) -> 1; default -> 0; }; } }";
    let (root, errors) = parse(src);
    assert!(
        errors.is_empty(),
        "expected no parse errors, got: {errors:?}"
    );
    assert!(has_when_clause(&root), "guard should produce a WHEN_CLAUSE");
}

#[test]
fn unparenthesized_guard_still_parses() {
    let src = "class C { int m(Object o) { return switch (o) { \
               case Integer i when i > 5 -> 1; default -> 0; }; } }";
    let (root, errors) = parse(src);
    assert!(
        errors.is_empty(),
        "expected no parse errors, got: {errors:?}"
    );
    assert!(has_when_clause(&root), "guard should produce a WHEN_CLAUSE");
}

#[test]
fn parenthesized_guard_in_colon_case_parses() {
    let src = "class C { int m(Object o) { switch (o) { \
               case Integer i when (i > 5): return 1; default: return 0; } return 2; } }";
    let (root, errors) = parse(src);
    assert!(
        errors.is_empty(),
        "expected no parse errors, got: {errors:?}"
    );
    assert!(has_when_clause(&root), "guard should produce a WHEN_CLAUSE");
}

#[test]
fn guard_with_nested_lambda_argument_still_parses() {
    // A lambda inside a method call within the guard must still be recognized.
    let src = "class C { int m(Object o, java.util.List<Integer> xs) { \
               return switch (o) { \
               case Integer i when xs.stream().anyMatch(x -> x > i) -> 1; \
               default -> 0; }; } }";
    let (root, errors) = parse(src);
    assert!(
        errors.is_empty(),
        "expected no parse errors, got: {errors:?}"
    );
    assert!(has_when_clause(&root), "guard should produce a WHEN_CLAUSE");
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
        "nested lambda inside the guard should parse as a LAMBDA_EXPR"
    );
}

#[test]
fn mockito_style_when_call_still_parses_as_call() {
    // `when(...)` in expression position must remain a method call, not a guard.
    let src = "class C { void m() { when(mock.foo()).thenReturn(1); } }";
    let (root, errors) = parse(src);
    assert!(
        errors.is_empty(),
        "expected no parse errors, got: {errors:?}"
    );
    assert!(
        !has_when_clause(&root),
        "a Mockito when(...) call must not be parsed as a guard"
    );
}
