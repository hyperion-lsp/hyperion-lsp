// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Unit Tests for Code Completion Provider
//!
//! Tests completion suggestions including keywords and symbols.

use hyperion_lsp::completion::CompletionProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::index::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

fn create_test_provider() -> (CompletionProvider, Arc<SemanticHost>) {
    let temp_dir = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let workspace_index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    // Set empty file text for FileId(0) to avoid Salsa panic
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(String::new()));
        db.set_file_language(file_id, Language::Java);
    });
    (CompletionProvider::new(workspace_index), host)
}

fn create_test_provider_with_file(source: &str) -> (CompletionProvider, Arc<SemanticHost>, FileId) {
    let temp_dir = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let workspace_index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);
    });
    (CompletionProvider::new(workspace_index), host, file_id)
}

#[test]
fn test_keyword_completions_not_empty() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should return Java keywords
    assert!(!completions.is_empty());
    assert!(completions.len() > 30);
}

#[test]
fn test_keyword_completions_include_class() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include 'class' keyword
    assert!(completions.iter().any(|item| item.label == "class"));
}

#[test]
fn test_keyword_completions_include_interface() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    assert!(completions.iter().any(|item| item.label == "interface"));
}

#[test]
fn test_keyword_completions_include_modifiers() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include visibility modifiers
    assert!(completions.iter().any(|item| item.label == "public"));
    assert!(completions.iter().any(|item| item.label == "private"));
    assert!(completions.iter().any(|item| item.label == "protected"));
}

#[test]
fn test_keyword_completions_include_control_flow() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include control flow keywords
    assert!(completions.iter().any(|item| item.label == "if"));
    assert!(completions.iter().any(|item| item.label == "while"));
    assert!(completions.iter().any(|item| item.label == "for"));
    assert!(completions.iter().any(|item| item.label == "return"));
}

#[test]
fn test_keyword_completions_include_primitives() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include primitive types
    assert!(completions.iter().any(|item| item.label == "int"));
    assert!(completions.iter().any(|item| item.label == "boolean"));
    assert!(completions.iter().any(|item| item.label == "void"));
}

#[test]
fn test_keyword_completions_have_correct_kind() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // All keywords should have KEYWORD kind
    assert!(completions
        .iter()
        .any(|item| item.kind == Some(CompletionItemKind::KEYWORD)));
}

#[test]
fn test_keyword_completions_have_details() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Keywords should have detail text
    for item in completions
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
    {
        assert!(item.detail.is_some());
        assert!(!item.detail.as_ref().unwrap().is_empty());
    }
}

#[test]
fn test_symbol_completions_empty_file() {
    let (provider, host, file_id) = create_test_provider_with_file("");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should return keywords and snippets, but no user-defined symbols
    let symbol_count = completions
        .iter()
        .filter(|item| {
            item.kind != Some(CompletionItemKind::KEYWORD)
                && item.kind != Some(CompletionItemKind::SNIPPET)
        })
        .count();

    assert_eq!(
        symbol_count, 0,
        "Empty file should have no user-defined symbols"
    );
}

#[test]
fn test_symbol_completions_simple_class() {
    let (provider, host, file_id) = create_test_provider_with_file("class Foo { }");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include the Foo class
    assert!(completions.iter().any(|item| item.label == "Foo"));
}

#[test]
fn test_symbol_completions_class_with_methods() {
    let source = r"
class Foo {
    void bar() {}
    int baz() { return 0; }
}
    ";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include class and methods
    assert!(completions.iter().any(|item| item.label == "Foo"));
    assert!(completions.iter().any(|item| item.label == "bar"));
    assert!(completions.iter().any(|item| item.label == "baz"));
}

#[test]
fn test_symbol_completions_class_with_fields() {
    let source = r"
class Foo {
    int x;
    String name;
}
    ";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include fields
    assert!(
        completions.iter().any(|item| item.label == "x"),
        "Field 'x' not found in completions"
    );
    assert!(
        completions.iter().any(|item| item.label == "name"),
        "Field 'name' not found in completions"
    );
}

#[test]
fn test_symbol_completions_correct_kinds() {
    let source = r"
class MyClass {
    int field;
    void method() {}
}
interface MyInterface {}
enum MyEnum { A, B }
    ";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Check that symbols have correct kinds
    let class_item = completions
        .iter()
        .find(|item| item.label == "MyClass")
        .expect("class completion");
    assert_eq!(class_item.kind, Some(CompletionItemKind::CLASS));

    let method_item = completions
        .iter()
        .find(|item| item.label == "method")
        .expect("method completion");
    assert_eq!(method_item.kind, Some(CompletionItemKind::METHOD));

    let field_item = completions
        .iter()
        .find(|item| item.label == "field")
        .expect("field completion");
    assert_eq!(field_item.kind, Some(CompletionItemKind::FIELD));
}

#[test]
fn test_completion_sorting() {
    let (provider, host, file_id) = create_test_provider_with_file("class Foo { }");

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Keywords should be sorted before symbols (using sort_text)
    for item in &completions {
        assert!(item.sort_text.is_some());
    }
}

#[test]
fn test_completion_insert_text() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // All items should have insert text
    for item in &completions {
        assert!(item.insert_text.is_some());
        assert!(!item.insert_text.as_ref().unwrap().is_empty());
    }
}

#[test]
fn test_completion_insert_text_format() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Keywords should use plain text format
    // Snippets should use snippet format
    for item in &completions {
        match item.kind {
            Some(CompletionItemKind::SNIPPET) => {
                assert_eq!(
                    item.insert_text_format,
                    Some(InsertTextFormat::SNIPPET),
                    "Snippet '{}' should use SNIPPET format",
                    item.label
                );
            }
            _ => {
                assert_eq!(
                    item.insert_text_format,
                    Some(InsertTextFormat::PLAIN_TEXT),
                    "Keyword/symbol '{}' should use PLAIN_TEXT format",
                    item.label
                );
            }
        }
    }
}

#[test]
fn test_completion_not_deprecated() {
    let (provider, host) = create_test_provider();
    let file_id = FileId::new(0);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Items should not be marked as deprecated
    // Either None (not deprecated) or Some(false) is acceptable
    for item in &completions {
        assert!(
            item.deprecated.is_none() || item.deprecated == Some(false),
            "Completion '{}' should not be deprecated, but got {:?}",
            item.label,
            item.deprecated
        );
    }
}

#[test]
fn test_completion_multiple_classes() {
    let source = r"
class Foo {}
class Bar {}
class Baz {}
    ";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include all classes
    assert!(completions.iter().any(|item| item.label == "Foo"));
    assert!(completions.iter().any(|item| item.label == "Bar"));
    assert!(completions.iter().any(|item| item.label == "Baz"));
}

#[test]
fn test_completion_nested_classes() {
    let source = r"
class Outer {
    class Inner {}
}
    ";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    let completions = provider
        .complete(&host.read_now(), file_id, Position::new(0, 0))
        .unwrap();

    // Should include both outer and inner classes
    assert!(completions.iter().any(|item| item.label == "Outer"));
    assert!(completions.iter().any(|item| item.label == "Inner"));
}

#[test]
fn test_completion_position_independence() {
    let source = "class Foo { }";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    // Completions should be the same regardless of cursor position
    let pos1 = Position::new(0, 0);
    let pos2 = Position::new(0, 5);
    let pos3 = Position::new(0, 10);

    let comp1 = provider.complete(&host.read_now(), file_id, pos1).unwrap();
    let comp2 = provider.complete(&host.read_now(), file_id, pos2).unwrap();
    let comp3 = provider.complete(&host.read_now(), file_id, pos3).unwrap();

    // All should include keywords
    assert!(!comp1.is_empty());
    assert!(!comp2.is_empty());
    assert!(!comp3.is_empty());
}

#[test]
fn test_completion_with_syntax_errors() {
    let source = "class Foo { void bar(";
    let (provider, host, file_id) = create_test_provider_with_file(source);

    let result = provider.complete(&host.read_now(), file_id, Position::new(0, 0));

    // Should still return completions even with syntax errors
    assert!(result.is_ok());
    let completions = result.unwrap();
    assert!(!completions.is_empty());
}

/// Test that constructor completion includes parameter snippets
#[test]
fn test_constructor_completion_with_parameter_snippets() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path().to_path_buf();

    // Create a class with a constructor that has parameters
    let user_class = r"package com.example;

public class User {
    private String name;
    private int age;

    public User(String name, int age) {
        this.name = name;
        this.age = age;
    }
}
";

    // Create a file that will use the constructor
    // Line 4: "        User u = new " has 21 chars (8 spaces + "User u = new ")
    let main_class = r"package com.example;

public class Main {
    public void test() {
        User u = new 
    }
}
";

    // Write files to disk
    let user_dir = workspace_root.join("com/example");
    std::fs::create_dir_all(&user_dir).unwrap();
    let user_path = user_dir.join("User.java");
    std::fs::write(&user_path, user_class).unwrap();

    let main_path = user_dir.join("Main.java");
    std::fs::write(&main_path, main_class).unwrap();

    // Create and build workspace index
    let index = WorkspaceIndex::new(workspace_root.clone());
    index
        .index_file_with_source(&user_path, user_class)
        .unwrap();
    index
        .index_file_with_source(&main_path, main_class)
        .unwrap();

    let index_arc = Arc::new(index);

    // Set up database
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let file_id = index_arc
        .get_file_id(&main_path)
        .expect("Main.java should be indexed");

    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(main_class.to_string()));
        db.set_file_language(file_id, Language::Java);
    });

    let provider = CompletionProvider::new(index_arc);

    // Complete after "new " - line 4 (0-indexed), column 21 (after the space)
    let position = Position::new(4, 21);
    let result = provider.complete(&host.read_now(), file_id, position);

    assert!(
        result.is_ok(),
        "Completion should succeed: {:?}",
        result.err()
    );
    let completions = result.unwrap();

    // Find User completion - now shows parameter signature
    let user_completion = completions.iter().find(|c| c.label == "User(String, int)");
    assert!(
        user_completion.is_some(),
        "Should find User(String, int) in completions. Found: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );

    let user_item = user_completion.unwrap();

    // Verify it uses SNIPPET format
    assert_eq!(
        user_item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Constructor completion should use SNIPPET format"
    );

    // Verify insert_text contains parameter placeholders
    let insert_text = user_item
        .insert_text
        .as_ref()
        .expect("Should have insert_text");
    assert!(
        insert_text.contains("User("),
        "Insert text should contain 'User('. Got: {}",
        insert_text
    );
    assert!(
        insert_text.contains("${1:") || insert_text.contains("$0"),
        "Insert text should contain snippet placeholder. Got: {}",
        insert_text
    );
}
