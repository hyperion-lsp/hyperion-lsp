// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Kotlin non-null assertion operator (!!) in type contexts
//!
//! Parser should handle !! tokens that appear after type names
//! Note: !! in type positions is rare/non-standard Kotlin, but parser should handle it gracefully
//! Most real occurrences are from IntelliJ diagnostic markers like <!ERROR!>, not actual !!

use hyperion_kotlin::lexer::Lexer;
use hyperion_kotlin::parser::Parser;

fn assert_no_errors(source: &str) {
    let tokens = Lexer::new(source).collect();
    let (_green, errors) = Parser::parse(tokens);
    assert!(
        errors.is_empty(),
        "Expected no parse errors, but got {} errors:\n{:#?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_non_null_assertion_in_parameter_type() {
    let source = r"
        fun test(x: String!!) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_in_generic_type_argument() {
    let source = r"
        fun test(x: List<String!!>) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_in_type_parameter_bound() {
    let source = r"
        fun <T: String!!> test() {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_in_return_type() {
    let source = r#"
        fun test(): String!! {
            return "test"
        }
    "#;
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_in_property_type() {
    let source = r#"
        class Test {
            val x: String!! = "test"
        }
    "#;
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_in_nested_generics() {
    let source = r"
        fun test(x: Map<String!!, List<Int!!>>) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_multiple_non_null_assertions() {
    let source = r"
        fun test(x: String!!!!) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_with_nullable() {
    let source = r"
        fun test(x: String!!?) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_on_function_type() {
    let source = r"
        fun test(x: (() -> Int)!!) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_non_null_assertion_complex_type() {
    let source = r"
        class Test<T: Comparable<T!!>>  {
            fun process(items: List<T!!>): Map<String!!, T!!> {
                return emptyMap()
            }
        }
    ";
    assert_no_errors(source);
}
