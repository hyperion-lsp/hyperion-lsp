// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: text-block content quotes adjacent to the closing `"""`.
//!
//! Per JLS §3.10.6 a text block is closed by three double-quotes; up to two
//! content quotes may appear directly before the closing delimiter (so
//! `"""\ntext""""` has content `text"`). A prior regex-based lexer could not
//! absorb those content quotes and produced a truncated TEXT_BLOCK_LITERAL plus
//! a spurious error token.

use hyperion_parser::lexer::Lexer;
use hyperion_parser::syntax::SyntaxKind;

fn lex_all(source: &str) -> Vec<(SyntaxKind, String)> {
    Lexer::new(source)
        .map(|(kind, text, _)| (kind, text.to_string()))
        .collect()
}

#[test]
fn text_block_content_ends_with_one_quote() {
    // Opening """, newline, `text`, then one content quote + closing """ = 4 quotes.
    let source = "\"\"\"\ntext\"\"\"\"";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 1, "expected a single token, got: {tokens:?}");
    assert_eq!(tokens[0].0, SyntaxKind::TEXT_BLOCK_LITERAL);
    assert_eq!(tokens[0].1, source, "token must span the whole literal");
}

#[test]
fn text_block_content_ends_with_two_quotes() {
    // `text` + two content quotes + closing """ = 5 trailing quotes.
    let source = "\"\"\"\ntext\"\"\"\"\"";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 1, "expected a single token, got: {tokens:?}");
    assert_eq!(tokens[0].0, SyntaxKind::TEXT_BLOCK_LITERAL);
    assert_eq!(tokens[0].1, source);
}

#[test]
fn plain_text_block_still_lexes() {
    let source = "\"\"\"\nhello world\n\"\"\"";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, SyntaxKind::TEXT_BLOCK_LITERAL);
    assert_eq!(tokens[0].1, source);
}

#[test]
fn empty_text_block_lexes() {
    let source = "\"\"\"\"\"\"";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, SyntaxKind::TEXT_BLOCK_LITERAL);
}

#[test]
fn text_block_with_interior_single_and_double_quotes() {
    // A JSON-ish body with interior quotes must lex as one token.
    let source = "\"\"\"\n{\"a\": \"b\"}\n\"\"\"";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 1, "got: {tokens:?}");
    assert_eq!(tokens[0].0, SyntaxKind::TEXT_BLOCK_LITERAL);
    assert_eq!(tokens[0].1, source);
}

#[test]
fn text_block_full_java_declaration_parses() {
    use hyperion_parser::Parser;
    let source = "class C { String s = \"\"\"\ntext\"\"\"\"; }";
    let tokens = Lexer::new(source).collect();
    let (_green, errors) = Parser::parse(tokens);
    assert!(errors.is_empty(), "expected clean parse, got: {errors:?}");
}
