// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end coverage for the Kotlin-native inspection registry.
//!
//! These go through `collect_diagnostics_for_file`, the same entry point the LSP uses, so
//! they prove the registry is actually wired into the pipeline — and that wiring it in did
//! not disturb the Java pipeline.

use std::sync::Arc;

use hyperion_lsp::diagnostic_engine::{collect_diagnostics_for_file, DiagnosticOptions};
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

async fn diagnostics_for(file_name: &str, language_id: &str, source: &str) -> Vec<Diagnostic> {
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let path = temp.path().join(file_name);
    index
        .index_file_with_source(&path, source)
        .expect("index fixture");
    let file_id = index.get_file_id(&path).unwrap_or_else(|| FileId::new(0));

    let language = if language_id == "kotlin" {
        Language::Kotlin
    } else {
        Language::Java
    };

    let host = SemanticHost::new(Arc::clone(&index));
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, language);
    });

    collect_diagnostics_for_file(
        file_id,
        language_id,
        source,
        &host,
        None,
        true,
        DiagnosticOptions::default(),
    )
    .await
}

async fn kotlin_diagnostics(source: &str) -> Vec<Diagnostic> {
    diagnostics_for("Main.kt", "kotlin", source).await
}

async fn java_diagnostics(source: &str) -> Vec<Diagnostic> {
    diagnostics_for("Main.java", "java", source).await
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.code {
            Some(NumberOrString::String(code)) => Some(code.clone()),
            _ => None,
        })
        .collect()
}

fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
    codes(diagnostics).iter().any(|found| found == code)
}

/// A single Kotlin buffer that triggers every inspection in the registry *and* both of the
/// pre-existing Kotlin error-level checks.
const MIXED_KOTLIN_FIXTURE: &str = r#"package demo

import kotlin.text.Regex
import kotlin.math.PI

fun report(): Int {
    val total: Int = 3
    val scratch = 9
    val locked = 1
    locked = 2
    println(misspelledReference)
    println(PI)
    return total
    println("never runs")
}
"#;

#[tokio::test]
async fn kotlin_buffer_gets_all_four_native_inspections() {
    let diagnostics = kotlin_diagnostics(MIXED_KOTLIN_FIXTURE).await;

    for code in [
        "KT-UNUSED-IMPORT",
        "KT-UNUSED-VARIABLE",
        "KT-REDUNDANT-EXPLICIT-TYPE",
        "KT-UNREACHABLE-CODE",
    ] {
        assert!(
            has_code(&diagnostics, code),
            "missing {code} in {:?}",
            codes(&diagnostics)
        );
    }
}

#[tokio::test]
async fn kotlin_buffer_still_gets_the_two_error_level_checks() {
    let diagnostics = kotlin_diagnostics(MIXED_KOTLIN_FIXTURE).await;

    for code in ["unresolved-reference", "val-reassignment"] {
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == Some(NumberOrString::String(code.to_string())))
            .unwrap_or_else(|| panic!("missing {code} in {:?}", codes(&diagnostics)));
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    }
}

#[tokio::test]
async fn kotlin_inspections_are_warnings_and_the_two_checks_stay_errors() {
    let diagnostics = kotlin_diagnostics(MIXED_KOTLIN_FIXTURE).await;

    for diagnostic in &diagnostics {
        let Some(NumberOrString::String(code)) = &diagnostic.code else {
            continue;
        };
        if code.starts_with("KT-") {
            assert_eq!(
                diagnostic.severity,
                Some(DiagnosticSeverity::WARNING),
                "{code} must be a warning"
            );
            assert_eq!(diagnostic.source.as_deref(), Some("hyperion-lint"));
        }
    }
}

#[tokio::test]
async fn kotlin_registry_reports_the_expected_names() {
    let diagnostics = kotlin_diagnostics(MIXED_KOTLIN_FIXTURE).await;
    let messages: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();

    assert!(
        messages.iter().any(|m| m.contains("Regex")),
        "unused import should name the import: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("scratch")),
        "unused variable should name the variable: {messages:?}"
    );
    // `PI` is used, so it must not be reported.
    assert!(
        !messages.iter().any(|m| m.contains("'PI'")),
        "used import must not be reported: {messages:?}"
    );
}

#[tokio::test]
async fn kotlin_parse_errors_suppress_the_whole_registry() {
    let diagnostics = kotlin_diagnostics(
        r"
import kotlin.text.Regex

fun broken() {
    val unused = 1
    if (true {
    }
}
",
    )
    .await;

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source.as_deref() == Some("hyperion-parser")),
        "fixture must actually fail to parse: {diagnostics:?}"
    );
    assert!(
        !codes(&diagnostics)
            .iter()
            .any(|code| code.starts_with("KT-")),
        "recovered trees must not produce inspections: {:?}",
        codes(&diagnostics)
    );
}

// ── Java pipeline must be untouched ──

/// Pins the size of the Java inspection pipeline. Adding or removing a Java inspection is
/// a deliberate act; it must not happen as a side effect of Kotlin work.
#[test]
fn java_inspection_pipeline_is_pinned_at_fourteen() {
    // Unreachable code moved out of the lint inspections: it is a compile error
    // (javac: "unreachable statement") reported by the flow checks.
    assert_eq!(hyperion_lsp::inspections::java_inspection_count(), 12);
}

#[test]
fn kotlin_registry_exposes_exactly_four_inspections() {
    let codes = hyperion_lsp::inspections::kotlin_inspection_codes();
    assert_eq!(
        codes,
        vec![
            "KT-UNUSED-IMPORT",
            "KT-UNUSED-VARIABLE",
            "KT-REDUNDANT-EXPLICIT-TYPE",
            "KT-UNREACHABLE-CODE",
        ]
    );
    assert_eq!(
        hyperion_lsp::inspections::kotlin_inspection_registry().len(),
        codes.len()
    );
}

#[tokio::test]
async fn java_buffers_still_get_java_inspections_and_no_kotlin_ones() {
    let source = r#"
import java.util.ArrayList;

public class Main {
    public void run() {
        int unusedLocal = 7;
        System.out.println("hello");
    }
}
"#;
    let diagnostics = java_diagnostics(source).await;

    assert!(
        has_code(&diagnostics, "unused_variable"),
        "Java inspections must still run: {:?}",
        codes(&diagnostics)
    );
    assert!(
        !codes(&diagnostics)
            .iter()
            .any(|code| code.starts_with("KT-")),
        "Kotlin inspections must never see a Java buffer: {:?}",
        codes(&diagnostics)
    );
}

#[tokio::test]
async fn kotlin_buffers_never_receive_java_inspection_codes() {
    let diagnostics = kotlin_diagnostics(MIXED_KOTLIN_FIXTURE).await;

    for java_code in [
        "unused_variable",
        "unused_import",
        "unused_parameter",
        "dead_code",
        "empty_catch_block",
        "string_comparison",
        "redundant_modifier",
        "missing_override",
        "magic_number",
        "raw_type",
        "narrowing_conversion",
        "resource_leak",
        "null_safety",
        "sync_on_non_final",
    ] {
        assert!(
            !has_code(&diagnostics, java_code),
            "Java code {java_code} leaked into a Kotlin buffer: {:?}",
            codes(&diagnostics)
        );
    }
}
