// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial tests for cleanup execution and pending abrupt completions.
//!
//! A shared `finally` must transform each incoming completion together with
//! that completion's own flow state.  Abrupt completion from the cleanup then
//! replaces the pending return, yield, jump, or exception.

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
fn distinct_returns_keep_their_state_through_one_conditional_finally() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test(boolean firstLane) {
                Object lane = null;
                try {
                    if (firstLane) {
                        return null;
                    }
                    lane = new Object();
                    return lane;
                } finally {
                    if (lane == null) {
                        return new Object();
                    }
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != DiagnosticKind::NullReturnFromNonNull),
        "the null return is overridden only on its own lane, while the other lane retains its captured non-null return: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn outer_cleanup_validates_only_surviving_lane_of_shared_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test(boolean first) {
                Object state = null;
                try {
                    try {
                        if (first) {
                            return null;
                        }
                        state = new Object();
                        return state;
                    } finally {
                        return state;
                    }
                } finally {
                    if (state == null) {
                        return new Object();
                    }
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != DiagnosticKind::NullReturnFromNonNull),
        "the nullable invocation of the shared inner return is overridden by the outer cleanup: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn inferred_return_joins_only_surviving_lanes_of_shared_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Object source(boolean first) {
                Object state = null;
                try {
                    try {
                        if (first) {
                            return null;
                        }
                        state = new Object();
                        return state;
                    } finally {
                        return state;
                    }
                } finally {
                    if (state == null) {
                        return new Object();
                    }
                }
            }

            void test(boolean first) {
                source(first).toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the source summary must exclude the inner return lane overridden by the outer cleanup: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn distinct_yields_keep_their_state_through_one_conditional_finally() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, boolean firstLane) {
                Object result = switch (selector) {
                    default -> {
                        Object lane = null;
                        try {
                            if (firstLane) {
                                yield null;
                            }
                            lane = new Object();
                            yield lane;
                        } finally {
                            if (lane == null) {
                                yield new Object();
                            }
                        }
                    }
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the cleanup override and the pending non-null yield must not be cross-paired: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn distinct_break_and_continue_lanes_share_a_conditional_finally() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(boolean firstLane) {
                Object state = null;
                outer: do {
                    try {
                        if (firstLane) {
                            break outer;
                        }
                        state = new Object();
                        continue outer;
                    } finally {
                        if (state == null) {
                            state = new Object();
                            break outer;
                        }
                    }
                } while (false);
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the break lane is repaired and overridden, while the continue lane resumes with its own non-null state: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn return_in_finally_overrides_pending_return_or_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test(boolean fail) {
                try {
                    if (fail) {
                        throw new IllegalStateException();
                    }
                    return null;
                } finally {
                    return new Object();
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != DiagnosticKind::NullReturnFromNonNull),
        "the cleanup return replaces both pending completions: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn yield_in_finally_overrides_pending_yield_or_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, boolean fail) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            if (fail) {
                                throw new IllegalStateException();
                            }
                            yield null;
                        } finally {
                            yield new Object();
                        }
                    }
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the cleanup yield replaces both pending completions: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn break_in_finally_overrides_a_pending_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                exit: {
                    try {
                        throw new IllegalStateException();
                    } finally {
                        state = new Object();
                        break exit;
                    }
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the cleanup break discards the exception and carries cleanup state to its target: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn continue_in_finally_overrides_a_pending_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                outer: do {
                    try {
                        return;
                    } finally {
                        state = new Object();
                        continue outer;
                    }
                } while (false);
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the cleanup continue replaces the return and reaches the loop successor with cleanup state: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_normal_cleanups_run_from_inner_to_outer() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        state = new Object();
                    } finally {
                        state = null;
                    }
                } finally {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the outer cleanup must observe and run after the inner cleanup: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_exceptional_cleanups_run_from_inner_to_outer_before_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        try {
                            throw new IllegalStateException();
                        } finally {
                            state = null;
                        }
                    } finally {
                        state = new Object();
                    }
                } catch (IllegalStateException exception) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "exception propagation must execute inner then outer cleanup before entering the handler: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exception_from_finally_skips_the_same_trys_catches() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        state = new Object();
                    } catch (IllegalStateException inner) {
                        state = new Object();
                    } finally {
                        state = null;
                        throw new IllegalStateException();
                    }
                } catch (IllegalStateException outer) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "a cleanup is outside its own try's protected region, so only the enclosing handler receives its exception"
    );
}

#[test]
fn cleanup_exception_overrides_pending_return_and_reaches_outer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        return;
                    } finally {
                        state = null;
                        throw new IllegalStateException();
                    }
                } catch (IllegalStateException outer) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the cleanup exception replaces the return and enters only the enclosing handler"
    );
}

#[test]
fn nested_cleanup_does_not_resume_pending_return_from_enclosing_cleanup() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object value) {
                try {
                    return;
                } finally {
                    try {
                    } finally {
                    }
                    value.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "only the enclosing cleanup may resume its pending return"
    );
}

#[test]
fn nested_cleanup_does_not_dispatch_pending_exception_from_enclosing_cleanup() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        throw new IllegalStateException();
                    } finally {
                        try {
                        } finally {
                        }
                        state = null;
                    }
                } catch (IllegalStateException exception) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "only the enclosing cleanup may dispatch its pending exception"
    );
}

#[test]
fn local_break_inside_cleanup_preserves_pending_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                try {
                    return null;
                } finally {
                    while (true) {
                        break;
                    }
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::NullReturnFromNonNull),
        "a jump contained within a cleanup must not discard its pending return: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn local_switch_yield_inside_cleanup_preserves_pending_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                try {
                    return null;
                } finally {
                    Object ignored = switch (0) {
                        default -> new Object();
                    };
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::NullReturnFromNonNull),
        "a switch-local yield must not discard the completion that entered the cleanup: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn local_completion_crossing_nested_cleanup_restores_older_pending_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                try {
                    return null;
                } finally {
                    local: try {
                        break local;
                    } finally {
                    }
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::NullReturnFromNonNull),
        "a temporary completion through a nested cleanup must restore the older pending lane: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn locally_caught_exception_inside_cleanup_restores_pending_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                try {
                    return null;
                } finally {
                    try {
                        throw new IllegalArgumentException();
                    } catch (IllegalArgumentException exception) {
                    }
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::NullReturnFromNonNull),
        "a handled nested exception must restore the completion which entered the cleanup: {:?}",
        analysis.diagnostics
    );
}
