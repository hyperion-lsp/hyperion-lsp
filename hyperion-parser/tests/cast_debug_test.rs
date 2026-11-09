// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_simple_cast() {
    let source = "class Test { void test() { Object o = (String) other; } }";
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Error: {}", error.message);
    }

    assert!(errors.is_empty(), "Simple cast should parse without errors");
}

#[test]
fn test_generic_cast() {
    let source = "class Test { void test() { Object o = (List<String>) other; } }";
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Error: {}", error.message);
    }

    assert!(
        errors.is_empty(),
        "Generic cast should parse without errors"
    );
}

#[test]
fn test_wildcard_cast() {
    let source = "class Test { void test() { Object o = (List<?>) other; } }";
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Error: {}", error.message);
    }

    assert!(
        errors.is_empty(),
        "Wildcard cast should parse without errors"
    );
}

#[test]
fn test_qualified_inner_cast() {
    let source = "class Test { void test() { Object o = (Graph<?>.Edge) other; } }";
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Error: {}", error.message);
    }

    assert!(
        errors.is_empty(),
        "Qualified inner type cast should parse without errors"
    );
}

#[test]
fn test_exact_failing_case() {
    let source = r#"
        class Test {
            void test() {
                Graph<?>.Edge otherEdge = (Graph<?>.Edge) other;
            }
        }
        "#;
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Error at {:?}: {}", error.range, error.message);
    }

    assert!(
        errors.is_empty(),
        "Exact failing case should now parse without errors"
    );
}

#[test]
fn test_var_decl_then_cast() {
    // Single line version
    let source = "class Test { void test() { Graph<?>.Edge o = (Graph<?>.Edge) x; } }";
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Error at {:?}: {}", error.range, error.message);
    }

    assert!(
        errors.is_empty(),
        "Vardecl then cast should parse without errors"
    );
}
