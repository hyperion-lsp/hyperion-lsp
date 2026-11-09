// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for wildcard generic array types
//!
//! These tests verify JLS §4.3 + §4.5.1 compliance:
//! - Array types can have any reference type as element type
//! - Wildcards (?, ? extends T, ? super T) are valid type arguments
//! - Therefore Type<?>[] is legal

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
// Basic Wildcard Array Tests
// ============================================================================

#[test]
fn test_unbounded_wildcard_array() {
    // Class<?>[]
    parse_ok(
        r#"
        class Test {
            Class<?>[] classes;
        }
        "#,
    );
}

#[test]
fn test_unbounded_wildcard_array_initialization() {
    parse_ok(
        r#"
        class Test {
            void test() {
                Class<?>[] classes = new Class<?>[5];
            }
        }
        "#,
    );
}

#[test]
fn test_unbounded_wildcard_array_local_variable() {
    parse_ok(
        r#"
        class Test {
            void test() {
                Class<?>[] argTypes = new Class<?>[10];
            }
        }
        "#,
    );
}

// ============================================================================
// Bounded Wildcard Arrays
// ============================================================================

#[test]
fn test_extends_wildcard_array() {
    // List<? extends Number>[]
    parse_ok(
        r#"
        class Test {
            List<? extends Number>[] lists;
        }
        "#,
    );
}

#[test]
fn test_super_wildcard_array() {
    // List<? super Integer>[]
    parse_ok(
        r#"
        class Test {
            List<? super Integer>[] lists;
        }
        "#,
    );
}

#[test]
fn test_complex_bounded_wildcard_array() {
    parse_ok(
        r#"
        class Test {
            Map<String, ? extends List<Integer>>[] maps;
        }
        "#,
    );
}

// ============================================================================
// Multiple Wildcard Parameters
// ============================================================================

#[test]
fn test_map_double_wildcard_array() {
    // Map<?, ?>[]
    parse_ok(
        r#"
        class Test {
            Map<?, ?>[] maps;
        }
        "#,
    );
}

#[test]
fn test_map_mixed_wildcard_array() {
    // Map<String, ?>[]
    parse_ok(
        r#"
        class Test {
            Map<String, ?>[] maps;
        }
        "#,
    );
}

#[test]
fn test_map_wildcard_extends_array() {
    parse_ok(
        r#"
        class Test {
            Map<? extends String, ? extends Number>[] maps;
        }
        "#,
    );
}

// ============================================================================
// Multi-Dimensional Wildcard Arrays
// ============================================================================

#[test]
fn test_2d_wildcard_array() {
    // Class<?>[][]
    parse_ok(
        r#"
        class Test {
            Class<?>[][] matrix;
        }
        "#,
    );
}

#[test]
fn test_3d_wildcard_array() {
    parse_ok(
        r#"
        class Test {
            List<?>[][][] cube;
        }
        "#,
    );
}

#[test]
fn test_mixed_dimension_wildcard_array() {
    parse_ok(
        r#"
        class Test {
            Map<String, ? extends List<Integer>>[][] maps;
        }
        "#,
    );
}

#[test]
fn test_method_parameters_wildcard_array() {
    parse_ok(
        r#"
        class Test {
            void process(Class<?>[] types) {
                // Process types
            }
        }
        "#,
    );
}

#[test]
fn test_method_return_wildcard_array() {
    parse_ok(
        r#"
        class Test {
            Class<?>[] getTypes() {
                return new Class<?>[0];
            }
        }
        "#,
    );
}

// ============================================================================
// Complex Scenarios
// ============================================================================

#[test]
fn test_wildcard_array_with_varargs() {
    parse_ok(
        r#"
        class Test {
            void method(Class<?>... types) {
                // Varargs are arrays internally
            }
        }
        "#,
    );
}

#[test]
fn test_wildcard_array_in_generic_method() {
    parse_ok(
        r#"
        class Test {
            <T> void method(Class<? extends T>[] types) {
                // Generic method with wildcard array parameter
            }
        }
        "#,
    );
}

#[test]
fn test_nested_generic_wildcard_array() {
    parse_ok(
        r#"
        class Test {
            List<Set<? extends Number>>[] listOfSets;
        }
        "#,
    );
}

#[test]
fn test_wildcard_array_cast() {
    parse_ok(
        r#"
        class Test {
            void test(Object obj) {
                Class<?>[] types = (Class<?>[]) obj;
            }
        }
        "#,
    );
}

#[test]
fn test_wildcard_array_instanceof() {
    parse_ok(
        r#"
        class Test {
            void test(Object obj) {
                if (obj instanceof Class<?>[]) {
                    // Type check
                }
            }
        }
        "#,
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_wildcard_array_null_assignment() {
    parse_ok(
        r#"
        class Test {
            void test() {
                Class<?>[] types = null;
                List<? extends Number>[] lists = null;
            }
        }
        "#,
    );
}

#[test]
fn test_wildcard_array_field_initialization() {
    parse_ok(
        r#"
        class Test {
            private Class<?>[] types = new Class<?>[0];
            private List<?>[] lists = new List<?>[10];
        }
        "#,
    );
}

#[test]
fn test_wildcard_array_in_ternary() {
    parse_ok(
        r#"
        class Test {
            void test(boolean flag) {
                Class<?>[] types = flag ? new Class<?>[1] : new Class<?>[2];
            }
        }
        "#,
    );
}

#[test]
fn test_wildcard_array_method_chaining() {
    parse_ok(
        r#"
        class Test {
            Class<?>[] getTypes() {
                return new Class<?>[0];
            }

            void test() {
                int length = getTypes().length;
            }
        }
        "#,
    );
}

// ============================================================================
// Combined Patterns (Wildcards + Trailing Commas)
// ============================================================================

#[test]
fn test_wildcard_array_initializer_with_trailing_comma() {
    parse_ok(
        r#"
        class Test {
            void test() {
                Class<?>[] types = {
                    String.class,
                    Integer.class,
                    Boolean.class,
                };
            }
        }
        "#,
    );
}

#[test]
fn test_complex_wildcard_array_with_trailing_comma() {
    parse_ok(
        r#"
        class Test {
            void test() {
                Map<String, ? extends Number>[] maps = {
                    new HashMap<>(),
                    new TreeMap<>(),
                };
            }
        }
        "#,
    );
}
