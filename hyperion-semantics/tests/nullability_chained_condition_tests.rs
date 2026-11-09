// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: conditions with 3+ same-precedence
//! conjuncts parse as sibling continuation nodes
//! (`a && b && c` → `BINARY_EXPR [a, &&, b]`, `BINARY_EXPR [&&, c]`), and the
//! narrowing analysis previously saw only the first node — dropping the later
//! conjuncts' null checks and producing false potential-null-dereference
//! warnings.

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

#[test]
fn test_three_conjunct_condition_narrows_last_variable() {
    // `y != null` is the third conjunct — it lives in a sibling continuation
    // node. Its narrowing must reach the then-branch.
    let code = r#"
        class Test {
            void test(boolean flag, Service x, Service y) {
                if (flag && x != null && y != null) {
                    x.foo();
                    y.bar();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        deref_warnings_for(&analysis, "x"),
        0,
        "x is narrowed by the second conjunct"
    );
    assert_eq!(
        deref_warnings_for(&analysis, "y"),
        0,
        "y is narrowed by the third conjunct"
    );
}

#[test]
fn test_four_conjunct_condition_narrows_all_variables() {
    let code = r#"
        class Test {
            void test(boolean flag, Service x, Service y, Service z) {
                if (flag && x != null && y != null && z != null) {
                    x.foo();
                    y.bar();
                    z.baz();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    for var in ["x", "y", "z"] {
        assert_eq!(
            deref_warnings_for(&analysis, var),
            0,
            "{var} is narrowed by its conjunct in a 4-way chain"
        );
    }
}

#[test]
fn test_three_disjunct_null_checks_narrow_else_branch() {
    // `x == null || y == null || flag` — on the false branch (else / fallthrough)
    // both x and y are non-null.
    let code = r#"
        class Test {
            void test(boolean flag, Service x, Service y) {
                if (x == null || y == null || flag) {
                    return;
                }
                x.foo();
                y.bar();
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(deref_warnings_for(&analysis, "x"), 0);
    assert_eq!(
        deref_warnings_for(&analysis, "y"),
        0,
        "y == null sits in a continuation node; the negated Or must still narrow y"
    );
}

#[test]
fn test_parenthesized_chain_condition_narrows() {
    // Same chain shape, but nested inside parentheses.
    let code = r#"
        class Test {
            void test(boolean flag, Service x, Service y) {
                if ((flag && x != null && y != null)) {
                    y.bar();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        deref_warnings_for(&analysis, "y"),
        0,
        "chain head inside PAREN_EXPR is the last expression child"
    );
}

#[test]
fn test_two_conjunct_condition_still_narrows() {
    // The pre-existing 2-conjunct shape must keep working after the
    // last-node change.
    let code = r#"
        class Test {
            void test(@Nullable Service x, Service y) {
                if (x != null && y != null) {
                    x.foo();
                    y.bar();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(deref_warnings_for(&analysis, "x"), 0);
    assert_eq!(deref_warnings_for(&analysis, "y"), 0);
}

#[test]
fn test_deref_in_first_conjunct_still_checked() {
    // The dereference `x.isReady()` sits in the FIRST chain node; the
    // condition head is now the last node, so the deref check must walk the
    // chain predecessors too.
    let code = r#"
        class Test {
            void test(@Nullable Service x, Service y) {
                if (x.isReady() && y != null && y.isReady()) {
                    y.bar();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        deref_warnings_for(&analysis, "x"),
        1,
        "unguarded dereference of x in the first conjunct must still be reported"
    );
    assert_eq!(deref_warnings_for(&analysis, "y"), 0);
}

#[test]
fn test_while_loop_chain_condition_narrows() {
    let code = r#"
        class Test {
            void test(boolean flag, Service x, Service y) {
                while (flag && x != null && y != null) {
                    x.foo();
                    y.bar();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(deref_warnings_for(&analysis, "x"), 0);
    assert_eq!(
        deref_warnings_for(&analysis, "y"),
        0,
        "while-loop conditions share condition_expr_for_stmt"
    );
}
