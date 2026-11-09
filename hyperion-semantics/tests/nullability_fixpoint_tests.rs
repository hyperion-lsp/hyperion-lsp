// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: diagnostics were emitted inside the
//! fixpoint worklist, so anti-monotone diagnostics (RedundantNullCheck)
//! computed from optimistic pre-fixpoint states were never retracted when
//! later iterations widened the state, and re-processed blocks could emit
//! duplicates. Diagnostics now come from a single post-fixpoint pass over
//! the converged states.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::DiagnosticKind;

fn count_of(
    analysis: &hyperion_semantics::nullability::NullabilityAnalysis,
    kind_filter: fn(&DiagnosticKind) -> bool,
) -> usize {
    analysis
        .diagnostics
        .iter()
        .filter(|d| kind_filter(&d.kind))
        .count()
}

#[test]
fn test_no_false_redundant_check_when_loop_widens_state() {
    // First worklist visit of the loop body sees x = NonNull (from the
    // initializer; the back-edge hasn't propagated yet), so a pre-fixpoint
    // emission claims `x != null` is redundant. After convergence x may be
    // null at the check (the loop's else-branch assigns null), so the check
    // is NOT redundant.
    let code = r#"
        class Test {
            void test(boolean flag, int n) {
                Service x = new Service();
                for (int i = 0; i < n; i = i + 1) {
                    if (x != null) {
                        x.foo();
                    }
                    if (flag) {
                        x = null;
                    }
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        count_of(&analysis, |k| matches!(k, DiagnosticKind::RedundantNullCheck)),
        0,
        "null check inside a loop whose later iterations may assign null is not redundant, got: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn test_straight_line_redundant_check_still_reported() {
    // Control: with no widening, a genuinely redundant check must still be
    // reported exactly once.
    let code = r#"
        class Test {
            void test() {
                Service x = new Service();
                if (x != null) {
                    x.foo();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        count_of(&analysis, |k| matches!(
            k,
            DiagnosticKind::RedundantNullCheck
        )),
        1,
        "straight-line redundant check must be reported exactly once, got: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn test_loop_body_deref_warning_not_duplicated() {
    // A dereference diagnostic in a block the worklist visits more than once
    // (loop body) must be reported exactly once.
    let code = r#"
        class Test {
            void test(Service x, boolean flag, int n) {
                for (int i = 0; i < n; i = i + 1) {
                    x.foo();
                    if (flag) {
                        x = null;
                    }
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        count_of(&analysis, |k| matches!(
            k,
            DiagnosticKind::PotentialNullDereference
        )),
        1,
        "loop-body dereference must warn exactly once, got: {:?}",
        analysis.diagnostics
    );
}
