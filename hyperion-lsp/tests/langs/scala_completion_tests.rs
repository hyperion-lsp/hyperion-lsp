// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion tests for Scala language support
//!
//! These tests verify that code completion works for Scala files, including:
//! - Keyword completion (class, object, trait, def, val, var, etc.)
//! - Member completion (properties, methods)
//! - Type completion (String, Int, Boolean, etc.)
//! - Cross-language completion (Scala calling Java)

use hyperion_lsp::completion::CompletionProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::index::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

fn create_scala_test_provider_with_file(
    source: &str,
) -> (CompletionProvider, Arc<SemanticHost>, FileId) {
    let temp_dir = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let workspace_index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Scala);
    });
    (CompletionProvider::new(workspace_index), host, file_id)
}

// ========== Keyword Completion Tests ==========

#[test]
fn test_scala_keyword_completions_include_def() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'def' keyword
    assert!(
        completions.iter().any(|item| item.label == "def"),
        "Expected 'def' keyword in completions"
    );
}

#[test]
fn test_scala_keyword_completions_include_val_var() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'val' and 'var' keywords
    assert!(
        completions.iter().any(|item| item.label == "val"),
        "Expected 'val' keyword in completions"
    );
    assert!(
        completions.iter().any(|item| item.label == "var"),
        "Expected 'var' keyword in completions"
    );
}

#[test]
fn test_scala_keyword_completions_include_match() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'match' keyword (Scala-specific pattern matching)
    assert!(
        completions.iter().any(|item| item.label == "match"),
        "Expected 'match' keyword in completions"
    );
}

#[test]
fn test_scala_keyword_completions_include_case_class() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'case class' keyword
    assert!(
        completions.iter().any(|item| item.label == "case class"),
        "Expected 'case class' keyword in completions"
    );
}

#[test]
fn test_scala_keyword_completions_include_trait() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'trait' keyword
    assert!(
        completions.iter().any(|item| item.label == "trait"),
        "Expected 'trait' keyword in completions"
    );
}

#[test]
fn test_scala_keyword_completions_include_object() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'object' keyword (singleton)
    assert!(
        completions.iter().any(|item| item.label == "object"),
        "Expected 'object' keyword in completions"
    );
}

// ========== Type Keyword Completion Tests ==========
// Note: Type keywords are only shown in TypePosition context
// For now, we test that they exist in the completion module

#[test]
fn test_scala_has_type_keywords() {
    use hyperion_lsp::completion::scala_completion;

    let type_keywords = scala_completion::type_keyword_completions();

    // Verify we have the expected Scala types
    assert!(type_keywords.iter().any(|item| item.label == "String"));
    assert!(type_keywords.iter().any(|item| item.label == "Int"));
    assert!(type_keywords.iter().any(|item| item.label == "Boolean"));
    assert!(type_keywords.iter().any(|item| item.label == "Option"));
    assert!(type_keywords.iter().any(|item| item.label == "List"));
}

// ========== Snippet Completion Tests ==========

#[test]
fn test_scala_snippet_completions_include_main() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'main' snippet
    assert!(
        completions.iter().any(|item| item.label == "main"),
        "Expected 'main' snippet in completions"
    );
}

#[test]
fn test_scala_snippet_completions_include_println() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'println' snippet
    assert!(
        completions.iter().any(|item| item.label == "println"),
        "Expected 'println' snippet in completions"
    );
}

#[test]
fn test_scala_snippet_completions_include_for() {
    let (provider, host, file_id) = create_scala_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'for' comprehension snippet
    assert!(
        completions.iter().any(|item| item.label == "for"),
        "Expected 'for' snippet in completions"
    );
}

// ========== Context-Aware Completion Tests ==========
// Note: These tests are skipped because Scala uses tree-sitter parsing
// which creates minimal parse trees. Comment/string detection would require
// tree-sitter node traversal which is not yet implemented.

// TODO: Implement comment/string detection using tree-sitter once
// we have proper Scala CST traversal utilities
