// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Array types as generic type arguments
//!
//! Java allows array types to be used as generic type arguments per JLS §4.5.1.
//! Example: `BookStore<CoverKey, byte[]>` where `byte[]` is an array type argument.
//!
//! This test file documents patterns that should parse correctly once array types as generic type arguments are supported.

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_simple_array_as_type_argument() {
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
        "Should parse array as type argument, got: {:?}",
        errors
    );
}

#[test]
fn test_primitive_array_in_generic() {
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
fn test_class_array_as_type_argument() {
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
fn test_array_in_nested_generic() {
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
fn test_book_store_with_byte_array() {
    // A primitive array occupies the second generic argument.
    let source = r#"
        class Test {
            void method() {
                BookStore<CoverKey, byte[]> store;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse BookStore<CoverKey, byte[]>, got: {:?}",
        errors
    );
}

#[test]
fn test_triple_generic_with_array() {
    // Three generic arguments, with a nested two-argument type.
    let source = r#"
        class Test {
            void method() {
                CatalogIndex<String, Integer, BookStore<CoverKey, byte[]>> config;
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
fn test_array_with_wildcard_bound() {
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
fn test_array_with_super_wildcard() {
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
fn test_complex_generic_with_multiple_arrays() {
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
fn test_array_in_method_return_type_with_generic() {
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
fn test_array_in_method_parameter_generic() {
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
fn test_catalog_index_nested_array_argument() {
    // A final declaration with nested generic and primitive-array arguments.
    let source = r#"
        class ShelfIndexBuilder {
            void buildIndex() {
                final CatalogIndex<String, Integer, BookStore<CoverKey, byte[]>> catalogIndex;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse catalog index declaration, got: {:?}",
        errors
    );
}

#[test]
fn test_codec_with_array_serde() {
    // An array type occupies a generic value slot.
    let source = r#"
        class Test {
            void test() {
                BookCodec<String, byte[]> codec;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse BookCodec<String, byte[]>, got: {:?}",
        errors
    );
}

#[test]
fn test_array_type_variable() {
    // Generic type variable with array bound
    let source = r#"
        class Test<T extends byte[]> {
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    // Note: This tests if array dimensions are allowed in type bounds
    // May be more complex than standard type arguments
    assert!(
        errors.is_empty(),
        "Should parse array in type variable bound, got: {:?}",
        errors
    );
}
