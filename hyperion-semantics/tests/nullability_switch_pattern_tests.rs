// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::{DiagnosticKind, NullabilityAnalysis};

fn dereference_warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect()
}

#[test]
fn matched_type_pattern_is_nonnull_in_its_guard_and_body() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object value) {
                switch (value) {
                    case String text when text.isEmpty() -> text.length();
                    default -> {}
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a successful type pattern produces a non-null binding: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn pattern_binding_does_not_hide_a_nullable_field_after_the_arm() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable String text;

            void test(Object value) {
                switch (value) {
                    case String text -> text.length();
                    default -> {}
                }
                text.length();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["text"]);
}

#[test]
fn repeated_pattern_names_have_independent_lexical_identity() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object value) {
                switch (value) {
                    case String item -> item.length();
                    case StringBuilder item -> item.length();
                    default -> {}
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "sibling pattern bindings must not share flow state: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn pattern_bindings_follow_nested_statement_control_flow() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object value, boolean clear) {
                switch (value) {
                    case String text -> {
                        if (clear) {
                            text = null;
                        }
                        text.length();
                    }
                    default -> {}
                }
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["text"]);
}

#[test]
fn unconditional_record_component_patterns_may_bind_null() {
    let analysis = analyze_single_method(
        r#"
        record Pair(String left, String right) {}

        class Test {
            void test(Object value) {
                switch (value) {
                    case Pair(String left, String right) -> {
                        left.length();
                        right.length();
                    }
                    default -> {}
                }
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["left", "right"]);
}

#[test]
fn inferred_and_array_record_components_may_bind_null() {
    let analysis = analyze_single_method(
        r#"
        record Box(String value) {}
        record Arrays(int[] values) {}

        class Test {
            void test(Object value) {
                switch (value) {
                    case Box(var item) -> item.length();
                    case Arrays(int[] items) -> items.clone();
                    default -> {}
                }
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["item", "items"]);
}

#[test]
fn guard_local_pattern_is_not_bound_before_its_match() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Foo candidate;

            void test(Object value, Object other) {
                switch (value) {
                    case String text
                        when candidate.ready() && other instanceof Foo candidate -> use(text);
                    default -> {}
                }
            }

            void use(String value) {}
        }
        class Foo { boolean ready() { return true; } }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["candidate"]);
}

#[test]
fn nested_switch_yields_do_not_contaminate_the_outer_result() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outer, int inner) {
                Object result = switch (outer) {
                    case 0 -> {
                        Object ignored = switch (inner) {
                            default -> { yield null; }
                        };
                        yield new Object();
                    }
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "only yields owned by the outer switch determine its result: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_arm_assignment_is_not_the_enclosing_local_initializer() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int value) {
                Object scratch = new Object();
                Object result = switch (value) {
                    case 0 -> {
                        scratch = null;
                        yield new Object();
                    }
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an assignment inside an arm is not the outer local initializer: {:?}",
        analysis.diagnostics
    );
}
