// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for array constructor references (JLS §15.13)
//!
//! Array constructor references use the syntax `Type[]::new` where:
//! - Type can be any reference type (class, interface, type parameter)
//! - [] indicates array dimensions (can be multiple: Type[][], Type[][][], etc.)
//! - ::new is the method reference to the array constructor
//!
//! Examples:
//! - String[]::new
//! - List<String>[]::new
//! - CompletableFuture<Void>[]::new
//! - int[]::new (primitive arrays)
//!
//! This was added in Java 8 as part of method references feature.

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::{ParseError, Parser};

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

// ============================================================================
// Basic Array Constructor Reference Tests
// ============================================================================

#[test]
fn test_simple_array_constructor_reference() {
    let source = r#"
        class Test {
            void test() {
                String[] arr = stream.toArray(String[]::new);
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "Simple array constructor reference should parse without errors: {:?}",
        errors
    );
}

#[test]
fn test_primitive_array_constructor_reference() {
    let source = r#"
        class Test {
            void test() {
                int[] arr = stream.toArray(int[]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Primitive array constructor reference should parse"
    );
}

#[test]
fn test_multidimensional_array_constructor_reference() {
    let source = r#"
        class Test {
            void test() {
                String[][] arr = stream.toArray(String[][]::new);
                int[][][] arr2 = stream.toArray(int[][][]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Multi-dimensional array constructor references should parse"
    );
}

// ============================================================================
// Generic Type Array Constructor References
// ============================================================================

#[test]
fn test_raw_generic_type_array_constructor_reference() {
    // Note: Per JLS §15.13, array constructor references use raw types, not parameterized types
    // Valid: List[]::new (raw type)
    // Invalid: List<String>[]::new (parameterized type - not allowed in array constructor refs)
    let source = r#"
        class Test {
            void test() {
                List[] arr = stream.toArray(List[]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Raw generic type array constructor reference should parse"
    );
}

#[test]
fn test_nested_raw_generic_array_constructor_reference() {
    // Array constructor references use raw types even for nested generics
    let source = r#"
        class Test {
            void test() {
                Map[] arr = stream.toArray(Map[]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Nested raw generic array constructor reference should parse"
    );
}

#[test]
fn test_nested_future_array_constructor_reference() {
    // A raw generic array constructor is nested in two method calls.
    let source = r#"
        class Test {
            void test() {
                CompletableFuture.allOf(bookLoads.toArray(CompletableFuture[]::new));
            }
        }
    "#;
    let errors = parse_and_get_errors(source);
    assert!(
        errors.is_empty(),
        "CompletableFuture[]::new nested array constructor should parse without errors: {:?}",
        errors
    );
}

#[test]
fn test_completable_future_raw_type_array_constructor() {
    // Array constructor references must use raw types per JLS §15.13
    let source = r#"
        class Test {
            void test() {
                CompletableFuture.allOf(bookLoads.toArray(CompletableFuture[]::new));
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "CompletableFuture[]::new (raw type) should parse"
    );
}

// ============================================================================
// Array Constructor vs Array Access Disambiguation
// ============================================================================

#[test]
fn test_array_access_still_works() {
    let source = r#"
        class Test {
            void test() {
                int[] arr = new int[10];
                int x = arr[0];
                int y = arr[1 + 2];
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Regular array access should still work"
    );
}

#[test]
fn test_array_access_vs_constructor_reference() {
    let source = r#"
        class Test {
            void test() {
                // Array access
                String[] arr = new String[10];
                String first = arr[0];

                // Array constructor reference
                String[] arr2 = stream.toArray(String[]::new);

                // Both in same expression (hypothetically)
                arr[stream.toArray(String[]::new).length];
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Parser should correctly distinguish array access from constructor reference"
    );
}

// ============================================================================
// Method Reference with Array Type
// ============================================================================

#[test]
fn test_qualified_array_constructor_reference() {
    let source = r#"
        class Test {
            void test() {
                java.lang.String[] arr = stream.toArray(java.lang.String[]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Qualified type names with array constructor reference should parse"
    );
}

#[test]
fn test_inner_class_array_constructor_reference() {
    let source = r#"
        class Outer {
            class Inner {
                void test() {
                    Outer.Inner[] arr = stream.toArray(Outer.Inner[]::new);
                }
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Inner class array constructor reference should parse"
    );
}

// ============================================================================
// Edge Cases and Combinations
// ============================================================================

#[test]
fn test_array_constructor_in_chain() {
    let source = r#"
        class Test {
            void test() {
                Arrays.stream(new String[]{"a", "b"})
                    .map(String::toUpperCase)
                    .toArray(String[]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Array constructor reference in method chain should parse"
    );
}

#[test]
fn test_multiple_array_constructor_references() {
    let source = r#"
        class Test {
            void test() {
                String[] arr1 = stream1.toArray(String[]::new);
                Integer[] arr2 = stream2.toArray(Integer[]::new);
                Object[][] arr3 = stream3.toArray(Object[][]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Multiple array constructor references should parse"
    );
}

#[test]
fn test_array_constructor_reference_as_argument() {
    let source = r#"
        class Test {
            void process(IntFunction<String[]> func) {}

            void test() {
                process(String[]::new);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Array constructor reference as method argument should parse"
    );
}

#[test]
fn test_array_constructor_reference_assigned_to_variable() {
    let source = r#"
        class Test {
            void test() {
                IntFunction<String[]> constructor = String[]::new;
                String[] arr = constructor.apply(10);
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Array constructor reference assigned to variable should parse"
    );
}

// ============================================================================
// Regression Tests - Ensure We Don't Break Existing Functionality
// ============================================================================

#[test]
fn test_regular_constructor_reference_still_works() {
    let source = r#"
        class Test {
            void test() {
                Supplier<String> s = String::new;
                Function<String, Integer> f = Integer::new;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Regular constructor references should still work"
    );
}

#[test]
fn test_method_reference_still_works() {
    let source = r#"
        class Test {
            void test() {
                Function<String, Integer> f = String::length;
                Predicate<String> p = String::isEmpty;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Method references should still work"
    );
}

#[test]
fn test_static_method_reference_still_works() {
    let source = r#"
        class Test {
            void test() {
                Function<String, Integer> f = Integer::parseInt;
                BiFunction<Integer, Integer, Integer> max = Math::max;
            }
        }
    "#;
    assert!(
        parses_successfully(source),
        "Static method references should still work"
    );
}
