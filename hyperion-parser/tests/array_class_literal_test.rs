// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for array class literals (JLS §15.8.2)
//!
//! Class literals use the syntax `Type.class` where:
//! - Type can be a primitive type (int, boolean, etc.)
//! - Type can be a reference type (String, List, etc.)
//! - Type can be an array type (String[], int[][], etc.)
//! - Type can be void
//!
//! Examples:
//! - int.class
//! - String.class
//! - byte[].class
//! - Object[][].class
//! - void.class
//!
//! Array class literals are particularly common in:
//! - Mockito: any(byte[].class), eq(String[].class)
//! - Reflection: Class.forName(...), map.put(int[].class, ...)
//! - Serialization frameworks

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::{ParseError, Parser};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Helper to parse and return errors
fn parse_and_get_errors(source: &str) -> Vec<ParseError> {
    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);
    errors
}

/// Helper to check if parsing succeeds without errors
fn parses_successfully(source: &str) -> bool {
    parse_and_get_errors(source).is_empty()
}

/// Helper to verify that class literal nodes are created correctly
fn has_class_literal_expr(source: &str) -> bool {
    let tokens = Lexer::new(source).collect();
    let (root, _) = Parser::parse(tokens);

    // Convert GreenNode to SyntaxNode for traversal
    let syntax_node = SyntaxNode::new_root(root);

    // Walk the tree to find CLASS_LITERAL_EXPR nodes
    syntax_node
        .descendants()
        .any(|node| node.kind() == SyntaxKind::CLASS_LITERAL_EXPR)
}

// ============================================================================
// Primitive Type Class Literals
// ============================================================================

#[test]
fn test_primitive_class_literals() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = int.class;
                Class<?> c2 = boolean.class;
                Class<?> c3 = byte.class;
                Class<?> c4 = short.class;
                Class<?> c5 = long.class;
                Class<?> c6 = char.class;
                Class<?> c7 = float.class;
                Class<?> c8 = double.class;
                Class<?> c9 = void.class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Primitive class literals should parse without errors: {:?}",
        errors
    );
    assert!(
        has_class_literal_expr(source),
        "Should generate CLASS_LITERAL_EXPR nodes"
    );
}

// ============================================================================
// Reference Type Class Literals
// ============================================================================

#[test]
fn test_reference_type_class_literals() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = String.class;
                Class<?> c2 = Object.class;
                Class<?> c3 = Integer.class;
                Class<?> c4 = List.class;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Reference type class literals should parse"
    );
    assert!(
        has_class_literal_expr(source),
        "Should generate CLASS_LITERAL_EXPR nodes"
    );
}

// ============================================================================
// Array Class Literals - The Main Focus
// ============================================================================

#[test]
fn test_primitive_array_class_literals() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = byte[].class;
                Class<?> c2 = int[].class;
                Class<?> c3 = boolean[].class;
                Class<?> c4 = long[].class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Primitive array class literals should parse without errors: {:?}",
        errors
    );
    assert!(
        has_class_literal_expr(source),
        "Should generate CLASS_LITERAL_EXPR nodes"
    );
}

#[test]
fn test_reference_array_class_literals() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = String[].class;
                Class<?> c2 = Object[].class;
                Class<?> c3 = Integer[].class;
                Class<?> c4 = List[].class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Reference array class literals should parse without errors: {:?}",
        errors
    );
    assert!(
        has_class_literal_expr(source),
        "Should generate CLASS_LITERAL_EXPR nodes"
    );
}

#[test]
fn test_multidimensional_array_class_literals() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = int[][].class;
                Class<?> c2 = String[][].class;
                Class<?> c3 = Object[][][].class;
                Class<?> c4 = byte[][][][].class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Multi-dimensional array class literals should parse without errors: {:?}",
        errors
    );
    assert!(
        has_class_literal_expr(source),
        "Should generate CLASS_LITERAL_EXPR nodes"
    );
}

// ============================================================================
// Real-World Usage Patterns (Mockito, Reflection, Serialization)
// ============================================================================

#[test]
fn test_map_put_class_literal_pattern() {
    // Common pattern in serialization frameworks
    let source = r#"
        class Test {
            void test() {
                map.put(String[].class, Arrays.asList("a", "b"));
                map.put(byte[].class, Arrays.asList(null));
                map.put(int[].class, Arrays.asList(1, 2, 3));
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Map.put with array class literals should parse without errors: {:?}",
        errors
    );
}

#[test]
fn test_class_for_name_pattern() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = Class.forName("[Ljava.lang.String;");
                Class<?> c2 = String[].class;
                assert c1 == c2;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Class.forName pattern with array class literal should parse"
    );
}

#[test]
fn test_catalog_codec_array_literal() {
    // A primitive array type appears both in a generic and a class literal.
    let source = r#"
        class CatalogCodecs {
            public static final Codec<byte[]> cover = new CoverCodec();

            void test() {
                Class<?> c = byte[].class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "CatalogCodecs byte[].class pattern should parse without errors: {:?}",
        errors
    );
}

// ============================================================================
// Edge Cases and Combinations
// ============================================================================

#[test]
fn test_array_class_literal_in_expressions() {
    let source = r#"
        class Test {
            void test() {
                // In method calls
                process(String[].class);

                // In variable declarations
                Class<?> c = int[].class;

                // In ternary expressions
                Class<?> x = flag ? String[].class : Object[].class;

                // In comparisons
                if (obj.getClass() == byte[].class) {}

                // In instanceof (not valid Java, but should parse)
                // if (obj instanceof String[].class) {} // Not valid
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Array class literals in various expression contexts should parse"
    );
}

#[test]
fn test_array_class_literal_with_whitespace() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = byte[  ].class;
                Class<?> c2 = String[  ][  ].class;
                Class<?> c3 = int
                    [
                    ]
                    .
                    class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Array class literals with whitespace should parse: {:?}",
        errors
    );
}

#[test]
fn test_qualified_type_array_class_literal() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = java.lang.String[].class;
                Class<?> c2 = java.util.List[].class;
                Class<?> c3 = com.example.MyClass[][].class;
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Qualified type array class literals should parse: {:?}",
        errors
    );
}

// ============================================================================
// Disambiguation Tests
// ============================================================================

#[test]
fn test_array_access_vs_array_class_literal() {
    let source = r#"
        class Test {
            void test() {
                // Array access
                String[] arr = new String[10];
                String first = arr[0];

                // Array class literal
                Class<?> c = String[].class;

                // Both in same expression
                if (arr[0].getClass() == String.class) {}
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Parser should correctly distinguish array access from array class literal"
    );
}

#[test]
fn test_array_constructor_ref_vs_array_class_literal() {
    let source = r#"
        class Test {
            void test() {
                // Array constructor reference
                String[] arr1 = stream.toArray(String[]::new);

                // Array class literal
                Class<?> c = String[].class;

                // Both in same line
                stream.toArray(String[]::new).getClass() == String[].class;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Parser should handle both array constructor refs and class literals"
    );
}

// ============================================================================
// Regression Tests - Ensure We Don't Break Existing Functionality
// ============================================================================

#[test]
fn test_regular_class_literal_still_works() {
    let source = r#"
        class Test {
            void test() {
                Class<?> c1 = String.class;
                Class<?> c2 = int.class;
                Class<?> c3 = void.class;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Regular class literals should still work"
    );
}

#[test]
fn test_field_access_still_works() {
    let source = r#"
        class Test {
            String field;

            void test() {
                this.field = "value";
                obj.field = "value";
                String x = this.field;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Field access should still work after fixing class literals"
    );
}

#[test]
fn test_method_call_still_works() {
    let source = r#"
        class Test {
            void test() {
                obj.method();
                this.method();
                String.valueOf(123);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Method calls should still work after fixing class literals"
    );
}
