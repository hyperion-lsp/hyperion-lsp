// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for hover functionality on pattern variables (Java 16+)

use crate::common_workspace::indexed_workspace_paths as create_test_workspace;
use hyperion_lsp::hover::HoverProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{Language, SourceDatabase};
use hyperion_workspace::index::WorkspaceIndex;
use std::path::Path;
use std::sync::Arc;
use tower_lsp::lsp_types::Position;

/// Helper to get hover text at a specific position
fn get_hover_at_position(
    provider: &HoverProvider,
    index: &WorkspaceIndex,
    file_path: &Path,
    source_text: &str,
    position: Position,
) -> Option<String> {
    let file_id = index.get_file_id(file_path)?;
    let hover = provider.hover(file_id, position, source_text).ok()??;

    match hover.contents {
        tower_lsp::lsp_types::HoverContents::Markup(markup) => Some(markup.value),
        tower_lsp::lsp_types::HoverContents::Scalar(scalar) => match scalar {
            tower_lsp::lsp_types::MarkedString::String(s) => Some(s),
            tower_lsp::lsp_types::MarkedString::LanguageString(ls) => Some(ls.value),
        },
        tower_lsp::lsp_types::HoverContents::Array(arr) => {
            let parts: Vec<String> = arr
                .into_iter()
                .map(|s| match s {
                    tower_lsp::lsp_types::MarkedString::String(s) => s,
                    tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
                })
                .collect();
            Some(parts.join("\n"))
        }
    }
}

#[test]
fn test_hover_on_simple_pattern_variable() {
    let (_temp, index, paths) = create_test_workspace(vec![(
        "com/example/Test.java",
        r"package com.example;

public class Test {
    void method(Object obj) {
        if (obj instanceof String s) {
            String x = s;
        }
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_guard| {
                db_guard.set_file_text(file_id, Arc::new(content));
                db_guard.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "s" in the usage "String x = s;"
    // Line 5, column 23 (0-indexed line 5, after "String x = ")
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(5, 23),
    )
    .expect("Should have hover text for pattern variable");

    println!("Hover text: {hover_text}");

    // Should show the type String
    assert!(
        hover_text.contains("String"),
        "Hover should show the type 'String': {hover_text}"
    );

    // Should indicate it's a pattern variable
    assert!(
        hover_text.contains("Pattern variable"),
        "Hover should indicate this is a pattern variable: {hover_text}"
    );

    // Should show "final" since pattern variables are implicitly final
    assert!(
        hover_text.contains("final"),
        "Hover should show 'final' modifier: {hover_text}"
    );

    // Should NOT show "IDENT"
    assert!(
        !hover_text.to_lowercase().starts_with("ident"),
        "Hover should not show IDENT: {hover_text}"
    );
}

#[test]
fn test_pattern_variable_vs_local_variable() {
    let (_temp, index, paths) = create_test_workspace(vec![(
        "com/example/Test.java",
        r#"package com.example;

public class Test {
    void method(Object obj) {
        if (obj instanceof String s) {
            String local = "test";
            System.out.println(s + local);
        }
    }
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_guard| {
                db_guard.set_file_text(file_id, Arc::new(content));
                db_guard.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over pattern variable "s"
    let hover_pattern = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 31), // "s" in "s + local"
    )
    .expect("Should have hover for pattern variable");

    // Hover over local variable "local"
    let hover_local = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 35), // "local" in "s + local"
    )
    .expect("Should have hover for local variable");

    println!("Pattern variable hover: {hover_pattern}");
    println!("Local variable hover: {hover_local}");

    // Pattern variable should be labeled as such
    assert!(
        hover_pattern.contains("Pattern variable"),
        "Pattern variable should be labeled: {hover_pattern}"
    );

    // Local variable should NOT be labeled as pattern variable
    assert!(
        !hover_local.contains("Pattern variable"),
        "Local variable should not be labeled as pattern variable: {hover_local}"
    );

    // Local variable should be labeled as "Local variable"
    assert!(
        hover_local.contains("Local variable"),
        "Local variable should be labeled: {hover_local}"
    );
}
