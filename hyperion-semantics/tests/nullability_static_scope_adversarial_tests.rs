// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial contracts for static member qualification and lexical identity.
//!
//! Java evaluates an expression used to qualify a static member, but does not
//! dereference the resulting value. Pattern and block-local identities are
//! likewise determined by lexical scope, rather than by their spelling alone.

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

fn nonnull_argument_warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect()
}

#[test]
fn static_method_access_through_null_expression_does_not_dereference_it() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object reachable) {
                Service service = null;
                service.ping();
                reachable.toString();
            }
        }
        class Service { static void ping() {} }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["reachable"],
        "the qualifier expression is evaluated, but its null value is not dereferenced: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn static_field_access_through_null_expression_does_not_dereference_it() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object reachable) {
                Service service = null;
                Object ignored = service.VALUE;
                reachable.toString();
            }
        }
        class Service { static Object VALUE = new Object(); }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["reachable"],
        "reading a static field through an expression has no receiver null check: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn expression_qualifying_static_method_is_still_evaluated() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Service service = new Service();
                (service = null).ping();
                consumeNonnull(service);
            }

            void consumeNonnull(@NonNull Service service) {}
        }
        class Service { static void ping() {} }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        Vec::<&str>::new(),
        "the null result of the qualifier assignment is not dereferenced"
    );
    assert_eq!(
        nonnull_argument_warnings(&analysis),
        ["service"],
        "the expression which qualifies a static method must still run"
    );
}

#[test]
fn expression_qualifying_static_field_is_still_evaluated() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Service service = new Service();
                Object ignored = (service = null).VALUE;
                consumeNonnull(service);
            }

            void consumeNonnull(@NonNull Service service) {}
        }
        class Service { static Object VALUE = new Object(); }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        Vec::<&str>::new(),
        "the null result of the qualifier assignment is not dereferenced"
    );
    assert_eq!(
        nonnull_argument_warnings(&analysis),
        ["service"],
        "the expression which qualifies a static field must still run"
    );
}

#[test]
fn type_qualified_static_members_do_not_create_value_dereferences() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Service.ping();
                Object ignored = Service.VALUE;
            }
        }
        class Service {
            static Object VALUE = new Object();
            static void ping() {}
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a type name is not a nullable runtime receiver: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_instance_selection_inside_static_qualifier_still_dereferences_its_receiver() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Holder holder) {
                holder.service.ping();
            }
        }
        class Holder { Service service; }
        class Service { static void ping() {} }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["holder"],
        "static selection suppresses only its final receiver check, not nested instance selections"
    );
}

#[test]
fn interface_field_is_implicitly_static() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object reachable) {
                Constants constants = null;
                Object ignored = constants.VALUE;
                reachable.toString();
            }
        }
        interface Constants { Object VALUE = new Object(); }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["reachable"],
        "every interface field is static even without an explicit modifier"
    );
}

#[test]
fn expression_qualified_static_field_write_evaluates_qualifier_before_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Service service = new Service();
                (service = null).VALUE = new Object();
                consumeNonnull(service);
            }

            void consumeNonnull(@NonNull Service service) {}
        }
        class Service { static Object VALUE; }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        Vec::<&str>::new(),
        "a static field store performs no receiver null check"
    );
    assert_eq!(
        nonnull_argument_warnings(&analysis),
        ["service"],
        "the value qualifier still runs before the stored value is evaluated"
    );
}

#[test]
fn later_local_declaration_does_not_shadow_an_earlier_field_access() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Object value;

            void test() {
                value.toString();
                Object value = new Object();
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "a local variable's scope begins at its declarator, not at method entry: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn block_local_shadows_field_only_until_the_block_ends() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Object value;

            void test() {
                {
                    Object value = new Object();
                    value.toString();
                }
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "the field must become visible again after the block-local binding expires: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn sibling_block_locals_with_the_same_name_have_distinct_flow_state() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                {
                    Object value = candidate;
                    value.toString();
                }
                {
                    Object value = new Object();
                    value.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "same-named locals in disjoint scopes must not share a variable ID: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn branch_local_state_does_not_leak_into_same_named_sibling_binding() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(boolean choose, @Nullable Object candidate) {
                if (choose) {
                    Object value = candidate;
                    value.toString();
                } else {
                    Object value = new Object();
                    value.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "branch-local identities must remain distinct at a control-flow join: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn instanceof_pattern_binding_is_nonnull_in_true_branch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                if (candidate instanceof String text) {
                    text.length();
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a successfully matched type pattern always binds a non-null value: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn instanceof_pattern_binding_is_available_after_negated_guard_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                if (!(candidate instanceof String text)) {
                    return;
                }
                text.length();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "pattern scope follows the only normally completing branch: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn instanceof_pattern_binding_is_nonnull_in_and_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                if (candidate instanceof String text && text.length() > 0) {
                    text.toString();
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the RHS of && executes only after the pattern has matched: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn pattern_binding_shadows_field_only_in_its_flow_scope() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable String text;

            void test(@Nullable Object candidate) {
                if (candidate instanceof String text) {
                    text.length();
                }
                text.length();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["text"],
        "the nullable field is visible again outside the pattern's flow scope: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn successful_type_pattern_narrows_its_selector() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                if (candidate instanceof String text) {
                    candidate.toString();
                    text.length();
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "matching proves both the selector and binding non-null: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn pattern_binding_is_available_in_negated_or_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                if (!(candidate instanceof String text) || text.length() == 0) {
                    return;
                }
                text.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the RHS of || sees the pattern when the negated left operand is false: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn pattern_binding_is_available_in_while_body() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                while (candidate instanceof String text) {
                    text.length();
                    break;
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a while body runs only after its type pattern matched: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn sibling_pattern_bindings_with_the_same_name_are_distinct() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object first, @Nullable Object second) {
                if (first instanceof String text) {
                    text.length();
                }
                if (second instanceof String text) {
                    text.length();
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "same-named pattern variables in disjoint flow scopes cannot alias: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn writing_the_selector_does_not_change_the_pattern_binding() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object candidate) {
                if (candidate instanceof String text && (candidate = null) == null) {
                    text.length();
                    candidate.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["candidate"],
        "the pattern binding captures the matched value independently of later selector writes: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn null_boolean_condition_unboxes_before_branching() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Boolean condition = null;
                if (condition) {}
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["condition"],
        "unboxing null throws before either branch or the following statement: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn null_integer_initializer_unboxes_before_assignment() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Integer number = null;
                int primitive = number;
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["number"],
        "an unboxing conversion in an initializer has no normal exit for null: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn null_integer_arithmetic_unboxes_before_operator_execution() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Integer number = null;
                int primitive = number + 1;
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["number"],
        "numeric operators implicitly dereference boxed operands: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn simple_assignment_to_primitive_unboxes_before_the_store() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                int target = 0;
                Integer value = null;
                target = value;
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["value"]);
}

#[test]
fn compound_assignment_unboxes_saved_lhs_before_evaluating_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Integer value = null;
                Object state = new Object();
                try {
                    value += ((state = null) == null ? 1 : 2);
                } catch (NullPointerException expected) {}
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "failed LHS unboxing skips every RHS side effect: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn mixed_wrapper_primitive_equality_unboxes_but_wrapper_identity_does_not() {
    let mixed = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Integer value = null;
                boolean same = value == 1;
                unreachable.toString();
            }
        }
        "#,
    );
    assert_eq!(dereference_warnings(&mixed), ["value"]);

    let identity = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object reachable) {
                Integer left = null;
                Integer right = null;
                boolean same = left == right;
                reachable.toString();
            }
        }
        "#,
    );
    assert_eq!(dereference_warnings(&identity), ["reachable"]);
}

#[test]
fn bitwise_and_shift_operators_unbox_boxed_numeric_operands() {
    for expression in ["value & 1", "value << 1"] {
        let analysis = analyze_single_method(&format!(
            r#"
            class Test {{
                void test(@Nullable Object unreachable) {{
                    Integer value = null;
                    int result = {expression};
                    unreachable.toString();
                }}
            }}
            "#,
        ));
        assert_eq!(
            dereference_warnings(&analysis),
            ["value"],
            "missing conversion for {expression}: {:?}",
            analysis.diagnostics
        );
    }
}

#[test]
fn source_effect_fast_path_does_not_hide_implicit_unboxing() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private static void explode(Integer value) {
                int ignored = value;
            }

            void test() {
                Object state = new Object();
                try {
                    explode(null);
                } catch (NullPointerException possible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "a visible callee's unchecked conversion must remain in its effect summary: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn array_dimensions_indices_and_primitive_initializers_unbox() {
    for statement in [
        "int[] values = new int[value];",
        "int[][] values = new int[1][value];",
        "int[] values = new int[]{value};",
    ] {
        let analysis = analyze_single_method(&format!(
            r#"
            class Test {{
                void test(@Nullable Object unreachable) {{
                    Integer value = null;
                    {statement}
                    unreachable.toString();
                }}
            }}
            "#,
        ));
        assert_eq!(
            dereference_warnings(&analysis),
            ["value"],
            "missing array conversion for {statement}: {:?}",
            analysis.diagnostics
        );
    }
}

#[test]
fn conditional_numeric_conversion_unboxes_only_the_selected_boxed_arm() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Integer value = null;
                int selected = true ? value : 1;
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["value"]);
}

#[test]
fn primitive_argument_conversion_precedes_later_argument_evaluation() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static void consume(int value, Object ignored) {}

            void test() {
                Integer value = null;
                Object state = new Object();
                try {
                    consume(value, state = null);
                } catch (NullPointerException expected) {}
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "failed first-argument conversion skips later arguments: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn boxed_boolean_return_is_unboxed() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            boolean test(@Nullable Boolean result) {
                return result;
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["result"],
        "return conversion to a primitive boolean must report possible null unboxing: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unary_not_unboxes_its_boxed_operand() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Boolean condition = null;
                if (!condition) {}
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["condition"],
        "logical negation unboxes before it can produce a boolean: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unary_minus_unboxes_its_boxed_operand() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object unreachable) {
                Integer number = null;
                int primitive = -number;
                unreachable.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["number"],
        "unary numeric promotion unboxes before evaluating the operator: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nullable_string_concatenation_does_not_unbox() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object reachable) {
                String text = null;
                String result = text + " suffix";
                reachable.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["reachable"],
        "String conversion accepts a null reference without dereferencing it: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn failed_left_unboxing_skips_the_right_numeric_operand() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object skipped) {
                Integer number = null;
                int primitive = number + skipped.hashCode();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["number"],
        "a left-operand unboxing failure happens before the RHS is evaluated: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn failed_left_boolean_unboxing_skips_short_circuit_rhs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object skipped) {
                Boolean condition = null;
                boolean result = condition && skipped.toString() != null;
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["condition"],
        "short-circuit evaluation cannot reach the RHS after unboxing throws: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn static_primitive_field_does_not_inherit_its_qualifiers_boxed_kind() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Object reachable) {
                Integer qualifier = null;
                int max = qualifier.MAX_VALUE;
                reachable.toString();
            }
        }
        class Integer { static int MAX_VALUE = 1; }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["reachable"],
        "an access path has its member's type, not its root qualifier's type: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn boxed_increment_writes_a_fresh_nonnull_value() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Integer value) {
                if (value == null) {
                    value = 0;
                }
                value++;
                value.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "successful increment unboxes and reboxes the target: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn inapplicable_static_overload_does_not_hide_inherited_instance_dispatch() {
    let analysis = analyze_single_method(
        r#"
        class Base { void ping(Object value) {} }
        class Sub extends Base { static void ping(String value) {} }
        class Test {
            void test() {
                Sub receiver = null;
                Object argument = new Object();
                receiver.ping(argument);
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["receiver"],
        "dispatch cannot be inferred from an inapplicable same-arity static overload: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn primitive_widening_selects_static_over_inapplicable_instance_overload() {
    let analysis = analyze_single_method(
        r#"
        class Service {
            static void ping(long value) {}
            void ping(String value) {}
        }
        class Test {
            void test() {
                Service service = null;
                service.ping(1);
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "int widens to the selected static long overload without dereferencing the qualifier: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn local_value_named_objects_does_not_activate_jdk_condition_contract() {
    let analysis = analyze_single_method(
        r#"
        class FakeObjects {
            boolean nonNull(Object value) { return true; }
        }
        class Test {
            void test() {
                FakeObjects Objects = new FakeObjects();
                Object value = null;
                if (Objects.nonNull(value)) {
                    value.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "contract recognition requires the resolved java.util.Objects owner: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn local_value_named_preconditions_does_not_activate_guava_contract() {
    let analysis = analyze_single_method(
        r#"
        class FakePreconditions {
            void checkArgument(boolean condition) {}
        }
        class Test {
            void test() {
                FakePreconditions Preconditions = new FakePreconditions();
                Object value = null;
                Preconditions.checkArgument(value != null);
                value.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["value"],
        "contract recognition requires the resolved Guava owner: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn later_nested_local_does_not_retype_an_earlier_library_qualifier() {
    let analysis = analyze_single_method(
        r#"
        class FakeObjects {
            boolean nonNull(Object value) { return true; }
        }
        class Test {
            void test() {
                Object value = null;
                if (Objects.nonNull(value)) {
                    value.toString();
                }
                {
                    FakeObjects Objects = new FakeObjects();
                    Objects.nonNull(value);
                }
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "owner inference must resolve each qualifier at its lexical use site: {:?}",
        analysis.diagnostics
    );
}
