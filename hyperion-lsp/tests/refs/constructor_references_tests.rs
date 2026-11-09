// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Indexing diagnostics intentionally retain Debug formatting for escaped paths and quotes.
#![allow(clippy::unnecessary_debug_formatting)]

//! Constructor References Tests - Phase 1: Argument Counting
//!
//! Tests the find references functionality for constructors with overloading support.
//! Phase 1 implementation uses argument count to distinguish between constructor overloads.
//!
//! Tests cover:
//! - No-arg constructor references
//! - Single-arg constructor references
//! - Multi-arg constructor references
//! - Multiple overloads in same class
//! - Cross-file constructor references
//! - Ensuring overload isolation (no-arg doesn't match single-arg, etc.)

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
        .expect("constructor find-references request must not fail")
}

#[test]
fn test_no_arg_constructor_references_isolated() {
    // Test that finding references to a no-arg constructor only returns calls to that constructor,
    // not calls to other overloaded constructors.
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int age;

    // No-arg constructor
    public Person() {
        this.name = "Unknown";
        this.age = 0;
    }

    // Single-arg constructor
    public Person(String name) {
        this.name = name;
        this.age = 0;
    }

    // Two-arg constructor
    public Person(String name, int age) {
        this.name = name;
        this.age = age;
    }

    public void createInstances() {
        Person p1 = new Person();           // Should be found
        Person p2 = new Person("Alice");    // Should NOT be found
        Person p3 = new Person("Bob", 25);  // Should NOT be found
    }
}
"#,
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database
    for (path, file_id) in &files {
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|db_guard| {
            db_guard.set_file_text(*file_id, Arc::new(content));
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to the no-arg constructor (line 8, "Person" in "public Person()")
    // Position is 0-indexed: line 7, character ~11
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(7, 11),
        true, // include declaration
    );

    println!("Found {} references to no-arg constructor", refs.len());
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + only `new Person()` call
    // NOT the single-arg or two-arg calls
    assert_eq!(refs.len(), 2, "expected one declaration and one exact call");
}

#[test]
fn test_single_arg_constructor_references_isolated() {
    // Test that finding references to a single-arg constructor only returns calls to that constructor.
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int age;

    public Person() {
        this.name = "Unknown";
        this.age = 0;
    }

    // Single-arg constructor
    public Person(String name) {
        this.name = name;
        this.age = 0;
    }

    public Person(String name, int age) {
        this.name = name;
        this.age = age;
    }

    public void createInstances() {
        Person p1 = new Person();           // Should NOT be found
        Person p2 = new Person("Alice");    // Should be found
        Person p3 = new Person("Bob", 25);  // Should NOT be found
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to the single-arg constructor (line 13, "Person" in "public Person(String name)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(12, 11),
        true,
    );

    println!("Found {} references to single-arg constructor", refs.len());
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + only `new Person("Alice")` call
    assert_eq!(refs.len(), 2, "expected one declaration and one exact call");
}

#[test]
fn test_multi_arg_constructor_references_isolated() {
    // Test that finding references to a multi-arg constructor only returns calls to that constructor.
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int age;

    public Person() {
        this.name = "Unknown";
        this.age = 0;
    }

    public Person(String name) {
        this.name = name;
        this.age = 0;
    }

    // Two-arg constructor
    public Person(String name, int age) {
        this.name = name;
        this.age = age;
    }

    public void createInstances() {
        Person p1 = new Person();           // Should NOT be found
        Person p2 = new Person("Alice");    // Should NOT be found
        Person p3 = new Person("Bob", 25);  // Should be found
        Person p4 = new Person("Carol", 30); // Should be found
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to the two-arg constructor (line 18, "Person" in "public Person(String name, int age)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(17, 11),
        true,
    );

    println!("Found {} references to two-arg constructor", refs.len());
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + two calls: `new Person("Bob", 25)` and `new Person("Carol", 30)`
    assert_eq!(
        refs.len(),
        3,
        "expected one declaration and two exact calls"
    );
}

#[test]
fn test_constructor_references_cross_file() {
    // Test that constructor references work across multiple files
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Person.java",
            r#"package com.example;

public class Person {
    private String name;

    public Person() {
        this.name = "Unknown";
    }

    public Person(String name) {
        this.name = name;
    }
}
"#,
        ),
        (
            "com/example/Main.java",
            r#"package com.example;

public class Main {
    public void test() {
        Person p1 = new Person();
        Person p2 = new Person("Alice");
        Person p3 = new Person("Bob");
    }
}
"#,
        ),
        (
            "com/example/Other.java",
            r#"package com.example;

public class Other {
    public Person createPerson() {
        return new Person("Charlie");
    }

    public Person createDefault() {
        return new Person();
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
            db_guard.set_file_language(*file_id, Language::Java);
        });
    }

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    // Test no-arg constructor references
    let (person_path, person_file_id) = &files[0];
    let source = std::fs::read_to_string(person_path).unwrap();

    // Find references to no-arg constructor (line 6, "Person" in "public Person()")
    let refs_no_arg = find_refs_at_position(
        &provider,
        *person_file_id,
        &source,
        person_path,
        Position::new(5, 11),
        true,
    );

    println!(
        "Found {} references to no-arg constructor (cross-file)",
        refs_no_arg.len()
    );
    for (i, location) in refs_no_arg.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + calls from Main.java:5 and Other.java:9
    assert_eq!(
        refs_no_arg.len(),
        3,
        "expected the declaration and both no-arg calls"
    );

    // Test single-arg constructor references
    // Find references to single-arg constructor (line 10, "Person" in "public Person(String name)")
    let refs_single_arg = find_refs_at_position(
        &provider,
        *person_file_id,
        &source,
        person_path,
        Position::new(9, 11),
        true,
    );

    println!(
        "Found {} references to single-arg constructor (cross-file)",
        refs_single_arg.len()
    );
    for (i, location) in refs_single_arg.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + calls from Main.java:6,7 and Other.java:5
    assert_eq!(
        refs_single_arg.len(),
        4,
        "expected the declaration and all three single-arg calls"
    );
}

// ===== Phase 2 Tests: Literal Type Inference =====
//
// These tests verify that constructor references distinguish between overloads
// based on inferred literal types, not just argument counts.

#[test]
fn test_phase2_string_literal_matching() {
    // Test that string literals are correctly inferred and matched
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int id;

    // Constructor with String parameter
    public Person(String name) {
        this.name = name;
    }

    // Constructor with int parameter
    public Person(int id) {
        this.id = id;
    }

    public void createInstances() {
        Person p1 = new Person("Alice");  // Should match String constructor
        Person p2 = new Person(123);      // Should NOT match String constructor
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to String constructor (line 8, "Person" in "public Person(String name)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(7, 11),
        true,
    );

    println!("Found {} references to String constructor", refs.len());
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + `new Person("Alice")` only
    // NOT the `new Person(123)` call
    assert_eq!(
        refs.len(),
        2,
        "expected the declaration and String call only"
    );
}

#[test]
fn test_phase2_int_literal_matching() {
    // Test that int literals are correctly inferred and matched
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int id;

    public Person(String name) {
        this.name = name;
    }

    // Constructor with int parameter
    public Person(int id) {
        this.id = id;
    }

    public void createInstances() {
        Person p1 = new Person("Alice");  // Should NOT match int constructor
        Person p2 = new Person(123);      // Should match int constructor
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to int constructor (line 12, "Person" in "public Person(int id)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(11, 11),
        true,
    );

    println!("Found {} references to int constructor", refs.len());
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + `new Person(123)` only
    assert_eq!(refs.len(), 2, "expected the declaration and int call only");
}

#[test]
fn test_phase2_multi_param_literal_matching() {
    // Test that multi-parameter constructors with different type signatures are distinguished
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int age;
    private int id;
    private String email;

    // Constructor(String, int)
    public Person(String name, int age) {
        this.name = name;
        this.age = age;
    }

    // Constructor(int, String)
    public Person(int id, String email) {
        this.id = id;
        this.email = email;
    }

    public void createInstances() {
        Person p1 = new Person("Alice", 25);  // Should match (String, int)
        Person p2 = new Person(123, "bob@example.com");  // Should NOT match (String, int)
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to (String, int) constructor (line 10, "Person" in "public Person(String name, int age)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(9, 11),
        true,
    );

    println!(
        "Found {} references to (String, int) constructor",
        refs.len()
    );
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + `new Person("Alice", 25)` only
    // NOT the `new Person(123, "bob@example.com")` call (wrong parameter order)
    assert_eq!(
        refs.len(),
        2,
        "expected the declaration and correctly ordered literal call only"
    );
}

#[test]
fn test_phase2_null_literal_handling() {
    // Test that null literals are handled correctly (match any reference type)
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r"package com.example;

public class Person {
    private String name;

    public Person(String name) {
        this.name = name;
    }

    public void createInstances() {
        Person p1 = new Person(null);  // Should match String constructor
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to String constructor (line 6, "Person" in "public Person(String name)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(5, 11),
        true,
    );

    println!(
        "Found {} references to String constructor (with null arg)",
        refs.len()
    );
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + `new Person(null)` call
    assert_eq!(refs.len(), 2, "null must resolve to the reference overload");
}

#[test]
fn test_phase2_mixed_literal_and_variable() {
    // Test that mixed literal and variable arguments work correctly
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int age;

    public Person(String name, int age) {
        this.name = name;
        this.age = age;
    }

    public void createInstances() {
        int x = 25;
        Person p1 = new Person("Alice", x);  // String literal, variable (should match)
        Person p2 = new Person("Bob", 30);   // Both literals (should match)
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to (String, int) constructor (line 7, "Person" in "public Person(String name, int age)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(6, 11),
        true,
    );

    println!(
        "Found {} references to (String, int) constructor with mixed args",
        refs.len()
    );
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + both constructor calls
    // (String literal, wildcard) matches (String, int) definition
    // (String literal, int literal) matches (String, int) definition
    assert_eq!(
        refs.len(),
        3,
        "expected the declaration and both valid calls"
    );
}

// ===== Phase 1 Tests (retained for backward compatibility) =====

#[test]
fn test_constructor_with_complex_arguments() {
    // Test constructor calls with complex argument expressions
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Person.java",
        r#"package com.example;

public class Person {
    private String name;
    private int age;

    public Person(String name, int age) {
        this.name = name;
        this.age = age;
    }

    public void createInstances() {
        // Complex arguments with method calls and nested expressions
        Person p1 = new Person("Alice".toUpperCase(), 25);
        Person p2 = new Person(getName(), getAge());
        Person p3 = new Person("Bob", Math.max(10, 20));
    }

    private String getName() { return "Test"; }
    private int getAge() { return 30; }
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
            db_guard.set_file_language(*file_id, Language::Java);
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

    // Find references to the two-arg constructor (line 7, "Person" in "public Person(String name, int age)")
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(6, 11),
        true,
    );

    println!(
        "Found {} references to constructor with complex args",
        refs.len()
    );
    for (i, location) in refs.iter().enumerate() {
        println!("  [{}] {:?} at {:?}", i, location.uri, location.range);
    }

    // Should find: declaration + 3 calls (all with 2 args each, despite complex expressions)
    assert_eq!(
        refs.len(),
        4,
        "expected the declaration and all three complex-argument calls"
    );
}
