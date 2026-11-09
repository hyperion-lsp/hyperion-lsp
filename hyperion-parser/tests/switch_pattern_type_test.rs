// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{Lexer, Parser};

fn assert_parses(source: &str) {
    let tokens = Lexer::new(source).collect();
    let (_tree, errors) = Parser::parse(tokens);
    assert!(errors.is_empty(), "fixture must parse cleanly: {errors:?}");
}

#[test]
fn switch_record_component_accepts_array_type_patterns() {
    assert_parses(
        r#"
        record Arrays(int[] values) {}
        class Test {
            void test(Object value) {
                switch (value) {
                    case Arrays(int[] items) -> use(items);
                    default -> {}
                }
            }
        }
        "#,
    );
}

#[test]
fn switch_and_instanceof_share_nested_generic_pattern_lookahead() {
    assert_parses(
        r#"
        class Test {
            void test(Object value) {
                if (value instanceof Map<String, List<Integer>> map) {
                    use(map);
                }
                switch (value) {
                    case Map<String, List<Integer>> map -> use(map);
                    default -> {}
                }
            }
        }
        "#,
    );
}

#[test]
fn type_pattern_lookahead_accepts_type_use_annotations() {
    assert_parses(
        r#"
        @interface A { String value() default ""; }
        class Outer { class Inner {} }
        class Test {
            void test(Object value) {
                switch (value) {
                    case @module @A("leading") String text -> use(text);
                    case Outer.@A("inner") Inner inner -> use(inner);
                    case Map<@A("argument") String, ?> values -> use(values);
                    default -> {}
                }
                if (value instanceof Map<@A("argument") String, ?> values) {
                    use(values);
                }
            }
        }
        "#,
    );
}
