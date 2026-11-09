// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test to understand parser error behavior

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_valid_code_has_no_errors() {
    let source = r"
package com.example;

public class Calculator {
    private double lastResult;

    public double add(double a, double b) {
        lastResult = a + b;
        return lastResult;
    }
}
";

    let lexer = Lexer::new(source);
    let tokens: Vec<_> = lexer.collect();
    let (_green, errors) = Parser::parse(tokens);

    println!("Errors for valid code: {}", errors.len());
    for error in &errors {
        println!("  - {} at {:?}", error.message, error.range);
    }

    // Valid code should have NO errors
    assert!(
        errors.is_empty(),
        "Valid code should not have errors, but got: {errors:?}"
    );
}

#[test]
fn test_missing_semicolon() {
    let source = r"
package com.example;

public class Test {
    public void foo() {
        int x = 5
    }
}
";

    let lexer = Lexer::new(source);
    let tokens: Vec<_> = lexer.collect();
    let (_green, errors) = Parser::parse(tokens);

    println!("Errors for missing semicolon: {}", errors.len());
    for error in &errors {
        println!("  - {} at {:?}", error.message, error.range);
    }

    // Should have at least one error about missing semicolon
    assert!(
        !errors.is_empty(),
        "Missing semicolon should produce an error"
    );
}

#[test]
fn test_unclosed_brace() {
    let source = r"
package com.example;

public class Test {
    public void foo() {
        int x = 5;
";

    let lexer = Lexer::new(source);
    let tokens: Vec<_> = lexer.collect();
    let (_green, errors) = Parser::parse(tokens);

    println!("Errors for unclosed brace: {}", errors.len());
    for error in &errors {
        println!("  - {} at {:?}", error.message, error.range);
    }

    // Should have errors about unclosed braces
    assert!(!errors.is_empty(), "Unclosed brace should produce an error");
}

#[test]
fn test_simple_class() {
    let source = r"
public class Foo {
}
";

    let lexer = Lexer::new(source);
    let tokens: Vec<_> = lexer.collect();
    let (_green, errors) = Parser::parse(tokens);

    println!("Errors for simple class: {}", errors.len());
    for error in &errors {
        println!("  - {} at {:?}", error.message, error.range);
    }

    // Simple class should have NO errors
    assert_eq!(errors.len(), 0, "Simple valid class should not have errors");
}
