// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for dereference-as-evidence in nullability analysis.
//!
//! When code dereferences a variable without a null check (e.g., `something.foo()`),
//! the analysis should:
//! 1. Warn on the first dereference if the variable may be null
//! 2. Treat the variable as NonNull for subsequent code (avoid re-warning)
//!
//! This eliminates false positives where every use of a field triggers a warning,
//! even after the programmer has already demonstrated intent by dereferencing it.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::DiagnosticKind;

fn deref_warnings_for(
    analysis: &hyperion_semantics::nullability::NullabilityAnalysis,
    var_name: &str,
) -> usize {
    analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(d.kind, DiagnosticKind::PotentialNullDereference)
                && d.variable_name == var_name
        })
        .count()
}

// =============================================================================
// B2: Dereference as non-null evidence in statements
// =============================================================================

#[test]
fn test_sequential_method_calls_warn_once() {
    // something.foo(); something.bar();
    // Should warn on first dereference, not on second.
    let code = r#"
        class Test {
            void test(@Nullable Service something) {
                something.foo();
                something.bar();
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    let warnings = deref_warnings_for(&analysis, "something");

    assert_eq!(
        warnings, 1,
        "Expected exactly 1 null dereference warning for 'something' (first use only), got {}",
        warnings
    );
}

#[test]
fn test_field_access_after_method_call_warn_once() {
    // something.foo(); int x = something.field;
    // Should warn on first dereference, not on field access.
    let code = r#"
        class Test {
            void test(@Nullable Config something) {
                something.init();
                String x = something.value;
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    let warnings = deref_warnings_for(&analysis, "something");

    assert_eq!(
        warnings, 1,
        "Expected exactly 1 null dereference warning for 'something' (method call only), got {}",
        warnings
    );
}

#[test]
fn test_nonnull_variable_no_warnings() {
    // A variable assigned via `new` is NonNull — no warnings at all.
    let code = r#"
        class Test {
            void test() {
                Service s = new Service();
                s.foo();
                s.bar();
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    let warnings = deref_warnings_for(&analysis, "s");

    assert_eq!(
        warnings, 0,
        "Expected no warnings for NonNull variable, got {}",
        warnings
    );
}

// =============================================================================
// B3: Dereference checks in terminator conditions
// =============================================================================

#[test]
fn test_if_condition_dereference_warns_once() {
    // if (something.foo()) { something.bar(); }
    // Should warn once (on something.foo() in condition), not on something.bar() in body.
    let code = r#"
        class Test {
            void test(@Nullable Service something) {
                if (something.isReady()) {
                    something.process();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    let warnings = deref_warnings_for(&analysis, "something");

    assert_eq!(
        warnings, 1,
        "Expected exactly 1 warning (condition dereference only), got {}",
        warnings
    );
}

#[test]
fn test_sequential_if_conditions_warn_once() {
    // if (something.foo()) { ... } if (something.bar()) { ... }
    // Should warn once on first condition, not on second.
    let code = r#"
        class Test {
            void test(@Nullable Service something) {
                if (something.isReady()) {
                    System.out.println("ready");
                }
                if (something.isActive()) {
                    System.out.println("active");
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    let warnings = deref_warnings_for(&analysis, "something");

    assert_eq!(
        warnings, 1,
        "Expected exactly 1 warning (first condition only), got {}",
        warnings
    );
}

#[test]
fn test_method_call_then_if_condition_warn_once() {
    // something.foo(); if (something.bar()) { ... }
    // Statement dereference followed by condition dereference — warn once.
    let code = r#"
        class Test {
            void test(@Nullable Service something) {
                something.init();
                if (something.isReady()) {
                    something.process();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    let warnings = deref_warnings_for(&analysis, "something");

    assert_eq!(
        warnings, 1,
        "Expected exactly 1 warning (first statement dereference only), got {}",
        warnings
    );
}
