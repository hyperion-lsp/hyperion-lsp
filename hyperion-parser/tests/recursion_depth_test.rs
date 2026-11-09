// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// M5: Test for correct recursion depth tracking in unary expressions
//
// The bug was that when recursion depth limit is exceeded in parse_unary_expr(),
// the code called decrease_recursion_depth() and then parse_postfix_expr(),
// which could check recursion depth again, throwing off the counter.

use hyperion_parser::{lexer::Lexer, parser::Parser};

#[test]
fn test_recursion_depth_tracking_unary() {
    // Create a very deeply nested unary expression that exceeds recursion limit
    // !!!!!!!!...!!!!value (1000+ levels)
    let mut source = String::new();
    for _ in 0..1100 {
        source.push('!');
    }
    source.push_str("value");

    // Parse - this should not panic or hang due to incorrect depth tracking
    let tokens = Lexer::new(&source).collect::<Vec<_>>();
    let (tree, _errors) = Parser::parse(tokens);

    // The key requirement is completion without crashing or hanging.
    assert!(!tree.to_string().is_empty());
}

#[test]
fn test_recursion_depth_tracking_prefix_operators() {
    // Test with various prefix operators that can chain
    let source = "++".repeat(1100) + "x";

    let tokens = Lexer::new(&source).collect::<Vec<_>>();
    let (_tree, _errors) = Parser::parse(tokens);

    // Should complete without stack overflow
    // This verifies the fix prevents incorrect depth tracking
}

#[test]
fn test_normal_depth_unary_expressions() {
    // Verify normal depth expressions still work correctly
    let source = "!!!value";

    let tokens = Lexer::new(source).collect::<Vec<_>>();
    let (tree, errors) = Parser::parse(tokens);

    // Should parse without errors
    assert_eq!(errors.len(), 0);
    assert!(!tree.to_string().is_empty());
}
