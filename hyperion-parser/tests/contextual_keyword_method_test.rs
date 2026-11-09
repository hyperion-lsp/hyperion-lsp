// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test generic method calls with contextual keyword method names
//!
//! Explicit type arguments before contextual keyword method names.
//! Fixes: `Class.<Type>with(...)` where 'with' is a contextual keyword

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_generic_method_with_contextual_keyword_with() {
    let source = r#"
        class Test {
            void test() {
                CatalogIndex.<String, Integer, BookStore>with();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse generic method call with 'with' keyword, got: {errors:?}"
    );
}

#[test]
fn test_catalog_factory_with_nested_type_arguments() {
    // Nested type arguments followed by a fluent call.
    let source = r#"
        class CatalogBuilder {
            void table() {
                CatalogIndex.<K, V, BookIndex<CoverKey, byte[]>>with(
                    catalog.coverCodec(),
                    catalog.pageCodec()
                ).withLabelsEnabled();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse catalog factory pattern, got: {errors:?}"
    );
}

#[test]
fn test_catalog_factory_with_nested_calls() {
    // A contextual keyword method takes two constructor-like calls.
    let source = r#"
        class Test {
            void test() {
                ShelfPair.<String, String, Long>with(
                    Stores.inMemoryBookStore("reading", rows(4), columns(8), true),
                    Stores.inMemoryBookStore("archive", rows(4), columns(8), true)
                );
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse shelf factory pattern, got: {errors:?}"
    );
}

#[test]
fn test_generic_method_with_record_keyword() {
    let source = r#"
        class Test {
            void test() {
                Builder.<String, Integer>record();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse generic method call with 'record' contextual keyword, got: {errors:?}"
    );
}

#[test]
fn test_generic_method_with_var_keyword() {
    let source = r#"
        class Test {
            void test() {
                Factory.<String>var();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse generic method call with 'var' contextual keyword, got: {errors:?}"
    );
}

#[test]
fn test_generic_method_with_yield_keyword() {
    let source = r#"
        class Test {
            void test() {
                Producer.<Integer>yield();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse generic method call with 'yield' contextual keyword, got: {errors:?}"
    );
}

#[test]
fn test_nested_generic_with_contextual_keyword() {
    let source = r#"
        class Test {
            void test() {
                Builder.<Map<String, List<Integer>>>with();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse nested generics with contextual keyword method, got: {errors:?}"
    );
}

#[test]
fn test_multiple_type_args_with_contextual_keyword() {
    let source = r#"
        class Test {
            void test() {
                Tuple.<String, Integer, Boolean, Double>with();
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 4 type arguments with contextual keyword method, got: {errors:?}"
    );
}
