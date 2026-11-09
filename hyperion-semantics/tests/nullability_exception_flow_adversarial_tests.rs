// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial regressions for expression evaluation and exceptional control flow.
//!
//! These fixtures distinguish the state at the exact point an exception can be
//! raised from the state before or after the containing expression. They also
//! exercise Java's innermost-handler rule and the ordering of cleanup code.

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
fn call_exception_observes_the_exact_post_argument_state() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                Object marker = null;
                try {
                    state = null;
                    risky(state = new Object());
                } catch (Throwable exception) {
                    state.toString();
                    marker.toString();
                }
            }

            static void risky(Object ignored) { unknownDependency(); }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["marker"],
        "the call can throw only after its argument has installed the non-null value"
    );
}

#[test]
fn call_exception_does_not_observe_a_later_repair() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    state = null;
                    risky();
                    state = new Object();
                } catch (Throwable exception) {
                    state.toString();
                }
            }

            static void risky() { unknownDependency(); }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the handler sees the state at the call, not the normally completed suffix"
    );
}

#[test]
fn nullable_receiver_is_checked_after_argument_effects() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object receiver) {
                Object state = null;
                try {
                    receiver.equals(state = new Object());
                } catch (NullPointerException exception) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["receiver"],
        "Java evaluates every argument before a null receiver raises NullPointerException"
    );
}

#[test]
fn exhaustive_inner_catch_consumes_an_implicit_call_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        state = null;
                        risky();
                    } catch (Throwable inner) {
                        state = new Object();
                    }
                } catch (RuntimeException outer) {
                    state.toString();
                }
            }

            static void risky() { unknownDependency(); }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an implicit call exception is consumed by the first exhaustive handler: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exhaustive_inner_catch_consumes_an_implicit_field_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Holder receiver = null;
                Object state = new Object();
                try {
                    try {
                        state = null;
                        Object ignored = receiver.value;
                    } catch (Throwable inner) {
                        state = new Object();
                    }
                } catch (NullPointerException outer) {
                    state.toString();
                }
            }
        }

        class Holder {
            Object value;
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["receiver"],
        "the field-access NPE reaches the inner Throwable handler, never the outer catch"
    );
}

#[test]
fn exhaustive_inner_catch_consumes_an_implicit_array_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object[] values = null;
                Object state = new Object();
                try {
                    try {
                        state = null;
                        Object ignored = values[0];
                    } catch (Throwable inner) {
                        state = new Object();
                    }
                } catch (RuntimeException outer) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["values"],
        "the array-access exception is consumed by the inner exhaustive handler"
    );
}

#[test]
fn implicit_exception_runs_one_finally_before_the_outer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    try {
                        risky();
                    } finally {
                        state = null;
                    }
                } catch (Throwable exception) {
                    state.toString();
                }
            }

            static void risky() { unknownDependency(); }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "an implicit exception crosses the intervening cleanup before entering the handler"
    );
}

#[test]
fn implicit_exception_runs_two_finally_blocks_in_lexical_order() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object innerState = new Object();
                Object outerState = null;
                try {
                    try {
                        try {
                            risky();
                        } finally {
                            innerState = null;
                        }
                    } finally {
                        outerState = new Object();
                    }
                } catch (Throwable exception) {
                    innerState.toString();
                    outerState.toString();
                }
            }

            static void risky() { unknownDependency(); }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["innerState"],
        "both cleanup mutations precede the outer handler, from inner to outer"
    );
}

#[test]
fn return_operand_exception_uses_its_own_type_and_crosses_cleanup() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Object test() {
                Holder receiver = null;
                Object state = new Object();
                try {
                    try {
                        return receiver.value;
                    } finally {
                        state = null;
                    }
                } catch (NullPointerException exception) {
                    state.toString();
                    return new Object();
                }
            }
        }

        class Holder {
            Object value;
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["receiver", "state"],
        "an exception raised while evaluating return replaces the return and still runs cleanup"
    );
}

#[test]
fn yield_operand_exception_uses_its_own_type_and_crosses_cleanup() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Holder receiver = null;
                Object state = new Object();
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                yield receiver.value;
                            } finally {
                                state = null;
                            }
                        } catch (NullPointerException exception) {
                            state.toString();
                            yield new Object();
                        }
                    }
                };
                result.toString();
            }
        }

        class Holder {
            Object value;
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["receiver", "state"],
        "an exception raised while evaluating yield replaces the yield and still runs cleanup"
    );
}

#[test]
fn throw_operand_exception_replaces_nominal_type_and_crosses_cleanup() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Holder receiver = null;
                Object npeState = new Object();
                Object boomState = new Object();
                try {
                    try {
                        throw new Boom(receiver.value);
                    } finally {
                        npeState = null;
                        boomState = null;
                    }
                } catch (NullPointerException exception) {
                    npeState.toString();
                } catch (Boom exception) {
                    boomState.toString();
                }
            }
        }

        class Holder {
            Object value;
        }

        class Boom extends RuntimeException {
            Boom(Object ignored) {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["receiver", "npeState"],
        "the operand NPE replaces the unconstructed Boom and follows the NPE route through cleanup"
    );
}

#[test]
fn null_switch_selector_exception_reaches_its_handler() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                String selector = null;
                Object state = null;
                try {
                    Object ignored = switch (selector) {
                        default -> new Object();
                    };
                } catch (NullPointerException exception) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["selector", "state"],
        "a null selector raises NPE at dispatch and enters the matching catch"
    );
}

#[test]
fn throw_inside_lambda_does_not_make_the_enclosing_expression_throw() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    Runnable callback = () -> {
                        throw new IllegalArgumentException();
                    };
                } catch (IllegalArgumentException exception) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "constructing a lambda does not execute its body: {:?}",
        analysis.diagnostics
    );
}
