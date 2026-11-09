// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for Kotlin find-references functionality
//!
//! Tests the `textDocument/references` LSP capability for Kotlin files.

use crate::common_workspace::indexed_workspace as create_test_workspace;
use hyperion_lsp::navigation::NavigationProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::QueryEngine;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Position, Url};

/// Helper to find references at a specific position
fn find_refs_at_position(
    provider: &NavigationProvider,
    file_id: FileId,
    source_text: &str,
    file_path: &PathBuf,
    position: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let uri = Url::from_file_path(file_path).unwrap();
    provider
        .find_references(file_id, position, source_text, &uri, include_declaration)
        .expect("Kotlin find-references request must not fail")
}

fn starts(refs: &[Location]) -> Vec<Position> {
    refs.iter().map(|location| location.range.start).collect()
}

#[test]
fn test_find_class_references_same_file() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/User.kt",
        r#"package com.example

class User {
    val name: String = ""
}

fun main() {
    val user = User()
    val user2: User
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Kotlin
    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "User" class (at class declaration)
    // "class User {"
    //        ^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 6),
        false, // exclude declaration to count only usages
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(7, 15), Position::new(8, 15)]
    );
}

#[test]
fn test_find_function_references_same_file() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Test.kt",
        r#"package com.example

fun greet(name: String) {
    println("Hello, $name")
}

fun main() {
    greet("Alice")
    greet("Bob")
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "greet" function
    // "fun greet("
    //      ^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 4),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(7, 4), Position::new(8, 4)]
    );
}

#[test]
fn test_find_property_references_same_file() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/User.kt",
        r#"package com.example

class User {
    val name: String = "John"

    fun printName() {
        println(name)
    }

    fun upperName(): String {
        return name.uppercase()
    }
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "name" property
    // "    val name: String = "John""
    //          ^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 8),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(6, 16), Position::new(10, 15)]
    );
}

#[test]
fn test_find_local_variable_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Test.kt",
        r"package com.example

fun process() {
    val count = 0
    val result = count + 1
    println(count)
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "count" local variable
    // "    val count = 0"
    //          ^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 8),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(4, 17), Position::new(5, 12)]
    );
}

#[test]
fn test_find_data_class_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Model.kt",
        r#"package com.example

data class Person(val name: String, val age: Int)

fun main() {
    val p1 = Person("Alice", 30)
    val p2 = Person("Bob", 25)
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "Person" data class
    // "data class Person("
    //             ^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 11),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(5, 13), Position::new(6, 13)]
    );
}

#[test]
fn test_find_object_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Config.kt",
        r#"package com.example

object Config {
    const val API_KEY = "secret"
}

fun main() {
    println(Config.API_KEY)
    val c = Config
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "Config" object
    // "object Config {"
    //         ^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 7),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(7, 12), Position::new(8, 12)]
    );
}

#[test]
fn test_find_interface_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Repository.kt",
        r"package com.example

interface Repository {
    fun save()
}

class UserRepository : Repository {
    override fun save() {}
}

fun test(repo: Repository) {
    repo.save()
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "Repository" interface
    // "interface Repository {"
    //            ^^^^^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 10),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(6, 23), Position::new(10, 15)]
    );
}

#[test]
fn test_find_top_level_function_cross_file() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Utils.kt",
            r"package com.example

fun formatName(name: String): String {
    return name.uppercase()
}
",
        ),
        (
            "com/example/Main.kt",
            r#"package com.example

fun main() {
    val result = formatName("alice")
    println(formatName("bob"))
}
"#,
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (utils_file_path, utils_file_id) = &files[0];
    let utils_source = std::fs::read_to_string(utils_file_path).unwrap();

    // Find references to "formatName" function from Utils.kt
    // "fun formatName("
    //      ^^^^^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *utils_file_id,
        &utils_source,
        utils_file_path,
        Position::new(2, 4),
        false,
    );

    let main_uri = Url::from_file_path(&files[1].0).unwrap();
    assert_eq!(
        refs.iter()
            .map(|location| (location.uri.clone(), location.range.start))
            .collect::<Vec<_>>(),
        vec![
            (main_uri.clone(), Position::new(3, 17)),
            (main_uri, Position::new(4, 12)),
        ]
    );
}

#[test]
fn test_find_class_references_cross_file() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/model/User.kt",
            r#"package com.example.model

class User {
    val name: String = ""
}
"#,
        ),
        (
            "com/example/Main.kt",
            r"package com.example

import com.example.model.User

fun main() {
    val user = User()
    val user2: User
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (user_file_path, user_file_id) = &files[0];
    let user_source = std::fs::read_to_string(user_file_path).unwrap();

    // Find references to "User" class from User.kt
    // "class User {"
    //        ^^^^
    let refs = find_refs_at_position(
        &provider,
        *user_file_id,
        &user_source,
        user_file_path,
        Position::new(2, 6),
        false,
    );

    let main_uri = Url::from_file_path(&files[1].0).unwrap();
    assert_eq!(
        refs.iter()
            .map(|location| (location.uri.clone(), location.range.start))
            .collect::<Vec<_>>(),
        vec![
            (main_uri.clone(), Position::new(5, 15)),
            (main_uri, Position::new(6, 15)),
        ]
    );
}

#[test]
fn test_find_class_references_prefers_imported_package() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/a/UserA.kt",
            r"package com.a

class User {
    val id: Int = 1
}
",
        ),
        (
            "com/b/UserB.kt",
            r"package com.b

class User {
    val id: Int = 2
}
",
        ),
        (
            "com/example/Main.kt",
            r"package com.example

import com.b.User

fun main() {
    val user: User = User()
    println(user.id)
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (main_file_path, main_file_id) = files
        .iter()
        .find(|(path, _)| path.to_string_lossy().ends_with("com/example/Main.kt"))
        .expect("Main.kt should exist");
    let main_source = std::fs::read_to_string(main_file_path).unwrap();

    // Cursor on type usage: `val user: User = User()`
    let refs = find_refs_at_position(
        &provider,
        *main_file_id,
        &main_source,
        main_file_path,
        Position::new(5, 14),
        true,
    );

    let has_b_definition = refs.iter().any(|loc| {
        loc.uri
            .to_file_path()
            .is_ok_and(|p| p.to_string_lossy().ends_with("com/b/UserB.kt"))
    });
    assert!(
        has_b_definition,
        "Expected references to include imported com.b.User definition, got: {:?}",
        refs
    );

    let has_a_definition = refs.iter().any(|loc| {
        loc.uri
            .to_file_path()
            .is_ok_and(|p| p.to_string_lossy().ends_with("com/a/UserA.kt"))
    });
    assert!(
        !has_a_definition,
        "References should not include non-imported com.a.User definition: {:?}",
        refs
    );
}

#[test]
fn test_find_no_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Empty.kt",
        r"package com.example

class Empty {
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Try to find references to "Empty" class - should find very few since it's not used
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 6),
        false, // exclude declaration
    );

    assert!(refs.is_empty(), "unused class should have no references");
}

#[test]
fn test_find_parameter_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Test.kt",
        r"package com.example

fun process(input: String) {
    println(input)
    val upper = input.uppercase()
    if (input.isEmpty()) {
        return
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "input" parameter
    // "fun process(input: String) {"
    //              ^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 12),
        false,
    );

    assert_eq!(
        starts(&refs),
        vec![
            Position::new(3, 12),
            Position::new(4, 16),
            Position::new(5, 8),
        ]
    );
}

#[test]
fn test_find_companion_object_member() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Factory.kt",
        r"package com.example

class Factory {
    companion object {
        fun create(): Factory {
            return Factory()
        }
    }
}

fun main() {
    val f = Factory.create()
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (file_path, file_id) = &files[0];
    let source = std::fs::read_to_string(file_path).unwrap();

    // Find references to "create" companion object function
    // "        fun create(): Factory {"
    //              ^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(4, 12),
        false,
    );

    assert_eq!(starts(&refs), vec![Position::new(11, 20)]);
}

#[test]
fn unresolved_receiver_does_not_borrow_a_unique_workspace_method_identity() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "unrelated/Worker.kt",
            "package unrelated\nclass Worker { fun run() {} }",
        ),
        (
            "app/Main.kt",
            "package app\n\nfun caller() {\n    missing.run()\n}\n",
        ),
    ]);
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Kotlin);
        });
    }
    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(QueryEngine::new(index.as_ref().clone())),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let (main_path, main_id) = files
        .iter()
        .find(|(path, _)| path.ends_with("Main.kt"))
        .unwrap();
    let source = std::fs::read_to_string(main_path).unwrap();

    let refs = find_refs_at_position(
        &provider,
        *main_id,
        &source,
        main_path,
        Position::new(3, 12),
        true,
    );
    assert!(
        refs.is_empty(),
        "unresolved member references must fail closed"
    );
}
