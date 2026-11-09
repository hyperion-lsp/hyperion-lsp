// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Cast >> Depth
//!
//! The parser had a critical bug where casting to nested generic types caused
//! `type_arg_depth` to go negative, leading to incorrect parsing.
//!
//! Root cause: When encountering `(Foo<Bar<T>>)`, the parser would:
//! 1. Enter cast context, increment type_arg_depth
//! 2. Parse `Foo<Bar<T>>` correctly
//! 3. On `>>`, decrement type_arg_depth TWICE (treating >> as two >)
//! 4. This could cause type_arg_depth to go negative
//!
//! The fix ensures type_arg_depth is properly managed during cast expressions
//! and that >> is correctly handled as either:
//! - Two closing angle brackets in nested generics
//! - A right shift operator in non-generic contexts

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_simple_generic_cast() {
    // Basic cast with single generic parameter
    let source = r#"
        class Test {
            void method(Object obj) {
                List<String> list = (List<String>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse simple generic cast, got: {:?}",
        errors
    );
}

#[test]
fn test_nested_generic_cast_with_double_angle() {
    // This is the core bug case: (Foo<Bar<T>>)
    // The >> should be treated as two closing angle brackets, not right shift
    let source = r#"
        class Test {
            void method(Object obj) {
                Map<String, List<Integer>> map = (Map<String, List<Integer>>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse nested generic cast with >>, got: {:?}",
        errors
    );
}

#[test]
fn test_triple_nested_generic_cast() {
    // Even deeper nesting: (Foo<Bar<Baz<T>>>)
    // Three consecutive > characters
    let source = r#"
        class Test {
            void method(Object obj) {
                Box<Container<Wrapper<String>>> box = (Box<Container<Wrapper<String>>>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse triple nested generic cast with >>>, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_followed_by_method_call() {
    // Cast in method chain
    let source = r#"
        class Test {
            void method(Object obj) {
                String value = ((List<String>) obj).get(0);
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast followed by method call, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_in_return_statement() {
    // Cast as return value
    let source = r#"
        class Test {
            List<Integer> method(Object obj) {
                return (List<Integer>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast in return statement, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_with_wildcard() {
    // Cast to generic with wildcard
    let source = r#"
        class Test {
            void method(Object obj) {
                List<?> list = (List<?>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast with wildcard, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_with_bounded_wildcard() {
    // Cast to generic with bounded wildcard
    let source = r#"
        class Test {
            void method(Object obj) {
                List<? extends Number> list = (List<? extends Number>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast with bounded wildcard, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_nested_in_expression() {
    // Cast as part of larger expression
    let source = r#"
        class Test {
            void method(Object obj) {
                int size = ((List<String>) obj).size() + 10;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast nested in expression, got: {:?}",
        errors
    );
}

#[test]
fn test_multiple_casts_in_method() {
    // Multiple casts in same method body
    let source = r#"
        class Test {
            void method(Object obj1, Object obj2) {
                List<String> list = (List<String>) obj1;
                Map<String, Integer> map = (Map<String, Integer>) obj2;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse multiple casts in method, got: {:?}",
        errors
    );
}

#[test]
fn test_right_shift_vs_generic_close() {
    // Ensure actual right shift operator still works
    let source = r#"
        class Test {
            void method() {
                int x = 16 >> 2;  // Right shift operator
                List<List<String>> nested;  // Generic close
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should distinguish >> operator from generic close, got: {:?}",
        errors
    );
}

#[test]
fn test_unsigned_right_shift_vs_generic_close() {
    // Ensure >>> (unsigned right shift) works
    let source = r#"
        class Test {
            void method() {
                int x = 16 >>> 2;  // Unsigned right shift
                Box<Container<Wrapper<String>>> nested;  // Triple generic close
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should distinguish >>> operator from generic close, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_in_assignment_chain() {
    // Cast in chained assignment
    let source = r#"
        class Test {
            void method(Object obj) {
                List<String> list1, list2;
                list1 = list2 = (List<String>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast in assignment chain, got: {:?}",
        errors
    );
}

#[test]
fn test_cast_with_array_and_generic() {
    // Cast to generic containing array type
    let source = r#"
        class Test {
            void method(Object obj) {
                List<String[]> list = (List<String[]>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse cast with array in generic, got: {:?}",
        errors
    );
}
