// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Annotation Operators
//!
//! Java annotations can contain constant expressions as values, including:
//! - Arithmetic operators: +, -, *, /, %
//! - Bitwise operators: &, |, ^, ~
//! - Shift operators: <<, >>, >>>
//! - Logical operators: &&, ||, !
//! - Comparison operators: ==, !=, <, >, <=, >=
//! - Ternary operator: ? :
//! - String concatenation: +
//!
//! The parser initially had issues handling certain operators in annotation contexts,
//! particularly:
//! 1. Bitwise operators (&, |, ^) being confused with other syntax
//! 2. Shift operators (<<, >>) conflicting with generic type syntax
//! 3. Ternary expressions in annotations
//! 4. Complex nested expressions
//!
//! Per JLS §9.7.1, annotation element values must be constant expressions that can
//! be evaluated at compile time.

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_annotation_with_addition() {
    // Simple arithmetic in annotation
    let source = r#"
        class Test {
            @SuppressWarnings(value = "test" + "ing")
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with addition, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_bitwise_and() {
    // Bitwise AND in annotation - core bug case
    let source = r#"
        @interface Flags {
            int value();
        }

        class Test {
            @Flags(0x01 & 0xFF)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with bitwise AND, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_bitwise_or() {
    // Bitwise OR in annotation
    let source = r#"
        @interface Flags {
            int value();
        }

        class Test {
            @Flags(0x01 | 0x02)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with bitwise OR, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_bitwise_xor() {
    // Bitwise XOR in annotation
    let source = r#"
        @interface Flags {
            int value();
        }

        class Test {
            @Flags(0xFF ^ 0x0F)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with bitwise XOR, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_left_shift() {
    // Left shift operator in annotation
    let source = r#"
        @interface Size {
            int value();
        }

        class Test {
            @Size(1 << 10)  // 1024
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with left shift, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_right_shift() {
    // Right shift operator in annotation - can conflict with generics
    let source = r#"
        @interface Size {
            int value();
        }

        class Test {
            @Size(1024 >> 2)  // 256
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with right shift, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_unsigned_right_shift() {
    // Unsigned right shift in annotation
    let source = r#"
        @interface Size {
            int value();
        }

        class Test {
            @Size(-1024 >>> 2)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with unsigned right shift, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_logical_and() {
    // Logical AND in annotation
    let source = r#"
        @interface Check {
            boolean value();
        }

        class Test {
            @Check(true && false)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with logical AND, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_logical_or() {
    // Logical OR in annotation
    let source = r#"
        @interface Check {
            boolean value();
        }

        class Test {
            @Check(true || false)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with logical OR, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_ternary() {
    // Ternary operator in annotation - complex case
    let source = r#"
        @interface Config {
            int value();
        }

        class Test {
            @Config(true ? 1 : 2)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with ternary, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_nested_ternary() {
    // Nested ternary expressions
    let source = r#"
        @interface Config {
            int value();
        }

        class Test {
            @Config(true ? (false ? 1 : 2) : 3)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with nested ternary, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_comparison() {
    // Comparison operators in annotation
    let source = r#"
        @interface Check {
            boolean value();
        }

        class Test {
            @Check(5 > 3)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with comparison, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_complex_expression() {
    // Complex expression combining multiple operators
    let source = r#"
        @interface Flags {
            int value();
        }

        class Test {
            @Flags((0x01 | 0x02) & 0xFF)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with complex expression, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_all_bitwise_operators() {
    // All bitwise operators in one annotation
    let source = r#"
        @interface Multi {
            int and();
            int or();
            int xor();
            int not();
        }

        class Test {
            @Multi(
                and = 0xFF & 0x0F,
                or = 0x01 | 0x02,
                xor = 0xFF ^ 0x0F,
                not = ~0x0F
            )
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with all bitwise operators, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_all_shift_operators() {
    // All shift operators
    let source = r#"
        @interface Shifts {
            int left();
            int right();
            int unsigned();
        }

        class Test {
            @Shifts(
                left = 1 << 10,
                right = 1024 >> 2,
                unsigned = -1 >>> 8
            )
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with all shift operators, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_constant_field_reference() {
    // Reference to constant field with operators
    let source = r#"
        class Constants {
            static final int FLAG_A = 0x01;
            static final int FLAG_B = 0x02;
        }

        @interface Flags {
            int value();
        }

        class Test {
            @Flags(Constants.FLAG_A | Constants.FLAG_B)
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with constant reference, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_parenthesized_expression() {
    // Parentheses in annotation expression
    let source = r#"
        @interface Config {
            int value();
        }

        class Test {
            @Config((1 + 2) * (3 + 4))
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with parenthesized expression, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_unary_operators() {
    // Unary operators in annotations
    let source = r#"
        @interface Values {
            int neg();
            int not();
            boolean bool();
        }

        class Test {
            @Values(
                neg = -42,
                not = ~0xFF,
                bool = !true
            )
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with unary operators, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_array_expression() {
    // Array with expressions
    let source = r#"
        @interface Values {
            int[] value();
        }

        class Test {
            @Values({1 << 0, 1 << 1, 1 << 2})
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with array of expressions, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_operator_precedence() {
    // Test that operator precedence is maintained
    let source = r#"
        @interface Config {
            int value();
        }

        class Test {
            @Config(1 + 2 * 3)  // Should be 7, not 9
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation respecting precedence, got: {:?}",
        errors
    );
}

#[test]
fn test_annotation_with_mixed_operators() {
    // Complex expression with mixed operator types
    let source = r#"
        @interface Complex {
            int value();
        }

        class Test {
            @Complex((1 << 8) | (1 << 4) | (1 << 0))
            void method() {
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse annotation with mixed operators, got: {:?}",
        errors
    );
}
