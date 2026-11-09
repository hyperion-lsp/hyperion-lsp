// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: static vs instance initializer classification.
//!
//! A `static { ... }` initializer must be parsed as STATIC_INIT and a plain
//! `{ ... }` initializer as INSTANCE_INIT. A prior bug used a raw token
//! lookback that landed on the whitespace between `static` and `{`, so any
//! normally-formatted static initializer (with a space/newline before the
//! brace) was misclassified as an instance initializer.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{Lexer, Parser};

fn parse(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green)
}

fn init_kinds(source: &str) -> Vec<SyntaxKind> {
    parse(source)
        .descendants()
        .map(|n| n.kind())
        .filter(|k| matches!(k, SyntaxKind::STATIC_INIT | SyntaxKind::INSTANCE_INIT))
        .collect()
}

#[test]
fn static_initializer_with_space_before_brace_is_static() {
    assert_eq!(
        init_kinds("class Foo { static { int x = 1; } }"),
        vec![SyntaxKind::STATIC_INIT]
    );
}

#[test]
fn static_initializer_with_newline_before_brace_is_static() {
    assert_eq!(
        init_kinds("class Foo { static\n{ int x = 1; } }"),
        vec![SyntaxKind::STATIC_INIT]
    );
}

#[test]
fn static_initializer_no_space_still_static() {
    assert_eq!(
        init_kinds("class Foo { static{ int x = 1; } }"),
        vec![SyntaxKind::STATIC_INIT]
    );
}

#[test]
fn instance_initializer_is_instance() {
    assert_eq!(
        init_kinds("class Foo { { int x = 1; } }"),
        vec![SyntaxKind::INSTANCE_INIT]
    );
}

#[test]
fn instance_after_static_field_is_instance() {
    // The token before the instance `{` (skipping trivia) is `;`, not `static`.
    assert_eq!(
        init_kinds("class Foo { static int a = 1; { int x = 1; } }"),
        vec![SyntaxKind::INSTANCE_INIT]
    );
}
