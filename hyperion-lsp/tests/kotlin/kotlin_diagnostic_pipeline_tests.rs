// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_lsp::diagnostic_engine::{collect_diagnostics_for_file, DiagnosticOptions};
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

async fn kotlin_diagnostics(source: &str) -> Vec<Diagnostic> {
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let path = temp.path().join("Main.kt");
    index
        .index_file_with_source(&path, source)
        .expect("index Kotlin fixture");
    let file_id = index.get_file_id(&path).unwrap_or_else(|| FileId::new(0));

    let host = SemanticHost::new(Arc::clone(&index));
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Kotlin);
    });

    collect_diagnostics_for_file(
        file_id,
        "kotlin",
        source,
        &host,
        None,
        true,
        DiagnosticOptions::default(),
    )
    .await
}

fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_ref() == Some(&NumberOrString::String(code.to_string()))
    })
}

#[tokio::test]
async fn kotlin_unresolved_and_val_reassignment_are_errors() {
    let diagnostics = kotlin_diagnostics(
        r"
fun test() {
    val answer = 41
    answer = 42
    println(answr)
}
",
    )
    .await;

    let unresolved = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == Some(NumberOrString::String("unresolved-reference".to_string()))
        })
        .expect("unresolved-reference diagnostic");
    assert_eq!(unresolved.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(unresolved.message, "unresolved reference: answr");

    let reassignment = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == Some(NumberOrString::String("val-reassignment".to_string()))
        })
        .expect("val-reassignment diagnostic");
    assert_eq!(reassignment.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(reassignment.message, "val cannot be reassigned");
}

#[tokio::test]
async fn kotlin_parse_error_does_not_cascade_unresolved_references() {
    let diagnostics = kotlin_diagnostics(
        r#"
fun test() {
    val text = "ok"
    println(text)
    val values = listOf(1, 2, 3)
    println(values)
    if (true {
        println(text)
        println(deliberateMisspelling)
    }
}
"#,
    )
    .await;

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source.as_deref() == Some("hyperion-parser")),
        "fixture must contain a parser diagnostic: {diagnostics:?}"
    );
    assert!(
        !has_code(&diagnostics, "unresolved-reference"),
        "parse recovery must not cascade unresolved references: {diagnostics:?}"
    );
}

#[tokio::test]
async fn java_typed_inspections_are_not_run_on_kotlin_trees() {
    let diagnostics = kotlin_diagnostics(
        r"
fun test() {
    val unused = 42
}
",
    )
    .await;

    // Java inspection codes are snake_case; the Kotlin-native registry uses `KT-` codes.
    // Seeing a Java code here would mean a Kotlin green tree was reinterpreted as Java.
    for java_code in [
        "unused_variable",
        "unused_import",
        "dead_code",
        "magic_number",
        "raw_type",
        "missing_override",
    ] {
        assert!(
            !has_code(&diagnostics, java_code),
            "Java inspections must not reinterpret Kotlin syntax kinds: {diagnostics:?}"
        );
    }

    // The Kotlin-native registry does run here.
    assert!(
        has_code(&diagnostics, "KT-UNUSED-VARIABLE"),
        "Kotlin inspections must run on Kotlin buffers: {diagnostics:?}"
    );
}
