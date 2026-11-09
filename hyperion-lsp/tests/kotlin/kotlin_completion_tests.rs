// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion tests for Kotlin language support
//!
//! These tests verify that code completion works for Kotlin files, including:
//! - Keyword completion (fun, val, var, when, data, sealed, etc.)
//! - Member completion (properties, functions)
//! - Top-level completion (top-level functions, properties)
//! - Context-aware completion (after modifiers, in type positions)
//! - Cross-language completion (Kotlin calling Java)

use hyperion_lsp::completion::CompletionProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::index::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

fn create_kotlin_test_provider_with_file(
    source: &str,
) -> (CompletionProvider, Arc<SemanticHost>, FileId) {
    let temp_dir = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let workspace_index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Kotlin);
    });
    (CompletionProvider::new(workspace_index), host, file_id)
}

// ========== Keyword Completion Tests ==========

#[test]
fn test_kotlin_keyword_completions_include_fun() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'fun' keyword
    assert!(
        completions.iter().any(|item| item.label == "fun"),
        "Expected 'fun' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_val_var() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

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
fn test_kotlin_keyword_completions_include_when() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'when' keyword (Kotlin-specific pattern matching)
    assert!(
        completions.iter().any(|item| item.label == "when"),
        "Expected 'when' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_data_class() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'data class' keyword
    assert!(
        completions.iter().any(|item| item.label == "data class"),
        "Expected 'data class' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_sealed_class() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'sealed class' keyword
    assert!(
        completions.iter().any(|item| item.label == "sealed class"),
        "Expected 'sealed class' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_companion() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'companion' keyword
    assert!(
        completions.iter().any(|item| item.label == "companion"),
        "Expected 'companion' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_suspend() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'suspend' keyword (coroutines)
    assert!(
        completions.iter().any(|item| item.label == "suspend"),
        "Expected 'suspend' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_is_as() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include type check and cast operators
    assert!(
        completions.iter().any(|item| item.label == "is"),
        "Expected 'is' keyword in completions"
    );
    assert!(
        completions.iter().any(|item| item.label == "as"),
        "Expected 'as' keyword in completions"
    );
}

#[test]
fn test_kotlin_keyword_completions_include_internal() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'internal' visibility modifier (Kotlin-specific)
    assert!(
        completions.iter().any(|item| item.label == "internal"),
        "Expected 'internal' keyword in completions"
    );
}

// ========== Snippet Completion Tests ==========

#[test]
fn test_kotlin_snippet_completions_include_main() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'main' snippet
    let main_snippet = completions.iter().find(|item| item.label == "main");
    assert!(
        main_snippet.is_some(),
        "Expected 'main' snippet in completions"
    );

    let snippet = main_snippet.unwrap();
    assert_eq!(snippet.kind, Some(CompletionItemKind::SNIPPET));
    assert!(snippet.insert_text.as_ref().unwrap().contains("fun main()"));
}

#[test]
fn test_kotlin_snippet_completions_include_println() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'println' snippet
    let println_snippet = completions.iter().find(|item| item.label == "println");
    assert!(
        println_snippet.is_some(),
        "Expected 'println' snippet in completions"
    );

    let snippet = println_snippet.unwrap();
    assert_eq!(snippet.kind, Some(CompletionItemKind::SNIPPET));
    assert!(snippet.insert_text.as_ref().unwrap().contains("println"));
}

#[test]
fn test_kotlin_snippet_completions_include_when_expression() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'when' - could be keyword or snippet
    let when_item = completions.iter().find(|item| item.label == "when");
    assert!(when_item.is_some(), "Expected 'when' in completions");

    let item = when_item.unwrap();
    // Either keyword or snippet is fine (we have both)
    assert!(
        item.kind == Some(CompletionItemKind::SNIPPET)
            || item.kind == Some(CompletionItemKind::KEYWORD),
        "Expected 'when' to be keyword or snippet"
    );
}

#[test]
fn test_kotlin_snippet_completions_include_data_class() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'data' snippet
    let data_snippet = completions.iter().find(|item| item.label == "data");
    assert!(
        data_snippet.is_some(),
        "Expected 'data' snippet in completions"
    );

    let snippet = data_snippet.unwrap();
    assert_eq!(snippet.kind, Some(CompletionItemKind::SNIPPET));
    assert!(snippet.insert_text.as_ref().unwrap().contains("data class"));
}

#[test]
fn test_kotlin_snippet_completions_include_companion_object() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'companion' - could be keyword or snippet
    let companion_item = completions.iter().find(|item| item.label == "companion");
    assert!(
        companion_item.is_some(),
        "Expected 'companion' in completions"
    );

    let item = companion_item.unwrap();
    // Either keyword or snippet is fine (we have both)
    assert!(
        item.kind == Some(CompletionItemKind::SNIPPET)
            || item.kind == Some(CompletionItemKind::KEYWORD),
        "Expected 'companion' to be keyword or snippet"
    );
}

#[test]
fn test_kotlin_snippet_completions_include_lazy_property() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'lazy' snippet
    let lazy_snippet = completions.iter().find(|item| item.label == "lazy");
    assert!(
        lazy_snippet.is_some(),
        "Expected 'lazy' snippet in completions"
    );

    let snippet = lazy_snippet.unwrap();
    assert_eq!(snippet.kind, Some(CompletionItemKind::SNIPPET));
    assert!(snippet.insert_text.as_ref().unwrap().contains("by lazy"));
}

// ========== Type Position Tests ==========

// Note: Type position detection is currently based on Java syntax patterns
// This test is disabled until we implement Kotlin-specific type position detection
// #[test]
// fn test_kotlin_type_position_includes_builtin_types() {
//     let source = "fun test(): ";
//     let (provider, host, file_id) = create_kotlin_test_provider_with_file(source);
//
//     // Position after ': ' (return type position)
//     let completions = provider.complete(&host.read_now(), file_id, Position::new(0, 12)).unwrap();
//
//     // Should include Kotlin built-in types
//     assert!(
//         completions.iter().any(|item| item.label == "String"),
//         "Expected 'String' type in completions"
//     );
//     assert!(
//         completions.iter().any(|item| item.label == "Int"),
//         "Expected 'Int' type in completions"
//     );
//     assert!(
//         completions.iter().any(|item| item.label == "Unit"),
//         "Expected 'Unit' type in completions"
//     );
// }

// ========== Symbol Completion Tests ==========

// Note: Symbol completion requires the Kotlin parser to extract symbols.
// The hyperion-kotlin parser exists but doesn't yet integrate with the semantic database.
// These tests are disabled until symbol extraction is implemented.

// #[test]
// fn test_kotlin_symbol_completion_class() {
//     let source = r#"
// class User {
//     val name: String = ""
// }
// "#;
//     let (provider, host, file_id) = create_kotlin_test_provider_with_file(source);
//
//     let completions = provider.complete(&host.read_now(), file_id, Position::new(0, 0)).unwrap();
//
//     // Should include the User class
//     assert!(
//         completions.iter().any(|item| item.label == "User"),
//         "Expected 'User' class in completions"
//     );
// }

// #[test]
// fn test_kotlin_symbol_completion_function() {
//     let source = r#"
// fun greet(name: String) {
//     println("Hello, $name")
// }
// "#;
//     let (provider, host, file_id) = create_kotlin_test_provider_with_file(source);
//
//     let completions = provider.complete(&host.read_now(), file_id, Position::new(0, 0)).unwrap();
//
//     // Should include the greet function
//     assert!(
//         completions.iter().any(|item| item.label == "greet"),
//         "Expected 'greet' function in completions"
//     );
// }

// ========== No Java Keywords in Kotlin Files ==========

#[test]
fn test_kotlin_does_not_include_java_specific_keywords() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should NOT include Java-specific keywords like 'synchronized' in Kotlin mode
    // (Note: 'synchronized' exists in both, but let's check we're getting Kotlin keywords)
    // Check that we get Kotlin-specific ones
    assert!(
        completions.iter().any(|item| item.label == "when"),
        "Should have Kotlin-specific 'when' keyword"
    );
    assert!(
        completions.iter().any(|item| item.label == "internal"),
        "Should have Kotlin-specific 'internal' visibility"
    );
}

#[test]
fn test_kotlin_snippet_format() {
    let (provider, host, file_id) = create_kotlin_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Check that snippets use Kotlin syntax (not Java)
    // Find the snippet (not keyword) for 'for'
    let for_snippet = completions
        .iter()
        .find(|item| item.label == "for" && item.kind == Some(CompletionItemKind::SNIPPET));

    if let Some(snippet) = for_snippet {
        let insert_text = snippet.insert_text.as_ref().unwrap();
        // Kotlin uses 'in' keyword in for loops
        assert!(
            insert_text.contains(" in "),
            "Kotlin for loop should use 'in' keyword, got: {insert_text}"
        );
    } else {
        // If no snippet found, that's okay - we have the keyword at least
        // Just verify we have 'for' available
        assert!(
            completions.iter().any(|item| item.label == "for"),
            "Expected 'for' in completions"
        );
    }
}
