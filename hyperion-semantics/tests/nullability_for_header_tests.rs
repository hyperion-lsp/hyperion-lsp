// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: `process_for` positional extraction of the
//! for-header. Expression-list initializers (`for (x = fetch(); x != null;
//! x = next())`) put NAME_EXPR/ASSIGNMENT_EXPR nodes before the condition, so
//! "first expression child = condition" picked the init's NAME_EXPR as the
//! condition (losing the real guard), and the init assignment was invisible
//! to the init block (which only accepted LOCAL_VAR_DECL | EXPR_STMT).

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
fn test_assignment_initializer_condition_guards_body() {
    // The condition `x != null` must be recognized even though the header's
    // first expression children are the init assignment's NAME_EXPR and
    // ASSIGNMENT_EXPR.
    let code = r#"
        class Test {
            void test(Service x) {
                for (x = fetch(); x != null; x = next()) {
                    x.foo();
                }
            }
            Service fetch() { return null; }
            Service next() { return null; }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        deref_warnings_for(&analysis, "x"),
        0,
        "body dereference is guarded by the for condition `x != null`"
    );
}

#[test]
fn test_local_var_initializer_still_processed() {
    // Classic declaration-initializer loops must keep working.
    let code = r#"
        class Test {
            void test(int n, Service y) {
                for (int i = 0; i < n && y != null; i = i + 1) {
                    y.bar();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        deref_warnings_for(&analysis, "y"),
        0,
        "condition narrows y in a declaration-initializer for loop"
    );
}

#[test]
fn test_chained_for_condition_narrows_last_conjunct() {
    // 3-conjunct for-loop condition: the last conjunct lives in a sibling
    // continuation node (applied to for headers).
    let code = r#"
        class Test {
            void test(int n, Service x, Service y) {
                for (int i = 0; i < n && x != null && y != null; i = i + 1) {
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
        "third conjunct in for-header must narrow y"
    );
}

#[test]
fn test_unguarded_for_body_still_warns() {
    // No null check in the condition — the body dereference must still warn
    // (the fix must not silence real positives).
    let code = r#"
        class Test {
            void test(int n, @Nullable Service x) {
                for (int i = 0; i < n; i = i + 1) {
                    x.foo();
                }
            }
        }
    "#;

    let analysis = analyze_single_method(code);
    assert_eq!(
        deref_warnings_for(&analysis, "x"),
        1,
        "unguarded dereference in for body must warn"
    );
}

#[test]
fn foreach_body_processes_local_initializers_before_dereferences() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(String[] values) {
                for (String value : values) {
                    Service present = new Service();
                    present.run();
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "present"),
        0,
        "the foreach body must execute its local initializer before the dereference"
    );
}

#[test]
fn foreach_iterable_is_evaluated_before_the_body() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Values values) {
                for (String value : values.items()) {
                    value.length();
                }
            }
        }
        class Values { String[] items() { return null; } }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "values"),
        1,
        "dereferencing the iterable receiver must be diagnosed"
    );
}

#[test]
fn foreach_may_execute_zero_times() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(String[] values) {
                @Nullable Service service = null;
                for (String value : values) {
                    service = new Service();
                }
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service"),
        1,
        "the body assignment is not guaranteed when the iterable is empty"
    );
}

#[test]
fn foreach_reference_elements_remain_conservatively_nullable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Service[] services) {
                for (Service service : services) {
                    service.run();
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service"),
        1,
        "an array or Iterable may contain null reference elements"
    );
}

#[test]
fn foreach_respects_an_explicit_nonnull_element_contract() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Service[] services) {
                for (@NonNull Service service : services) {
                    service.run();
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(deref_warnings_for(&analysis, "service"), 0);
}

#[test]
fn foreach_does_not_apply_a_generic_argument_contract_to_the_element() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Box<@NonNull String>[] boxes) {
                for (Box<@NonNull String> box : boxes) {
                    box.run();
                }
            }
        }
        class Box<T> { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "box"),
        1,
        "an annotation on a type argument does not describe the Box element itself: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn foreach_nullable_element_wins_over_a_nested_nonnull_argument() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Box<@NonNull String>[] boxes) {
                for (@Nullable Box<@NonNull String> box : boxes) {
                    box.run();
                }
            }
        }
        class Box<T> { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "box"),
        1,
        "a nested type-argument contract must not erase the element's nullable contract: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn foreach_implicitly_dereferences_a_bare_iterable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service[] services) {
                for (Service service : services) {
                    consume(service);
                }
            }
            void consume(Service service) {}
        }
        class Service {}
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "services"),
        1,
        "obtaining an iterator or array length implicitly dereferences the iterable"
    );
}

#[test]
fn foreach_binding_does_not_leak_over_a_shadowed_field() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Service service;

            void test(Service[] services) {
                for (Service service : services) {
                    consume(service);
                }
                requireNonnull(service);
            }

            void consume(Service service) {}
            void requireNonnull(@NonNull Service service) {}
        }
        class Service {}
        "#,
    );

    let warnings: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect();
    assert_eq!(warnings, ["service"]);
}

#[test]
fn foreach_binding_does_not_inherit_a_shadowed_field_contract() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Service service;

            void test(Service[] services) {
                for (Service service : services) {
                    service.run();
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service"),
        1,
        "the foreach element is distinct from the non-null field it shadows"
    );
}

#[test]
fn foreach_member_assignment_does_not_assign_the_binding() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Service[] services) {
                for (@NonNull Service service : services) {
                    service.child = null;
                    service.run();
                }
            }
        }
        class Service {
            Child child;
            void run() {}
        }
        class Child {}
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service"),
        0,
        "a member access and its receiver share a start offset but not a variable identity: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn foreach_parenthesized_binding_keeps_its_narrowing() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Service[] services) {
                for (Service service : services) {
                    if (service != null) {
                        synchronized ((service)) {
                            consume(service);
                        }
                    }
                }
            }
            void consume(Service service) {}
        }
        class Service {}
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service"),
        0,
        "parentheses must not make a scoped binding fall back to a same-named field ID: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn foreach_cast_binding_does_not_fall_back_to_a_nullable_field() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable Object value;

            void test(Object[] values) {
                for (@NonNull Object value : values) {
                    synchronized ((CharSequence) value) {}
                }
            }
        }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "value"),
        0,
        "the cast operand is the non-null scoped binding, not the shadowed field: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn foreach_cast_binding_does_not_inherit_a_nonnull_field() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Object value;

            void test(Object[] values) {
                for (Object value : values) {
                    synchronized ((CharSequence) value) {}
                }
            }
        }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "value"),
        1,
        "the conservatively-nullable element must not inherit the field contract: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn foreach_assignment_does_not_invalidate_a_shadowed_field_path() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @NonNull Service service = new Service();

            void test(Service[] services) {
                if (service.child == null) return;
                for (@NonNull Service service : services) {
                    service = new Service();
                }
                service.child.run();
            }
        }
        class Service { Child child; }
        class Child { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service.child"),
        0,
        "invalidating a scoped binding must not poison a shadowed field access path: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nested_foreach_preserves_the_captured_outer_binding() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Service[] services) {
                for (@NonNull Service service : services) {
                    for (Service child : service.children) {
                        service.run();
                        child.run();
                    }
                    service.run();
                }
            }
        }
        class Service {
            Service[] children;
            void run() {}
        }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "service"),
        0,
        "references inside and after the nested loop must retain the non-null outer binding: {:?}",
        analysis.diagnostics
    );
    assert_eq!(deref_warnings_for(&analysis, "child"), 1);
}

#[test]
fn foreach_parenthesized_assignment_updates_the_outer_variable_once() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Services services) {
                for (Service service : (services = load())) {
                    consume(service);
                }
                services.use();
            }

            @NonNull Services load() { return new Services(); }
            void consume(Service service) {}
        }
        class Services {
            java.util.Iterator<Service> iterator() { return null; }
            void use() {}
        }
        class Service {}
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "services"),
        0,
        "the iterable assignment is evaluated before iteration and retained afterwards: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn labeled_foreach_still_lowers_its_body_in_source_order() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(String[] values) {
                entries: for (String value : values) {
                    Service present = new Service();
                    present.run();
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        deref_warnings_for(&analysis, "present"),
        0,
        "a label must not turn the whole loop back into one recursive statement"
    );
}
