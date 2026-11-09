// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Pattern C: Qualified Inner Class Types with Generics
//!
//! JLS §6.5.5.2 - Qualified Type Names
//!
//! Qualified inner class type examples:
//! - Graph<?>.Edge
//! - Runtime<T, U>.Context
//! - Map.Entry<K, V>
//! - Outer<T>.Inner<U>

use hyperion_parser::{Lexer, Parser};

fn check_parse_success(source: &str) {
    // Tokenize the source
    let tokens: Vec<_> = Lexer::new(source).collect();

    // Parse the tokens
    let (_green_node, errors) = Parser::parse(tokens);

    // Print for debugging
    println!("=== Parsing: {} ===", source);
    for error in &errors {
        eprintln!("Parse error: {}", error.message);
    }

    // Check for no errors
    assert!(
        errors.is_empty(),
        "Expected no parse errors, but got: {:?}",
        errors
    );
}

#[test]
fn test_qualified_inner_type_simple() {
    // Simple case: Outer.Inner
    check_parse_success("class Test { Outer.Inner field; }");
}

#[test]
fn test_qualified_inner_type_with_outer_generic() {
    // Pattern C: Outer<?>.Inner
    check_parse_success("class Test { Graph<?>.Edge field; }");
}

#[test]
fn test_qualified_inner_type_with_outer_generic_cast() {
    // Cast to a qualified inner type with a wildcard outer generic
    check_parse_success(
        r#"
        class Test {
            void test() {
                Graph<?>.Edge otherEdge = (Graph<?>.Edge) other;
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_with_multiple_type_params() {
    // Qualified inner type with multiple outer type params
    check_parse_success(
        r#"
        class Test {
            void test() {
                Runtime<T, U>.Context ctx = runtime.contextOrThrow(tp);
            }
        }
        "#,
    );
}

#[test]
fn test_map_entry_pattern() {
    // Common pattern: Map.Entry<K, V>
    check_parse_success(
        r#"
        class Test {
            void test() {
                Map.Entry<String, Integer> entry = map.entrySet().iterator().next();
            }
        }
        "#,
    );
}

#[test]
fn test_nested_qualified_generics() {
    // Nested: A<T>.B<U>.C<V>
    check_parse_success(
        r#"
        class Test {
            void test() {
                Outer<T>.Middle<U>.Inner<V> obj = create();
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_in_for_loop() {
    // Pattern C in for-each loop
    check_parse_success(
        r#"
        class Test {
            void test(Map<String, Integer> map) {
                for (Map.Entry<String, Integer> entry : map.entrySet()) {
                    System.out.println(entry.getKey());
                }
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_array() {
    // Pattern C with arrays
    check_parse_success(
        r#"
        class Test {
            void test() {
                Map.Entry<String, Integer>[] entries = new Map.Entry[10];
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_wildcard_extends() {
    // Pattern C with bounded wildcards
    check_parse_success(
        r#"
        class Test {
            void test() {
                Graph<? extends Node>.Edge edge = graph.getEdge();
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_in_method_signature() {
    // Pattern C in method return type and parameters
    check_parse_success(
        r#"
        class Test {
            Map.Entry<String, Integer> getEntry(Map.Entry<String, Integer> input) {
                return input;
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_instanceof() {
    // Pattern C in instanceof
    check_parse_success(
        r#"
        class Test {
            void test(Object obj) {
                if (obj instanceof Graph<?>.Edge) {
                    Graph<?>.Edge edge = (Graph<?>.Edge) obj;
                }
            }
        }
        "#,
    );
}

#[test]
fn test_qualified_inner_type_new_expression() {
    // Pattern C in new expression
    check_parse_success(
        r#"
        class Test {
            void test() {
                Outer<String>.Inner obj = new Outer<String>().new Inner();
            }
        }
        "#,
    );
}
