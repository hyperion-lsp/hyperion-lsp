// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Array in Generics
//!
//! Java allows array types as generic type arguments per JLS §4.5.1.
//! Example: `Store<Bytes, byte[]>` where `byte[]` is an array type argument.
//!
//! The parser initially failed to handle array dimensions in generic type arguments,
//! treating them incorrectly or causing parse errors.
//!
//! This test suite verifies that:
//! 1. Primitive arrays work as type arguments (byte[], int[])
//! 2. Reference arrays work as type arguments (String[], User[])
//! 3. Multi-dimensional arrays work (int[][], String[][])
//! 4. Arrays in nested generics work (Map<String, byte[]>)
//! 5. Complex nested patterns work

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_primitive_array_as_type_argument() {
    // byte[] as generic type argument
    let source = r#"
        class Test {
            void method() {
                Store<byte[]> store;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse primitive array in generic, got: {:?}",
        errors
    );
}

#[test]
fn test_int_array_as_type_argument() {
    // int[] as generic type argument
    let source = r#"
        class Test {
            void method() {
                List<int[]> list;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse int array in generic, got: {:?}",
        errors
    );
}

#[test]
fn test_reference_array_as_type_argument() {
    // String[] as generic type argument
    let source = r#"
        class Test {
            void method() {
                Store<String[]> store;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse class array in generic, got: {:?}",
        errors
    );
}

#[test]
fn test_multidimensional_array_in_generic() {
    // double[][] as generic type argument
    let source = r#"
        class Test {
            void method() {
                Matrix<double[][]> matrix;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse multidimensional array in generic, got: {:?}",
        errors
    );
}

#[test]
fn test_array_as_second_type_argument() {
    // Array in second position of generic arguments
    let source = r#"
        class Test {
            void method() {
                Map<String, byte[]> map;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array in nested generic, got: {:?}",
        errors
    );
}

#[test]
fn test_multiple_arrays_in_generic() {
    // Both type arguments are arrays
    let source = r#"
        class Test {
            void method() {
                Map<String[], Integer[]> map;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse multiple arrays in generic, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_nested_generic_type() {
    // Array within deeply nested generic
    let source = r#"
        class Test {
            void method() {
                Config<String, Integer, Store<Bytes, byte[]>> config;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse triple generic with nested array, got: {:?}",
        errors
    );
}

#[test]
fn test_array_with_wildcard_extends() {
    // Wildcard extends with array
    let source = r#"
        class Test {
            void method() {
                List<? extends byte[]> list;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array with wildcard bound, got: {:?}",
        errors
    );
}

#[test]
fn test_array_with_wildcard_super() {
    // Wildcard super with array
    let source = r#"
        class Test {
            void method() {
                List<? super int[]> list;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array with super wildcard, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_method_return_type() {
    // Method returning generic with array
    let source = r#"
        class Test {
            List<byte[]> getData() {
                return null;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array in return type generic, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_method_parameter() {
    // Method parameter with generic containing array
    let source = r#"
        class Test {
            void process(List<byte[]> data) {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array in parameter generic, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_field_declaration() {
    // Field with generic containing array
    let source = r#"
        class Test {
            private List<String[]> items;
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array in field type, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_constructor_parameter() {
    // Constructor parameter with generic array
    let source = r#"
        class Test {
            public Test(Map<String, byte[]> data) {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array in constructor parameter, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_cast_expression() {
    // Cast to generic with array
    let source = r#"
        class Test {
            void method(Object obj) {
                List<byte[]> list = (List<byte[]>) obj;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse array in cast generic, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_instanceof() {
    // instanceof with generic containing array
    let source = r#"
        class Test {
            void method(Object obj) {
                if (obj instanceof List<byte[]>) {
                }
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    // instanceof with generic type parses cleanly - type-checking may reject it but the parser handles it
    assert!(
        errors.is_empty(),
        "Parser should handle instanceof with generic type without errors, got: {:?}",
        errors
    );
}

#[test]
fn test_array_in_type_parameter_bound() {
    // Type parameter with array bound
    let source = r#"
        class Test<T extends byte[]> {
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    // Note: This is likely invalid Java semantically, but tests parser handling
    assert!(
        errors.is_empty(),
        "Should parse array in type parameter bound, got: {:?}",
        errors
    );
}

#[test]
fn test_three_dimensional_array_in_generic() {
    // Three-dimensional array as type argument
    let source = r#"
        class Test {
            void method() {
                Tensor<int[][][]> tensor;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse 3D array in generic, got: {:?}",
        errors
    );
}

#[test]
fn test_mixed_array_dimensions_in_generics() {
    // Different array dimensions in different positions
    let source = r#"
        class Test {
            void method() {
                Map<String[], byte[][]> map;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse mixed array dimensions, got: {:?}",
        errors
    );
}
