// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial contracts for expression evaluation order and exceptional flow.
//!
//! These fixtures distinguish values captured during evaluation from the
//! bindings left behind afterward. They also pin down which dereference checks
//! happen only after a subexpression completes normally.

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
fn simple_field_store_does_not_check_null_receiver_after_abrupt_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object unreachable) {
                Holder holder = null;
                holder.value = switch (selector) {
                    default -> throw new IllegalStateException();
                };
                unreachable.toString();
            }
        }
        class Holder { Object value; }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a simple field store checks its captured receiver only after its RHS completes: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn simple_array_store_does_not_check_null_receiver_after_abrupt_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object unreachable) {
                Object[] values = null;
                values[0] = switch (selector) {
                    default -> throw new IllegalStateException();
                };
                unreachable.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a simple array store checks its captured array only after its RHS completes: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn definite_null_field_access_has_no_normal_continuation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Holder holder = null;
                Object ignored = holder.value;
                unreachable.toString();
            }
        }
        class Holder { Object value; }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["holder"]);
}

#[test]
fn definite_null_array_access_has_no_normal_continuation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Object[] values = null;
                Object ignored = values[0];
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["values"]);
}

#[test]
fn definite_null_method_receiver_has_no_normal_continuation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Service service = null;
                service.run();
                unreachable.toString();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn definite_null_bound_method_reference_has_no_normal_continuation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Service service = null;
                Action action = service::run;
                unreachable.toString();
            }
        }
        interface Action { void invoke(); }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn definite_null_foreach_iterable_has_no_normal_continuation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Object[] values = null;
                for (Object value : values) {
                    value.toString();
                }
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["values"]);
}

#[test]
fn nullable_field_receiver_is_narrowed_after_normal_access() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Holder holder) {
                Object first = holder.value;
                Object second = holder.value;
            }
        }
        class Holder { Object value; }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["holder"]);
}

#[test]
fn nullable_array_receiver_is_narrowed_after_normal_access() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object[] values) {
                Object first = values[0];
                Object second = values[0];
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["values"]);
}

#[test]
fn nullable_method_receiver_is_narrowed_after_normal_invocation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service) {
                service.first();
                service.second();
            }
        }
        class Service {
            void first() {}
            void second() {}
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn nullable_bound_method_reference_is_narrowed_after_capture() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service) {
                Action first = service::run;
                Action second = service::run;
            }
        }
        interface Action { void invoke(); }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn null_qualified_creation_skips_constructor_arguments() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service argument) {
                Outer outer = null;
                Outer.Inner ignored = outer.new Inner(argument.value());
                argument.value();
            }
        }
        class Outer {
            class Inner { Inner(Object argument) {} }
        }
        class Service { Object value() { return new Object(); } }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["outer"],
        "the null qualifier fails before either the argument or following statement can run"
    );
}

#[test]
fn nested_nonnull_and_precondition_calls_apply_effects_in_evaluation_order() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void consume(Object value) {}

            void test(
                @Nullable Object primary,
                @Nullable Object secondary,
                @Nullable Object tertiary
            ) {
                consume(Objects.requireNonNull(primary));
                Preconditions.checkArgument(
                    secondary != null && Objects.requireNonNull(tertiary) != null
                );
                primary.toString();
                secondary.toString();
                tertiary.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "effects belong to the nested calls that actually complete: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn short_circuit_does_not_apply_effect_from_skipped_nested_call() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object value) {
                Preconditions.checkArgument(
                    true || Objects.requireNonNull(value) != null
                );
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "a descendant scan must not apply requireNonNull when || skipped the call"
    );
}

#[test]
fn later_argument_write_invalidates_require_nonnull_source_narrowing() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object value) {
                Objects.requireNonNull(value, value = null);
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "the call checks the first argument's captured value, not the binding replaced by the message argument"
    );
}

#[test]
fn optional_or_else_uses_captured_fallback_value_before_finally_effects() {
    let analysis = analyze_single_method(
        r#"
        import java.util.Optional;

        class Test {
            void test(int selector) {
                Optional<Object> optional = Optional.empty();
                Object fallback = null;
                Object result = optional.orElse(switch (selector) {
                    default -> {
                        try {
                            yield fallback;
                        } finally {
                            fallback = new Object();
                        }
                    }
                });
                result.toString();
                fallback.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "orElse receives the null value captured by yield even though finally repairs the source binding"
    );
}

#[test]
fn user_methods_with_contract_like_names_do_not_narrow_arguments() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Object requireNonNull(Object value) { return new Object(); }
            Object checkNotNull(Object value) { return new Object(); }
            void checkArgument(boolean condition) {}
            void checkState(boolean condition) {}

            void test(
                @Nullable Object required,
                @Nullable Object checked,
                @Nullable Object argument,
                @Nullable Object state
            ) {
                requireNonNull(required);
                checkNotNull(checked);
                checkArgument(argument != null);
                checkState(state != null);
                required.toString();
                checked.toString();
                argument.toString();
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["required", "checked", "argument", "state"],
        "a familiar method name is not a nullability contract without a recognized owner"
    );
}

#[test]
fn user_require_nonnull_accepting_null_does_not_make_following_code_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Object requireNonNull(Object value) { return new Object(); }

            void test(@Nullable Object reached) {
                requireNonNull(null);
                reached.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["reached"]);
}

#[test]
fn user_check_not_null_accepting_null_does_not_make_following_code_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Object checkNotNull(Object value) { return new Object(); }

            void test(@Nullable Object reached) {
                checkNotNull(null);
                reached.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["reached"]);
}

#[test]
fn user_check_argument_with_false_condition_does_not_make_code_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void checkArgument(boolean condition) {}

            void test(@Nullable Object reached) {
                Object value = null;
                checkArgument(value != null);
                reached.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["reached"]);
}

#[test]
fn user_check_state_with_false_condition_does_not_make_code_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void checkState(boolean condition) {}

            void test(@Nullable Object reached) {
                Object value = null;
                checkState(value != null);
                reached.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["reached"]);
}

#[test]
fn qualified_library_contracts_still_narrow_on_normal_completion() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(
                @Nullable Object required,
                @Nullable Object checked,
                @Nullable Object argument,
                @Nullable Object state
            ) {
                java.util.Objects.requireNonNull(required);
                com.google.common.base.Preconditions.checkNotNull(checked);
                com.google.common.base.Preconditions.checkArgument(argument != null);
                com.google.common.base.Preconditions.checkState(state != null);
                required.toString();
                checked.toString();
                argument.toString();
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "recognized qualified contracts retain their normal-path effects: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn qualified_library_contract_failures_make_their_arm_unreachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, @Nullable Object unreachable) {
                switch (selector) {
                    case 0 -> {
                        java.util.Objects.requireNonNull(null);
                        unreachable.toString();
                    }
                    case 1 -> {
                        com.google.common.base.Preconditions.checkNotNull(null);
                        unreachable.toString();
                    }
                    case 2 -> {
                        Object value = null;
                        com.google.common.base.Preconditions.checkArgument(value != null);
                        unreachable.toString();
                    }
                    default -> {
                        Object value = null;
                        com.google.common.base.Preconditions.checkState(value != null);
                        unreachable.toString();
                    }
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "each recognized fail-fast contract prevents its following arm statement: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn precondition_uses_boolean_captured_before_later_arguments() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object value = new Object();
                com.google.common.base.Preconditions.checkArgument(
                    value != null,
                    value = null
                );
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "a later argument may mutate the variable but cannot change the already captured condition"
    );
}

#[test]
fn later_argument_cannot_resurrect_failed_precondition() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Object value = null;
                com.google.common.base.Preconditions.checkArgument(
                    value != null,
                    value = new Object()
                );
                unreachable.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "later arguments execute, but cannot change a false condition into a normal completion: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn constant_false_check_argument_routes_illegal_argument_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                try {
                    com.google.common.base.Preconditions.checkArgument(false);
                } catch (IllegalArgumentException exception) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the always-failing check must reach its exact handler: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn constant_false_check_state_routes_illegal_state_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = null;
                try {
                    com.google.common.base.Preconditions.checkState(false);
                } catch (IllegalStateException exception) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "checkState must route its distinct exception type: {:?}",
        analysis.diagnostics
    );
}
