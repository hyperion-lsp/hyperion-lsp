// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for Java nullability tracking
//!
//! Tests the dataflow-based nullability analysis that tracks null states
//! through control flow, with narrowing after null checks.

use hyperion_lsp::diagnostics::nullability_diagnostic_to_lsp;
use hyperion_semantics::db::{FileId, Language, SemanticDatabase, SourceDatabase};
use hyperion_semantics::nullability::{DiagnosticKind, NullabilityDiagnostic};
use std::sync::Arc;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

/// Test database for nullability analysis
#[salsa::database(
    hyperion_semantics::db::SourceDatabaseStorage,
    hyperion_semantics::db::ParseDatabaseStorage,
    hyperion_semantics::db::SemanticDatabaseStorage
)]
#[derive(Default)]
struct TestDatabase {
    storage: salsa::Storage<Self>,
}

impl salsa::Database for TestDatabase {}

/// Helper to get nullability diagnostics from Java source
fn get_nullability_diagnostics(source: &str) -> Vec<NullabilityDiagnostic> {
    let mut db = TestDatabase::default();
    let file_id = FileId(0);

    // Set up the file in the database
    db.set_file_text(file_id, Arc::new(source.to_string()));
    db.set_file_language(file_id, Language::Java);
    db.set_method_signature_version(0);
    db.set_method_signatures(hyperion_semantics::nullability::MethodSignatureSnapshot::empty());

    // Run analysis
    let analysis = db.file_nullability(file_id);
    analysis.diagnostics.clone()
}

/// Helper to get LSP diagnostics from Java source
fn get_lsp_diagnostics(source: &str) -> Vec<Diagnostic> {
    let diagnostics = get_nullability_diagnostics(source);
    diagnostics
        .iter()
        .filter_map(|d| {
            nullability_diagnostic_to_lsp(
                d,
                source,
                hyperion_lsp::diagnostic_engine::NullabilityUnknownSeverity::Info,
            )
        })
        .collect()
}

fn get_default_lsp_diagnostics(source: &str) -> Vec<Diagnostic> {
    let diagnostics = get_nullability_diagnostics(source);
    diagnostics
        .iter()
        .filter_map(|diagnostic| {
            nullability_diagnostic_to_lsp(
                diagnostic,
                source,
                hyperion_lsp::diagnostic_engine::NullabilityUnknownSeverity::default(),
            )
        })
        .collect()
}

/// Helper to check if any diagnostic matches a kind
fn has_diagnostic_kind(diagnostics: &[NullabilityDiagnostic], kind: DiagnosticKind) -> bool {
    diagnostics.iter().any(|d| d.kind == kind)
}

/// Helper to count diagnostics of a specific kind
fn count_diagnostic_kind(diagnostics: &[NullabilityDiagnostic], kind: DiagnosticKind) -> usize {
    diagnostics.iter().filter(|d| d.kind == kind).count()
}

#[test]
fn default_output_hides_unknown_platform_noise_but_keeps_proven_nullable() {
    let source = r#"
        class Test {
            void test(@Nullable Service nullable, Library library) {
                Service platform = library.load();
                nullable.run();
                platform.run();
            }
        }
    "#;

    let diagnostics = get_default_lsp_diagnostics(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::WARNING));
    assert!(diagnostics[0].message.contains("nullable"));
}

// =============================================================================
// Basic Null Check Narrowing Tests
// =============================================================================

#[test]
fn test_null_check_narrows_in_then_branch() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s != null) {
                    s.length();  // OK: narrowed to non-null
                }
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    // Should NOT have potential null dereference in the then branch
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no null dereference warning after null check, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Call-Site Nullability Inference Tests
// =============================================================================

#[test]
fn test_param_inferred_nonnull_from_call_sites() {
    let source = r#"
        class Test {
            void accepts(String value) {
                value.length();
            }

            void caller() {
                accepts("ok");
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no null dereference warning for inferred non-null param, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_nullable_argument_is_rejected_at_strict_call_site() {
    let source = r"
        class Test {
            void accepts(@NonNull String value) {
                value.length();
            }

            void caller() {
                String value = null;
                accepts(value);
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::NullPassedToNonNull),
        "Expected nullable argument warning at the strict call site, got: {:?}",
        diagnostics
    );
    assert!(!has_diagnostic_kind(
        &diagnostics,
        DiagnosticKind::PotentialNullDereference
    ));
}

#[test]
fn test_param_inferred_from_this_and_class_calls() {
    let source = r#"
        class Test {
            static void accepts(String value) {
                value.length();
            }

            void caller() {
                this.accepts("ok");
                Test.accepts("also ok");
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no null dereference warning for class/this call inference, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_strict_source_parameter_flows_through_calls() {
    let source = r"
        class Test {
            void accepts(String value) {
                value.length();
            }

            void caller(String value) {
                accepts(value);
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        diagnostics.is_empty(),
        "strict source parameters are non-null in callers and callees: {:?}",
        diagnostics
    );
}

#[test]
fn test_other_receiver_does_not_contaminate_same_named_source_method() {
    let source = r"
        class Other {
            void accepts(@NonNull String value) {
            }
        }

        class Test {
            void accepts(String value) {
                value.length();
            }

            void caller(Other other) {
                other.accepts(null);
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "a call on another receiver must not widen the same-named source method: {:?}",
        diagnostics
    );
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::NullPassedToNonNull),
        "the exact other-receiver declaration still rejects null: {:?}",
        diagnostics
    );
}

#[test]
fn test_nullable_without_check_warns() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                s.length();  // WARNING: s may be null
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected potential null dereference warning, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_nonnull_parameter_no_warning() {
    let source = r"
        import javax.annotation.Nonnull;

        class Test {
            void test(@Nonnull String s) {
                s.length();  // OK: s is non-null
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning for @Nonnull parameter, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_null_check_else_branch_is_null() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s == null) {
                    // s is null here, any dereference would be an error
                    return;
                } else {
                    s.length();  // OK: s is non-null in else
                }
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning in else branch after s == null check, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_outside_null_check_still_nullable() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s != null) {
                    s.length();  // OK: narrowed
                }
                s.length();  // WARNING: s is nullable again outside the if
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected warning outside null check scope, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Compound Condition Tests
// =============================================================================

#[test]
fn test_compound_and_narrows_both() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String a, @Nullable String b) {
                if (a != null && b != null) {
                    a.length();  // OK
                    b.length();  // OK
                }
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after compound && check, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_compound_or_false_narrows_both() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String a, @Nullable String b) {
                if (a == null || b == null) {
                    return;  // Early return
                }
                // After the if, both are non-null
                a.length();  // OK
                b.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    // After early return, the remaining code has both non-null
    // Note: This depends on CFG properly handling early returns
    assert!(
        diagnostics.is_empty()
            || !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after || early return pattern, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Annotation Recognition Tests
// =============================================================================

#[test]
fn test_jetbrains_notnull_recognized() {
    let source = r"
        import org.jetbrains.annotations.NotNull;

        class Test {
            void test(@NotNull String s) {
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected @NotNull to be recognized, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_jetbrains_nullable_recognized() {
    let source = r"
        import org.jetbrains.annotations.Nullable;

        class Test {
            void test(@Nullable String s) {
                s.length();  // WARNING
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected @Nullable to be recognized, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_spring_nonnull_recognized() {
    let source = r"
        import org.springframework.lang.NonNull;

        class Test {
            void test(@NonNull String s) {
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected Spring @NonNull to be recognized, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_android_nonnull_recognized() {
    let source = r"
        import androidx.annotation.NonNull;

        class Test {
            void test(@NonNull String s) {
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected Android @NonNull to be recognized, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_lombok_nonnull_recognized() {
    let source = r"
        import lombok.NonNull;

        class Test {
            void test(@NonNull String s) {
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected Lombok @NonNull to be recognized, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// LSP Diagnostic Format Tests
// =============================================================================

#[test]
fn test_lsp_diagnostic_severity_warning() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                s.length();
            }
        }
    ";

    let diagnostics = get_lsp_diagnostics(source);
    assert!(!diagnostics.is_empty(), "Expected at least one diagnostic");

    // Potential null dereference should be WARNING
    let warning = diagnostics.iter().find(|d| d.message.contains("null"));

    if let Some(w) = warning {
        assert_eq!(
            w.severity,
            Some(DiagnosticSeverity::WARNING),
            "Potential null dereference should be WARNING severity"
        );
    }
}

#[test]
fn test_lsp_diagnostic_source() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                s.length();
            }
        }
    ";

    let diagnostics = get_lsp_diagnostics(source);
    for diag in &diagnostics {
        assert_eq!(
            diag.source,
            Some("hyperion-nullability".to_string()),
            "Diagnostic source should be hyperion-nullability"
        );
    }
}

// =============================================================================
// Control Flow Edge Cases
// =============================================================================

#[test]
fn test_while_loop_null_check() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                while (s != null) {
                    s.length();  // OK: guarded by condition
                    s = getNext();
                }
            }

            @Nullable
            String getNext() { return null; }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    // Inside while loop with s != null condition, s should be non-null
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning inside while with null check, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_for_loop_null_check() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String[] items) {
                if (items != null) {
                    for (int i = 0; i < items.length; i++) {
                        // items is non-null here
                    }
                }
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning inside for with guarded null check, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_ternary_operator() {
    let source = r#"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                String result = s != null ? s : "default";
                result.length();  // OK: result is always non-null
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        diagnostics.is_empty(),
        "`result` is non-null on every path, so no dereference warning is expected, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Method Return Nullability Tests
// =============================================================================

#[test]
fn test_nullable_return_assignment() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            @Nullable
            String getValue() { return null; }

            void test() {
                String s = getValue();
                s.length();  // WARNING: getValue() returns @Nullable
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected warning for @Nullable return assignment, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_nonnull_return_no_warning() {
    let source = r#"
        import javax.annotation.Nonnull;

        class Test {
            @Nonnull
            String getValue() { return "value"; }

            void test() {
                String s = getValue();
                s.length();  // OK: getValue() returns @Nonnull
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning for @Nonnull return, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Redundant Check Detection Tests
// =============================================================================

#[test]
fn test_redundant_null_check_on_nonnull() {
    let source = r"
        import javax.annotation.Nonnull;

        class Test {
            void test(@Nonnull String s) {
                if (s != null) {  // INFO: redundant check
                    s.length();
                }
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::RedundantNullCheck),
        "Expected redundant null check info, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Primitives Tests
// =============================================================================

#[test]
fn test_primitives_always_nonnull() {
    let source = r"
        class Test {
            void test(int x, boolean b, char c) {
                // Primitives can never be null
                int y = x;
                boolean flag = b;
                char ch = c;
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics for primitive types, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Local Variable Tracking Tests
// =============================================================================

#[test]
fn test_local_variable_assignment_tracking() {
    let source = r#"
        import javax.annotation.Nullable;

        class Test {
            void test() {
                String s = null;  // s is null here
                s = "hello";      // s is now non-null
                s.length();       // OK
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    // After assignment to non-null, should not warn
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after assignment to non-null literal, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_local_variable_nullable_assignment() {
    let source = r#"
        import javax.annotation.Nullable;

        class Test {
            @Nullable
            String getValue() { return null; }

            void test() {
                String s = "hello";  // s is non-null
                s = getValue();      // s may now be null
                s.length();          // WARNING
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected warning after @Nullable assignment, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Try-Catch Block Tests
// =============================================================================

#[test]
fn test_null_check_in_try_block() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                try {
                    if (s != null) {
                        s.length();  // OK
                    }
                } catch (Exception e) {
                    // s nullability unchanged
                }
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning in guarded try block, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Multiple Methods Test
// =============================================================================

#[test]
fn test_multiple_methods_independent_analysis() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void method1(@Nullable String s) {
                if (s != null) {
                    s.length();  // OK
                }
            }

            void method2(@Nullable String s) {
                s.length();  // WARNING
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    // Should have exactly one warning from method2
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "Expected exactly one null dereference warning, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_static_class_qualifiers_not_nullable() {
    let source = r#"
        import java.util.Arrays;
        import java.util.stream.Collectors;

        enum ShelfCategory {
            ONE,
            TWO
        }

        class Test {
            void test() {
                var legalTypes = Arrays.stream(ShelfCategory.values())
                    .map(Enum::name)
                    .collect(Collectors.joining(", "));
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no null dereference warnings for class qualifiers, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_constructor_injected_field_not_nullable() {
    let source = r"
        class Service {
            void ping() {}
        }

        class Test {
            private Service service;

            Test(Service service) {
                this.service = service;
            }

            void run() {
                service.ping();
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no null dereference warnings for constructor-injected field, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_field_initializer_nonnull() {
    let source = r#"
        import java.util.HashMap;
        import java.util.Map;

        class Test {
            private final Map<String, Object> values = new HashMap<>();

            void run() {
                values.put("key", new Object());
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no null dereference warnings for initialized field, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_inner_class_initializer_does_not_mark_outer_field_nonnull() {
    let source = r"
        class Outer {
            private Object value;

            void run() {
                value.toString();
            }

            class Inner {
                private Object value = new Object();
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected null dereference warning for outer field, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Java Idiom Tests
// =============================================================================

#[test]
fn test_early_return_null_check_narrows() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s == null) {
                    return;
                }
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after early return null guard, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_early_return_nonnull_check_warns() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s != null) {
                    return;
                }
                s.length();  // WARNING: s is null here
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected warning after early return on non-null, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_throw_on_null_narrows() {
    let source = r"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s == null) {
                    throw new IllegalStateException();
                }
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after throw-based null guard, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_objects_require_nonnull_statement_narrows() {
    let source = r"
        import java.util.Objects;
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                Objects.requireNonNull(s);
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after Objects.requireNonNull, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_preconditions_check_not_null_statement_narrows() {
    let source = r"
        import com.google.common.base.Preconditions;
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                Preconditions.checkNotNull(s);
                s.length();  // OK
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after Preconditions.checkNotNull, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_require_nonnull_else_does_not_narrow() {
    let source = r#"
        import java.util.Objects;
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                Objects.requireNonNullElse(s, "fallback");
                s.length();  // WARNING: s may still be null
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected warning after requireNonNullElse without assignment, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_nullable_field_annotation_overrides_constructor_injection() {
    let source = r"
        import javax.annotation.Nullable;

        class Service {
            void ping() {}
        }

        class Test {
            @Nullable
            private Service service;

            Test(Service service) {
                this.service = service;
            }

            void run() {
                service.ping();  // WARNING
            }
        }
    ";

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected warning for @Nullable field even when constructor-assigned, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_branch_join_after_assignment_narrows() {
    let source = r#"
        import javax.annotation.Nullable;

        class Test {
            void test(@Nullable String s) {
                if (s == null) {
                    s = "fallback";
                }
                s.length();  // OK: s is non-null after branch
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "Expected no warning after branch assigns fallback, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_receiver_null_check_early_return_narrows() {
    let source = r#"
        import javax.annotation.Nullable;

        class Profile {
            String getEmailAddress() { return "x"; }
        }

        class Member {
            @Nullable Profile getProfile() { return null; }
        }

        class Test {
            void test(@Nullable Member member) {
                if (member.getProfile() == null) return;
                String email = member.getProfile().getEmailAddress();
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    // `member` is @Nullable, so dereferencing it (to call getProfile()) should warn.
    // The null comparison is about getProfile()'s return value, not `member` itself.
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| {
                d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "member"
            })
            .count(),
        1,
        "Expected exactly one PotentialNullDereference for @Nullable member, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_receiver_nonnull_check_then_branch_narrows() {
    let source = r#"
        import javax.annotation.Nullable;

        class Profile {
            String getEmailAddress() { return "x"; }
        }

        class Member {
            @Nullable Profile getProfile() { return null; }
        }

        class Test {
            void test(@Nullable Member member) {
                if (member.getProfile() != null) {
                    String email = member.getProfile().getEmailAddress();
                }
            }
        }
    "#;

    let diagnostics = get_nullability_diagnostics(source);
    // `member` is @Nullable, so dereferencing it (to call getProfile()) should warn.
    // The null comparison is about getProfile()'s return value, not `member` itself.
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| {
                d.kind == DiagnosticKind::PotentialNullDereference && d.variable_name == "member"
            })
            .count(),
        1,
        "Expected exactly one PotentialNullDereference for @Nullable member, got: {:?}",
        diagnostics
    );
}

// =============================================================================
// Braceless control-flow body regression tests
//
// Regression for a CFG-construction bug: `if`/`while`/`for`/`do-while` bodies
// written without braces are direct statement children of the control-flow
// statement (not wrapped in a BLOCK_STMT). The CFG builder previously extracted
// bodies by searching only for a BLOCK_STMT child, so braceless bodies were
// silently dropped — dereferences inside them produced no diagnostics.
// =============================================================================

#[test]
fn test_braceless_if_body_deref_is_analyzed() {
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable String x) {
                if (cond()) x.length();
            }
            boolean cond() { return true; }
        }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "braceless if body deref of @Nullable x should warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_braceless_if_null_check_guards_braceless_body() {
    // The deref is inside the braceless `then`, guarded by the `x != null` check.
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable String x) {
                if (x != null) x.length();
            }
        }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "guarded braceless if body should not warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_braceless_while_body_deref_is_analyzed() {
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable String x) {
                while (cond()) x.length();
            }
            boolean cond() { return true; }
        }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "braceless while body deref of @Nullable x should warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_braceless_for_body_deref_is_analyzed() {
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable String x) {
                for (int i = 0; i < 10; i++) x.length();
            }
        }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "braceless for body deref of @Nullable x should warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_nested_braceless_if_is_not_misparsed() {
    // `if (a) if (b) x.length();` must analyze the deref, not misattach the inner
    // `if` as an `else` of the outer one.
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable String x, boolean a, boolean b) {
                if (a) if (b) x.length();
            }
        }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "nested braceless if deref should warn exactly once, got: {diagnostics:?}"
    );
}

// =============================================================================
// Compound condition (&& / ||) narrowing regression tests
//
// Regression for a precision bug: `analyze_binary` discarded an entire
// `&&`/`||` condition if either operand was not itself a recognized null
// check. That dropped all narrowing for idiomatic guards like
// `x != null && x.isReady()`, producing false-positive null-deref warnings
// both inside the condition (the short-circuited deref) and in the guarded body.
// =============================================================================

#[test]
fn test_compound_and_null_check_narrows() {
    // `x.isReady()` (short-circuited) and `x.read()` (guarded body) are both safe.
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable Resource x) {
                if (x != null && x.isReady()) {
                    x.read();
                }
            }
        }
        class Resource { boolean isReady() { return true; } void read() {} }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "`x != null && x.isReady()` should narrow x everywhere it runs, got: {diagnostics:?}"
    );
}

#[test]
fn test_compound_and_null_check_on_right_narrows_body() {
    // The null check is the right operand: `flag && x != null`.
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable Resource x, boolean flag) {
                if (flag && x != null) {
                    x.read();
                }
            }
        }
        class Resource { void read() {} }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "`flag && x != null` should narrow x in the body, got: {diagnostics:?}"
    );
}

#[test]
fn test_compound_or_early_return_narrows_after() {
    // `x == null || x.isEmpty()` then early return: x is non-null afterwards,
    // and the short-circuited `x.isEmpty()` is itself safe.
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable Resource x) {
                if (x == null || x.isEmpty()) return;
                x.read();
            }
        }
        class Resource { boolean isEmpty() { return false; } void read() {} }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "`x == null || x.isEmpty()` guard should narrow x afterwards, got: {diagnostics:?}"
    );
}

#[test]
fn test_compound_and_without_null_check_still_warns() {
    // Soundness: a compound condition whose left operand does NOT guard x must
    // not suppress the deref. In `cond && x.isReady()` the left operand is a
    // plain boolean, so the short-circuited `x.isReady()` is still unguarded.
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void test(@Nullable Resource x, boolean cond) {
                if (cond && x.isReady()) {
                    noop();
                }
            }
            void noop() {}
        }
        class Resource { boolean isReady() { return true; } }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "unguarded deref in `cond && x.isReady()` should still warn, got: {diagnostics:?}"
    );
}

// =============================================================================
// Dereferences in return / throw expressions + ternary guarding
//
// Return and throw expressions live on the CFG terminator, not in a basic
// block's statement list, so their dereferences previously escaped checking.
// Checking them exposes a latent gap: `is_guarded_by_null_check` had no case
// for ternaries, so `x != null ? x.foo() : dflt` (common in returns) would
// false-positive. Both are addressed together.
// =============================================================================

#[test]
fn test_return_expression_deref_is_checked() {
    let source = r#"
        import javax.annotation.Nullable;
        class Test {
            String describe(@Nullable Resource x) {
                return x.name();
            }
        }
        class Resource { String name() { return "r"; } }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "deref in `return x.name()` should warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_throw_expression_deref_is_checked() {
    let source = r#"
        import javax.annotation.Nullable;
        class Test {
            void check(@Nullable Resource x) {
                throw new RuntimeException(x.name());
            }
        }
        class Resource { String name() { return "r"; } }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "deref in `throw new RuntimeException(x.name())` should warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_ternary_then_arm_guards_deref() {
    // `x != null ? x.name() : dflt`: the then-arm only runs when x != null.
    let source = r#"
        import javax.annotation.Nullable;
        class Test {
            String describe(@Nullable Resource x) {
                return x != null ? x.name() : "none";
            }
        }
        class Resource { String name() { return "r"; } }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "ternary then-arm deref guarded by `x != null` should not warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_ternary_else_arm_guards_deref() {
    // `x == null ? dflt : x.name()`: the else-arm only runs when x != null.
    let source = r#"
        import javax.annotation.Nullable;
        class Test {
            String describe(@Nullable Resource x) {
                return x == null ? "none" : x.name();
            }
        }
        class Resource { String name() { return "r"; } }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        "ternary else-arm deref guarded by `x == null` should not warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_ternary_without_null_check_still_warns() {
    // Soundness: a ternary whose condition does not guard x must still warn.
    let source = r#"
        import javax.annotation.Nullable;
        class Test {
            String describe(@Nullable Resource x, boolean cond) {
                return cond ? x.name() : "none";
            }
        }
        class Resource { String name() { return "r"; } }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "unguarded ternary then-arm deref should still warn, got: {diagnostics:?}"
    );
}

// =============================================================================
// Ternary value nullability (bug: joined condition + then instead of the arms)
// =============================================================================

#[test]
fn test_ternary_value_joins_both_arms_not_condition() {
    // Both arms are non-null, so the ternary value is non-null and a @Nonnull
    // return must NOT warn. (Previously the value was join(condition, then),
    // which for a comparison condition was Unknown -> spurious return warning.)
    let source = r#"
        import javax.annotation.Nonnull;
        class Test {
            @Nonnull String pick(int a, int b) {
                return a < b ? "x" : "y";
            }
        }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::NullReturnFromNonNull),
        "ternary with two non-null arms should be non-null, got: {diagnostics:?}"
    );
}

#[test]
fn test_ternary_value_nullable_when_one_arm_null() {
    // Soundness: a null arm makes the ternary nullable, so a @Nonnull return
    // must warn.
    let source = r#"
        import javax.annotation.Nonnull;
        class Test {
            @Nonnull String pick(int a) {
                return a > 0 ? "x" : null;
            }
        }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        has_diagnostic_kind(&diagnostics, DiagnosticKind::NullReturnFromNonNull),
        "ternary with a null arm should warn on @Nonnull return, got: {diagnostics:?}"
    );
}

// =============================================================================
// Switch scrutinee dereferences (bug: scrutinee expr escaped deref checking)
// =============================================================================

#[test]
fn test_switch_scrutinee_deref_is_checked() {
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void route(@Nullable Resource x) {
                switch (x.kind()) {
                    case 1: break;
                    default: break;
                }
            }
        }
        class Resource { int kind() { return 0; } }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "deref of x in `switch (x.kind())` should warn, got: {diagnostics:?}"
    );
}

#[test]
fn test_switch_scrutinee_deref_not_rewarned_in_cases() {
    // The scrutinee deref proves x non-null for the case bodies, so a deref
    // inside a case must not warn again (one warning total, for the scrutinee).
    let source = r"
        import javax.annotation.Nullable;
        class Test {
            void route(@Nullable Resource x) {
                switch (x.kind()) {
                    case 1: x.use(); break;
                    default: break;
                }
            }
        }
        class Resource { int kind() { return 0; } void use() {} }
    ";
    let diagnostics = get_nullability_diagnostics(source);
    assert_eq!(
        count_diagnostic_kind(&diagnostics, DiagnosticKind::PotentialNullDereference),
        1,
        "scrutinee deref should narrow x for case bodies (one warning total), got: {diagnostics:?}"
    );
}

#[test]
fn test_ternary_arm_evaluated_under_condition_narrowing() {
    // `x != null ? x : dflt`: in the then-arm x is non-null, so the ternary
    // value is non-null and a @Nonnull return must NOT warn. (Previously each
    // arm was evaluated without the condition's narrowing, so the then-arm saw
    // x as nullable -> spurious NullReturnFromNonNull.)
    let source = r#"
        import javax.annotation.Nonnull;
        import javax.annotation.Nullable;
        class Test {
            @Nonnull String orDefault(@Nullable String x) {
                return x != null ? x : "default";
            }
        }
    "#;
    let diagnostics = get_nullability_diagnostics(source);
    assert!(
        !has_diagnostic_kind(&diagnostics, DiagnosticKind::NullReturnFromNonNull),
        "`x != null ? x : dflt` is non-null; @Nonnull return should not warn, got: {diagnostics:?}"
    );
}
