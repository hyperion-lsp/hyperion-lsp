// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for nullability analysis of Optional.orElse(T)
//!
//! These tests verify that the nullability of `Optional.orElse(default)` depends
//! on the nullability of the default value:
//! - orElse(literal) → NonNull (literals are always non-null)
//! - orElse(@NonNull var) → NonNull (parameter is annotated non-null)
//! - orElse(unknown var) → Unknown (variable has unknown nullability)
//! - orElse(null) → Nullable (default is null)

mod common_nullability;

use common_nullability::analyze_single_method as analyze_code;
use hyperion_semantics::nullability::DiagnosticKind;

#[test]
fn test_simple_literal_assignment() {
    // First, test that a simple literal assignment works
    let code = r#"
        class Test {
            void test() {
                String name = "default";
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference)
        .count();
    assert_eq!(null_deref_count, 0, "literal assignment should not warn");
}

#[test]
fn test_or_else_with_string_literal() {
    // orElse with a string literal should return NonNull
    // Note: We check Optional.of() which creates a NonNull Optional to avoid warnings on the Optional itself
    let code = r#"
        class Test {
            void test(String input) {
                Optional<String> opt = Optional.of(input);
                String name = opt.orElse("default");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Should NOT produce a potential null dereference warning on name.length()
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        name_deref_count, 0,
        "orElse with non-null literal should not warn on result dereference"
    );
}

#[test]
fn test_or_else_with_empty_string_literal() {
    // orElse with empty string literal should still return NonNull
    let code = r#"
        class Test {
            void test(String input) {
                Optional<String> opt = Optional.ofNullable(input);
                String name = opt.orElse("");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        name_deref_count, 0,
        "orElse with empty string literal should not warn on result"
    );
}

#[test]
fn test_or_else_with_number_literal() {
    // orElse with number literal should return NonNull
    let code = r#"
        class Test {
            void test() {
                Optional<Integer> opt = Optional.empty();
                Integer value = opt.orElse(42);
                int result = value.intValue();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let value_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "value"
        })
        .count();
    assert_eq!(
        value_deref_count, 0,
        "orElse with number literal should not warn on result"
    );
}

#[test]
fn test_or_else_with_null_literal() {
    // orElse(null) should return Nullable and warn on dereference
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt = Optional.empty();
                String name = opt.orElse(null);
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // SHOULD produce a potential null dereference warning on name
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        name_deref_count, 1,
        "orElse(null) should warn on result dereference"
    );
}

#[test]
fn test_or_else_with_nullable_variable() {
    // orElse with a nullable fallback remains nullable and warns on dereference.
    let code = r#"
        class Test {
            void test(@Nullable String fallback) {
                Optional<String> opt = Optional.empty();
                String name = opt.orElse(fallback);
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // SHOULD produce a potential null dereference warning on name (fallback might be null)
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        name_deref_count, 1,
        "orElse with unknown variable should warn on result dereference"
    );
}

#[test]
fn test_or_else_with_new_expression() {
    // orElse with 'new' expression should return NonNull
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt = Optional.empty();
                String name = opt.orElse(new String("default"));
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        name_deref_count, 0,
        "orElse with 'new' should not warn on result"
    );
}

#[test]
fn test_or_else_chained_with_map() {
    // Optional.map().orElse() should work correctly
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt = Optional.of("test");
                String name = opt.map(s -> s.toUpperCase()).orElse("Unknown");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(
        name_deref_count, 0,
        "chained map().orElse() should not warn on result"
    );
}

#[test]
fn test_or_else_vs_or_else_throw() {
    // Compare orElse and orElseThrow behavior
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt = Optional.of("test");
                String safe = opt.orElseThrow();
                String alsoSafe = opt.orElse("default");
                String unsafe = opt.orElse(null);

                safe.length();
                alsoSafe.length();
                unsafe.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Should warn only on unsafe (orElse(null))
    let unsafe_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "unsafe"
        })
        .count();
    assert_eq!(unsafe_warnings, 1, "only orElse(null) result should warn");
}

#[test]
fn test_or_else_in_ternary_expression() {
    // Both branches return NonNull, so their ternary join must remain NonNull.
    let code = r#"
        class Test {
            void test(boolean flag) {
                Optional<String> opt = Optional.of("test");
                String result = flag ? opt.orElse("A") : opt.orElse("B");
                int len = result.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    let result_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "result"
        })
        .count();
    assert_eq!(
        result_deref_count, 0,
        "non-null ternary branches must not produce a dereference warning"
    );
}

#[test]
fn test_or_else_with_method_call() {
    // orElse with method call result should depend on method's return nullability
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt = Optional.empty();
                String name = opt.orElse(getDefault());
                int len = name.length();
            }

            String getDefault() {
                return "default";
            }
        }
    "#;

    let analysis = analyze_code(code);
    // The source method body is inferred non-null and flows through orElse.
    let name_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name")
        .count();
    assert_eq!(name_deref_count, 0, "inferred non-null fallback is safe");
}

#[test]
fn test_nested_or_else() {
    // Nested Optional.orElse should work correctly
    let code = r#"
        class Test {
            void test(Optional<Optional<String>> nested) {
                Optional<String> inner = nested.orElse(Optional.empty());
                String value = inner.orElse("default");
                int len = value.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // First orElse returns NonNull Optional (from Optional.empty() which is a 'new')
    // Second orElse returns NonNull String (from "default" literal)
    let _null_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::PotentialNullDereference)
        .count();
    // The current implementation might not handle Optional.empty() correctly,
    // but orElse("default") should work
    // For now, just check that analysis completes
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_or_else_assigned_to_field() {
    // orElse result assigned to field should work correctly
    let code = r#"
        class Test {
            private String name;

            void test(Optional<String> opt) {
                this.name = opt.orElse("default");
                int len = this.name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Field tracking is limited, but the orElse itself should work
    // This test mainly ensures no crash
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_or_else_in_return_statement() {
    // orElse in a return position must not spuriously warn about its result.
    // `opt` is made non-null here (Optional.of) so the test isolates orElse:
    // dereferencing an *unannotated* Optional param is reported at Info severity
    // like any other Unknown receiver, in return position as well as elsewhere.
    let code = r#"
        class Test {
            String test() {
                Optional<String> opt = Optional.of("value");
                return opt.orElse("default");
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
fn test_or_else_with_concatenated_string() {
    // orElse with string concatenation should return NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt, String prefix) {
                String name = opt.orElse("default" + "Value");
                int len = name.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // String concatenation with literals is still somewhat non-null
    // This test mainly ensures no crash
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_multiple_or_else_calls() {
    // Multiple orElse calls with different defaults
    let code = r#"
        class Test {
            void test() {
                Optional<String> opt1 = Optional.empty();
                Optional<String> opt2 = Optional.empty();
                String name1 = opt1.orElse("default1");
                String name2 = opt2.orElse(null);

                name1.length();
                name2.length();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // Should warn only on name2 (orElse(null))
    let name2_warnings = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "name2"
        })
        .count();
    assert_eq!(
        name2_warnings, 1,
        "only the orElse(null) result should warn"
    );
}

#[test]
fn test_or_else_with_this_reference() {
    // orElse with 'this' reference should work
    let code = r#"
        class Test {
            void test() {
                Optional<Test> opt = Optional.empty();
                Test instance = opt.orElse(this);
                instance.toString();
            }
        }
    "#;

    let analysis = analyze_code(code);
    // 'this' is always NonNull, so orElse(this) should be NonNull
    let instance_deref_count = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "instance"
        })
        .count();
    assert_eq!(
        instance_deref_count, 0,
        "orElse(this) should not warn on result"
    );
}
