// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for nullability analysis of Guava Preconditions
//!
//! These tests verify that Preconditions.checkArgument() and checkState()
//! properly narrow variable nullability by extracting and analyzing the
//! boolean condition argument.
//!
//! This matches the behavior of IntelliJ IDEA and other static analyzers
//! which recognize that if checkArgument(x != null) doesn't throw, then
//! x must be non-null after the call.

mod common_nullability;

use common_nullability::analyze_single_method;

#[test]
fn test_check_argument_simple() {
    // Preconditions.checkArgument(user != null, "User required");
    // user.getName() should not warn
    let code = r#"
        class Test {
            void test(User user) {
                Preconditions.checkArgument(user != null, "User required");
                user.getName(); // Should not warn - user is narrowed to NonNull
            }
        }
    "#;

    let analysis = analyze_single_method(code);

    assert!(!analysis.entry_states.is_empty());
    assert!(!analysis.exit_states.is_empty());

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
        "Expected no null dereference warnings after checkArgument"
    );
}

#[test]
fn test_check_argument_compound() {
    // Preconditions.checkArgument(first != null && last != null, "Names required");
    // first + last should not warn
    let code = r#"
        class Test {
            void test(String first, String last) {
                Preconditions.checkArgument(first != null && last != null, "Names required");
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
        "Expected no warnings after compound checkArgument with &&"
    );
}

#[test]
fn test_check_state_simple() {
    // Preconditions.checkState(initialized != null);
    // initialized.run() should not warn
    let code = r#"
        class Test {
            void test(Runnable initialized) {
                Preconditions.checkState(initialized != null);
                initialized.run(); // Should not warn
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

    assert_eq!(deref_warnings, 0, "Expected no warnings after checkState");
}

#[test]
fn test_check_argument_without_message() {
    // checkArgument(user != null) without message argument
    let code = r#"
        class Test {
            void test(User user) {
                Preconditions.checkArgument(user != null);
                user.getName(); // Should not warn
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
        "Expected no warnings after checkArgument without message"
    );
}

#[test]
fn test_check_argument_reversed_condition() {
    // checkArgument(null != user) with reversed operands
    let code = r#"
        class Test {
            void test(User user) {
                Preconditions.checkArgument(null != user, "User required");
                user.getName(); // Should not warn
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
fn test_check_argument_negation() {
    // checkArgument(!(user == null)) should narrow to NonNull
    let code = r#"
        class Test {
            void test(User user) {
                Preconditions.checkArgument(!(user == null), "User required");
                user.getName(); // Should not warn
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
fn test_check_state_compound_condition() {
    // checkState with compound condition
    let code = r#"
        class Test {
            void test(String name, String email) {
                Preconditions.checkState(name != null && email != null);
                String contact = name + " <" + email + ">"; // Neither should warn
                contact.length();
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
        "Expected no warnings after compound checkState"
    );
}

#[test]
fn test_multiple_check_arguments() {
    // Multiple sequential checkArgument calls should all be applied
    let code = r#"
        class Test {
            void test(String first, String last, String middle) {
                Preconditions.checkArgument(first != null, "First required");
                Preconditions.checkArgument(last != null, "Last required");
                Preconditions.checkArgument(middle != null, "Middle required");
                String full = first + " " + middle + " " + last;
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
        "Expected no warnings after multiple checkArgument calls"
    );
}

#[test]
fn test_check_argument_or_condition() {
    // checkArgument(a != null || b != null) - can't narrow either individually
    let code = r#"
        class Test {
            void test(String a, String b) {
                Preconditions.checkArgument(a != null || b != null, "At least one required");
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
fn test_check_argument_parenthesized_condition() {
    // checkArgument with extra parentheses should still work
    let code = r#"
        class Test {
            void test(User user) {
                Preconditions.checkArgument((user != null), "User required");
                user.getName(); // Should not warn
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
        "Expected no warnings with parenthesized condition"
    );
}

#[test]
fn test_check_argument_in_conditional() {
    // checkArgument inside an if block should narrow within that block
    let code = r#"
        class Test {
            void test(User user, boolean flag) {
                if (flag) {
                    Preconditions.checkArgument(user != null, "User required");
                    user.getName(); // Should not warn within this block
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
fn test_mixed_preconditions_and_asserts() {
    // Mix of assert and checkArgument should both work
    let code = r#"
        class Test {
            void test(String name, String email) {
                assert name != null;
                Preconditions.checkArgument(email != null, "Email required");
                String contact = name + " <" + email + ">";
                contact.length();
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
        "Expected no warnings with mixed assert and checkArgument"
    );
}

#[test]
fn test_check_state_with_message_format() {
    // checkState with formatted message (multiple arguments)
    let code = r#"
        class Test {
            void test(String value) {
                Preconditions.checkState(value != null, "Value %s is null", "test");
                value.length(); // Should not warn
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
        "Expected no warnings after checkState with formatted message"
    );
}
