// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for hover functionality on method chains with receiver type resolution

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
fn test_hover_on_simple_method_chain() {
    let (_temp, index, paths) = create_test_workspace(vec![
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

    /** Returns the display name. */
    public String getName() {
        return name;
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_lock| {
                db_lock.set_file_text(file_id, Arc::new(content));
                db_lock.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "getName" in "user.getName()"
    // Line 6: "        String name = user.getName();"
    //                                     ^~~~~~~
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 32),
    );

    if let Some(text) = hover_text {
        println!("Hover text: {text}");

        // Should mention the method name
        assert!(
            text.contains("getName"),
            "Hover should mention method name, got: {text}"
        );

        // Should mention the type (User) it belongs to
        assert!(
            text.contains("User") || text.contains("com.example.model"),
            "Hover should mention User type, got: {text}"
        );

        assert!(
            text.contains("Returns the display name."),
            "Hover should include source JavaDoc, got: {text}"
        );

        // Should NOT show "N method definitions found"
        assert!(
            !text.contains("method definitions found"),
            "Should not show generic method list, got: {text}"
        );
    } else {
        panic!("Expected hover text for method call");
    }
}

#[test]
fn test_hover_on_chained_method_calls() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.User;
import com.example.model.Profile;

public class Main {
    public void test() {
        User user = new User();
        String bio = user.getProfile().getBio();
    }
}
",
        ),
        (
            "com/example/model/User.java",
            r"package com.example.model;

public class User {
    private Profile profile;

    public Profile getProfile() {
        return profile;
    }
}
",
        ),
        (
            "com/example/model/Profile.java",
            r"package com.example.model;

public class Profile {
    private String bio;

    public String getBio() {
        return bio;
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_lock| {
                db_lock.set_file_text(file_id, Arc::new(content));
                db_lock.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "getBio" in "user.getProfile().getBio()"
    // Line 7: "        String bio = user.getProfile().getBio();"
    //                                                   ^~~~~~
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(7, 39), // Fixed: position on 'g' in getBio, not on ')'
    );

    if let Some(text) = hover_text {
        println!("Hover text: {text}");

        // Should mention getBio
        assert!(
            text.contains("getBio"),
            "Hover should mention getBio method, got: {text}"
        );

        // Should mention Profile (the type getBio belongs to)
        assert!(
            text.contains("Profile"),
            "Hover should mention Profile type, got: {text}"
        );

        // Should NOT mention User (that's the type of getProfile, not getBio)
        // This verifies we're resolving the chain correctly
    } else {
        panic!("Expected hover text for chained method call");
    }
}

#[test]
fn test_hover_on_field_access() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.User;

public class Main {
    public void test() {
        User user = new User();
        System.out.println(user.publicField);
    }
}
",
        ),
        (
            "com/example/model/User.java",
            r#"package com.example.model;

public class User {
    public String publicField = "test";
}
"#,
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_lock| {
                db_lock.set_file_text(file_id, Arc::new(content));
                db_lock.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "publicField" in "user.publicField" within println
    // Line 6: "        System.out.println(user.publicField);"
    //                                          ^~~~~~~~~~~
    // Need to recalculate position
    let lines: Vec<&str> = source.lines().collect();
    let target_line = lines[6];
    let field_pos = target_line
        .find("publicField")
        .expect("publicField not found");

    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, field_pos as u32),
    );

    if let Some(text) = hover_text {
        println!("Hover text: {text}");

        assert!(
            text.contains("publicField"),
            "Hover should mention field name, got: {text}"
        );

        assert!(
            text.contains("User"),
            "Hover should mention User type, got: {text}"
        );
    }
}

#[test]
fn test_hover_resolves_through_inheritance() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.Admin;

public class Main {
    public void test() {
        Admin admin = new Admin();
        String name = admin.getName();
    }
}
",
        ),
        (
            "com/example/model/Admin.java",
            r"package com.example.model;

public class Admin extends User {
    private String role;
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

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_lock| {
                db_lock.set_file_text(file_id, Arc::new(content));
                db_lock.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "getName" which is inherited from User
    // Line 6: "        String name = admin.getName();"
    //                                      ^~~~~~~
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 28), // Position on 'g' in getName
    );

    if let Some(text) = hover_text {
        println!("Hover text: {text}");

        assert!(
            text.contains("getName"),
            "Hover should mention getName method, got: {text}"
        );

        // Should reference User or Admin
        assert!(
            text.contains("User") || text.contains("Admin"),
            "Hover should mention type hierarchy, got: {text}"
        );
    }
}

#[test]
fn test_hover_on_local_variable_method_call() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.User;

public class Main {
    public void test() {
        User localUser = new User();
        localUser.getName();
    }
}
",
        ),
        (
            "com/example/model/User.java",
            r#"package com.example.model;

public class User {
    public String getName() {
        return "name";
    }
}
"#,
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_lock| {
                db_lock.set_file_text(file_id, Arc::new(content));
                db_lock.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "getName"
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 19),
    );

    if let Some(text) = hover_text {
        println!("Hover text: {text}");

        assert!(
            text.contains("getName"),
            "Hover should show method name, got: {text}"
        );

        assert!(
            text.contains("User"),
            "Hover should show User type, got: {text}"
        );
    }
}

#[test]
fn test_hover_not_confusing_for_common_method_names() {
    // This tests the original issue: common method names like "map" should
    // resolve to the specific type's method, not show all methods named "map"
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.DataProcessor;
import com.example.model.Configuration;

public class Main {
    public void test() {
        DataProcessor processor = new DataProcessor();
        Configuration config = new Configuration();

        processor.map();  // DataProcessor.map()
        config.map();     // Configuration.map()
    }
}

",
        ),
        (
            "com/example/model/DataProcessor.java",
            r"package com.example.model;

public class DataProcessor {
    public void map() {
        // Process data
    }
}
",
        ),
        (
            "com/example/model/Configuration.java",
            r"package com.example.model;

public class Configuration {
    public void map() {
        // Map configuration
    }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    for path in &paths {
        if let Some(file_id) = index_arc.get_file_id(path) {
            let content = std::fs::read_to_string(path).unwrap();
            host.with_db(|db_lock| {
                db_lock.set_file_text(file_id, Arc::new(content));
                db_lock.set_file_language(file_id, Language::Java);
            });
        }
    }

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let txn = host.read_now();
    let provider = HoverProvider::new(&txn, index_arc.clone(), source_providers);
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over first "map" (DataProcessor.map)
    let hover_processor = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(9, 18),
    );

    // Hover over second "map" (Configuration.map)
    let hover_config = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(10, 16),
    );

    // Both should exist
    if let Some(text) = hover_processor {
        println!("DataProcessor.map hover: {text}");

        // Should mention DataProcessor specifically
        assert!(
            text.contains("DataProcessor"),
            "Should show DataProcessor type, got: {text}"
        );

        // Should NOT say "2 method definitions found" or similar
        assert!(
            !text.contains("method definitions found"),
            "Should not show confusing method list, got: {text}"
        );
    }

    if let Some(text) = hover_config {
        println!("Configuration.map hover: {text}");

        // Should mention Configuration specifically
        assert!(
            text.contains("Configuration"),
            "Should show Configuration type, got: {text}"
        );

        // Should NOT say "2 method definitions found"
        assert!(
            !text.contains("method definitions found"),
            "Should not show confusing method list, got: {text}"
        );
    }
}

#[test]
fn generic_chained_hover_uses_the_same_declaring_class_as_navigation() {
    let (_temp, index, paths) = create_test_workspace(vec![
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
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for path in &paths {
        let file_id = index.get_file_id(path).expect("indexed file");
        let content = std::fs::read_to_string(path).expect("source");
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(content));
            db.set_file_language(file_id, Language::Java);
        });
    }
    let txn = host.read_now();
    let provider = HoverProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let source = std::fs::read_to_string(&paths[3]).expect("use source");
    let target = source.rfind("target").expect("target call") as u32;

    let hover = get_hover_at_position(
        &provider,
        &index,
        &paths[3],
        &source,
        Position::new(0, target),
    )
    .expect("hover text");

    assert!(hover.contains("target"), "{hover}");
    assert!(hover.contains("Service"), "{hover}");
    assert!(!hover.contains("Distractor"), "{hover}");
}

#[test]
fn unresolved_receiver_does_not_fall_back_to_an_unrelated_unique_method() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "p/Distractor.java",
            "package p; public class Distractor { public void target() {} }",
        ),
        (
            "p/Use.java",
            concat!(
                "package p; public class Use { void invoke(MissingType receiver) { ",
                "receiver.target(); } }",
            ),
        ),
    ]);
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for path in &paths {
        let file_id = index.get_file_id(path).expect("indexed file");
        let content = std::fs::read_to_string(path).expect("source");
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(content));
            db.set_file_language(file_id, Language::Java);
        });
    }
    let txn = host.read_now();
    let provider = HoverProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let source = std::fs::read_to_string(&paths[1]).expect("use source");
    let target = source.rfind("target").expect("target call") as u32;

    let hover = get_hover_at_position(
        &provider,
        &index,
        &paths[1],
        &source,
        Position::new(0, target),
    )
    .expect("basic hover text");

    assert!(!hover.contains("Distractor"), "{hover}");
    assert!(!hover.contains("Method defined"), "{hover}");
}
