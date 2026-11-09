// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for synchronized monitor evaluation and body ordering.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::{DiagnosticKind, NullabilityAnalysis};

fn deref_warnings_for(analysis: &NullabilityAnalysis, name: &str) -> usize {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == name
        })
        .count()
}

#[test]
fn synchronized_implicitly_dereferences_its_monitor() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object monitor) {
                synchronized (monitor) {}
            }
        }
        "#,
    );

    assert_eq!(deref_warnings_for(&analysis, "monitor"), 1);
}

#[test]
fn synchronized_body_processes_local_initializers_in_order() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object monitor) {
                synchronized (monitor) {
                    Service present = new Service();
                    present.run();
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(deref_warnings_for(&analysis, "present"), 0);
}

#[test]
fn synchronized_cast_monitor_preserves_null_identity() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object monitor) {
                synchronized ((CharSequence) monitor) {}
            }
        }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "monitor"),
        1,
        "a reference cast does not make its operand non-null: {:?}",
        analysis.diagnostics
    );
}
