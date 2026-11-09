// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Indexing diagnostics intentionally retain Debug formatting for escaped paths and quotes.
#![allow(clippy::unnecessary_debug_formatting)]

//! Tests for cursor position accuracy in LSP navigation features
//!
//! These tests verify that goto-definition and find-references return results
//! that point to the IDENTIFIER token, not to keywords like "class", "public", etc.
//!
//! This is critical for good UX - when jumping to a definition, the cursor should
//! land on the symbol name, not on a keyword before it.

use crate::common_workspace::indexed_workspace as create_test_workspace;
use hyperion_lsp::navigation::NavigationProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::SourceDatabase;
use hyperion_workspace::QueryEngine;
use std::sync::Arc;
use tower_lsp::lsp_types::{Position, Url};

/// Helper to find the character at a specific position in source text
fn get_char_at_position(source: &str, position: Position) -> Option<char> {
    let lines: Vec<&str> = source.lines().collect();
    if position.line as usize >= lines.len() {
        return None;
    }

    let line = lines[position.line as usize];
    let chars: Vec<char> = line.chars().collect();

    if position.character as usize >= chars.len() {
        return None;
    }

    Some(chars[position.character as usize])
}

/// Helper to get the word at a specific position in source text
fn get_word_at_position(source: &str, position: Position) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    if position.line as usize >= lines.len() {
        return None;
    }

    let line = lines[position.line as usize];
    let chars: Vec<char> = line.chars().collect();

    if position.character as usize >= chars.len() {
        return None;
    }

    // Find the start of the word
    let mut start = position.character as usize;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }

    // Find the end of the word
    let mut end = position.character as usize;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }

    if start < end {
        Some(chars[start..end].iter().collect())
    } else {
        None
    }
}

#[test]
fn test_goto_definition_cursor_on_class_name() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.User;

public class Main {
    public void test() {
        User user = new User();
    }
}
",
        ),
        (
            "com/example/model/User.java",
            r"package com.example.model;

public class User {
    private String name;
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents and language in database
    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = &files[0];
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    // Go to definition from "User" usage in Main.java
    // "User user = new User();"
    //  ^^^^
    let main_uri = Url::from_file_path(main_file_path).unwrap();
    let location = provider
        .goto_definition(*main_file_id, Position::new(6, 8), &main_source, &main_uri)
        .expect("goto-definition request should succeed")
        .expect("User definition should be found");

    // Read the target file
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();

    // The cursor should be on the identifier "User", not on "class" or "public"
    let cursor_char = get_char_at_position(&target_source, location.range.start);
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    println!(
        "Target position: line={}, char={}",
        location.range.start.line, location.range.start.character
    );
    println!("Character at cursor: {:?}", cursor_char);
    println!("Word at cursor: {:?}", word_at_cursor);

    // The character should be alphanumeric (start of identifier), not a space or keyword
    assert!(
        cursor_char.is_some() && cursor_char.unwrap().is_alphanumeric(),
        "Cursor should be on an alphanumeric character (identifier), not on whitespace or keyword. Found: {:?}",
        cursor_char
    );

    // The word at cursor should be "User", not "class" or "public"
    assert!(
        word_at_cursor == Some("User".into()),
        "Cursor should be on identifier 'User', not on a keyword. Found: {:?}",
        word_at_cursor
    );
}

#[test]
fn test_goto_definition_cursor_on_method_name() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.User;

public class Main {
    public void test() {
        User user = new User();
        String name = user.getName();
    }
}
",
        ),
        (
            "com/example/model/User.java",
            r"package com.example.model;

public class User {
    private String name;

    public String getName() {
        return name;
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = &files[0];
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    // Go to definition from "getName" call
    // "user.getName()"
    //       ^^^^^^^^
    let main_uri = Url::from_file_path(main_file_path).unwrap();
    let location = provider
        .goto_definition(*main_file_id, Position::new(7, 27), &main_source, &main_uri)
        .expect("goto-definition request should succeed")
        .expect("getName definition should be found");
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    assert_eq!(
        word_at_cursor,
        Some("getName".into()),
        "cursor should land on the method identifier"
    );
}

#[test]
fn test_goto_definition_cursor_on_field_name() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/User.java",
        r"package com.example;

public class User {
    private String name;

    public void test() {
        System.out.println(name);
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Go to definition from "name" usage
    // "System.out.println(name);"
    //                     ^^^^
    let uri = Url::from_file_path(file_path).unwrap();
    let location = provider
        .goto_definition(*file_id, Position::new(6, 28), &source, &uri)
        .expect("goto-definition request should succeed")
        .expect("field definition should be found");
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    assert_eq!(
        word_at_cursor,
        Some("name".into()),
        "cursor should land on the field identifier"
    );
}

#[test]
fn test_find_references_cursor_on_identifier() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/User.java",
        r"package com.example;

public class User {
    private String name;

    public String getName() {
        return name;
    }

    public void setName(String newName) {
        this.name = newName;
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "name" field
    let uri = Url::from_file_path(file_path).unwrap();
    let refs = provider
        .find_references(*file_id, Position::new(3, 20), &source, &uri, true)
        .expect("find-references request should succeed");

    assert!(!refs.is_empty(), "Should find at least one reference");

    // Check each reference location
    for (idx, location) in refs.iter().enumerate() {
        let target_path = location.uri.to_file_path().unwrap();
        let target_source = std::fs::read_to_string(&target_path).unwrap();

        let cursor_char = get_char_at_position(&target_source, location.range.start);
        let word_at_cursor = get_word_at_position(&target_source, location.range.start);

        println!(
            "Reference {}: line={}, char={}",
            idx, location.range.start.line, location.range.start.character
        );
        println!("  Character: {:?}", cursor_char);
        println!("  Word: {:?}", word_at_cursor);

        // Each reference should point to an identifier, not keywords
        assert!(
            cursor_char.is_some()
                && (cursor_char.unwrap().is_alphanumeric() || cursor_char.unwrap() == '_'),
            "Reference {} cursor should be on identifier character, found: {:?}",
            idx,
            cursor_char
        );

        assert_eq!(
            word_at_cursor,
            Some("name".into()),
            "reference {idx} should point to the selected field"
        );
    }
}

#[test]
fn test_goto_definition_interface_cursor_on_name() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.service.UserService;

public class Main {
    private UserService service;
}
",
        ),
        (
            "com/example/service/UserService.java",
            r"package com.example.service;

public interface UserService {
    void saveUser(String name);
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = &files[0];
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    let main_uri = Url::from_file_path(main_file_path).unwrap();
    let location = provider
        .goto_definition(*main_file_id, Position::new(5, 12), &main_source, &main_uri)
        .expect("goto-definition request should succeed")
        .expect("interface definition should be found");
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    assert_eq!(word_at_cursor, Some("UserService".into()));
}

#[test]
fn test_goto_definition_enum_cursor_on_name() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.Status;

public class Main {
    private Status status;
}
",
        ),
        (
            "com/example/model/Status.java",
            r"package com.example.model;

public enum Status {
    ACTIVE, INACTIVE, PENDING
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = &files[0];
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    let main_uri = Url::from_file_path(main_file_path).unwrap();
    let location = provider
        .goto_definition(*main_file_id, Position::new(5, 12), &main_source, &main_uri)
        .expect("goto-definition request should succeed")
        .expect("enum definition should be found");
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    assert_eq!(word_at_cursor, Some("Status".into()));
}

#[test]
fn test_goto_definition_constructor_cursor_on_name() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.Person;

public class Main {
    public void test() {
        Person p = new Person();
    }
}
",
        ),
        (
            "com/example/model/Person.java",
            r"package com.example.model;

public class Person {
    public Person() {}
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = &files[0];
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    // Jump to constructor from "new Person()"
    let main_uri = Url::from_file_path(main_file_path).unwrap();
    let location = provider
        .goto_definition(*main_file_id, Position::new(6, 23), &main_source, &main_uri)
        .expect("goto-definition request should succeed")
        .expect("constructor definition should be found");
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    assert_eq!(
        word_at_cursor,
        Some("Person".into()),
        "cursor should land on the constructor identifier"
    );
}

#[test]
fn test_find_references_class_all_on_identifier() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/model/User.java",
            r"package com.example.model;

public class User {
    private String name;
}
",
        ),
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.User;

public class Main {
    private User user1;
    private User user2;

    public void test() {
        User user = new User();
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (user_file_path, user_file_id) = &files[0];
    let user_source = std::fs::read_to_string(user_file_path).unwrap();

    // Find all references to User class
    let uri = Url::from_file_path(user_file_path).unwrap();
    let refs = provider
        .find_references(
            *user_file_id,
            Position::new(2, 14),
            &user_source,
            &uri,
            true,
        )
        .expect("find-references request should succeed");

    println!("Found {} references to User class", refs.len());
    assert!(!refs.is_empty(), "User references should be found");

    // Every reference should point to an identifier
    for (idx, location) in refs.iter().enumerate() {
        let target_path = location.uri.to_file_path().unwrap();
        let target_source = std::fs::read_to_string(&target_path).unwrap();

        let cursor_char = get_char_at_position(&target_source, location.range.start);
        let word_at_cursor = get_word_at_position(&target_source, location.range.start);

        println!(
            "Reference {}: line={}, char={}, word={:?}",
            idx, location.range.start.line, location.range.start.character, word_at_cursor
        );

        assert_eq!(
            word_at_cursor,
            Some("User".into()),
            "reference {idx} should land on the class identifier"
        );

        // Should be on alphanumeric character
        assert!(
            cursor_char.is_some() && cursor_char.unwrap().is_alphanumeric(),
            "Reference {} should be on identifier character, found: {:?}",
            idx,
            cursor_char
        );
    }
}

#[test]
fn test_goto_definition_static_method_cursor_on_name() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.Utils;

public class Main {
    public void test() {
        Utils.helper();
    }
}
",
        ),
        (
            "com/example/Utils.java",
            r#"package com.example;

public class Utils {
    public static void helper() {
        System.out.println("Help");
    }
}
"#,
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        let language = hyperion_workspace::Language::from_path(path)
            .unwrap_or(hyperion_workspace::Language::Java);
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, language);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = &files[0];
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    let main_uri = Url::from_file_path(main_file_path).unwrap();
    let location = provider
        .goto_definition(*main_file_id, Position::new(6, 14), &main_source, &main_uri)
        .expect("goto-definition request should succeed")
        .expect("static method definition should be found");
    let target_path = location.uri.to_file_path().unwrap();
    let target_source = std::fs::read_to_string(&target_path).unwrap();
    let word_at_cursor = get_word_at_position(&target_source, location.range.start);

    assert_eq!(word_at_cursor, Some("helper".into()));
}
