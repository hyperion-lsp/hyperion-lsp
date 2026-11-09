// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for array initializer parsing
//!
//! These tests verify JLS §10.6 compliance, including:
//! - Trailing commas in array initializers
//! - Nested array initializers
//! - Empty array initializers
//! - Complex expressions in initializers

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::SyntaxNode;

/// Helper function to parse source code and return the root node
fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

/// Helper function to parse and expect no errors
fn parse_ok(source: &str) -> SyntaxNode {
    let (node, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");
    node
}

// ============================================================================
// Trailing Comma Tests (JLS §10.6)
// ============================================================================

#[test]
fn test_array_init_trailing_comma_primitive() {
    // JLS §10.6: ArrayInitializer: { [VariableInitializerList] [,] }
    // The trailing comma is optional
    parse_ok(
        r#"
        class Test {
            int[] arr = {1, 2, 3,};
        }
        "#,
    );
}

#[test]
fn test_array_init_trailing_comma_string() {
    parse_ok(
        r#"
        class Test {
            String[] arr = {"a", "b", "c",};
        }
        "#,
    );
}

#[test]
fn test_array_init_trailing_comma_in_new_expr() {
    parse_ok(
        r#"
        class Test {
            void test() {
                int[] arr = new int[]{1, 2, 3,};
                String[] strs = new String[]{"a", "b",};
            }
        }
        "#,
    );
}

#[test]
fn test_array_init_trailing_comma_nested() {
    parse_ok(
        r#"
        class Test {
            int[][] matrix = {
                {1, 2,},
                {3, 4,},
            };
        }
        "#,
    );
}

#[test]
fn test_array_init_trailing_comma_complex_expressions() {
    parse_ok(
        r#"
        class Test {
            void test() {
                Object[] arr = {
                    new User("Alice"),
                    calculate(42),
                    list.get(0),
                };
            }
        }
        "#,
    );
}

#[test]
fn test_array_init_no_trailing_comma() {
    // Should still work without trailing comma
    parse_ok(
        r#"
        class Test {
            int[] arr = {1, 2, 3};
        }
        "#,
    );
}

#[test]
fn test_array_init_empty() {
    parse_ok(
        r#"
        class Test {
            int[] arr = {};
            String[] strs = new String[]{};
        }
        "#,
    );
}

#[test]
fn test_array_init_single_element_with_comma() {
    parse_ok(
        r#"
        class Test {
            int[] arr = {42,};
        }
        "#,
    );
}

// ============================================================================
// Nested and Complex Initializers
// ============================================================================

#[test]
fn test_deeply_nested_array_init() {
    parse_ok(
        r#"
        class Test {
            int[][][] cube = {
                {
                    {1, 2,},
                    {3, 4,},
                },
                {
                    {5, 6,},
                    {7, 8,},
                },
            };
        }
        "#,
    );
}

#[test]
fn test_mixed_nested_initializers() {
    parse_ok(
        r#"
        class Test {
            Object[][] mixed = {
                {1, "two", 3.0,},
                {new Object(), null, getValue(),},
            };
        }
        "#,
    );
}

#[test]
fn test_array_init_with_method_calls() {
    parse_ok(
        r#"
        class Test {
            void test() {
                int[] arr = {
                    method1(),
                    method2(arg),
                    obj.method3(),
                };
            }
        }
        "#,
    );
}

#[test]
fn test_array_init_with_ternary() {
    parse_ok(
        r#"
        class Test {
            void test(boolean flag) {
                int[] arr = {
                    flag ? 1 : 2,
                    condition ? getValue() : getDefault(),
                };
            }
        }
        "#,
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_array_init_multiline_trailing_comma() {
    parse_ok(
        r#"
        class Test {
            String[] arr = {
                "first",
                "second",
                "third",
            };
        }
        "#,
    );
}

#[test]
fn test_array_init_inline_trailing_comma() {
    parse_ok(
        r#"
        class Test {
            int[] arr = {1, 2, 3,};
        }
        "#,
    );
}

#[test]
fn test_array_init_with_comments() {
    parse_ok(
        r#"
        class Test {
            int[] arr = {
                1, // first
                2, // second
                3, // third
            };
        }
        "#,
    );
}

#[test]
fn test_anonymous_array_trailing_comma() {
    parse_ok(
        r#"
        class Test {
            void test() {
                processArray(new int[]{1, 2, 3,});
            }
        }
        "#,
    );
}
