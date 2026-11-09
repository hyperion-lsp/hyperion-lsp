// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial flow tests for switch expressions.
//!
//! These fixtures deliberately put mutations and abrupt completion below the
//! direct case-arm block.  Switch-expression result inference must follow the
//! reachable path to each owned `yield`, while ordinary flow state must remain
//! local to the arm that produced it.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use hyperion_semantics::nullability::{ControlFlowGraph, DiagnosticKind, NullabilityAnalysis};

fn dereference_warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect()
}

#[test]
fn assignment_in_nested_block_reaches_the_owned_yield() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object candidate = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        {
                            candidate = null;
                        }
                        yield candidate;
                    }
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn conditional_assignment_before_yield_is_joined_into_the_result() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, boolean clear) {
                Object candidate = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        if (clear) {
                            candidate = null;
                        }
                        yield candidate;
                    }
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn conditional_assignment_reaches_a_later_arm_dereference() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, boolean clear) {
                Object candidate = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        if (clear) {
                            candidate = null;
                        }
                        candidate.toString();
                        yield candidate;
                    }
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["candidate"],
        "the dereference is unsafe, but a normally completed dereference is evidence that the yielded value is non-null"
    );
}

#[test]
fn statements_after_yield_do_not_mutate_the_result_or_enclosing_flow() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object candidate = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        yield candidate;
                        candidate = null;
                    }
                    default -> new Object();
                };
                result.toString();
                candidate.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an abrupt yield makes every following arm statement unreachable: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn yields_owned_by_a_nested_switch_do_not_join_into_the_outer_result() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outerSelector, int innerSelector) {
                Object result = switch (outerSelector) {
                    case 0 -> {
                        Object ignored = switch (innerSelector) {
                            case 0 -> null;
                            default -> {
                                yield null;
                            }
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
        "only yields whose nearest switch-expression ancestor is the outer switch contribute to its result: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn throw_only_arms_do_not_contribute_unknown_to_the_result() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    case 0 -> new Object();
                    default -> throw new IllegalStateException();
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an arm that cannot complete normally has no value to join: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn dereference_evidence_from_one_arm_does_not_escape_the_switch_expression() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate, int selector) {
                Object result = switch (selector) {
                    case 0 -> {
                        candidate.toString();
                        yield new Object();
                    }
                    default -> new Object();
                };
                result.toString();
                candidate.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["candidate", "candidate"],
        "the selected arm is not known after the expression, so its successful-dereference evidence is branch-local"
    );
}

#[test]
fn successful_nonnull_guard_narrows_a_nullable_record_component() {
    let analysis = analyze_single_method(
        r#"
        record Box(String value) {}

        class Test {
            void test(Object candidate) {
                Object result = switch (candidate) {
                    case Box(String value) when value != null -> {
                        value.toString();
                        yield value;
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
        "an unconditional record-component type pattern may bind null, but its successful `!= null` guard narrows the arm: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn null_guard_does_not_make_a_record_component_nonnull() {
    let analysis = analyze_single_method(
        r#"
        record Box(String value) {}

        class Test {
            void test(Object candidate) {
                Object result = switch (candidate) {
                    case Box(String value) when value == null -> value;
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn assignments_from_normal_arms_are_joined_after_the_switch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        state = null;
                        yield new Object();
                    }
                    default -> {
                        state = new Object();
                        yield new Object();
                    }
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "post-switch flow joins state from every normally completing arm"
    );
}

#[test]
fn all_throw_switch_makes_following_statements_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object candidate) {
                Object result = switch (selector) {
                    case 0 -> throw new IllegalArgumentException();
                    default -> throw new IllegalStateException();
                };
                result.toString();
                candidate.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a switch expression with no normal arm has no normal exit: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_switch_normal_exit_state_reaches_the_outer_yield() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outerSelector, int innerSelector) {
                Object state = new Object();
                Object result = switch (outerSelector) {
                    case 0 -> {
                        Object ignored = switch (innerSelector) {
                            case 0 -> {
                                state = null;
                                yield new Object();
                            }
                            default -> {
                                state = new Object();
                                yield new Object();
                            }
                        };
                        yield state;
                    }
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn nested_switch_throwing_arm_side_effects_do_not_reach_outer_normal_flow() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outerSelector, int innerSelector) {
                Object state = new Object();
                Object result = switch (outerSelector) {
                    case 0 -> {
                        Object ignored = switch (innerSelector) {
                            case 0 -> {
                                state = null;
                                throw new IllegalStateException();
                            }
                            default -> new Object();
                        };
                        yield state;
                    }
                    default -> new Object();
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "state from an inner arm that throws cannot contaminate either outer value or post-switch state: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn selector_dereference_is_evaluated_once_and_narrows_normal_exit() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Selector selector) {
                Object result = switch (selector.key()) {
                    default -> new Object();
                };
                result.toString();
                selector.toString();
            }
        }

        class Selector {
            int key() { return 0; }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["selector"],
        "the selector dereference warns at evaluation, then its successful completion is evidence on every normal exit"
    );
}

#[test]
fn guard_side_effect_is_joined_with_the_unmatched_path() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    case String text when (state = null) == null -> new Object();
                    default -> new Object();
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the guard mutation happens only after its label matches, so post-switch state joins it with the unmatched/default path"
    );
}

#[test]
fn guard_dereference_evidence_does_not_escape_the_unmatched_path() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object selector, @Nullable Probe probe) {
                Object result = switch (selector) {
                    case String text when probe.ready() -> new Object();
                    default -> new Object();
                };
                result.toString();
                probe.toString();
            }
        }

        class Probe {
            boolean ready() { return true; }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["probe", "probe"],
        "the guard warns when evaluated, but it is not evaluated when the label fails"
    );
}

#[test]
fn throwing_arm_assignment_does_not_reach_switch_normal_exit() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        state = null;
                        throw new IllegalStateException();
                    }
                    default -> state;
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "only a normally completing arm may contribute state or a value: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn assignment_in_selector_reaches_arms_and_normal_exit() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                Object result = switch ((state = new Object()).hashCode()) {
                    default -> state;
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the selector is evaluated before dispatch, including its assignment side effect: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn assignment_in_yield_value_reaches_switch_normal_exit() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = null;
                Object result = switch (selector) {
                    case 0 -> (state = new Object());
                    default -> {
                        yield (state = new Object());
                    }
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "every produced value is evaluated before its arm reaches the normal merge: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_all_throw_switch_makes_the_following_outer_yield_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outerSelector, int innerSelector) {
                Object result = switch (outerSelector) {
                    case 0 -> {
                        Object ignored = switch (innerSelector) {
                            case 0 -> throw new IllegalArgumentException();
                            default -> throw new IllegalStateException();
                        };
                        yield null;
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
        "an all-throw nested switch cuts off the enclosing arm before its later null yield: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn dereference_evidence_does_not_move_past_a_later_arm_assignment() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object state, int selector) {
                Object ignored = state.toString() + switch (selector) {
                    default -> {
                        state = null;
                        yield "";
                    }
                };
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state", "state"]);
}

#[test]
fn direct_nested_all_throw_value_does_not_poison_outer_result() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outerSelector, int innerSelector) {
                Object result = switch (outerSelector) {
                    case 0 -> switch (innerSelector) {
                        default -> throw new IllegalStateException();
                    };
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "{:?}",
        analysis.diagnostics
    );
}

#[test]
fn deferred_lambda_switch_does_not_mutate_enclosing_flow() {
    let analysis = analyze_single_method(
        r#"
        import java.util.function.Supplier;

        class Test {
            void test(Object state, int selector) {
                Supplier<Object> delayed = () -> switch (selector) {
                    default -> {
                        state = null;
                        yield state;
                    }
                };
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "{:?}",
        analysis.diagnostics
    );
}

#[test]
fn switch_arm_local_does_not_shadow_nullable_field_after_expression() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Object value;

            void test(int selector) {
                Object ignored = switch (selector) {
                    default -> {
                        Object value = new Object();
                        yield value;
                    }
                };
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["value"]);
}

#[test]
fn yielded_dereference_evidence_reaches_normal_exit() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object state, int selector) {
                Object result = switch (selector) {
                    default -> state.toString();
                };
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn yield_value_is_captured_before_finally_and_cleanup_state_escapes() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    default -> {
                        try {
                            yield state;
                        } finally {
                            state = null;
                        }
                    }
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the produced value is evaluated before finally, while cleanup effects precede normal resumption"
    );
}

#[test]
fn throwing_finally_prevents_pending_yield_from_producing_a_value() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            yield null;
                        } finally {
                            throw new IllegalStateException();
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
        "a pending yield discarded by an abrupt finally has no normal result: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn yield_in_finally_overrides_the_pending_yield() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
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
        "{:?}",
        analysis.diagnostics
    );
}

#[test]
fn catch_produced_yield_participates_in_value_flow() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            throw new IllegalStateException();
                        } catch (IllegalStateException exception) {
                            yield null;
                        }
                    }
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn all_throw_switch_still_evaluates_and_diagnoses_its_selector() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Selector selector, @Nullable Object unreachable) {
                Object result = switch (selector.key()) {
                    default -> throw new IllegalStateException();
                };
                result.toString();
                unreachable.toString();
            }
        }

        class Selector {
            int key() { return 0; }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["selector"],
        "selector evaluation precedes dispatch, while statements after the all-throw switch remain unreachable"
    );
}

#[test]
fn constant_ternary_skips_the_unselected_assignment_branch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                Object result = true ? state : (state = null);
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "only the selected ternary operand is evaluated: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn throwing_ternary_branch_does_not_contribute_state_to_normal_exit() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(boolean chooseThrow, int selector) {
                Object state = new Object();
                Object result = chooseThrow
                    ? switch (selector) {
                        default -> {
                            state = null;
                            throw new IllegalStateException();
                        }
                    }
                    : state;
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an abruptly completing ternary operand has neither a value nor normal-exit state: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn constant_false_and_skips_its_assignment_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                boolean ignored = false && (state = null) == null;
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a false left operand makes the `&&` right operand unreachable: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn constant_true_or_skips_its_assignment_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                boolean ignored = true || (state = null) == null;
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a true left operand makes the `||` right operand unreachable: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn method_receiver_then_arguments_are_evaluated_left_to_right() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Service service = null;
                Object state = null;
                (service = new Service()).accept(
                    state = new Object(),
                    state.toString()
                );
                service.toString();
                state.toString();
            }
        }

        class Service {
            void accept(Object first, Object second) {}
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "receiver evaluation precedes argument evaluation, and arguments are evaluated left-to-right: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn switch_side_effect_in_earlier_argument_reaches_later_argument() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                accept(
                    switch (selector) {
                        default -> {
                            state = null;
                            yield new Object();
                        }
                    },
                    state.toString()
                );
                state.toString();
            }

            void accept(Object first, Object second) {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the later argument warns, then its normally completed dereference supplies post-call evidence"
    );
}

#[test]
fn all_throw_switch_argument_prevents_later_arguments_and_call_from_evaluating() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object unreachable) {
                accept(
                    switch (selector) {
                        default -> throw new IllegalStateException();
                    },
                    unreachable.toString()
                );
                unreachable.toString();
            }

            void accept(Object first, Object second) {}
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an abrupt earlier argument prevents every later argument and the invocation itself: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn constructor_arguments_are_evaluated_left_to_right() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                Holder holder = new Holder(
                    state = new Object(),
                    state.toString()
                );
                holder.toString();
                state.toString();
            }
        }

        class Holder {
            Holder(Object first, Object second) {}
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "constructor arguments share Java's left-to-right evaluation order: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_finally_yield_override_runs_inner_cleanup_first() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                yield null;
                            } finally {
                                state = null;
                            }
                        } finally {
                            yield new Object();
                        }
                    }
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the outer yield overrides the pending value only after the inner cleanup mutates state"
    );
}

#[test]
fn outer_throwing_finally_overrides_inner_finally_yield() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object unreachable) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                yield null;
                            } finally {
                                yield new Object();
                            }
                        } finally {
                            throw new IllegalStateException();
                        }
                    }
                };
                result.toString();
                unreachable.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the outermost abrupt cleanup wins and leaves no normal switch value: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn yield_captures_null_before_finally_repairs_the_variable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = null;
                Object result = switch (selector) {
                    default -> {
                        try {
                            yield state;
                        } finally {
                            state = new Object();
                        }
                    }
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "yield evaluates and captures its value before cleanup, but cleanup state reaches normal continuation"
    );
}

#[test]
fn known_throw_reaches_only_its_matching_catch_value() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            throw new IllegalArgumentException();
                        } catch (NullPointerException exception) {
                            yield null;
                        } catch (IllegalArgumentException exception) {
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
        "a statically known thrown type must not flow through unrelated catch clauses: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn empty_recovery_arm_degrades_the_switch_value_conservatively() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    case 0 -> {}
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "an invalid recovery arm with no produced value leaves the recovered switch value unknown"
    );
}

#[test]
fn nested_arm_local_does_not_shadow_the_outer_field() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Object value;

            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        {
                            Object value = new Object();
                            value.toString();
                        }
                        yield this.value;
                    }
                };
                result.toString();
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result", "value"],
        "the nested local's state must not replace the shadowed field"
    );
}

#[test]
fn same_named_locals_in_sibling_arms_have_distinct_value_state() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    case 0 -> {
                        Object item = null;
                        yield item;
                    }
                    default -> {
                        Object item = new Object();
                        yield item;
                    }
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn definitely_null_guard_skips_and_rhs_assignment() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object guard = null;
                Object state = new Object();
                boolean ignored = guard != null && (state = null) == null;
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "known-null flow proves the `&&` right operand unreachable: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn definitely_nonnull_guard_skips_or_rhs_assignment() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object guard = new Object();
                Object state = new Object();
                boolean ignored = guard != null || (state = null) == null;
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "known-non-null flow proves the `||` right operand unreachable: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn known_null_ternary_condition_skips_the_else_assignment() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object guard = null;
                Object state = new Object();
                Object result = guard == null ? state : (state = null);
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "known-null flow selects only the ternary's true operand: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn outer_catch_handles_an_exception_that_overrides_inner_yield_in_finally() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                yield null;
                            } finally {
                                throw new IllegalArgumentException();
                            }
                        } catch (IllegalArgumentException exception) {
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
        "an exception raised by an inner cleanup is handled by its enclosing try's catch: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn catch_belonging_to_try_does_not_catch_its_own_finally_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object unreachable) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            yield null;
                        } catch (IllegalArgumentException exception) {
                            yield new Object();
                        } finally {
                            throw new IllegalArgumentException();
                        }
                    }
                };
                result.toString();
                unreachable.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a try's catch clauses are outside the protected region of its own finally: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nullable_selector_without_null_label_is_an_implicit_dereference() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable String selector) {
                Object result = switch (selector) {
                    default -> new Object();
                };
                result.toString();
                selector.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["selector"],
        "without `case null`, dispatch itself warns once; every normal arm proves the selector non-null afterward"
    );
}

#[test]
fn explicit_null_label_makes_nullable_selector_dispatch_safe() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable String selector) {
                Object result = switch (selector) {
                    case null -> new Object();
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an explicit null label makes null a handled selector value: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn resource_initializer_runs_before_try_body_yield() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    case 0 -> {
                        try (Resource resource = new Resource(state = null)) {
                            yield state;
                        }
                    }
                    default -> new Object();
                };
                result.toString();
                state.toString();
            }
        }

        class Resource implements AutoCloseable {
            Resource(Object value) {}
            public void close() {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result", "state"],
        "resource specifications are evaluated left-to-right before entering the protected body"
    );
}

#[test]
fn catch_parameter_is_nonnull_and_does_not_hide_shadowed_field_after_catch() {
    let source = r#"
        class Test {
            @Nullable Object exception;

            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            throw new IllegalArgumentException();
                        } catch (IllegalArgumentException exception) {
                            exception.toString();
                            yield new Object();
                        }
                    }
                };
                result.toString();
                this.exception.toString();
            }
        }
        "#;
    let analysis = analyze_single_method(source);

    let warnings: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .collect();
    assert_eq!(warnings.len(), 1, "{:?}", analysis.diagnostics);
    let warning = warnings[0];
    assert_eq!(warning.variable_name, "this.exception");
    assert_eq!(
        &source[warning.location.start as usize..warning.location.end as usize],
        "this.exception.toString()",
        "the warning belongs to the field access after the catch, not its non-null parameter"
    );
}

#[test]
fn array_dimension_expression_side_effect_reaches_following_flow() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                int[] values = new int[(state = null) == null ? 1 : 0];
                values.clone();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "array dimensions are evaluated as part of the new expression"
    );
}

#[test]
fn return_operand_is_captured_before_finally_mutates_its_source() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                Object state = null;
                try {
                    return state;
                } finally {
                    state = new Object();
                }
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::NullReturnFromNonNull
                && diagnostic.variable_name == "<return>"
        }),
        "return evaluates and captures its operand before running finally: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn throw_operand_side_effect_happens_before_finally() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Boom(state = null);
                } finally {
                    state.toString();
                }
            }
        }

        class Boom extends RuntimeException {
            Boom(Object value) {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "the thrown expression is fully evaluated before cleanup begins"
    );
}

#[test]
fn break_target_inside_the_same_try_does_not_run_finally_early() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    state = null;
                    inner: {
                        break inner;
                    }
                    state.toString();
                } finally {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "breaking a construct contained by the try remains inside that try; finally runs only when the try itself exits"
    );
}

#[test]
fn continue_target_inside_the_same_try_does_not_run_finally_early() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    do {
                        state = null;
                        continue;
                    } while (false);
                    state.toString();
                } finally {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "continuing a loop contained by the try remains inside that try; finally runs only after the loop and try complete"
    );
}

#[test]
fn rethrow_from_inner_catch_runs_inner_finally_before_outer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw new IllegalArgumentException();
                            } catch (IllegalArgumentException exception) {
                                throw exception;
                            } finally {
                                state = null;
                            }
                        } catch (IllegalArgumentException exception) {
                            yield new Object();
                        }
                    }
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "a rethrow leaves the inner try through its finally, then enters the outer handler"
    );
}

#[test]
fn yield_from_switch_nested_in_try_does_not_run_surrounding_finally_early() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = null;
                try {
                    Object result = switch (selector) {
                        default -> {
                            yield new Object();
                        }
                    };
                    result.toString();
                    state.toString();
                } finally {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "yield exits only its switch; a finally surrounding the whole switch runs later when its try exits"
    );
}

#[test]
fn invocation_receiver_value_is_captured_before_argument_mutates_receiver_variable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Service service = new Service();
                service.accept(service = null);
                service.toString();
            }
        }

        class Service {
            void accept(Service value) {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["service"],
        "the invocation uses its already captured receiver, while the argument mutation controls later flow"
    );
}

#[test]
fn assignment_lhs_receiver_is_dereferenced_before_rhs_mutates_it() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Holder holder) {
                holder.value = (holder = new Holder());
                holder.toString();
            }
        }

        class Holder {
            Object value;
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["holder"],
        "a field-assignment target captures the old receiver before the right-hand side; assigning the variable later does not repair that captured null target"
    );
}

#[test]
fn cloned_finally_paths_emit_one_diagnostic_for_one_source_dereference() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(boolean stop, @Nullable Object value) {
                try {
                    if (stop) {
                        return;
                    }
                } finally {
                    value.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "cloned CFG cleanup paths still represent one source dereference and must not duplicate its diagnostic"
    );
}

#[test]
fn catch_after_nonthrowing_try_body_does_not_contribute_a_value() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            yield "safe";
                        } catch (NullPointerException exception) {
                            yield null;
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
        "a literal-producing try body cannot enter an unrelated catch: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exhaustive_inner_catch_prevents_original_exception_from_also_entering_outer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw new IllegalArgumentException();
                            } catch (RuntimeException exception) {
                                yield new Object();
                            }
                        } catch (IllegalArgumentException exception) {
                            yield null;
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
        "one thrown exception follows the first matching handler chain, not inner and outer catches simultaneously: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn throwing_return_operand_enters_its_own_catch_before_finally() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Object test(int selector) {
                Object state = null;
                try {
                    return switch (selector) {
                        default -> throw new IllegalArgumentException();
                    };
                } catch (IllegalArgumentException exception) {
                    state = new Object();
                } finally {
                    state.toString();
                }
                return state;
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an exception from return-operand evaluation is handled by the return statement's own try before finally: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unknown_rethrow_can_escape_inner_catch_chain_to_outer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, RuntimeException failure) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw failure;
                            } catch (IllegalArgumentException exception) {
                                yield new Object();
                            }
                        } catch (RuntimeException exception) {
                            yield null;
                        }
                    }
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "a runtime value not known to match the inner catch may reach the outer catch"
    );
}

#[test]
fn abruptly_throwing_argument_prevents_invocation_receiver_null_check() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service, int selector) {
                service.accept(switch (selector) {
                    default -> throw new IllegalStateException();
                });
            }
        }

        class Service {
            void accept(Object value) {}
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the receiver value is captured first, but invocation's null check occurs only after every argument completes: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn successful_zero_argument_invocation_supplies_dereference_evidence() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service) {
                service.ping();
                service.ping();
            }
        }

        class Service {
            void ping() {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["service"],
        "after the first zero-argument invocation completes, its receiver is known non-null"
    );
}

#[test]
fn argument_list_keeps_assignment_and_binary_continuation_as_single_arguments() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Boolean flag) {
                Object value = new Object();
                acceptAssignment(value = null);
                acceptBinary(false || flag);
            }

            void acceptAssignment(@NonNull Object value) {}
            void acceptBinary(boolean value) {}
            void acceptBinary(boolean left, @NonNull Boolean right) {}
        }
        "#,
    );

    let contract_violations = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull)
        .count();
    assert_eq!(
        contract_violations, 1,
        "the assignment is one null argument, while the binary continuation is one boolean argument"
    );
}

#[test]
fn second_multi_catch_alternative_handles_before_outer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw new IllegalStateException();
                            } catch (NullPointerException | IllegalStateException exception) {
                                yield new Object();
                            }
                        } catch (IllegalStateException exception) {
                            yield null;
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
        "the second alternative of a multi-catch is an exhaustive inner match: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nullable_selector_emits_one_dispatch_warning_across_many_arms() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable String selector) {
                Object result = switch (selector) {
                    case "first" -> new Object();
                    case "second" -> new Object();
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["selector"],
        "dispatch evaluates its selector once regardless of arm count"
    );
}

#[test]
fn malformed_throw_recovery_does_not_panic_cfg_construction() {
    let tokens: Vec<_> = Lexer::new("class Test { void test() { throw; } }").collect();
    let (green, errors) = Parser::parse(tokens);
    assert!(!errors.is_empty(), "fixture must exercise parser recovery");
    let root = SyntaxNode::new_root(green);
    let body = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
        .expect("recovered method body");

    let _cfg = ControlFlowGraph::build(&body);
}

#[test]
fn array_access_keeps_captured_receiver_separate_from_index_mutation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object[] values = new Object[] { new Object() };
                Object ignored = values[(values = null) == null ? 0 : 0];
                values.clone();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["values"],
        "array access uses the receiver captured before its index runs, but the index's later assignment controls following flow"
    );
}

#[test]
fn finally_return_overrides_pending_null_return_without_contract_violation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                try {
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
        "the finally return replaces the pending null return: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn throwing_finally_overrides_pending_null_return_without_contract_violation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object test() {
                try {
                    return null;
                } finally {
                    throw new IllegalStateException();
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
        "an abrupt finally discards the pending null return entirely: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn continue_in_cloned_finally_targets_lexical_outer_loop_not_inner_loop() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                outer: do {
                    try {
                        break outer;
                    } finally {
                        do {
                            state = new Object();
                            continue outer;
                        } while (false);
                        state = null;
                    }
                } while (false);
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the labeled continue overrides the pending break and makes the later cleanup assignment unreachable: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exceptional_state_from_nested_switch_reaches_parent_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int outerSelector, int innerSelector) {
                Object state = new Object();
                Object result = switch (outerSelector) {
                    case 0 -> {
                        try {
                            Object ignored = switch (innerSelector) {
                                default -> {
                                    state = null;
                                    throw new IllegalArgumentException();
                                }
                            };
                            yield new Object();
                        } catch (IllegalArgumentException exception) {
                            yield state;
                        }
                    }
                    default -> new Object();
                };
                result.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result", "state"],
        "the parent handler observes side effects completed before the nested switch throws"
    );
}

#[test]
fn thrown_call_type_is_not_inferred_from_descendant_argument_constructor() {
    let analysis = analyze_single_method(
        r#"
        import java.io.IOException;

        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw choose(new IOException());
                            } catch (IOException exception) {
                                yield new Object();
                            }
                        } catch (Exception exception) {
                            yield null;
                        }
                    }
                };
                result.toString();
            }

            Exception choose(Exception ignored) {
                return new IllegalStateException();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "the `new IOException()` argument does not make the enclosing call expression an IOException"
    );
}

#[test]
fn assignment_updates_captured_lhs_path_not_replacement_receiver_path() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Holder holder = new Holder();
                holder.value = (holder = new Holder());
                holder.value.toString();
            }
        }

        class Holder {
            @Nullable Object value;
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis).len(),
        1,
        "the store updates the old captured object's field, not the replacement holder's field: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn multiple_local_declarators_bind_and_evaluate_left_to_right() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object first = null,
                    second = (first = new Object()),
                    third = first;
                first.toString();
                second.toString();
                third.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "each declarator becomes visible before the initializer to its right: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn failed_guard_resumes_matching_and_evaluates_the_next_guard_before_its_body() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object selector) {
                Object state = null;
                Object result = switch (selector) {
                    case String text when false -> null;
                    case Integer number when state != null -> null;
                    default -> new Object();
                };
                result.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "guard failure resumes matching; it cannot jump directly into the next arm's body: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn three_operand_short_circuit_preserves_known_false_prefix() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object first = null;
                Object second = new Object();
                Object state = new Object();
                boolean ignored = first != null
                    && second != null
                    && (state = null) == null;
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a known-false first conjunct skips every later operand in the chain: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn custom_subtype_throw_is_assignable_to_source_supertype_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            throw new Boom();
                        } catch (RuntimeException exception) {
                            yield null;
                        }
                    }
                };
                result.toString();
            }
        }

        class Boom extends RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "a source subtype is caught by its declared source/JDK supertype"
    );
}

#[test]
fn resource_local_shadow_ends_at_try_with_resources_boundary() {
    let source = r#"
        class Test {
            @Nullable Resource resource;

            void test(boolean flag) {
                try (Resource resource = new Resource()) {
                    resource.toString();
                    throw new RuntimeException();
                } catch (RuntimeException exception) {
                    resource = flag ? null : new Resource();
                    resource.hashCode();
                } finally {
                    resource = flag ? null : new Resource();
                    resource.toString();
                }
                resource = flag ? null : new Resource();
                resource.getClass();
            }
        }

        class Resource implements AutoCloseable {
            public void close() {}
        }
        "#;
    let analysis = analyze_single_method(source);

    let mut warning_sources: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| {
            (
                diagnostic.location.start,
                &source[diagnostic.location.start as usize..diagnostic.location.end as usize],
            )
        })
        .collect();
    warning_sources.sort_by_key(|(start, _)| *start);
    let warning_sources: Vec<_> = warning_sources
        .into_iter()
        .map(|(_, source)| source)
        .collect();
    assert_eq!(
        warning_sources,
        [
            "resource.hashCode()",
            "resource.toString()",
            "resource.getClass()"
        ],
        "the resource local is scoped to the specification and try block; catch, finally, and following code resolve the nullable field"
    );
}

#[test]
fn combined_null_default_label_handles_null_but_keeps_selector_nullable_in_arm() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable String selector) {
                Object result = switch (selector) {
                    case null, default -> {
                        selector.toString();
                        yield new Object();
                    }
                };
                result.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["selector"],
        "the combined label prevents an implicit dispatch warning, while its body still joins null and default selector states"
    );
}

#[test]
fn qualified_inner_creation_evaluates_and_dereferences_qualifier_before_constructor_args() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Outer outer) {
                Object state = null;
                Outer.Inner value = outer.new Inner(state = new Object());
                value.toString();
                state.toString();
            }
        }

        class Outer {
            class Inner {
                Inner(Object argument) {}
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["outer"],
        "qualified class creation dereferences its enclosing-instance qualifier and still applies argument effects"
    );
}

#[test]
fn unknown_inner_catch_match_does_not_drop_known_outer_supertype_handler() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw new Boom();
                            } catch (IllegalArgumentException exception) {
                                yield new Object();
                            }
                        } catch (RuntimeException exception) {
                            yield null;
                        }
                    }
                };
                result.toString();
            }
        }

        class Boom extends RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "an uncertain inner match may handle one path, but cannot discard the known outer RuntimeException handler"
    );
}

#[test]
fn explicit_null_and_default_arms_narrow_selector_then_join_after_switch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable String selector) {
                Object captured = new Object();
                Object result = switch (selector) {
                    case null -> {
                        captured = selector;
                        yield new Object();
                    }
                    default -> {
                        selector.toString();
                        yield new Object();
                    }
                };
                result.toString();
                captured.toString();
                selector.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["captured", "selector"],
        "the null arm captures null, the default arm sees non-null, and selector state rejoins to nullable after both normal arms"
    );
}

#[test]
fn qualified_inner_creation_applies_qualifier_effect_before_argument_effect() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Outer outer = null;
                Object state = null;
                Outer.Inner value = (outer = new Outer()).new Inner(state = outer);
                value.toString();
                outer.toString();
                state.toString();
            }
        }

        class Outer {
            class Inner {
                Inner(Object argument) {}
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the qualifier assignment is visible to the later constructor argument and normal continuation: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn array_initializer_elements_apply_side_effects_left_to_right() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                Object[] values = new Object[] {
                    state = null,
                    state = new Object(),
                    state = null
                };
                values.clone();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "array initializer elements are evaluated in source order and the final assignment reaches following flow"
    );
}

#[test]
fn successful_invocation_with_nonmutating_argument_supplies_receiver_evidence() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service) {
                Object argument = new Object();
                service.accept(argument);
                service.ping();
            }
        }

        class Service {
            void accept(Object value) {}
            void ping() {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["service"],
        "a normally completed invocation proves its captured receiver non-null when argument evaluation did not replace the receiver variable"
    );
}

#[test]
fn captured_field_assignment_survives_receiver_replacement_inside_switch_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Holder holder = new Holder();
                holder.value = switch (selector) {
                    default -> (holder = new Holder());
                };
                holder.value.toString();
            }
        }

        class Holder {
            @Nullable Object value;
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis).len(),
        1,
        "the assignment stores into the pre-switch captured holder; the replacement holder's nullable field stays unchanged: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn indexed_assignment_updates_captured_element_not_replacement_array_element() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object[] values = new Object[1];
                values[0] = (values = null);
                values.clone();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["values"],
        "the old array receives the indexed store; assigning the array variable on the RHS controls following flow"
    );
}

#[test]
fn traditional_switch_fallthrough_local_shadows_field_only_inside_switch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Object value;

            void test(int selector) {
                switch (selector) {
                    case 0:
                        Object value = null;
                    case 1:
                        value = new Object();
                        value.toString();
                        break;
                    default:
                        break;
                }
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "the colon-case local is assigned and used in the next clause, while the post-switch name resolves the nullable field"
    );
}

#[test]
fn array_access_uses_captured_nullable_receiver_when_index_replaces_variable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object[] values) {
                Object ignored = values[(values = new Object[1]) != null ? 0 : 0];
                values.clone();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["values"],
        "the array access checks its old captured receiver, while following flow sees the replacement array"
    );
}

#[test]
fn abruptly_throwing_index_prevents_array_receiver_null_check() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object[] values, int selector, @Nullable Object unreachable) {
                Object ignored = values[switch (selector) {
                    default -> throw new IllegalStateException();
                }];
                unreachable.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an abruptly completing index prevents the later array null check and all following statements: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn outer_assignment_applies_final_rhs_value_even_when_rhs_writes_same_variable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object value = new Object();
                value = ((value = null) == null ? new Object() : null);
                value.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "RHS self-writes affect its evaluation, but the enclosing assignment still performs the final non-null store: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn abruptly_throwing_assignment_index_skips_array_check_and_rhs_before_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object[] values, int selector) {
                Object state = null;
                try {
                    values[switch (selector) {
                        default -> throw new IllegalArgumentException();
                    }] = (state = new Object());
                } catch (IllegalArgumentException exception) {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "an abrupt index enters the handler before the array null check or assignment RHS can execute"
    );
}
