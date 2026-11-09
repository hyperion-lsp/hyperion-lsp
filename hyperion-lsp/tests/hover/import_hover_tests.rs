// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for hover functionality on import statements

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
fn test_hover_on_single_import() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.User;

public class Main {
    public static void main(String[] args) {
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
    private String email;

    public String getName() { return name; }
    public String getEmail() { return email; }
}
",
        ),
    ]);

    let index_arc = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index_arc));

    // Set file contents and language in database
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

    // Read the source for Main.java
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    // Hover over "User" in the import statement (line 1, after "model.")
    // "import com.example.model.User;"
    //                          ^~~~~
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(1, 26),
    )
    .expect("Should have hover text for User in import");

    // Should show the class name
    assert!(
        hover_text.contains("User"),
        "Hover should mention User class"
    );

    // Should NOT show "IDENT"
    assert!(
        !hover_text.to_lowercase().starts_with("ident"),
        "Hover should not show IDENT for imported class, got: {hover_text}"
    );

    // Should mention it's in an import
    assert!(
        hover_text.contains("Import") || hover_text.contains("import"),
        "Hover should indicate this is an import statement"
    );
}

#[test]
fn test_hover_shows_file_location() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.User;

public class Main {}
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

    // Hover over "User" in the import
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(1, 26),
    )
    .expect("Should have hover for User");

    // Should show file path
    assert!(
        hover_text.contains("User.java") || hover_text.contains("model"),
        "Hover should show file location: {hover_text}"
    );

    // Should show it's a CLASS (Debug format outputs "Class")
    assert!(
        hover_text.contains("CLASS")
            || hover_text.contains("Class")
            || hover_text.contains("class"),
        "Hover should indicate symbol kind: {hover_text}"
    );
}

#[test]
fn test_hover_on_interface_import() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.service.UserService;

public class Main {}
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

    // Hover over "UserService" in the import
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(1, 29),
    )
    .expect("Should have hover for UserService interface");

    assert!(hover_text.contains("UserService"));
    assert!(!hover_text.to_lowercase().starts_with("ident"));
    // Should show INTERFACE kind (Debug format outputs "Interface")
    assert!(
        hover_text.contains("INTERFACE")
            || hover_text.contains("Interface")
            || hover_text.contains("interface"),
        "Should indicate this is an interface: {hover_text}"
    );
}

#[test]
fn test_hover_on_enum_import() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.model.Status;

public class Main {}
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

    // Hover over "Status" in the import
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(1, 26),
    )
    .expect("Should have hover for Status enum");

    assert!(hover_text.contains("Status"));
    assert!(!hover_text.to_lowercase().starts_with("ident"));
    // Should show ENUM kind (Debug format outputs "Enum")
    assert!(
        hover_text.contains("ENUM") || hover_text.contains("Enum") || hover_text.contains("enum"),
        "Should indicate this is an enum: {hover_text}"
    );
}

#[test]
fn test_hover_consistency_import_vs_usage() {
    let (_temp, index, paths) = create_test_workspace(vec![
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

    // Hover over "User" in the import statement
    let hover_import = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(1, 26),
    )
    .expect("Should have hover on import");

    // Hover over "User" in the usage (line 5: "User user = ...")
    let hover_usage = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(5, 10),
    )
    .expect("Should have hover on usage");

    // Both should mention "User"
    assert!(hover_import.contains("User"));
    assert!(hover_usage.contains("User"));

    // Neither should show "IDENT" as the primary content
    assert!(
        !hover_import.to_lowercase().starts_with("ident"),
        "Import hover should not start with IDENT"
    );
    assert!(
        !hover_usage.to_lowercase().starts_with("ident"),
        "Usage hover should not start with IDENT"
    );
}

#[test]
fn hover_type_identity_follows_the_explicit_import() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "app/Main.java",
            "package app;\nimport right.Selected;\nclass Main { Selected value; }\n",
        ),
        (
            "left/Selected.java",
            "package left; public class Selected {}\n",
        ),
        (
            "right/Selected.java",
            "package right; public class Selected {}\n",
        ),
    ]);
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for path in &paths {
        let file_id = index.get_file_id(path).expect("indexed fixture file");
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|database| {
            database.set_file_text(file_id, Arc::new(content));
            database.set_file_language(file_id, Language::Java);
        });
    }
    let txn = host.read_now();
    let provider = HoverProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    let hover = get_hover_at_position(&provider, &index, &paths[0], &source, Position::new(2, 15))
        .expect("imported type hover");
    assert!(hover.contains("right.Selected"), "{hover}");
    assert!(!hover.contains("left.Selected"), "{hover}");
}

#[test]
fn hover_does_not_adopt_an_unimported_unique_type() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "app/Main.java",
            "package app;\nclass Main { MissingType value; }\n",
        ),
        (
            "unrelated/MissingType.java",
            "package unrelated; public class MissingType {}\n",
        ),
    ]);
    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    for path in &paths {
        let file_id = index.get_file_id(path).expect("indexed fixture file");
        let content = std::fs::read_to_string(path).unwrap();
        host.with_db(|database| {
            database.set_file_text(file_id, Arc::new(content));
            database.set_file_language(file_id, Language::Java);
        });
    }
    let txn = host.read_now();
    let provider = HoverProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let source = std::fs::read_to_string(&paths[0]).unwrap();

    let hover = get_hover_at_position(&provider, &index, &paths[0], &source, Position::new(1, 15));
    assert!(
        hover.is_none(),
        "unresolved type must fail closed instead of producing syntax-only identity: {hover:?}"
    );
}
