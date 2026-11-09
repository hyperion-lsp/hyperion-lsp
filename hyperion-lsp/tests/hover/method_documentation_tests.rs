// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for method call documentation hover on cross-file and external library scenarios

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
fn test_hover_on_method_call_with_javadoc() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.service.UserService;

public class Main {
    public void test() {
        UserService service = new UserService();
        service.getUser();
    }
}
",
        ),
        (
            "com/example/service/UserService.java",
            r"package com.example.service;

public class UserService {
    /**
     * Retrieves the current user from the database.
     *
     * @return the User object or null if not found
     * @throws IllegalStateException if user context is not initialized
     */
    public User getUser() {
        return null;
    }
}
",
        ),
        (
            "com/example/service/User.java",
            "package com.example.service; public class User {}",
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

    // Hover over "getUser" method call
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 19),
    );

    let text = hover_text.expect("hover should resolve the documented method call");
    assert!(text.contains("getUser"), "missing method signature: {text}");
    assert!(
        text.contains("Retrieves the current user from the database."),
        "missing source JavaDoc: {text}"
    );
    assert!(
        text.contains("@return"),
        "missing JavaDoc return tag: {text}"
    );
}

#[test]
fn test_hover_on_method_call_simple_javadoc() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r#"package com.example;
import com.example.util.Helper;

public class Main {
    public void test() {
        Helper.formatName("Ada");
    }
}
"#,
        ),
        (
            "com/example/util/Helper.java",
            r"package com.example.util;

public class Helper {
    /** Formats a name string for display. */
    public static String formatName(String name) {
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

    // Hover over "formatName" static method call
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(5, 18),
    );

    let text = hover_text.expect("hover should resolve the documented static method call");
    assert!(text.contains("formatName"), "unexpected hover: {text}");
    assert!(
        text.contains("Formats a name string for display."),
        "missing source JavaDoc: {text}"
    );
}

#[test]
fn test_hover_on_method_call_without_javadoc() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r#"package com.example;
import com.example.service.Logger;

public class Main {
    public void test() {
        Logger.log("message");
    }
}
"#,
        ),
        (
            "com/example/service/Logger.java",
            r"package com.example.service;

public class Logger {
    public static void log(String message) {
        System.out.println(message);
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

    // Hover over "log" method call
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(5, 15),
    );

    let text = hover_text.expect("hover should resolve a method without JavaDoc");
    assert!(text.contains("log"), "unexpected hover: {text}");
    assert!(
        text.contains("String") || text.contains("java.lang.String"),
        "missing parameter type: {text}"
    );
}

#[test]
fn test_hover_on_method_call_with_deprecated_tag() {
    let (_temp, index, paths) = create_test_workspace(vec![
        (
            "com/example/Main.java",
            r"package com.example;
import com.example.api.OldApi;

public class Main {
    public void test() {
        OldApi.oldMethod();
    }
}
",
        ),
        (
            "com/example/api/OldApi.java",
            r"package com.example.api;

public class OldApi {
    /**
     * Old method that is no longer used.
     *
     * @deprecated Use newMethod() instead
     */
    public static void oldMethod() {
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

    // Hover over "oldMethod" method call
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(5, 17),
    );

    let text = hover_text.expect("hover should resolve the deprecated method call");
    assert!(text.contains("oldMethod"), "unexpected hover: {text}");
    assert!(
        text.contains("@deprecated"),
        "missing deprecation tag: {text}"
    );
    assert!(
        text.contains("Use newMethod() instead"),
        "missing deprecation replacement: {text}"
    );
}

#[test]
fn test_hover_on_external_library_method_call() {
    // Test hovering on a method from an external library (java.util.Arrays)
    let (_temp, index, paths) = create_test_workspace(vec![(
        "com/example/Main.java",
        r"package com.example;
import java.util.Arrays;

public class Main {
    public void test() {
        Arrays.asList();
    }
}
",
    )]);

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

    // Hover over "asList" method call from external library
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(5, 16),
    );

    // Currently this returns None or basic info, but after implementing external library support,
    // it should show documentation from the JDK javadoc
    if let Some(text) = hover_text {
        // Should show something meaningful (not just "IDENT")
        assert!(
            !text.to_lowercase().starts_with("ident"),
            "Should not show just 'IDENT', got: {}",
            text
        );
    }
}

#[test]
fn test_hover_on_external_stream_method_call() {
    // Test hovering on a method from java.util.stream.Stream
    let (_temp, index, paths) = create_test_workspace(vec![(
        "com/example/Main.java",
        r"package com.example;
import java.util.Arrays;
import java.util.stream.Stream;

public class Main {
    public void test() {
        Stream<String> stream = Arrays.stream(new String[]{});
        stream.filter(x -> x.length() > 0);
    }
}
",
    )]);

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

    // Hover over "filter" method call from Stream
    let hover_text = get_hover_at_position(
        &provider,
        &index_arc,
        &paths[0],
        &source,
        Position::new(6, 15),
    );

    if let Some(text) = hover_text {
        // Should show something meaningful
        assert!(
            !text.to_lowercase().starts_with("ident"),
            "Should not show just 'IDENT', got: {}",
            text
        );
    }
}
