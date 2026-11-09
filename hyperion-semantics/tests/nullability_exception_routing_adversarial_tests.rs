// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial regressions for explicit exception completion.

mod common_nullability;

use std::sync::Arc;

use common_nullability::analyze_single_method;
use hyperion_semantics::db::{Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_semantics::nullability::{
    extract_declared_type_names, extract_method_info, extract_method_signatures, DiagnosticKind,
    NullabilityAnalysis,
};
use hyperion_workspace::Language;

fn dereference_warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect()
}

fn analyze_with_workspace_type(source: &str, dependency: &str) -> NullabilityAnalysis {
    let mut db = Database::new();
    let file = FileId::new(0);
    let dependency_file = FileId::new(1);
    db.set_file_text(dependency_file, Arc::new(dependency.to_owned()));
    db.set_file_language(dependency_file, Language::Java);
    let dependency_parse = db.parse_file(dependency_file);
    let signatures = extract_method_signatures(&dependency_parse.syntax(), dependency_file);
    let declared_types = extract_declared_type_names(&dependency_parse.syntax());
    assert!(db.method_signature_cache().update_file_facts(
        dependency_file,
        signatures,
        declared_types
    ));
    db.bump_method_signature_version();
    db.set_file_text(file, Arc::new(source.to_owned()));
    db.set_file_language(file, Language::Java);

    let parse = db.parse_file(file);
    assert!(
        parse.errors.is_empty(),
        "fixture errors: {:?}",
        parse.errors
    );
    let offset = parse
        .syntax()
        .descendants()
        .find_map(|node| {
            extract_method_info(&node)
                .filter(|info| info.name == "test")
                .map(|_| u32::from(node.text_range().start()))
        })
        .expect("test method");
    db.file_nullability(file)
        .methods
        .get(&offset)
        .expect("test analysis")
        .clone()
}

#[test]
fn explicit_throw_operand_effect_reaches_matching_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Boom(state = null);
                } catch (Boom exception) {
                    state.toString();
                }
            }
        }

        class Boom extends RuntimeException {
            Boom(Object ignored) {}
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["state"],
        "a catch observes side effects completed while evaluating the thrown value"
    );
}

#[test]
fn unknown_explicit_throw_runs_crossed_finally_before_outer_handler() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector, RuntimeException failure) {
                Object state = new Object();
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw failure;
                            } finally {
                                state = null;
                            }
                        } catch (RuntimeException exception) {
                            yield state;
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
        "an unmatched explicit throw crosses its inner cleanup and then enters the outer handler"
    );
}

#[test]
fn uncertain_source_subtype_match_keeps_outer_route_across_cleanup() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object state = new Object();
                Object result = switch (selector) {
                    default -> {
                        try {
                            try {
                                throw new Boom();
                            } catch (IllegalArgumentException exception) {
                                yield new Object();
                            } finally {
                                state = null;
                            }
                        } catch (RuntimeException exception) {
                            yield state;
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
        "an uncertain inner catch cannot discard the unmatched outer route, which runs the crossed cleanup first"
    );
}

#[test]
fn qualified_sibling_exception_names_do_not_match_by_simple_name() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new A.Failure();
                } catch (B.Failure wrong) {
                    state = null;
                } catch (A.Failure right) {
                    state = new Object();
                }
                state.toString();
            }
        }
        class A { static class Failure extends RuntimeException {} }
        class B { static class Failure extends RuntimeException {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "qualified type identity, not simple spelling, selects a catch: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn array_bounds_exception_skips_incompatible_null_pointer_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                int[] values = new int[0];
                try {
                    int ignored = values[0];
                } catch (NullPointerException wrong) {
                    state = null;
                } catch (ArrayIndexOutOfBoundsException right) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a known bounds exception must not enter an incompatible sibling handler: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn source_type_named_throwable_is_not_java_lang_throwable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Failure();
                } catch (Throwable wrong) {
                    state = null;
                } catch (RuntimeException right) {
                    state = new Object();
                }
                state.toString();
            }
        }
        class Throwable extends RuntimeException {}
        class Failure extends RuntimeException {}
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a source declaration shadows java.lang.Throwable in the compilation unit: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unrelated_nested_throwable_does_not_shadow_java_lang_throwable() {
    let analysis = analyze_single_method(
        r#"
        class Other {
            static class Throwable extends Exception {}
        }

        class Test {
            void test() {
                Object marker = null;
                try {
                    throw new NullPointerException();
                } catch (Throwable caught) {
                    // The inherited java.lang type catches the exception.
                }
                marker.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["marker"],
        "a member type of an unrelated top-level class is not in simple-name scope: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn statically_safe_object_cast_has_no_class_cast_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    Object ignored = (Object) new Object();
                } catch (ClassCastException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an identity/upcast to java.lang.Object cannot enter a ClassCastException handler: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unshadowed_bare_object_cast_is_a_safe_upcast() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    Object value = (Object) new String();
                } catch (ClassCastException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the implicit java.lang.Object target accepts every reference: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn primitive_cast_unboxes_its_boxed_operand() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Integer boxed) {
                Object state = new Object();
                try {
                    int value = (int) boxed;
                } catch (NullPointerException expected) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["boxed", "state"],
        "a primitive cast performs unboxing before primitive conversion: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn primitive_cast_from_broad_null_reference_routes_to_npe() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object boxed = null;
                Object state = new Object();
                try {
                    int value = (int) boxed;
                } catch (NullPointerException expected) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["boxed", "state"]);
}

#[test]
fn primitive_cast_from_incompatible_broad_reference_routes_to_cce() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object boxed = new Object();
                Object state = new Object();
                try {
                    int value = (int) boxed;
                } catch (ClassCastException expected) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn throwing_nullable_reference_has_null_and_nonnull_completion_lanes() {
    let analysis = analyze_single_method(
        r#"
        class Boom extends RuntimeException {}
        class Test {
            void test(@Nullable Boom failure) {
                Object state = new Object();
                try {
                    throw failure;
                } catch (NullPointerException nullThrow) {
                    state = null;
                } catch (Boom thrown) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn primitive_array_store_does_not_invent_null_pointer_exception() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                int[] values = new int[1];
                try {
                    values[0] = 1;
                } catch (NullPointerException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a non-null primitive array store has only bounds failure: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn same_package_object_does_not_activate_java_lang_upcast_rule() {
    let analysis = analyze_with_workspace_type(
        r#"
        package p;
        class Test {
            void test(java.lang.String value) {
                java.lang.Object state = new java.lang.Object();
                try {
                    Object cast = (Object) value;
                } catch (ClassCastException expected) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
        "package p; class Object {}",
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn same_package_string_does_not_activate_known_safe_jdk_constructor_rule() {
    let analysis = analyze_with_workspace_type(
        r#"
        package p;
        class Test {
            void test() {
                java.lang.Object state = new java.lang.Object();
                try {
                    new String();
                } catch (RuntimeException possible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
        "package p; class String { String() { dependency(); } }",
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn floating_division_has_no_arithmetic_exception_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(double divisor) {
                Object state = new Object();
                try {
                    double ignored = 1.0 / divisor;
                } catch (ArithmeticException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "IEEE-754 division never throws ArithmeticException: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn every_declarator_in_multi_field_declaration_keeps_its_value_kind() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            double first, divisor;

            void test() {
                Object state = new Object();
                try {
                    double ignored = 1 / divisor;
                } catch (ArithmeticException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the second declarator is still a double: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn inferred_assignment_and_conditional_floating_kinds_avoid_arithmetic_lane() {
    for body in [
        "var divisor = 2.0; double ignored = 1 / divisor;",
        "double divisor = 0.0; double ignored = 1 / (divisor = 2);",
        "boolean flag = true; double ignored = 1 / (flag ? 1.0 : 2.0);",
    ] {
        let analysis = analyze_single_method(&format!(
            r#"
            class Test {{
                void test() {{
                    Object state = new Object();
                    try {{ {body} }}
                    catch (ArithmeticException impossible) {{ state = null; }}
                    state.toString();
                }}
            }}
            "#,
        ));
        assert!(
            dereference_warnings(&analysis).is_empty(),
            "floating result kind was lost for {body}: {:?}",
            analysis.diagnostics
        );
    }
}

#[test]
fn nonzero_integral_literal_divisor_has_no_arithmetic_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    int ignored = 1 / 2;
                } catch (ArithmeticException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(dereference_warnings(&analysis).is_empty());
}

#[test]
fn floating_field_division_has_no_arithmetic_exception_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            double divisor;

            void test() {
                Object state = new Object();
                try {
                    double ignored = 1 / this.divisor;
                } catch (ArithmeticException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "resolved field types participate in binary numeric promotion: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nonthrowing_static_floating_result_has_no_arithmetic_exception_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static double divisor() { return 2.0; }

            void test() {
                Object state = new Object();
                try {
                    double ignored = 1 / divisor();
                } catch (ArithmeticException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a resolved nonthrowing static call contributes its floating return type only: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn floating_compound_remainder_has_no_arithmetic_exception_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(float divisor) {
                Object state = new Object();
                float value = 1.0f;
                try {
                    value %= divisor;
                } catch (ArithmeticException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "floating remainder assignment never throws ArithmeticException: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn integral_division_retains_arithmetic_exception_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int divisor) {
                Object state = new Object();
                try {
                    int ignored = 1 / divisor;
                } catch (ArithmeticException possible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "integral divide-by-zero must retain the ArithmeticException path: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn thrown_local_uses_its_declared_exception_type() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(java.io.IOException failure) {
                Object state = new Object();
                try {
                    throw failure;
                } catch (RuntimeException wrong) {
                    state = null;
                } catch (java.io.IOException right) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a declared IOException cannot enter a RuntimeException sibling catch: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn thrown_static_supertype_keeps_subtype_catches_possible() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Exception problem) {
                Object state = new Object();
                try {
                    throw problem;
                } catch (java.io.IOException possibleSubtype) {
                    state = null;
                } catch (Exception remainder) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "a declared upper bound is not an exact runtime exception identity: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn thrown_cast_uses_cast_result_bound_after_successful_conversion() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Exception problem) {
                Object state = new Object();
                try {
                    throw (java.io.IOException) problem;
                } catch (java.io.IOException converted) {
                    state = new Object();
                } catch (ClassCastException failedCast) {
                    state = new Object();
                } catch (Exception impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "normal cast completion constrains the thrown value to the cast type: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn precise_rethrow_of_multi_catch_keeps_every_alternative() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(boolean io) {
                Object state = new Object();
                try {
                    if (io) {
                        throw new java.io.IOException();
                    }
                    throw new IllegalStateException();
                } catch (java.io.IOException | IllegalStateException problem) {
                    try {
                        throw problem;
                    } catch (java.io.IOException first) {
                        state = new Object();
                    } catch (IllegalStateException second) {
                        state = null;
                    }
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "a multi-catch binding must not be collapsed to its first alternative: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn later_local_throwable_does_not_shadow_an_earlier_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object marker = null;
                try {
                    throw new NullPointerException();
                } catch (Throwable caught) {
                    // Resolves to java.lang.Throwable here.
                }
                class Throwable extends Exception {}
                marker.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["marker"],
        "a later block-local declaration is not a member visible at the catch: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn earlier_local_exception_type_shadows_java_lang_in_its_block() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                class RuntimeException extends Exception {}
                Object state = new Object();
                try {
                    throw new NullPointerException();
                } catch (RuntimeException custom) {
                    state = null;
                } catch (java.lang.RuntimeException actual) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the in-scope local class is distinct from java.lang.RuntimeException: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn source_superclass_resolves_in_the_declarations_owner() {
    let analysis = analyze_single_method(
        r#"
        class A {
            static class Base extends RuntimeException {}
            static class Boom extends Base {}
        }

        class Test {
            static class Base extends RuntimeException {}

            void test() {
                Object state = new Object();
                try {
                    throw new A.Boom();
                } catch (A.Base expected) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "A.Boom extends A.Base, irrespective of Test.Base: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn constructor_invocation_can_fail_before_new_value_is_produced() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Boom();
                } catch (SetupFailure constructorFailure) {
                    state = null;
                } catch (Boom thrownValue) {
                    state = new Object();
                }
                state.toString();
            }
        }
        class Boom extends RuntimeException {
            Boom() { throw new SetupFailure(); }
        }
        class SetupFailure extends RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "constructor invocation may fail before `throw` receives its value: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn explicit_array_dimension_can_throw_negative_array_size() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(int size) {
                Object state = new Object();
                try {
                    int[] values = new int[size];
                } catch (NegativeArraySizeException expected) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "dimension validation occurs after evaluating the size: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn array_initializer_without_dimension_has_no_negative_size_lane() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    int[] values = new int[]{1, 2};
                } catch (NegativeArraySizeException impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "initializer-only array creation cannot reject a negative dimension: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn imported_object_cast_is_not_assumed_to_target_java_lang_object() {
    let analysis = analyze_single_method(
        r#"
        import external.Object;

        class Test {
            void test() {
                java.lang.Object state = new java.lang.Object();
                try {
                    Object value = (Object) new java.lang.Object();
                } catch (ClassCastException possible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "a bare Object target needs import/workspace resolution before it is a safe upcast: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn imported_exception_name_does_not_alias_java_lang_type() {
    let analysis = analyze_single_method(
        r#"
        import external.RuntimeException;

        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new NullPointerException();
                } catch (RuntimeException custom) {
                    state = null;
                } catch (java.lang.RuntimeException actual) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an explicit import shadows the implicit java.lang simple name: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn same_package_workspace_type_shadows_java_lang_name() {
    let analysis = analyze_with_workspace_type(
        r#"
        package p;

        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new NullPointerException();
                } catch (RuntimeException custom) {
                    state = null;
                } catch (java.lang.RuntimeException actual) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
        "package p; public class RuntimeException extends Exception {}",
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "same-package workspace types outrank the implicit java.lang import: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn same_package_workspace_object_disables_safe_java_lang_upcast_assumption() {
    let analysis = analyze_with_workspace_type(
        r#"
        package p;

        class Test {
            void test() {
                java.lang.Object state = new java.lang.Object();
                try {
                    Object value = (Object) new java.lang.Object();
                } catch (ClassCastException possible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
        "package p; public class Object {}",
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "a same-package Object shadows java.lang.Object for cast conversion: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn same_package_workspace_string_disables_safe_jdk_constructor_assumption() {
    let analysis = analyze_with_workspace_type(
        r#"
        package p;

        class Test {
            void test() {
                java.lang.Object state = new java.lang.Object();
                try {
                    String value = new String();
                } catch (SetupFailure possibleDuringConstruction) {
                    state = null;
                }
                state.toString();
            }
        }

        class SetupFailure extends RuntimeException {}
        "#,
        "package p; public class String {}",
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "a same-package String is not java.lang.String's known-safe zero-argument constructor: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn java_lang_error_does_not_enter_exception_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Error();
                } catch (Exception wrongBranch) {
                    state = null;
                } catch (Error expected) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "java.lang.Error is a direct Throwable branch, not an Exception: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn java_lang_assertion_error_does_not_enter_exception_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new AssertionError();
                } catch (Exception wrongBranch) {
                    state = null;
                } catch (Error expected) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "java.lang.AssertionError belongs to the Error branch, not Exception: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn inherited_member_exception_type_is_in_simple_name_scope() {
    let analysis = analyze_single_method(
        r#"
        class Base {
            static class Failure extends RuntimeException {}
        }

        class Test extends Base {
            void test() {
                Object state = new Object();
                try {
                    throw new Base.Failure();
                } catch (Failure inherited) {
                    state = new Object();
                } catch (RuntimeException impossibleRemainder) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "a superclass member type is inherited into the subclass's lexical scope: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn interface_member_exception_type_is_in_implementor_scope() {
    let analysis = analyze_single_method(
        r#"
        interface FailureTypes {
            class Failure extends RuntimeException {}
        }

        class Test implements FailureTypes {
            void test() {
                Object state = new Object();
                try {
                    throw new FailureTypes.Failure();
                } catch (Failure inherited) {
                    state = new Object();
                } catch (RuntimeException impossibleRemainder) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "an interface member type is inherited into its implementor's lexical scope: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn subclass_construction_includes_superclass_initialization() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new DerivedFailure();
                } catch (SetupFailure possibleDuringSuperConstruction) {
                    state = null;
                } catch (DerivedFailure constructedValue) {
                    state = new Object();
                }
                state.toString();
            }
        }

        class BaseFailure extends RuntimeException {
            BaseFailure() { dependency(); }
        }
        class DerivedFailure extends BaseFailure {}
        class SetupFailure extends RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "implicit super() runs before an otherwise-empty subclass constructor completes: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn source_instance_initializer_contributes_constructor_effects() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Failure();
                } catch (SetupFailure possibleDuringInitialization) {
                    state = null;
                } catch (Failure constructedValue) {
                    state = new Object();
                }
                state.toString();
            }
        }

        class Failure extends RuntimeException {
            { dependency(); }
        }
        class SetupFailure extends RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "instance initializer blocks run as part of every constructor: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn anonymous_class_initializer_contributes_construction_effects() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object state = new Object();
                try {
                    Object value = new Object() {
                        { dependency(); }
                    };
                } catch (SetupFailure possibleDuringInitialization) {
                    state = null;
                }
                state.toString();
            }
        }
        class SetupFailure extends RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "anonymous-class instance initialization runs after the superclass constructor: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn imported_java_lang_names_do_not_make_external_super_constructors_infallible() {
    let analysis = analyze_single_method(
        r#"
        import external.RuntimeException;

        class Test {
            void test() {
                Object state = new Object();
                try {
                    throw new Failure();
                } catch (SetupFailure possibleDuringExternalSuperConstruction) {
                    state = null;
                } catch (Failure constructedValue) {
                    state = new Object();
                }
                state.toString();
            }
        }

        class Failure extends RuntimeException {}
        class SetupFailure extends java.lang.RuntimeException {}
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        vec!["state"],
        "an explicit import shadows java.lang before superclass effects are summarized: {:?}",
        analysis.diagnostics
    );
}
