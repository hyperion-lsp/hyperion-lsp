// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Kotlin annotation use-site targets
//!
//! Parser should support @target:Annotation syntax
//! where target can be: file, property, field, get, set, receiver, param, setparam, delegate

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
fn test_annotation_setparam_target() {
    let source = r"
        annotation class Ann

        class X(@setparam:Ann var x: Int)
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_field_target() {
    let source = r"
        annotation class Ann

        class X(@field:Ann var x: Int)
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_param_target() {
    let source = r"
        annotation class Ann

        fun test(@param:Ann x: Int) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_get_target() {
    let source = r"
        annotation class Ann

        class X {
            @get:Ann
            val x: Int = 0
        }
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_set_target() {
    let source = r"
        annotation class Ann

        class X {
            @set:Ann
            var x: Int = 0
        }
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_property_target() {
    let source = r"
        annotation class Ann

        class X {
            @property:Ann
            var x: Int = 0
        }
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_file_target() {
    let source = r#"
        @file:JvmName("Utils")
        package com.example

        fun test() {}
    "#;
    assert_no_errors(source);
}

#[test]
fn test_annotation_receiver_target() {
    let source = r"
        annotation class Ann

        fun String.test(@receiver:Ann x: String) {}
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_multiple_use_sites() {
    let source = r"
        annotation class Ann
        annotation class Other

        class X(
            @param:Ann @field:Other var x: Int
        )
    ";
    assert_no_errors(source);
}

#[test]
fn test_annotation_use_site_with_arguments() {
    let source = r#"
        annotation class Ann(val value: String)

        class X(@param:Ann("test") var x: Int)
    "#;
    assert_no_errors(source);
}

#[test]
fn test_annotation_without_use_site_still_works() {
    let source = r"
        annotation class Ann

        class X(@Ann var x: Int)
    ";
    assert_no_errors(source);
}
