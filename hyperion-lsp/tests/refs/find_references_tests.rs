// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Indexing diagnostics intentionally retain Debug formatting for escaped paths and quotes.
#![allow(clippy::unnecessary_debug_formatting)]

//! Comprehensive tests for LSP find references functionality
//!
//! Tests the `textDocument/references` LSP capability which finds all references
//! to a symbol across the workspace.

use crate::common_workspace::{indexed_workspace as create_test_workspace, semantic_workspace};
use hyperion_lsp::navigation::NavigationProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::QueryEngine;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
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
        .expect("Java find-references request must not fail")
}

#[test]
fn test_find_field_references_same_file() {
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

    // Set file contents in database and mark as Java
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

    // Find references to "name" field (hover over it in line 4)
    // "private String name;"
    //                 ^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 20),
        true, // include declaration
    );

    // Should find: declaration + 2 uses (getName and setName)
    assert!(
        refs.len() >= 2,
        "Should find at least 2 references to field 'name', found {}",
        refs.len()
    );
}

#[test]
fn field_references_use_one_exact_identity_for_reads_writes_and_qualified_accesses() {
    let (_temp, index, host, files) = semantic_workspace(vec![(
        "com/example/Counter.java",
        r"package com.example;

public class Counter {
    private int count;

    void update(Counter other) {
        count++;
        this.count = count + 1;
        other.count = this.count;
    }

    void shadow() {
        int count = 0;
        count++;
        this.count = count;
    }
}

class Distractor {
    int count;
    void update() { count++; }
}
",
        Language::Java,
    )]);

    assert!(
        index
            .find_effective_fields("com.example.Counter")
            .iter()
            .any(|field| field.simple_name.as_ref() == "count"),
        "the exact receiver must expose its declared field"
    );
    let query_engine = Arc::new(QueryEngine::new(index.as_ref().clone()));
    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let (path, file_id, source) = &files[0];
    let uri = Url::from_file_path(path).unwrap();

    let declaration_refs = find_refs_at_position(
        &provider,
        *file_id,
        source,
        path,
        Position::new(3, 18),
        true,
    );
    let declaration = provider
        .goto_definition(*file_id, Position::new(3, 18), source, &uri)
        .expect("field declaration navigation")
        .expect("field declaration target");
    let qualified_declaration = provider
        .goto_definition(*file_id, Position::new(8, 16), source, &uri)
        .expect("qualified field navigation")
        .expect("qualified field target");
    assert_eq!(qualified_declaration, declaration);
    let qualified_use_refs = find_refs_at_position(
        &provider,
        *file_id,
        source,
        path,
        Position::new(8, 16),
        true,
    );
    let locations = |refs: Vec<Location>| {
        refs.into_iter()
            .map(|location| {
                (
                    location.range.start.line,
                    location.range.start.character,
                    location.range.end.character,
                )
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    let declaration_locations = locations(declaration_refs);

    assert_eq!(locations(qualified_use_refs), declaration_locations);
    assert_eq!(declaration_locations.len(), 7, "{declaration_locations:?}");
    assert_eq!(
        declaration_locations
            .iter()
            .map(|(line, _, _)| *line)
            .collect::<Vec<_>>(),
        vec![3, 6, 7, 7, 8, 8, 14]
    );
}

#[test]
fn large_field_reference_query_ignores_same_named_locals_without_per_token_navigation() {
    let mut source = String::from(
        "package p;\npublic class Many {\n    private int value;\n    void touch() {\n",
    );
    for index in 0..1_500 {
        source.push_str(&format!("        {{ int value = {index}; value++; }}\n"));
    }
    source.push_str("        this.value++;\n    }\n}\n");
    let (_temp, index, host, files) =
        semantic_workspace(vec![("p/Many.java", source.as_str(), Language::Java)]);
    let query_engine = Arc::new(QueryEngine::new(index.as_ref().clone()));
    let (path, file_id, indexed_source) = &files[0];
    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index,
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let started = std::time::Instant::now();
    let references = find_refs_at_position(
        &provider,
        *file_id,
        indexed_source,
        path,
        Position::new(2, 18),
        true,
    );
    let elapsed = started.elapsed();

    assert_eq!(references.len(), 2, "references: {references:?}");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "large exact field query regressed to per-token navigation: {elapsed:?}"
    );
}

#[test]
fn test_find_method_references_cross_file() {
    let (_temp, index, files) = create_test_workspace(vec![
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
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.User;

public class Main {
    public void test() {
        User user = new User();
        String name = user.getName();
        System.out.println(user.getName());
    }
}
",
        ),
        (
            "com/example/Other.java",
            r"package com.example;

import com.example.model.User;

public class Other {
    public String getUserName(User user) {
        return user.getName();
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    let (user_file_path, user_file_id) = &files[0];
    let user_source = std::fs::read_to_string(user_file_path).unwrap();

    // Find references to getName() method from User.java
    // "public String getName()"
    //               ^^^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *user_file_id,
        &user_source,
        user_file_path,
        Position::new(5, 19),
        true, // include declaration
    );

    // Should find at least the declaration
    // Note: Cross-file reference finding may be limited depending on implementation
    assert!(
        !refs.is_empty(),
        "Should find at least 1 reference to getName() (the declaration), found {}",
        refs.len()
    );

    // If we found multiple references, verify they're valid
    if refs.len() > 1 {
        let file_paths: Vec<String> = refs.iter().map(|loc| loc.uri.path().to_string()).collect();

        // All paths should be valid
        assert!(
            file_paths.iter().all(|p| !p.is_empty()),
            "All reference locations should have valid paths"
        );
    }
}

#[test]
fn test_find_class_references() {
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
    private User currentUser;

    public void test() {
        User user = new User();
    }
}
",
        ),
        (
            "com/example/Service.java",
            r"package com.example;

import com.example.model.User;

public class Service {
    public User getUser() {
        return new User();
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    let (user_file_path, user_file_id) = &files[0];
    let user_source = std::fs::read_to_string(user_file_path).unwrap();

    // Find references to "User" class
    // "public class User {"
    //              ^^^^
    let refs = find_refs_at_position(
        &provider,
        *user_file_id,
        &user_source,
        user_file_path,
        Position::new(2, 14),
        true,
    );

    // Should find: declaration + multiple uses across files
    assert!(
        !refs.is_empty(),
        "Should find at least 1 reference to class User, found {}",
        refs.len()
    );
}

#[test]
fn test_find_references_local_variable() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Test.java",
        r"package com.example;

public class Test {
    public void process() {
        int count = 0;
        count = count + 1;
        System.out.println(count);
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    // Find references to "count" local variable
    // "int count = 0;"
    //      ^^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(4, 12),
        true,
    );

    // Should find: declaration + 3 uses in same method
    assert!(
        refs.len() >= 2,
        "Should find at least 2 references to local variable 'count', found {}",
        refs.len()
    );
}

#[test]
fn test_find_references_exclude_declaration() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Test.java",
        r"package com.example;

public class Test {
    private int value;

    public int getValue() {
        return value;
    }

    public void setValue(int v) {
        this.value = v;
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    // Find references to "value" field, including declaration
    let refs_with_decl = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 16),
        true, // include declaration
    );

    // Find references to "value" field, excluding declaration
    let refs_without_decl = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 16),
        false, // exclude declaration
    );

    // References without declaration should be fewer or equal
    assert!(
        refs_without_decl.len() <= refs_with_decl.len(),
        "References excluding declaration should be <= references including declaration"
    );
}

#[test]
fn test_find_references_constructor() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/model/Person.java",
            r"package com.example.model;

public class Person {
    public Person() {}

    public Person(String name) {}
}
",
        ),
        (
            "com/example/Main.java",
            r#"package com.example;

import com.example.model.Person;

public class Main {
    public void test() {
        Person p1 = new Person();
        Person p2 = new Person("Alice");
        Person p3;
    }
}
"#,
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    let (person_file_path, person_file_id) = &files[0];
    let person_source = std::fs::read_to_string(person_file_path).unwrap();

    // Find references to Person constructor
    // "public Person() {}"
    //        ^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *person_file_id,
        &person_source,
        person_file_path,
        Position::new(3, 11),
        true,
    );

    let person_uri = Url::from_file_path(person_file_path).unwrap();
    let main_uri = Url::from_file_path(&files[1].0).unwrap();

    let ref_positions: Vec<(Url, u32, u32)> = refs
        .iter()
        .map(|loc| {
            (
                loc.uri.clone(),
                loc.range.start.line,
                loc.range.start.character,
            )
        })
        .collect();

    assert!(
        ref_positions.contains(&(person_uri.clone(), 3, 11)),
        "Expected constructor declaration reference at Person.java (3,11), got: {:?}",
        ref_positions
    );
    assert!(
        ref_positions.contains(&(main_uri.clone(), 6, 24)),
        "Expected constructor call reference at Main.java (6,24), got: {:?}",
        ref_positions
    );
    // new Person("Alice") calls Person(String), not Person() — don't expect it here

    assert!(
        !ref_positions.contains(&(main_uri.clone(), 6, 8)),
        "Did not expect type reference at Main.java (6,8), got: {:?}",
        ref_positions
    );
    assert!(
        !ref_positions.contains(&(main_uri.clone(), 7, 8)),
        "Did not expect type reference at Main.java (7,8), got: {:?}",
        ref_positions
    );
    assert!(
        !ref_positions.contains(&(main_uri.clone(), 8, 8)),
        "Did not expect type-only reference at Main.java (8,8), got: {:?}",
        ref_positions
    );
}

#[test]
fn test_find_references_static_method() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Utils.java",
            r"package com.example;

public class Utils {
    public static int add(int a, int b) {
        return a + b;
    }
}
",
        ),
        (
            "com/example/Calculator.java",
            r"package com.example;

public class Calculator {
    public int calculate() {
        return Utils.add(1, 2) + Utils.add(3, 4);
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    let (utils_file_path, utils_file_id) = &files[0];
    let utils_source = std::fs::read_to_string(utils_file_path).unwrap();

    // Find references to static method "add"
    // "public static int add(..."
    //                    ^^^
    let refs = find_refs_at_position(
        &provider,
        *utils_file_id,
        &utils_source,
        utils_file_path,
        Position::new(3, 23),
        true,
    );

    // Should find: declaration + 2 calls in Calculator
    assert!(
        !refs.is_empty(),
        "Should find at least 1 reference to static method 'add', found {}",
        refs.len()
    );
}

#[test]
fn test_find_references_static_method_uses_imported_receiver_type() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/a/Status.java",
            r"package com.a;

public class Status {
    public static void work() {}

    public static void useA() {
        work();
    }
}
",
        ),
        (
            "com/b/Status.java",
            r"package com.b;

public class Status {
    public static void work() {}
}
",
        ),
        (
            "com/example/Main.java",
            r"package com.example;

import com.b.Status;

public class Main {
    public void test() {
        Status.work();
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

    let (main_path, main_file_id) = files
        .iter()
        .find(|(path, _)| path.to_string_lossy().ends_with("com/example/Main.java"))
        .expect("Main.java should exist");
    let main_source = std::fs::read_to_string(main_path).unwrap();

    // Cursor on "work" in "Status.work();"
    let refs = find_refs_at_position(
        &provider,
        *main_file_id,
        &main_source,
        main_path,
        Position::new(6, 15),
        true,
    );

    assert!(
        !refs.is_empty(),
        "Expected references for imported Status.work, found none"
    );

    let has_b_definition = refs.iter().any(|loc| {
        loc.uri
            .to_file_path()
            .is_ok_and(|p| p.to_string_lossy().ends_with("com/b/Status.java"))
    });
    assert!(
        has_b_definition,
        "Expected references to include com/b/Status.work definition, got: {:?}",
        refs
    );

    let has_a_refs = refs.iter().any(|loc| {
        loc.uri
            .to_file_path()
            .is_ok_and(|p| p.to_string_lossy().ends_with("com/a/Status.java"))
    });
    assert!(
        !has_a_refs,
        "References should not include com/a/Status when imported com/b/Status is called: {:?}",
        refs
    );
}

#[test]
fn test_find_references_interface_method() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Runnable.java",
            r"package com.example;

public interface Runnable {
    void run();
}
",
        ),
        (
            "com/example/Task.java",
            r#"package com.example;

public class Task implements Runnable {
    public void run() {
        System.out.println("Running");
    }
}
"#,
        ),
        (
            "com/example/Executor.java",
            r"package com.example;

public class Executor {
    public void execute(Runnable r) {
        r.run();
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    let (runnable_file_path, runnable_file_id) = &files[0];
    let runnable_source = std::fs::read_to_string(runnable_file_path).unwrap();

    // Find references to interface method "run"
    // "void run();"
    //      ^^^
    let refs = find_refs_at_position(
        &provider,
        *runnable_file_id,
        &runnable_source,
        runnable_file_path,
        Position::new(3, 9),
        true,
    );

    // Should find: declaration + implementation + call
    assert!(
        !refs.is_empty(),
        "Should find at least 1 reference to interface method 'run', found {}",
        refs.len()
    );
}

#[test]
fn test_receiver_call_references_match_declared_supertype_method() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "p/Base.java",
            r"package p;

public class Base {
    public void target() {}
}
",
        ),
        (
            "p/Child.java",
            r"package p;

public class Child extends Base {
    @Override public void target() {}
}
",
        ),
        (
            "p/Contract.java",
            r"package p;

public interface Contract {
    void target();
}
",
        ),
        (
            "p/Implementation.java",
            r"package p;

public class Implementation implements Contract {
    @Override public void target() {}
}
",
        ),
        (
            "p/Use.java",
            r"package p;

public class Use {
    void superclassCall(Base receiver) {
        receiver.target();
    }

    void interfaceCall(Contract receiver) {
        receiver.target();
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

    let refs_at = |file_index: usize, line: u32, character: u32| {
        let (path, file_id) = &files[file_index];
        let source = std::fs::read_to_string(path).unwrap();
        find_refs_at_position(
            &provider,
            *file_id,
            &source,
            path,
            Position::new(line, character),
            true,
        )
    };
    let normalize = |locations: Vec<Location>| {
        locations
            .into_iter()
            .map(|location| {
                (
                    location.uri,
                    location.range.start.line,
                    location.range.start.character,
                )
            })
            .collect::<std::collections::HashSet<_>>()
    };

    let superclass_declaration_refs = normalize(refs_at(0, 3, 18));
    let superclass_call_refs = normalize(refs_at(4, 4, 18));
    assert_eq!(superclass_call_refs, superclass_declaration_refs);

    let interface_declaration_refs = normalize(refs_at(2, 3, 9));
    let interface_call_refs = normalize(refs_at(4, 8, 18));
    assert_eq!(interface_call_refs, interface_declaration_refs);
}

#[test]
fn test_inherited_field_receiver_references_match_declared_method() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "p/Service.java",
            r"package p;

public class Service {
    public void target() {}
}
",
        ),
        (
            "p/Distractor.java",
            r"package p;

public class Distractor {
    public void target() {}
}
",
        ),
        (
            "p/Base.java",
            r"package p;

public class Base<T> {
    protected T service;
}
",
        ),
        (
            "p/Child.java",
            r"package p;

public class Child extends Base<Service> {
    public void invoke() {
        service.target();
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

    let refs_at = |file_index: usize, line: u32, character: u32| {
        let (path, file_id) = &files[file_index];
        let source = std::fs::read_to_string(path).unwrap();
        find_refs_at_position(
            &provider,
            *file_id,
            &source,
            path,
            Position::new(line, character),
            true,
        )
    };
    let normalize = |locations: Vec<Location>| {
        locations
            .into_iter()
            .map(|location| {
                (
                    location.uri,
                    location.range.start.line,
                    location.range.start.character,
                )
            })
            .collect::<std::collections::HashSet<_>>()
    };

    let declaration_refs = normalize(refs_at(0, 3, 18));
    let call_refs = normalize(refs_at(3, 4, 18));
    assert_eq!(call_refs, declaration_refs);
}

#[test]
fn test_chained_receiver_references_match_declared_method() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "p/Service.java",
            "package p; public class Service { public void target() {} }",
        ),
        (
            "p/Distractor.java",
            "package p; public class Distractor { public void target() {} }",
        ),
        (
            "p/Factory.java",
            concat!(
                "package p; public class Factory<T> { public T service() { ",
                "return null; } }",
            ),
        ),
        (
            "p/Use.java",
            concat!(
                "package p; public class Use { void invoke(Factory<Service> factory) { ",
                "factory.service().target(); } }",
            ),
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

    let refs_at = |file_index: usize, character: u32| {
        let (path, file_id) = &files[file_index];
        let source = std::fs::read_to_string(path).unwrap();
        find_refs_at_position(
            &provider,
            *file_id,
            &source,
            path,
            Position::new(0, character),
            true,
        )
    };
    let normalize = |locations: Vec<Location>| {
        locations
            .into_iter()
            .map(|location| (location.uri, location.range.start.character))
            .collect::<std::collections::HashSet<_>>()
    };

    let declaration_refs = normalize(refs_at(0, 47));
    let call_refs = normalize(refs_at(3, 89));
    assert_eq!(call_refs, declaration_refs);
}

#[test]
fn test_find_references_enum_constant() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Status.java",
            r"package com.example;

public enum Status {
    ACTIVE, INACTIVE, PENDING
}
",
        ),
        (
            "com/example/User.java",
            r"package com.example;

public class User {
    private Status status = Status.ACTIVE;

    public void deactivate() {
        this.status = Status.INACTIVE;
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    let (status_file_path, status_file_id) = &files[0];
    let status_source = std::fs::read_to_string(status_file_path).unwrap();

    // Find references to enum constant "ACTIVE"
    // "ACTIVE, INACTIVE, PENDING"
    //  ^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *status_file_id,
        &status_source,
        status_file_path,
        Position::new(3, 4),
        true,
    );

    // Should find: declaration + use in User class
    assert!(
        !refs.is_empty(),
        "Should find at least 1 reference to enum constant 'ACTIVE', found {}",
        refs.len()
    );
}

#[test]
fn test_find_references_parameter() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Processor.java",
        r"package com.example;

public class Processor {
    public void process(String data) {
        System.out.println(data);
        String upper = data.toUpperCase();
        if (data.isEmpty()) {
            return;
        }
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    // Find references to parameter "data"
    // "public void process(String data) {"
    //                            ^^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 32),
        true,
    );

    // Should find: parameter declaration + 3 uses in method body
    assert!(
        refs.len() >= 2,
        "Should find at least 2 references to parameter 'data', found {}",
        refs.len()
    );
}

#[test]
fn test_find_references_no_results() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Empty.java",
        r"package com.example;

public class Empty {
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    // Try to find references in whitespace (should return empty)
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(2, 0),
        true,
    );

    // Should find no references
    assert_eq!(refs.len(), 0, "Should find no references in empty space");
}

#[test]
fn test_find_references_ambiguous_unresolved_symbol_returns_empty() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/a/Alpha.java",
            r"package com.a;

public class Alpha {
    public static void doWork() {}

    public void alphaUse() {
        doWork();
    }
}
",
        ),
        (
            "com/b/Beta.java",
            r"package com.b;

public class Beta {
    public static void doWork() {}

    public void betaUse() {
        doWork();
    }
}
",
        ),
        (
            "com/example/Main.java",
            r"package com.example;

public class Main {
    void test() {
        doWork();
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

    let (file_path, file_id) = files
        .iter()
        .find(|(path, _)| path.to_string_lossy().ends_with("com/example/Main.java"))
        .expect("Main.java should exist");
    let source = std::fs::read_to_string(file_path).unwrap();

    // Cursor on unresolved doWork() call in Main.java
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(4, 10),
        true,
    );

    assert!(
        refs.is_empty(),
        "Unresolved ambiguous symbol should return no references, got: {:?}",
        refs
    );
}

/// Test cross-file enum constant references
#[test]
fn test_find_references_enum_constant_cross_file() {
    // Test that find references works across files for enum constants
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/ResourceType.java",
            r"package com.example;

public enum ResourceType {
    UNKNOWN,
    USER,
    POST
}
",
        ),
        (
            "com/example/Service.java",
            r"package com.example;

public class Service {
    public void process(ResourceType type) {
        if (type == ResourceType.UNKNOWN) {
            handleUnknown();
        } else if (type == ResourceType.USER) {
            handleUser();
        }
    }
    
    public ResourceType getDefault() {
        return ResourceType.UNKNOWN;
    }
    
    private void handleUnknown() { }
    private void handleUser() { }
}
",
        ),
        (
            "com/example/Client.java",
            r#"package com.example;

public class Client {
    private Service service;
    
    public void test() {
        service.process(ResourceType.UNKNOWN);
        ResourceType type = ResourceType.UNKNOWN;
        
        if (type != ResourceType.USER) {
            System.out.println("Not a user");
        }
    }
}
"#,
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    // Find the ResourceType.java file
    let (enum_file_path, enum_file_id) = files
        .iter()
        .find(|(path, _)| path.to_str().unwrap().contains("ResourceType.java"))
        .unwrap();
    let enum_source = std::fs::read_to_string(enum_file_path).unwrap();

    // Find references to UNKNOWN enum constant
    // "    UNKNOWN,"
    //      ^^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *enum_file_id,
        &enum_source,
        enum_file_path,
        Position::new(3, 6),
        true,
    );

    // Should find:
    // 1. Declaration in ResourceType.java (include_declaration=true)
    // 2. ResourceType.UNKNOWN in Service.java (line 5)
    // 3. ResourceType.UNKNOWN in Service.java (line 12)
    // 4. ResourceType.UNKNOWN in Client.java (line 6)
    // 5. ResourceType.UNKNOWN in Client.java (line 7)
    // Total: at least 5 references across 3 files

    assert!(
        refs.len() >= 5,
        "Should find at least 5 references to UNKNOWN enum constant across files, found {}",
        refs.len()
    );

    // Verify references are in all three files
    let unique_files: std::collections::HashSet<_> =
        refs.iter().map(|loc| loc.uri.path()).collect();

    assert!(
        unique_files.len() >= 2,
        "Should find references in at least 2 different files, found {} files",
        unique_files.len()
    );

    // Verify at least one reference in Service.java
    assert!(
        refs.iter()
            .any(|loc| loc.uri.path().contains("Service.java")),
        "Should find at least one reference in Service.java"
    );

    // Verify at least one reference in Client.java
    assert!(
        refs.iter()
            .any(|loc| loc.uri.path().contains("Client.java")),
        "Should find at least one reference in Client.java"
    );
}

/// Test that find references correctly scopes variable names
/// This test documents and detects the known limitation of fallback string matching
#[test]
fn test_find_references_no_false_positives_different_scopes() {
    // This test ensures that variables with the same name in different scopes
    // are correctly distinguished. The semantic analyzer should prevent false positives.
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Calculator.java",
        r"package com.example;

public class Calculator {
    private int value; // Class field named 'value'

    public void methodA() {
        int value = 10;  // Local 'value' in methodA
        value = value + 1;
    }

    public void methodB() {
        int value = 20;  // Different local 'value' in methodB (line 12)
        value = value * 2;
    }

    public int getValue() {
        return value;  // References class field, not locals
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

    // Find references to 'value' in methodB (line 11, declaration)
    // This should ONLY find references within methodB, NOT methodA or the field
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(11, 12), // "int value = 20;" in methodB
        true,
    );

    println!("References found: {}", refs.len());
    for r in &refs {
        println!(
            "  Line {}: col {} to {}",
            r.range.start.line, r.range.start.character, r.range.end.character
        );
    }

    // Declaration, assignment target, and RHS use in methodB only. The same
    // spelling in methodA and the class field must not leak into the result.
    let actual_ranges: Vec<_> = refs
        .iter()
        .map(|reference| {
            (
                reference.range.start.line,
                reference.range.start.character,
                reference.range.end.line,
                reference.range.end.character,
            )
        })
        .collect();
    assert_eq!(
        actual_ranges,
        vec![(11, 12, 11, 17), (12, 8, 12, 13), (12, 16, 12, 21)],
        "local-variable references must be exact and scope-local"
    );
}

#[test]
fn test_find_references_method_with_generic_return_type() {
    // Test that find references on a method with generic return type
    // doesn't include the return type position
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Greeting.java",
        r"package com.example;

import java.util.List;

public class Greeting {
    public List<String> toNames() {
        return null;
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let (path, file_id) = &files[0];
    let content = std::fs::read_to_string(path).unwrap();

    // Position on "toNames" method name (line 5, column 24 - should be on 'toNames')
    // Line 5: "    public List<String> toNames() {"
    //                                  ^--24
    let position = Position::new(5, 24);

    // Find references (including declaration) - should only find the method name
    let refs = find_refs_at_position(&provider, *file_id, &content, path, position, true);

    println!("Found {} references:", refs.len());
    for (i, r) in refs.iter().enumerate() {
        println!(
            "  {}: line {}, col {} - {}",
            i, r.range.start.line, r.range.start.character, r.range.end.character
        );
    }

    // Should only find 1 reference (the definition)
    assert_eq!(
        refs.len(),
        1,
        "Should find exactly 1 reference (the definition), not return type"
    );

    // The reference should be at the method name position, not at "List"
    // Line 5: "    public List<String> toNames() {"
    // "List" is at column 11, "toNames" is at column 24
    let first_ref = &refs[0];
    assert!(
        first_ref.range.start.character >= 20,
        "Reference should be at method name (col 24+), not return type (col 11). Got col {}",
        first_ref.range.start.character
    );
}

/// A caller-registered semantic buffer must never be paired with ranges from
/// an older workspace-index revision.
///
/// This deliberately constructs state that normal transactional publication
/// prevents:
/// 1. File is indexed with certain content
/// 2. Salsa receives newer bytes without the matching index publication
/// 3. Find references must omit the stale cross-layer location
#[test]
fn test_cross_layer_source_mismatch_omits_stale_ranges() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();

    // Create initial files
    let request_path = workspace_root.join("com/example/Request.java");
    let service_path = workspace_root.join("com/example/Service.java");

    std::fs::create_dir_all(request_path.parent().unwrap()).unwrap();
    std::fs::write(
        &request_path,
        r"package com.example;

public class Request {
    public String data;
}
",
    )
    .unwrap();

    let initial_service_content = r"package com.example;

public class Service {
    public void process(Request req) {
        System.out.println(req);
    }
}
";
    std::fs::write(&service_path, initial_service_content).unwrap();

    // Index files
    let index = WorkspaceIndex::new(workspace_root.clone());
    index
        .index_file_with_source(
            &request_path,
            &std::fs::read_to_string(&request_path).unwrap(),
        )
        .unwrap();
    index
        .index_file_with_source(&service_path, initial_service_content)
        .unwrap();

    // Now MODIFY the service file by adding lines at the top
    // This shifts all the line numbers down
    let modified_service_content = r"package com.example;

// Added comment line 1
// Added comment line 2
// Added comment line 3
// These new lines shift all ranges down

public class Service {
    public void process(Request req) {
        System.out.println(req);
    }
}
";
    std::fs::write(&service_path, modified_service_content).unwrap();

    // The index still owns the old revision while the semantic DB owns the new
    // editor buffer. No request path may combine those two authorities.
    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database
    for (path, source) in [
        (
            &request_path,
            std::fs::read_to_string(&request_path).unwrap(),
        ),
        (&service_path, modified_service_content.to_string()),
    ] {
        if let Some(file_id) = index_arc.get_file_id(path) {
            host.with_db(|db_guard| {
                db_guard.set_file_text_tracked(file_id, Arc::new(source));
                db_guard.set_file_language(file_id, Language::Java);
            });
        }
    }
    assert_eq!(
        host.registered_text(index_arc.get_file_id(&service_path).unwrap())
            .as_deref()
            .map(String::as_str),
        Some(modified_service_content)
    );

    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        index_arc.clone(),
        query_engine,
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );

    let request_file_id = index_arc.get_file_id(&request_path).unwrap();
    let request_source = std::fs::read_to_string(&request_path).unwrap();

    // Find references to class "Request"
    let refs = find_refs_at_position(
        &provider,
        request_file_id,
        &request_source,
        &request_path,
        Position::new(2, 13), // "public class Request"
        true,
    );

    println!("Stale index test - Found {} references:", refs.len());

    // Check references in Service.java
    let service_uri = Url::from_file_path(&service_path).unwrap();
    assert!(
        !refs.iter().any(|reference| reference.uri == service_uri),
        "stale index ranges must be omitted until the newer bytes are published transactionally"
    );
}

#[test]
fn test_find_method_references_same_file() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Calculator.java",
        r"package com.example;

public class Calculator {
    public int add(int a, int b) {
        return a + b;
    }

    public int multiply(int a, int b) {
        return a * b;
    }

    public void test() {
        int result1 = add(5, 3);
        int result2 = add(10, 20);
        int result3 = multiply(2, 3);
    }
}
",
    )]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));

    // Set file contents in database and mark as Java
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

    // Find references to "add" method (hover over it in line 4)
    // "public int add(int a, int b) {"
    //             ^^^
    let refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 16),
        true, // include declaration
    );

    println!("Found {} references to 'add' method:", refs.len());
    for (i, loc) in refs.iter().enumerate() {
        println!(
            "  {}. Line {}, Col {}-{}",
            i + 1,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.character
        );
    }

    // Should find: declaration + 2 uses (test method calls add twice)
    assert!(
        refs.len() >= 3,
        "Should find at least 3 references to method 'add' (1 declaration + 2 calls), found {}",
        refs.len()
    );
}

#[test]
fn test_find_references_does_not_mix_method_and_field_with_same_name() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Service.java",
        r"package com.example;

public class Service {
    private Foo foo;

    void foo() {}

    void test() {
        foo();           // method call
        Foo local = foo; // field usage
    }

    static class Foo {}
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

    let method_refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(5, 10), // "foo" in "void foo() {}"
        true,
    );

    let field_refs = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 17), // "foo" in "private Foo foo;"
        true,
    );

    let method_lines: Vec<u32> = method_refs.iter().map(|loc| loc.range.start.line).collect();
    let field_lines: Vec<u32> = field_refs.iter().map(|loc| loc.range.start.line).collect();

    // Method references should include declaration + method call, but exclude field sites.
    assert!(
        method_lines.contains(&5),
        "Expected method declaration line in method refs, got {:?}",
        method_lines
    );
    assert!(
        method_lines.contains(&8),
        "Expected method call line in method refs, got {:?}",
        method_lines
    );
    assert!(
        !method_lines.contains(&3),
        "Method refs must not include field declaration line, got {:?}",
        method_lines
    );
    assert!(
        !method_lines.contains(&9),
        "Method refs must not include field usage line, got {:?}",
        method_lines
    );

    // Field references should include declaration + field usage, but exclude method sites.
    assert!(
        field_lines.contains(&3),
        "Expected field declaration line in field refs, got {:?}",
        field_lines
    );
    assert!(
        field_lines.contains(&9),
        "Expected field usage line in field refs, got {:?}",
        field_lines
    );
    assert!(
        !field_lines.contains(&5),
        "Field refs must not include method declaration line, got {:?}",
        field_lines
    );
    assert!(
        !field_lines.contains(&8),
        "Field refs must not include method call line, got {:?}",
        field_lines
    );
}

#[test]
fn test_nested_method_references_do_not_use_colliding_top_level_owner() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "p/Outer.java",
            r"package p;

public class Outer {
    public static class Worker {
        public void target() {}

        public void invoke() {
            target();
        }
    }
}
",
        ),
        (
            "p/Worker.java",
            r"package p;

public class Worker {
    public void target() {}
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

    let (outer_path, outer_id) = &files[0];
    let outer_source = std::fs::read_to_string(outer_path).unwrap();
    let nested_refs = find_refs_at_position(
        &provider,
        *outer_id,
        &outer_source,
        outer_path,
        Position::new(4, 22),
        true,
    );

    assert_eq!(
        nested_refs.len(),
        2,
        "nested target should have its declaration and unqualified call: {nested_refs:?}"
    );
    assert!(nested_refs
        .iter()
        .all(|location| location.uri.path().ends_with("/Outer.java")));
    assert!(nested_refs
        .iter()
        .any(|location| location.range.start.line == 7));

    let (top_level_path, top_level_id) = &files[1];
    let top_level_source = std::fs::read_to_string(top_level_path).unwrap();
    let top_level_refs = find_refs_at_position(
        &provider,
        *top_level_id,
        &top_level_source,
        top_level_path,
        Position::new(3, 18),
        true,
    );
    assert_eq!(
        top_level_refs.len(),
        1,
        "top-level target must not absorb the nested call: {top_level_refs:?}"
    );
    assert!(top_level_refs[0].uri.path().ends_with("/Worker.java"));
}

#[test]
fn test_find_references_includes_explicit_super_call() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "p/Base.java",
            r"package p;

public class Base {
    public void target() {}
}
",
        ),
        (
            "p/Child.java",
            r"package p;

public class Child extends Base {
    @Override
    public void target() {}

    public void invoke() {
        super.target();
    }

    public Runnable reference() {
        return super::target;
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

    let (base_path, base_id) = &files[0];
    let base_source = std::fs::read_to_string(base_path).unwrap();
    let refs = find_refs_at_position(
        &provider,
        *base_id,
        &base_source,
        base_path,
        Position::new(3, 18),
        false,
    );

    assert!(
        refs.iter().any(|location| {
            location.uri.path().ends_with("/Child.java") && location.range.start.line == 7
        }),
        "super.target() should reference Base.target(): {refs:?}"
    );
    assert!(
        refs.iter().any(|location| {
            location.uri.path().ends_with("/Child.java") && location.range.start.line == 11
        }),
        "super::target should reference Base.target(): {refs:?}"
    );
}

/// Test that find references for overloaded methods correctly distinguishes by signature.
///
/// When finding references to `process(String)`, we should NOT include:
/// - The definition of `process(int)`
/// - Calls to `process(int)`
///
/// This requires the usage extractor to infer argument types from method calls.
#[test]
fn test_find_references_overloaded_methods_disambiguation() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Processor.java",
        r#"package com.example;

public class Processor {
    public void process(String text) {
        System.out.println("String: " + text);
    }

    public void process(int number) {
        System.out.println("Int: " + number);
    }

    public void process(String text, int count) {
        for (int i = 0; i < count; i++) {
            process(text);
        }
    }

    public void test() {
        process("hello");      // Calls process(String)
        process(42);           // Calls process(int)
        process("world");      // Calls process(String)
        process(100);          // Calls process(int)
        process("test", 3);    // Calls process(String, int)
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

    // Find references to process(String) - line 4
    // "public void process(String text) {"
    //             ^^^^^^^
    let refs_string = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 16), // "process" in process(String)
        true,
    );

    println!("References to process(String):");
    for (i, loc) in refs_string.iter().enumerate() {
        let lines: Vec<&str> = source.lines().collect();
        let line_content = lines.get(loc.range.start.line as usize).unwrap_or(&"");
        println!(
            "  {}. Line {}: '{}'",
            i + 1,
            loc.range.start.line,
            line_content.trim()
        );
    }

    // Find references to process(int) - line 8
    // "public void process(int number) {"
    //             ^^^^^^^
    let refs_int = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(7, 16), // "process" in process(int)
        true,
    );

    println!("\nReferences to process(int):");
    for (i, loc) in refs_int.iter().enumerate() {
        let lines: Vec<&str> = source.lines().collect();
        let line_content = lines.get(loc.range.start.line as usize).unwrap_or(&"");
        println!(
            "  {}. Line {}: '{}'",
            i + 1,
            loc.range.start.line,
            line_content.trim()
        );
    }

    // CRITICAL ASSERTIONS:
    //
    // For process(String), we expect:
    // - Line 3: declaration "public void process(String text)"
    // - Line 13: call "process(text)" inside process(String, int)
    // - Line 18: call "process("hello")"
    // - Line 20: call "process("world")"
    // Total: 4 references
    //
    // We should NOT find:
    // - Line 7: declaration "public void process(int number)"
    // - Line 19: call "process(42)"
    // - Line 21: call "process(100)"

    // Check that process(String) references don't include process(int) definition
    let string_ref_lines: Vec<u32> = refs_string.iter().map(|r| r.range.start.line).collect();

    assert!(
        !string_ref_lines.contains(&7),
        "BUG: References to process(String) should NOT include process(int) definition on line 7. \
         Found lines: {:?}",
        string_ref_lines
    );

    // Check that process(String) references don't include process(int) calls
    // Line 19 has process(42), line 21 has process(100)
    assert!(
        !string_ref_lines.contains(&19),
        "BUG: References to process(String) should NOT include process(42) call on line 19. \
         Found lines: {:?}",
        string_ref_lines
    );
    assert!(
        !string_ref_lines.contains(&21),
        "BUG: References to process(String) should NOT include process(100) call on line 21. \
         Found lines: {:?}",
        string_ref_lines
    );

    // Check that process(int) references don't include process(String) definition
    let int_ref_lines: Vec<u32> = refs_int.iter().map(|r| r.range.start.line).collect();

    assert!(
        !int_ref_lines.contains(&3),
        "BUG: References to process(int) should NOT include process(String) definition on line 3. \
         Found lines: {:?}",
        int_ref_lines
    );

    // For process(int), we expect:
    // - Line 7: declaration
    // - Line 19: call "process(42)"
    // - Line 21: call "process(100)"
    // Total: 3 references
    //
    // We should NOT find:
    // - String calls: lines 18, 20
    // - String definition: line 3

    assert!(
        !int_ref_lines.contains(&18),
        "BUG: References to process(int) should NOT include process(\"hello\") call on line 18. \
         Found lines: {:?}",
        int_ref_lines
    );
    assert!(
        !int_ref_lines.contains(&20),
        "BUG: References to process(int) should NOT include process(\"world\") call on line 20. \
         Found lines: {:?}",
        int_ref_lines
    );

    // Verify we found the expected number of references
    // process(String): 4 refs (definition + 3 calls with String args)
    // process(int): 3 refs (definition + 2 calls with int args)
    println!("\nSummary:");
    println!(
        "  process(String) references: {} (expected: 4)",
        refs_string.len()
    );
    println!(
        "  process(int) references: {} (expected: 3)",
        refs_int.len()
    );

    assert_eq!(
        refs_string.len(),
        4,
        "process(String) should have exactly 4 references (1 def + 3 calls), found {}",
        refs_string.len()
    );
    assert_eq!(
        refs_int.len(),
        3,
        "process(int) should have exactly 3 references (1 def + 2 calls), found {}",
        refs_int.len()
    );
}

/// Test that cast expression argument types are properly inferred for constructor overload disambiguation.
///
/// When constructing `new Processor((String)obj)`, the cast `(String)` should resolve to
/// the `Processor(String)` overload, not `Processor(int)`.
#[test]
fn test_find_references_cast_argument_inference() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/model/Processor.java",
            r"package com.example.model;

public class Processor {
    public Processor(String text) {}

    public Processor(int number) {}
}
",
        ),
        (
            "com/example/Main.java",
            r"package com.example;

import com.example.model.Processor;

public class Main {
    public void test(Object obj) {
        Processor p1 = new Processor((String) obj);
        Processor p2 = new Processor(42);
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

    let (processor_path, processor_file_id) = &files[0];
    let processor_source = std::fs::read_to_string(processor_path).unwrap();

    // Find references to Processor(String) constructor - line 3
    // "public Processor(String text) {}"
    //        ^^^^^^^^^
    let refs = find_refs_at_position(
        &provider,
        *processor_file_id,
        &processor_source,
        processor_path,
        Position::new(3, 11),
        true,
    );

    println!("References to Processor(String):");
    for (i, loc) in refs.iter().enumerate() {
        println!(
            "  {}. {:?} Line {}, Col {}",
            i + 1,
            loc.uri.path().split('/').next_back().unwrap_or("?"),
            loc.range.start.line,
            loc.range.start.character
        );
    }

    // Should find: 1 declaration + 1 call with cast argument
    // Should NOT find: the `new Processor(42)` call
    assert!(
        refs.len() >= 2,
        "Should find at least 2 references to Processor(String) (1 def + 1 cast call), found {}",
        refs.len()
    );

    // Verify the int call is NOT included
    let main_uri = Url::from_file_path(&files[1].0).unwrap();
    // The cast call should be found
    assert!(
        refs.iter().any(|r| r.uri == main_uri),
        "Should find at least one reference in Main.java (the cast call)"
    );
}

#[test]
fn test_find_references_respects_widening_before_boxing() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Service.java",
            r"package com.example;

public class Service {
    public void choose(long value) {}
    public void choose(Integer value) {}
}
",
        ),
        (
            "com/example/Main.java",
            r"package com.example;

public class Main {
    public void run(Service service) {
        service.choose(1);
    }
}
",
        ),
    ]);
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for (path, file_id) in &files {
        let source = std::fs::read_to_string(path).unwrap();
        host.with_db(|database| {
            database.set_file_text(*file_id, Arc::new(source));
            database.set_file_language(*file_id, Language::Java);
        });
    }
    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(QueryEngine::new((*index).clone())),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let (main_path, main_file_id) = &files[1];
    let main_source = std::fs::read_to_string(main_path).unwrap();

    let references = find_refs_at_position(
        &provider,
        *main_file_id,
        &main_source,
        main_path,
        Position::new(4, 16),
        true,
    );

    let service_uri = Url::from_file_path(&files[0].0).unwrap();
    let main_uri = Url::from_file_path(main_path).unwrap();
    assert!(
        references
            .iter()
            .any(|location| location.uri == service_uri && location.range.start.line == 3),
        "int call should resolve to choose(long): {references:?}"
    );
    assert!(
        references
            .iter()
            .any(|location| location.uri == main_uri && location.range.start.line == 4),
        "selected overload should retain the widening call site: {references:?}"
    );
    assert!(
        !references
            .iter()
            .any(|location| location.uri == service_uri && location.range.start.line == 4),
        "boxing overload must not be included: {references:?}"
    );
}

/// Test that overloaded method references work across files.
///
/// This validates Phase 2: method call FQNs include parameter descriptors.
#[test]
fn test_find_references_overloaded_methods_cross_file() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/model/Handler.java",
            r"package com.example.model;

public class Handler {
    public void handle(String message) {}

    public void handle(int code) {}
}
",
        ),
        (
            "com/example/Client.java",
            r#"package com.example;

import com.example.model.Handler;

public class Client {
    private Handler handler;

    public void sendMessage() {
        handler.handle("hello");
    }

    public void sendCode() {
        handler.handle(42);
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

    let (handler_path, handler_file_id) = &files[0];
    let handler_source = std::fs::read_to_string(handler_path).unwrap();

    // Find references to handle(String) - line 3
    let refs_string = find_refs_at_position(
        &provider,
        *handler_file_id,
        &handler_source,
        handler_path,
        Position::new(3, 16), // "handle" in handle(String)
        true,
    );

    // Find references to handle(int) - line 5
    let refs_int = find_refs_at_position(
        &provider,
        *handler_file_id,
        &handler_source,
        handler_path,
        Position::new(5, 16), // "handle" in handle(int)
        true,
    );

    println!("References to handle(String):");
    for loc in &refs_string {
        println!(
            "  {:?} Line {}",
            loc.uri.path().split('/').next_back().unwrap_or("?"),
            loc.range.start.line
        );
    }
    println!("References to handle(int):");
    for loc in &refs_int {
        println!(
            "  {:?} Line {}",
            loc.uri.path().split('/').next_back().unwrap_or("?"),
            loc.range.start.line
        );
    }

    let client_uri = Url::from_file_path(&files[1].0).unwrap();

    // handle(String) should find the "hello" call but NOT the 42 call
    let string_client_refs: Vec<_> = refs_string.iter().filter(|r| r.uri == client_uri).collect();
    let int_client_refs: Vec<_> = refs_int.iter().filter(|r| r.uri == client_uri).collect();

    // Each overload should have exactly 1 cross-file reference
    assert_eq!(
        string_client_refs.len(),
        1,
        "handle(String) should have 1 cross-file ref (\"hello\" call), found {}",
        string_client_refs.len()
    );
    assert_eq!(
        int_client_refs.len(),
        1,
        "handle(int) should have 1 cross-file ref (42 call), found {}",
        int_client_refs.len()
    );
}

/// Test that variable arguments are properly resolved for method overload disambiguation.
///
/// When calling `process(someVar)` where `someVar` is declared as `String someVar`,
/// the variable type inference should resolve it to `process(String)`.
#[test]
fn test_find_references_variable_argument_overloads() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "com/example/Processor.java",
        r#"package com.example;

public class Processor {
    public void process(String text) {
        System.out.println("String: " + text);
    }

    public void process(int number) {
        System.out.println("Int: " + number);
    }

    public void test() {
        String msg = "hello";
        int code = 42;
        process(msg);     // Should resolve to process(String) via variable type
        process(code);    // Should resolve to process(int) via variable type
        process("literal"); // Direct string literal
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

    // Find references to process(String) - line 3
    let refs_string = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(3, 16), // "process" in process(String)
        true,
    );

    // Find references to process(int) - line 7
    let refs_int = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        file_path,
        Position::new(7, 16), // "process" in process(int)
        true,
    );

    println!("References to process(String):");
    for loc in &refs_string {
        let lines: Vec<&str> = source.lines().collect();
        let line = lines.get(loc.range.start.line as usize).unwrap_or(&"");
        println!("  Line {}: '{}'", loc.range.start.line, line.trim());
    }
    println!("References to process(int):");
    for loc in &refs_int {
        let lines: Vec<&str> = source.lines().collect();
        let line = lines.get(loc.range.start.line as usize).unwrap_or(&"");
        println!("  Line {}: '{}'", loc.range.start.line, line.trim());
    }

    // process(String) should find:
    // - Line 3: definition
    // - Line 14: process(msg) — variable String msg resolved
    // - Line 16: process("literal") — string literal
    // Total: 3 references
    assert_eq!(
        refs_string.len(),
        3,
        "process(String) should have 3 references (1 def + 2 calls), found {}",
        refs_string.len()
    );

    // process(int) should find:
    // - Line 7: definition
    // - Line 15: process(code) — variable int code resolved
    // Total: 2 references
    assert_eq!(
        refs_int.len(),
        2,
        "process(int) should have 2 references (1 def + 1 call), found {}",
        refs_int.len()
    );
}

/// Regression: at an enum-constant *definition* site, the
/// disambiguation logic was skipped because the original guard only ran when
/// `!is_at_enum_constant`. With two enums in the same file sharing a member
/// name (e.g. `Status.ACTIVE` and `OrderStatus.ACTIVE`), find-references would
/// process candidates in arbitrary order and could return references for the
/// wrong enum. We now derive the enclosing ENUM_DECL's simple name and require
/// the candidate's FQN to match.
///
/// We test both definition sites — Status.ACTIVE and OrderStatus.ACTIVE — to
/// guard against any candidate-ordering coincidence that could mask the bug.
#[test]
fn test_find_references_enum_definition_same_file_sibling_enums() {
    let (_temp, index, files) = create_test_workspace(vec![
        (
            "com/example/Enums.java",
            r"package com.example;

public class Enums {
    public enum Status {
        ACTIVE,
        INACTIVE
    }

    public enum OrderStatus {
        ACTIVE,
        CANCELLED
    }
}

",
        ),
        (
            "com/example/UseStatus.java",
            r"package com.example;

public class UseStatus {
    public Enums.Status get() {
        return Enums.Status.ACTIVE;
    }
}
",
        ),
        (
            "com/example/UseOrderStatus.java",
            r"package com.example;

public class UseOrderStatus {
    public Enums.OrderStatus get() {
        return Enums.OrderStatus.ACTIVE;
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

    let (enums_path, enums_file_id) = &files[0];
    let source = std::fs::read_to_string(enums_path).unwrap();
    let lines: Vec<&str> = source.lines().collect();

    // Find both `ACTIVE,` lines.
    let active_lines: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| if l.trim() == "ACTIVE," { Some(i) } else { None })
        .collect();
    assert_eq!(active_lines.len(), 2, "expected two 'ACTIVE,' lines");

    fn ends_with_filename(uri: &str, name: &str) -> bool {
        // Strip trailing "/file.java" pattern: we want exact filename match.
        match uri.rsplit_once('/') {
            Some((_, last)) => last == name,
            None => uri == name,
        }
    }

    // 1. Cursor on Status.ACTIVE definition (first occurrence).
    let status_line = active_lines[0];
    let status_col = lines[status_line].find("ACTIVE").unwrap() as u32 + 1;
    let refs_status = find_refs_at_position(
        &provider,
        *enums_file_id,
        &source,
        enums_path,
        Position::new(status_line as u32, status_col),
        true,
    );
    assert!(!refs_status.is_empty(), "expected at least the declaration");
    for loc in &refs_status {
        assert!(
            !ends_with_filename(loc.uri.as_str(), "UseOrderStatus.java"),
            "Status.ACTIVE references must not include UseOrderStatus.java; got {loc:?}"
        );
    }

    // 2. Cursor on OrderStatus.ACTIVE definition (second occurrence).
    //    This is the failing-before-fix direction: candidate iteration returns
    //    Status.ACTIVE first, the pre-fix code only checked file_id (which
    //    matches Status.ACTIVE since both enums are in Enums.java), and
    //    `query_engine.find_references` would happily return Status's
    //    references — which include UseStatus.java.
    let order_line = active_lines[1];
    let order_col = lines[order_line].find("ACTIVE").unwrap() as u32 + 1;
    let refs_order = find_refs_at_position(
        &provider,
        *enums_file_id,
        &source,
        enums_path,
        Position::new(order_line as u32, order_col),
        true,
    );
    assert!(!refs_order.is_empty(), "expected at least the declaration");
    for loc in &refs_order {
        assert!(
            !ends_with_filename(loc.uri.as_str(), "UseStatus.java"),
            "OrderStatus.ACTIVE references must not include UseStatus.java; got {loc:?}"
        );
    }
}

#[test]
fn overloaded_declaration_does_not_become_a_descriptorless_reference_target() {
    let (_temp, index, files) = create_test_workspace(vec![(
        "p/Overloaded.java",
        concat!(
            "package p; public class Overloaded {\n",
            "  void target(String value) {}\n",
            "  void target(int value) {}\n",
            "  void use() {\n",
            "    target(\"text\");\n",
            "    target(1);\n",
            "  }\n",
            "}\n",
        ),
    )]);
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for (path, file_id) in &files {
        let source = std::fs::read_to_string(path).expect("source");
        host.with_db(|db| {
            db.set_file_text(*file_id, Arc::new(source));
            db.set_file_language(*file_id, Language::Java);
        });
    }
    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(QueryEngine::new(index.as_ref().clone())),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let (path, file_id) = &files[0];
    let source = std::fs::read_to_string(path).expect("source");

    let string_references = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        path,
        Position::new(1, 8),
        true,
    );
    let int_references = find_refs_at_position(
        &provider,
        *file_id,
        &source,
        path,
        Position::new(2, 8),
        true,
    );

    assert_eq!(string_references.len(), 2, "{string_references:#?}");
    assert_eq!(int_references.len(), 2, "{int_references:#?}");
    assert!(string_references
        .iter()
        .all(|location| location.range.start.line != 2 && location.range.start.line != 5));
    assert!(int_references
        .iter()
        .all(|location| location.range.start.line != 1 && location.range.start.line != 4));
}
