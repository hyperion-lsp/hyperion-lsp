// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for nullability analysis of assert statements
//!
//! These tests verify that assert statements properly narrow variable nullability,
//! matching the behavior of IntelliJ IDEA, Eclipse, and Checker Framework.
//!
//! Note: This is conditionally sound - assertions can be disabled at runtime with -da.
//! However, this matches industry-standard behavior and significantly reduces false positives
//! in development code.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::DiagnosticKind;

fn dereference_warnings(source: &str) -> Vec<String> {
    analyze_single_method(source)
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.clone())
        .collect()
}

#[test]
fn test_assert_narrows_to_nonnull() {
    // After assert name != null, name should be treated as NonNull
    let code = r#"
        class Test {
            void test(String name) {
                assert name != null;
                name.length(); // Should not warn - name is narrowed to NonNull
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    // Check that analysis completed successfully
    assert!(!analysis.entry_states.is_empty());
    assert!(!analysis.exit_states.is_empty());

    // There should be no diagnostics for the name.length() call
    // (If narrowing works, the variable is treated as NonNull after the assert)
    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no null dereference warnings after assert statement"
    );
}

#[test]
fn test_assert_compound_condition_and() {
    // assert first != null && last != null should narrow both variables
    let code = r#"
        class Test {
            void test(String first, String last) {
                assert first != null && last != null;
                String full = first + last; // Neither should warn
                full.length();
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    assert!(!analysis.entry_states.is_empty());

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings after compound assert with &&"
    );
}

#[test]
fn test_assert_with_message() {
    // assert user != null : "User must not be null" should still narrow
    let code = r#"
        class Test {
            void test(User user) {
                assert user != null : "User must not be null";
                user.getName(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    assert!(!analysis.entry_states.is_empty());

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings after assert with message"
    );
}

#[test]
fn test_assert_reversed_condition() {
    // assert null != name should also narrow (reversed operands)
    let code = r#"
        class Test {
            void test(String name) {
                assert null != name;
                name.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings with reversed null check"
    );
}

#[test]
fn test_assert_negation() {
    // assert !(name == null) should narrow to NonNull
    let code = r#"
        class Test {
            void test(String name) {
                assert !(name == null);
                name.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings after negated null check"
    );
}

#[test]
fn test_assert_or_condition() {
    // assert a != null || b != null - can't narrow either (at least one is non-null)
    let code = r#"
        class Test {
            void test(String a, String b) {
                assert a != null || b != null;
                // Neither variable is guaranteed non-null individually
                // This test just ensures the analysis doesn't crash
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    // Should complete without crashing
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_multiple_asserts() {
    // Multiple sequential assert statements should all be applied
    let code = r#"
        class Test {
            void test(String first, String last, String middle) {
                assert first != null;
                assert last != null;
                assert middle != null;
                String full = first + middle + last;
                full.length(); // Should not warn - all variables narrowed
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings after multiple assert statements"
    );
}

#[test]
fn test_assert_in_conditional_block() {
    // Assert inside an if block should narrow within that block
    let code = r#"
        class Test {
            void test(String name, boolean flag) {
                if (flag) {
                    assert name != null;
                    name.length(); // Should not warn within this block
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    assert!(!analysis.entry_states.is_empty());

    // The analysis should handle the scoped narrowing
    // We don't expect dereference warnings in the then-block
}

#[test]
fn test_assert_parenthesized_condition() {
    // assert (name != null) with extra parentheses should still work
    let code = r#"
        class Test {
            void test(String name) {
                assert (name != null);
                name.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings with parenthesized assert condition"
    );
}

#[test]
fn test_assert_complex_expression() {
    // Assert with a more complex expression (method call result)
    let code = r#"
        class Test {
            void test(Container container) {
                assert container != null;
                container.getValue(); // Should not warn after assert
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    let deref_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                hyperion_semantics::nullability::DiagnosticKind::PotentialNullDereference
            )
        })
        .count();

    assert_eq!(
        deref_warnings, 0,
        "Expected no warnings after asserting non-null"
    );
}

#[test]
fn successful_assert_does_not_evaluate_message_expression() {
    assert_eq!(
        dereference_warnings(
            r#"
            class Test {
                void test(@Nullable Object message) {
                    assert true : message.toString();
                }
            }
            "#,
        ),
        Vec::<String>::new()
    );
}

#[test]
fn assert_evaluates_condition_before_optional_message() {
    assert_eq!(
        dereference_warnings(
            r#"
            class Test {
                void test(@Nullable Object condition) {
                    assert condition.toString() != null : "failed";
                }
            }
            "#,
        ),
        ["condition"]
    );
}

#[test]
fn skipped_assert_message_cannot_mutate_normal_state() {
    assert_eq!(
        dereference_warnings(
            r#"
            class Test {
                void test() {
                    Object value = new Object();
                    assert true : (value = null);
                    value.toString();
                }
            }
            "#,
        ),
        Vec::<String>::new()
    );
}

#[test]
fn assertion_failure_routes_to_error_not_exception() {
    assert_eq!(
        dereference_warnings(
            r#"
            class Test {
                void test() {
                    Object state = new Object();
                    try {
                        assert false;
                    } catch (Exception wrong) {
                        state = null;
                    } catch (Error expected) {
                        state = new Object();
                    }
                    state.toString();
                }
            }
            "#,
        ),
        Vec::<String>::new()
    );
}

#[test]
fn failing_assert_message_exception_preempts_assertion_error() {
    assert_eq!(
        dereference_warnings(
            r#"
            class Test {
                void test() {
                    Object message = null;
                    Object state = new Object();
                    try {
                        assert false : message.toString();
                    } catch (NullPointerException expected) {
                        state = new Object();
                    } catch (Error wrong) {
                        state = null;
                    }
                    state.toString();
                }
            }
            "#,
        ),
        ["message"]
    );
}

#[test]
fn boxed_assert_condition_unboxes_before_message_or_assertion_failure() {
    assert_eq!(
        dereference_warnings(
            r#"
            class Test {
                void test() {
                    Boolean condition = null;
                    Object messageState = null;
                    Object result = null;
                    try {
                        assert condition : (messageState = new Object());
                    } catch (AssertionError wrong) {
                        result = null;
                    } catch (NullPointerException expected) {
                        result = new Object();
                    }
                    result.toString();
                    messageState.toString();
                }
            }
            "#,
        ),
        ["condition", "messageState"],
        "null Boolean unboxing throws before the assertion message is evaluated"
    );
}
