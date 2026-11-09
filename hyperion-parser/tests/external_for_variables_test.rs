// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Pattern H: External For Loop Variables
//!
//! JLS §14.14.1 - Basic For Statement
//!
//! ForInit can be:
//! - LocalVariableDeclaration: for (int i = 0; ...)
//! - StatementExpressionList: for (i = 0; ...) or for (i = 0, j = 0; ...)
//!
//! Example:
//! ```java
//! int i;
//! for (i = 0; i < 10; i++) {
//!     // ...
//! }
//! ```

use hyperion_parser::{Lexer, Parser};

fn check_parse_success(source: &str) {
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (_green_node, errors) = Parser::parse(tokens);

    for error in &errors {
        eprintln!("Parse error: {}", error.message);
    }

    assert!(
        errors.is_empty(),
        "Expected no parse errors, but got: {:?}",
        errors
    );
}

#[test]
fn test_traditional_for_with_decl() {
    // Traditional for loop with variable declaration (should already work)
    check_parse_success(
        r#"
        class Test {
            void test() {
                for (int i = 0; i < 10; i++) {
                    System.out.println(i);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_with_external_variable_single() {
    // Pattern H: Variable declared outside, used in for loop
    check_parse_success(
        r#"
        class Test {
            void test() {
                int i;
                for (i = 0; i < 10; i++) {
                    System.out.println(i);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_with_external_variable_multiple() {
    // Pattern H: Multiple external variables
    check_parse_success(
        r#"
        class Test {
            void test() {
                int i, j;
                for (i = 0, j = 0; i < 10; i++, j++) {
                    System.out.println(i + j);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_with_field_variable() {
    // Pattern H: Using class field in for loop
    check_parse_success(
        r#"
        class Test {
            int index;

            void test() {
                for (index = 0; index < 10; index++) {
                    System.out.println(index);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_with_array_element() {
    // Pattern H: Using array element in for loop
    check_parse_success(
        r#"
        class Test {
            void test() {
                int[] indices = new int[3];
                for (indices[0] = 0; indices[0] < 10; indices[0]++) {
                    System.out.println(indices[0]);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_with_qualified_field() {
    // Pattern H: Using qualified field in for loop
    check_parse_success(
        r#"
        class Test {
            static class Counter {
                int value;
            }

            void test() {
                Counter c = new Counter();
                for (c.value = 0; c.value < 10; c.value++) {
                    System.out.println(c.value);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_empty_init() {
    // Edge case: Empty init clause
    check_parse_success(
        r#"
        class Test {
            void test() {
                int i = 0;
                for (; i < 10; i++) {
                    System.out.println(i);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_external_with_prefix_increment() {
    // Pattern H: External variable with ++i in update
    check_parse_success(
        r#"
        class Test {
            void test() {
                int i;
                for (i = 0; i < 10; ++i) {
                    System.out.println(i);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_for_external_with_compound_assignment() {
    // Pattern H: External variable with compound assignment
    check_parse_success(
        r#"
        class Test {
            void test() {
                int i;
                for (i = 0; i < 100; i += 2) {
                    System.out.println(i);
                }
            }
        }
        "#,
    );
}

#[test]
fn test_catalog_loop_with_external_index() {
    // The loop initializer assigns an index declared outside the loop.
    check_parse_success(
        r#"
        class ShelfPrinter {
            void process() {
                int i;
                for (i = 0; i < books.length; i++) {
                    printBook(books[i]);
                }
            }

            void printBook(Object r) {}
            Object[] books = new Object[10];
        }
        "#,
    );
}
