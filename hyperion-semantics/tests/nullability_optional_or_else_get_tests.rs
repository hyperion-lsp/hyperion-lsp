// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for nullability analysis of Optional.orElseGet(Supplier<T>)
//!
//! These tests verify that the nullability of `Optional.orElseGet(supplier)` depends
//! on the nullability of the value returned by the supplier:
//! - orElseGet(() -> "default") → NonNull (lambda returns non-null literal)
//! - orElseGet(() -> new String()) → NonNull (lambda returns new object)
//! - orElseGet(() -> null) → Nullable (lambda returns null)
//! - orElseGet(() -> { return "value"; }) → NonNull (block lambda returns non-null)
//! - orElseGet(this::method) → Unknown (method reference, conservative)

mod common_nullability;

use common_nullability::analyze_single_method as analyze_code;
use hyperion_semantics::nullability::DiagnosticKind;

#[test]
fn test_or_else_get_with_string_lambda() {
    // orElseGet with expression body lambda returning string literal → NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> "default");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "orElseGet with non-null lambda should not warn"
    );
}

#[test]
fn test_or_else_get_with_new_expression_lambda() {
    // orElseGet with lambda returning new expression → NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> new String("default"));
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "orElseGet with new expression lambda should not warn"
    );
}

#[test]
fn test_or_else_get_with_block_lambda() {
    // orElseGet with block body lambda containing return statement → NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    return "computed";
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "orElseGet with block lambda should not warn"
    );
}

#[test]
fn test_or_else_get_with_method_reference() {
    // orElseGet with method reference → Unknown (conservative approach)
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(this::getDefault);
                int len = name.length();
            }

            String getDefault() {
                return "default";
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    // Should warn because method references are treated conservatively
    assert_eq!(
        null_deref_count, 1,
        "orElseGet with method reference should warn (conservative)"
    );
}

#[test]
fn test_or_else_get_with_null_returning_lambda() {
    // orElseGet with lambda returning null → Nullable, should warn
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> null);
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 1,
        "orElseGet with null-returning lambda should warn"
    );
}

#[test]
fn test_or_else_get_with_block_returning_null() {
    // orElseGet with block lambda returning null → Nullable
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    return null;
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 1,
        "orElseGet with block lambda returning null should warn"
    );
}

#[test]
fn test_or_else_get_with_empty_string_lambda() {
    // orElseGet with lambda returning empty string literal → NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> "");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "orElseGet with empty string lambda should not warn"
    );
}

#[test]
fn test_or_else_get_with_number_lambda() {
    // orElseGet with lambda returning number literal → NonNull
    let code = r#"
        class Test {
            void test(Optional<Integer> opt) {
                Integer value = opt.orElseGet(() -> 42);
                int result = value.intValue();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "value"
        })
        .count();
    assert_eq!(
        null_deref_count, 0,
        "orElseGet with number literal lambda should not warn"
    );
}

#[test]
fn test_or_else_get_chained_with_map() {
    // Optional.map().orElseGet() should work correctly
    let code = r#"
        class Test {
            void test(String input) {
                Optional<String> opt = Optional.of(input);
                String name = opt.map(s -> s.toUpperCase()).orElseGet(() -> "Unknown");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "chained map().orElseGet() should not warn"
    );
}

#[test]
fn test_or_else_get_with_computation_lambda() {
    // orElseGet with lambda performing computation → depends on what it returns
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    String computed = "prefix" + "suffix";
                    return computed;
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Note: String concatenation might not be fully analyzed, but we shouldn't crash
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_or_else_get_with_this_reference_lambda() {
    // orElseGet with lambda returning 'this' → NonNull
    let code = r#"
        class Test {
            void test(Optional<Test> opt) {
                Test instance = opt.orElseGet(() -> this);
                instance.toString();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "instance"
        })
        .count();
    assert_eq!(
        null_deref_count, 0,
        "orElseGet with 'this' lambda should not warn"
    );
}

#[test]
fn test_or_else_get_vs_or_else() {
    // Compare orElseGet and orElse behavior
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String direct = opt.orElse("default");
                String lazy = opt.orElseGet(() -> "default");
                String nullDirect = opt.orElse(null);
                String nullLazy = opt.orElseGet(() -> null);

                direct.length();
                lazy.length();
                nullDirect.length();
                nullLazy.length();
            }
        }
    "#;

    let analysis = analyze_code(code);

    // Both direct and lazy non-null should not warn
    let direct_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "direct"
        })
        .count();
    assert_eq!(direct_warnings, 0, "orElse with non-null should not warn");

    let lazy_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "lazy")
        .count();
    assert_eq!(
        lazy_warnings, 0,
        "orElseGet with non-null lambda should not warn"
    );

    // Both null versions should warn
    let null_direct_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "nullDirect"
        })
        .count();
    assert_eq!(null_direct_warnings, 1, "orElse(null) should warn");

    let null_lazy_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "nullLazy"
        })
        .count();
    assert_eq!(null_lazy_warnings, 1, "orElseGet(() -> null) should warn");
}

#[test]
fn test_or_else_get_in_return_statement() {
    // orElseGet in a return position must not spuriously warn about its result.
    // `opt` is made non-null here (Optional.of) so the test isolates orElseGet:
    // dereferencing an *unannotated* Optional param is reported at Info severity
    // like any other Unknown receiver, in return position as well as elsewhere.
    let code = r#"
        class Test {
            String test() {
                Optional<String> opt = Optional.of("value");
                return opt.orElseGet(() -> "default");
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Should not produce warnings
    assert!(
        analysis.diagnostics.is_empty()
            || analysis
                .diagnostics
                .iter()
                .all(|d| d.kind != DiagnosticKind::PotentialNullDereference)
    );
}

#[test]
fn test_multiple_or_else_get_calls() {
    // Multiple orElseGet calls with different lambdas
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt1 = Optional.empty();
                Optional<String> opt2 = Optional.empty();
                String name1 = opt1.orElseGet(() -> "default1");
                String name2 = opt2.orElseGet(() -> null);

                name1.length();
                name2.length();
            }
        }
    "#;

    let analysis = analyze_code(code);

    // name1 should not warn (non-null lambda)
    let name1_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name1"
        })
        .count();
    assert_eq!(name1_warnings, 0, "non-null lambda should not warn");

    // name2 should warn (null lambda)
    let name2_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name2"
        })
        .count();
    assert_eq!(name2_warnings, 1, "null lambda should warn");
}

#[test]
fn test_or_else_get_nested_optionals() {
    // Nested Optional.orElseGet should work correctly
    let code = r#"
        class Test {
            void test(Optional<Optional<String>> nested) {
                Optional<String> inner = nested.orElseGet(() -> Optional.empty());
                String value = inner.orElseGet(() -> "default");
                int len = value.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // First orElseGet returns NonNull Optional (from Optional.empty() which is a 'new')
    // Second orElseGet returns NonNull String (from "default" literal)
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "value"
        })
        .count();
    assert_eq!(
        null_deref_count, 0,
        "nested orElseGet with non-null lambda should not warn"
    );
}

#[test]
fn test_or_else_get_with_block_and_multiple_statements() {
    // orElseGet with block lambda having multiple statements before return
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    String prefix = "default";
                    String suffix = "_value";
                    return prefix;
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Should analyze the return statement's expression
    // Since we return a variable with unknown nullability, this might warn
    // This test mainly ensures no crash
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_or_else_get_compared_to_or_else_throw() {
    // Compare different Optional terminal operations
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String safe = opt.orElseThrow();
                String alsoSafe = opt.orElseGet(() -> "default");
                String unsafe = opt.orElseGet(() -> null);

                safe.length();
                alsoSafe.length();
                unsafe.length();
            }
        }
    "#;

    let analysis = analyze_code(code);

    // orElseThrow should not warn
    let safe_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "safe")
        .count();
    assert_eq!(safe_warnings, 0, "orElseThrow should not warn");

    // orElseGet with non-null lambda should not warn
    let also_safe_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "alsoSafe"
        })
        .count();
    assert_eq!(
        also_safe_warnings, 0,
        "orElseGet with non-null lambda should not warn"
    );

    // orElseGet with null lambda should warn
    let unsafe_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "unsafe"
        })
        .count();
    assert_eq!(unsafe_warnings, 1, "orElseGet with null lambda should warn");
}

// ============================================================================
// Lambda try-catch return tests
// ============================================================================
// These tests verify that lambda return analysis correctly handles try-catch
// blocks with different return values in each branch.

#[test]
fn impossible_null_catch_does_not_widen_lambda_return() {
    // The literal return cannot throw into the catch. Structural reachability
    // alone must not make the supplier nullable.
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        return "success";
                    } catch (Exception e) {
                        return null;
                    }
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "an impossible null-returning catch must not widen the lambda"
    );
}

#[test]
fn possible_call_exception_keeps_null_catch_in_lambda_summary() {
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        return fetchFromDependency();
                    } catch (Exception possible) {
                        return null;
                    }
                });
                name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "name"
        })
        .count();
    assert_eq!(
        null_deref_count, 1,
        "a call can still complete abruptly into the null-returning catch"
    );
}

#[test]
fn test_or_else_get_with_try_catch_both_non_null() {
    // orElseGet with try-catch lambda where both branches return non-null → NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        return "success";
                    } catch (Exception e) {
                        return "fallback";
                    }
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "try-catch lambda with all non-null returns should not warn"
    );
}

#[test]
fn test_or_else_get_with_try_catch_both_null() {
    // orElseGet with try-catch lambda where both branches return null → Null
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        return null;
                    } catch (Exception e) {
                        return null;
                    }
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 1,
        "try-catch lambda with all null returns should warn"
    );
}

#[test]
fn impossible_null_catch_before_finally_does_not_widen_lambda_return() {
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        return "success";
                    } catch (Exception e) {
                        return null;
                    } finally {
                        cleanup();
                    }
                });
                int len = name.length();
            }
            void cleanup() {}
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "the finally call cannot retroactively enter the preceding catch"
    );
}

#[test]
fn impossible_nested_null_catch_does_not_widen_lambda_return() {
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        try {
                            return "inner";
                        } catch (RuntimeException e) {
                            return null;
                        }
                    } catch (Exception e) {
                        return "outer";
                    }
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "unreachable nested catches must not contribute lambda returns"
    );
}

#[test]
fn impossible_multiple_catches_do_not_widen_lambda_return() {
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String name = opt.orElseGet(() -> {
                    try {
                        return "success";
                    } catch (IOException e) {
                        return "io_error";
                    } catch (RuntimeException e) {
                        return null;
                    } catch (Exception e) {
                        return "generic_error";
                    }
                });
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        null_deref_count, 0,
        "none of the catches can observe a non-throwing literal return"
    );
}

#[test]
fn synchronously_invoked_supplier_reports_body_diagnostics() {
    let code = r#"
        class Test {
            void test(Optional<String> opt, @Nullable Object value) {
                opt.orElseGet(() -> {
                    value.toString();
                    return "fallback";
                });
            }
        }
    "#;

    let analysis = analyze_code(code);
    let diagnostic_count = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "value"
        })
        .count();
    assert_eq!(
        diagnostic_count, 1,
        "a synchronously analyzed body should contribute each diagnostic once: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn always_throwing_supplier_has_no_unknown_normal_return() {
    let analysis = analyze_code(
        r#"
        class Test {
            void test(Optional<String> opt) {
                String value = opt.orElseGet(() -> {
                    throw new IllegalStateException();
                });
                value.toString();
            }
        }
        "#,
    );

    assert!(
        !analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "value"
        }),
        "on normal completion an abrupt-only fallback was skipped, so Optional supplied the non-null value: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn normal_supplier_field_effect_is_joined_with_the_skipped_path() {
    let analysis = analyze_code(
        r#"
        class Test {
            Object state;

            void test(Optional<String> opt) {
                state = new Object();
                opt.orElseGet(() -> {
                    state = null;
                    return "fallback";
                });
                state.toString();
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name.ends_with("state")
        }),
        "normal continuation joins Optional's supplier-skipped and supplier-invoked states: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn normal_supplier_property_effect_flows_back_to_the_caller() {
    let analysis = analyze_code(
        r#"
        class Holder { Object value; }
        class Test {
            void test(Optional<String> opt) {
                Holder holder = new Holder();
                holder.value = new Object();
                opt.orElseGet(() -> {
                    holder.value = null;
                    return "fallback";
                });
                holder.value.toString();
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "holder.value"
        }),
        "captured property paths must retain the supplier's conditional normal write: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exceptional_only_supplier_effect_does_not_pollute_normal_continuation() {
    let analysis = analyze_code(
        r#"
        class Holder { Object value; }
        class Test {
            void test(Optional<String> opt) {
                Holder holder = new Holder();
                holder.value = new Object();
                opt.orElseGet(() -> {
                    holder.value = null;
                    throw new IllegalStateException();
                });
                holder.value.toString();
            }
        }
        "#,
    );

    assert!(
        !analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "holder.value"
        }),
        "a write that only reaches the supplier's exceptional completion is absent from the call's normal path: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn deferred_nested_lambda_parameter_does_not_alias_captured_outer_name() {
    let analysis = analyze_code(
        r#"
        class Test {
            void test(Optional<String> opt, @Nullable Object value) {
                opt.orElseGet(() -> {
                    value.toString();
                    Function<Object, String> deferred = value -> value.toString();
                    return "fallback";
                });
            }
        }
        "#,
    );

    let value_diagnostics = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "value"
        })
        .count();
    assert_eq!(
        value_diagnostics,
        1,
        "only the captured outer local is dereferenced; the nested parameter shadows it in the deferred body: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn enclosing_return_contract_is_not_applied_inside_supplier_body() {
    let analysis = analyze_code(
        r#"
        class Test {
            @NonNull Object test(Optional<Object> opt) {
                return opt.orElseGet(() -> {
                    return null;
                });
            }
        }
        "#,
    );

    let return_diagnostics = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullReturnFromNonNull)
        .count();
    assert_eq!(
        return_diagnostics,
        1,
        "the outer method contract diagnoses its return once; it is not the lambda target's contract: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exceptional_supplier_effect_reaches_the_matching_catch() {
    let analysis = analyze_code(
        r#"
        class Holder { Object value; }
        class Test {
            void test(Optional<String> opt) {
                Holder holder = new Holder();
                holder.value = new Object();
                try {
                    opt.orElseGet(() -> {
                        holder.value = null;
                        throw new IllegalStateException();
                    });
                } catch (IllegalStateException expected) {
                    holder.value.toString();
                }
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "holder.value"
        }),
        "the supplier's captured write belongs to its exceptional completion even though it is absent from normal flow: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn supplier_finally_effect_is_part_of_the_accepted_return_state() {
    let analysis = analyze_code(
        r#"
        class Holder { Object value; }
        class Test {
            void test(Optional<String> opt) {
                Holder holder = new Holder();
                holder.value = new Object();
                opt.orElseGet(() -> {
                    try {
                        return "fallback";
                    } finally {
                        holder.value = null;
                    }
                });
                holder.value.toString();
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "holder.value"
        }),
        "an accepted return exports the state after every crossed cleanup: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn supplier_finally_can_restore_state_before_the_return_is_accepted() {
    let analysis = analyze_code(
        r#"
        class Holder { Object value; }
        class Test {
            void test(Optional<String> opt) {
                Holder holder = new Holder();
                holder.value = new Object();
                opt.orElseGet(() -> {
                    try {
                        holder.value = null;
                        return "fallback";
                    } finally {
                        holder.value = new Object();
                    }
                });
                holder.value.toString();
            }
        }
        "#,
    );

    assert!(
        !analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "holder.value"
        }),
        "the exported state comes from the cleanup resume lane, not the pre-finally return operand: {:?}",
        analysis.diagnostics
    );
}
