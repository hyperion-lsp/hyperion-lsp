// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test explicit type arguments before method call
//! Pattern: `Class.<TypeArg1, TypeArg2>methodName(...)`

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_explicit_type_arg_single() {
    let source = r"
        class Test {
            void test() {
                Collections.<String>emptyList();
            }
        }
    ";
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse Collections.<String>emptyList(), got: {errors:?}"
    );
}

#[test]
fn test_explicit_type_arg_multiple() {
    let source = r"
        class Test {
            void test() {
                Builder.<String, Integer>create();
            }
        }
    ";
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse explicit type arguments with multiple types, got: {errors:?}"
    );
}

#[test]
fn test_explicit_type_arg_with_generic_type() {
    let source = r"
        class Test {
            void test() {
                Builder.<List<String>>create();
            }
        }
    ";
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse explicit type with nested generic, got: {errors:?}"
    );
}

#[test]
fn test_explicit_type_arg_three_types() {
    let source = r"
        class Test {
            void test() {
                Config.<String, Integer, Store>as();
            }
        }
    ";
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse three explicit type arguments, got: {errors:?}"
    );
}
