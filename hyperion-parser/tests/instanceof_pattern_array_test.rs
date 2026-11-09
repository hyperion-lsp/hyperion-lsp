// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test instanceof pattern matching with array types (Java 16+)
//!
//! Array types in pattern variables.
//! Fixes: `obj instanceof byte[] b` and similar array type patterns

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_instanceof_pattern_primitive_array() {
    let source = r#"
        class Test {
            void check(Object obj) {
                if (obj instanceof byte[] coverBytes) {
                    process(coverBytes);
                }
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse instanceof with primitive array type pattern, got: {errors:?}"
    );
}

#[test]
fn test_instanceof_pattern_reference_array() {
    let source = r#"
        class Test {
            void check(Object obj) {
                if (obj instanceof String[] strings) {
                    System.out.println(strings.length);
                }
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse instanceof with reference array type pattern, got: {errors:?}"
    );
}

#[test]
fn test_instanceof_pattern_multi_dimensional_array() {
    let source = r#"
        class Test {
            void check(Object obj) {
                if (obj instanceof int[][] matrix) {
                    System.out.println(matrix.length);
                }
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse instanceof with multi-dimensional array pattern, got: {errors:?}"
    );
}

// NOTE: Generic array types (List<String>[]) are a known edge case
// that would require additional parser work. Uncomment when fixed.
//
// #[test]
// fn test_instanceof_pattern_generic_array() {
//     let source = r#"
//         class Test {
//             void check(Object obj) {
//                 if (obj instanceof List<String>[] lists) {
//                     System.out.println(lists.length);
//                 }
//             }
//         }
//     "#;
//     let tokens = Lexer::new(source).collect();
//     let (_tree, errors) = Parser::parse(tokens);
//
//     assert!(
//         errors.is_empty(),
//         "Should parse instanceof with generic array type pattern, got: {errors:?}"
//     );
// }

#[test]
fn test_instanceof_regular_array_type() {
    // Non-pattern instanceof (no variable binding) should still work
    let source = r#"
        class Test {
            void check(Object obj) {
                if (obj instanceof byte[]) {
                    byte[] coverBytes = (byte[]) obj;
                }
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse regular instanceof with array type (no pattern), got: {errors:?}"
    );
}
