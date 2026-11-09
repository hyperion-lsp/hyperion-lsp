// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression contracts for exact and conservative source overload summaries.

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
fn return_inference_distinguishes_overloads_by_arity() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service select() { return new Service(); }
            Service select(boolean absent) { return null; }

            void test() {
                Service present = select();
                Service absent = select(true);
                present.run();
                absent.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["absent"]);
}

#[test]
fn return_inference_distinguishes_same_arity_overloads_by_type() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service select(String value) { return new Service(); }
            Service select(Integer value) { return null; }

            void test(String text, Integer number) {
                Service present = select(text);
                Service absent = select(number);
                present.run();
                absent.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["absent"]);
}

#[test]
fn parameter_contracts_distinguish_same_arity_overloads_by_type() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void consume(@Nonnull String value) {}
            void consume(Integer value) {}

            void test(@Nullable String text, @Nullable Integer number) {
                consume(text);
                consume(number);
            }
        }
        "#,
    );

    let rejected: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect();
    assert_eq!(rejected, ["text"]);
}

#[test]
fn same_simple_parameter_names_keep_qualified_overload_identity() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service select(left.Value value) { return new Service(); }
            Service select(right.Value value) { return null; }

            void test(left.Value presentValue, right.Value absentValue) {
                Service present = select(presentValue);
                Service absent = select(absentValue);
                present.run();
                absent.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["absent"]);
}

#[test]
fn overload_fallback_respects_field_shadowing_and_block_scope() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Integer value;

            Service select(String value) { return new Service(); }
            Service select(Integer value) { return null; }

            void test() {
                Service fieldSelection = select(value);
                {
                    String value = "local";
                    Service localSelection = select(value);
                    localSelection.run();
                }
                fieldSelection.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["fieldSelection"]);
}

#[test]
fn unresolved_same_arity_calls_join_overload_summaries() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service select(String value) { return new Service(); }
            Service select(Integer value) { return null; }
            Object unknown() { return new Object(); }

            void test() {
                Service selected = select(unknown());
                selected.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["selected"]);
}

#[test]
fn exact_fixed_arity_overload_precedes_varargs_fallback() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service select(String left, String right) { return new Service(); }
            Service select(Object... values) { return null; }

            void test(String left, String right, Object object) {
                Service present = select(left, right);
                Service absent = select(object);
                present.run();
                absent.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["absent"]);
}

#[test]
fn field_initializers_use_exact_overload_return_summaries() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service present = select("value");
            Service absent = select(1);

            Service select(String value) { return new Service(); }
            Service select(int value) { return null; }

            void test() {
                present.run();
                absent.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["absent"]);
}

#[test]
fn array_dimensions_are_part_of_field_overload_selection() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            String[] values;

            Service select(String[] value) { return new Service(); }
            Service select(String value) { return null; }

            void test() {
                Service selected = select(values);
                selected.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the array overload returns a non-null service: {:?}",
        analysis.diagnostics
    );
}
