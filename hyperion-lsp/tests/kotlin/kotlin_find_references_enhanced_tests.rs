// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Indexing diagnostics intentionally retain Debug formatting for escaped paths and quotes.
#![allow(clippy::unnecessary_debug_formatting)]

//! Comprehensive enhanced tests for Kotlin find-references (Phase 5)
//!
//! Tests workspace-wide + local scope reference finding with deduplication and ranking.

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
        .expect("enhanced Kotlin find-references request must not fail")
}

fn starts(refs: &[Location]) -> Vec<Position> {
    refs.iter().map(|location| location.range.start).collect()
}

// ============================================================================
// Task 5.5: Find local variable references (3 tests)
// ============================================================================

#[test]
fn test_local_var_declaration_and_usages() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Test.kt",
        r"fun process() {
    val count = 0
    val result = count + 1
    println(count)
    if (count > 0) {
        println(count * 2)
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

    // Find references to "count" at declaration (line 1, char 8)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(1, 8),
        true, // include declaration
    );

    assert_eq!(
        starts(&refs),
        vec![
            Position::new(1, 8),
            Position::new(2, 17),
            Position::new(3, 12),
            Position::new(4, 8),
            Position::new(5, 16),
        ]
    );

    let usages = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 12),
        false,
    );
    assert_eq!(
        starts(&usages),
        vec![
            Position::new(2, 17),
            Position::new(3, 12),
            Position::new(4, 8),
            Position::new(5, 16),
        ],
        "includeDeclaration=false must exclude the definition even when invoked on a use"
    );
}

#[test]
fn test_local_var_shadowing() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Shadow.kt",
        r"fun outer() {
    val x = 1
    println(x)  // outer x

    run {
        val x = 2  // shadows outer x
        println(x)  // inner x
    }

    println(x)  // outer x again
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

    // Find references to outer "x" (line 1, char 8)
    let outer_refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(1, 8),
        false, // exclude declaration to count only uses
    );

    assert_eq!(
        starts(&outer_refs),
        vec![Position::new(2, 12), Position::new(9, 12)],
        "references to the outer binding must exclude the shadowing declaration and its use"
    );
}

#[test]
fn test_lambda_parameter_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Lambda.kt",
        r"fun test() {
    val numbers = listOf(1, 2, 3)
    numbers.map { it * 2 }
    numbers.filter { it > 1 }
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

    // Find references to "numbers" (line 1, char 8)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(1, 8),
        true,
    );

    assert_eq!(
        starts(&refs),
        vec![
            Position::new(1, 8),
            Position::new(2, 4),
            Position::new(3, 4),
        ]
    );
}

// ============================================================================
// Task 5.6: Find function references (3 tests)
// ============================================================================

#[test]
fn test_function_calls_same_file() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Functions.kt",
        r"fun helper(): Int {
    return 42
}

fun main() {
    val x = helper()
    val y = helper() + helper()
    println(helper())
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

    // Find references to "helper" function (line 0, char 4)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(0, 4),
        false, // exclude declaration
    );

    assert_eq!(
        starts(&refs),
        vec![
            Position::new(5, 12),
            Position::new(6, 12),
            Position::new(6, 23),
            Position::new(7, 12),
        ]
    );
}

#[test]
fn test_function_calls_cross_file() {
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
    val result1 = formatName("alice")
    val result2 = formatName("bob")
    println(formatName("charlie"))
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

    let (utils_path, utils_id) = &files[0];
    let utils_source = std::fs::read_to_string(utils_path).unwrap();

    // Find references to "formatName" from Utils.kt
    let refs = find_refs_at_position(
        &provider,
        *utils_id,
        &utils_source,
        utils_path,
        Position::new(2, 4),
        true, // include declaration
    );

    assert_eq!(
        refs.len(),
        4,
        "expected the declaration and all three calls"
    );
    assert_eq!(
        refs.iter()
            .filter(|location| location.uri == Url::from_file_path(utils_path).unwrap())
            .count(),
        1
    );
    let main_uri = Url::from_file_path(&files[1].0).unwrap();
    assert_eq!(
        refs.iter()
            .filter(|location| location.uri == main_uri)
            .map(|location| location.range.start)
            .collect::<Vec<_>>(),
        vec![
            Position::new(3, 18),
            Position::new(4, 18),
            Position::new(5, 12),
        ]
    );
}

#[test]
fn test_overloaded_function_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Overload.kt",
        r#"fun process(x: Int): Int = x * 2
fun process(x: String): String = x.uppercase()

fun test() {
    val a = process(42)
    val b = process("hello")
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

    // Find references to first "process" function (Int version, line 0)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(0, 4),
        true,
    );

    assert_eq!(
        starts(&refs),
        vec![Position::new(0, 4), Position::new(4, 12)],
        "references to process(Int) must not include process(String)"
    );

    let string_refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(5, 12),
        true,
    );
    assert_eq!(
        starts(&string_refs),
        vec![Position::new(1, 4), Position::new(5, 12)],
        "resolving from a call must select the matching overload"
    );
}

#[test]
fn test_overloaded_function_references_cross_file() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/IntApi.kt",
            r#"package com.example

fun process(x: Int): Int = x * 2
"#,
        ),
        (
            "com/example/StringApi.kt",
            r#"package com.example

fun process(x: String): String = x.uppercase()
"#,
        ),
        (
            "com/example/Use.kt",
            r#"package com.example

fun use() {
    val number = process(42)
    val text = process("hello")
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

    let (api_path, api_id) = &files[0];
    let api_source = std::fs::read_to_string(api_path).unwrap();
    let string_api_uri = Url::from_file_path(&files[1].0).unwrap();
    let use_uri = Url::from_file_path(&files[2].0).unwrap();

    let int_refs = find_refs_at_position(
        &provider,
        *api_id,
        &api_source,
        api_path,
        Position::new(2, 4),
        true,
    );
    assert_eq!(
        int_refs,
        vec![
            Location::new(
                Url::from_file_path(api_path).unwrap(),
                tower_lsp::lsp_types::Range::new(Position::new(2, 4), Position::new(2, 11)),
            ),
            Location::new(
                use_uri.clone(),
                tower_lsp::lsp_types::Range::new(Position::new(3, 17), Position::new(3, 24)),
            ),
        ],
        "references to a cross-file process(Int) declaration must exclude process(String) and its call"
    );

    let (use_path, use_id) = &files[2];
    let use_source = std::fs::read_to_string(use_path).unwrap();
    let int_refs_from_call = find_refs_at_position(
        &provider,
        *use_id,
        &use_source,
        use_path,
        Position::new(3, 19),
        true,
    );
    assert_eq!(
        int_refs_from_call, int_refs,
        "resolving a cross-file Int call must recover the overload declaration that the shared FQN did not retain"
    );

    let string_refs = find_refs_at_position(
        &provider,
        *use_id,
        &use_source,
        use_path,
        Position::new(4, 17),
        true,
    );
    assert_eq!(
        string_refs,
        vec![
            Location::new(
                string_api_uri,
                tower_lsp::lsp_types::Range::new(Position::new(2, 4), Position::new(2, 11)),
            ),
            Location::new(
                use_uri,
                tower_lsp::lsp_types::Range::new(Position::new(4, 15), Position::new(4, 22)),
            ),
        ],
        "resolving a cross-file String call must select only process(String)"
    );
}

// ============================================================================
// Task 5.7: Find class references (3 tests)
// ============================================================================

#[test]
fn test_class_references_constructor_and_type() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/User.kt",
        r#"package com.example

class User(val name: String)

fun main() {
    val user1 = User("Alice")
    val user2: User = User("Bob")
    fun processUser(u: User) {}
    processUser(User("Charlie"))
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

    // Find references to "User" class (line 2, char 6)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 6),
        true,
    );

    assert_eq!(
        starts(&refs),
        vec![
            Position::new(2, 6),
            Position::new(5, 16),
            Position::new(6, 15),
            Position::new(6, 22),
            Position::new(7, 23),
            Position::new(8, 16),
        ]
    );
}

#[test]
fn test_class_references_cross_file_imports() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/model/Person.kt",
            r"package com.example.model

data class Person(val name: String, val age: Int)
",
        ),
        (
            "com/example/Service.kt",
            r"package com.example

import com.example.model.Person

class PersonService {
    fun createPerson(name: String): Person {
        return Person(name, 0)
    }

    fun processPeople(people: List<Person>) {
        people.forEach { p: Person ->
            println(p.name)
        }
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

    let (person_path, person_id) = &files[0];
    let person_source = std::fs::read_to_string(person_path).unwrap();

    // Find references to "Person" class from Person.kt
    let refs = find_refs_at_position(
        &provider,
        *person_id,
        &person_source,
        person_path,
        Position::new(2, 11),
        true,
    );

    assert_eq!(
        refs.len(),
        5,
        "expected the declaration and four type/call uses"
    );
    assert_eq!(
        refs.iter()
            .filter(|location| location.uri == Url::from_file_path(person_path).unwrap())
            .count(),
        1
    );
    assert_eq!(
        refs.iter()
            .filter(|location| location.uri == Url::from_file_path(&files[1].0).unwrap())
            .count(),
        4
    );
}

#[test]
fn test_sealed_class_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Result.kt",
        r"sealed class Result<out T> {
    data class Success<T>(val value: T) : Result<T>()
    data class Error(val message: String) : Result<Nothing>()
}

fun processResult(r: Result<String>) {
    when (r) {
        is Result.Success -> println(r.value)
        is Result.Error -> println(r.message)
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

    // Find references to "Result" sealed class (line 0, char 13)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(0, 13),
        true,
    );

    assert_eq!(
        refs.len(),
        6,
        "expected every Result declaration and type use"
    );
}

// ============================================================================
// Task 5.8: Complex scenarios (3 tests)
// ============================================================================

#[test]
fn test_property_references_across_scopes() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Properties.kt",
        r#"class Counter {
    var count = 0

    fun increment() {
        count++
    }

    fun reset() {
        count = 0
    }

    fun display() {
        println("Count: $count")
    }
}

fun test() {
    val c = Counter()
    c.count = 5
    println(c.count)
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

    // Find references to "count" property (line 1, char 8)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(1, 8),
        true,
    );

    let expected = vec![
        Position::new(1, 8),
        Position::new(4, 8),
        Position::new(8, 8),
        Position::new(12, 25),
        Position::new(18, 6),
        Position::new(19, 14),
    ];
    assert_eq!(starts(&refs), expected);

    let from_template = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(12, 26),
        true,
    );
    assert_eq!(starts(&from_template), expected);
}

#[test]
fn test_top_level_property_reference_in_cross_file_string_template() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Constants.kt",
            r#"package com.example

val appName: String = "Hyperion"
"#,
        ),
        (
            "com/example/Use.kt",
            r#"package com.example

fun banner(): String = "Welcome to $appName"
fun braced(): String = "Running ${appName} now"
fun shadowed(): String {
    val appName = "Local"
    return "Using $appName"
}
fun escaped(): String = "Not a reference: \$appName"
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

    let (constants_path, constants_id) = &files[0];
    let constants_source = std::fs::read_to_string(constants_path).unwrap();
    let use_uri = Url::from_file_path(&files[1].0).unwrap();
    let refs = find_refs_at_position(
        &provider,
        *constants_id,
        &constants_source,
        constants_path,
        Position::new(2, 5),
        true,
    );

    assert_eq!(
        refs,
        vec![
            Location::new(
                Url::from_file_path(constants_path).unwrap(),
                tower_lsp::lsp_types::Range::new(Position::new(2, 4), Position::new(2, 11)),
            ),
            Location::new(
                use_uri.clone(),
                tower_lsp::lsp_types::Range::new(Position::new(2, 36), Position::new(2, 43)),
            ),
            Location::new(
                use_uri,
                tower_lsp::lsp_types::Range::new(Position::new(3, 34), Position::new(3, 41)),
            ),
        ],
        "cross-file Kotlin templates must count both simple and braced property references"
    );
}

#[test]
fn test_extension_function_references() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "Extensions.kt",
        r#"fun String.isPalindrome(): Boolean {
    return this == this.reversed()
}

fun test() {
    val s1 = "radar"
    val result1 = s1.isPalindrome()

    val s2 = "hello"
    val result2 = s2.isPalindrome()
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

    // Find references to "isPalindrome" extension (line 0, char 12)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(0, 12),
        true,
    );

    assert_eq!(
        starts(&refs),
        vec![
            Position::new(0, 11),
            Position::new(6, 21),
            Position::new(9, 21),
        ]
    );
}

#[test]
fn test_companion_object_member_references() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Config.kt",
            r"package com.example

class Config {
    companion object {
        const val MAX_SIZE = 100
        fun getDefault(): Config = Config()
    }
}
",
        ),
        (
            "com/example/Main.kt",
            r#"package com.example

fun main() {
    val size = Config.MAX_SIZE
    val cfg1 = Config.getDefault()
    val cfg2 = Config.getDefault()

    if (size > Config.MAX_SIZE / 2) {
        println("Large")
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

    let (config_path, config_id) = &files[0];
    let config_source = std::fs::read_to_string(config_path).unwrap();

    // Find references to "MAX_SIZE" constant (line 4, char 19)
    let refs = find_refs_at_position(
        &provider,
        *config_id,
        &config_source,
        config_path,
        Position::new(4, 19),
        true,
    );

    assert_eq!(
        refs.len(),
        3,
        "expected the declaration and both cross-file reads"
    );
}
