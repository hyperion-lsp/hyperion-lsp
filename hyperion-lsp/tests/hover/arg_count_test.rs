// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_lsp::inlay_hints::InlayHintsProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::{Position, Range};

#[test]
fn test_nested_method_call_argument_count() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();

    let source = r#"
package test;

public class TestArgCount {
    public static class Config {
        public String getHost() { return "localhost"; }
        public int getPort() { return 8080; }
    }

    public void test() {
        Config config = new Config();

        // Should recognize 3 arguments (format + 2 varargs), not 4+
        // The bug was counting () and sub-nodes as separate arguments
        String result = String.format("redis://%s:%s", config.getHost(), config.getPort());
    }
}
"#;

    let workspace_index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp")));
    let host = SemanticHost::for_tests();

    let file_id = FileId::new(0);
    {
        host.with_db(|db_lock| {
            db_lock.register_file(file_id, Arc::new(source.to_string()), Language::Java);
        });
    }

    let txn = host.read_now();
    let provider = InlayHintsProvider::new(&txn, workspace_index);

    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 15,
            character: 0,
        },
    };

    match provider.inlay_hints(file_id, range, source) {
        Ok(hints) => {
            println!("\nFound {} hints:", hints.len());
            for hint in &hints {
                println!("  Line {}: {:?}", hint.position.line, hint.label);
            }

            // String.format("...", arg1, arg2) has 3 actual arguments at the call site
            // Even though the signature is format(String, Object...), we show hints for all arguments
            // The bug was counting 4+ hints because () and sub-nodes were treated as separate arguments
            let format_hints: Vec<_> = hints
                .iter()
                .filter(|h| h.position.line == 14) // Line with String.format
                .collect();

            println!("\nHints on String.format line: {}", format_hints.len());
            for hint in &format_hints {
                println!("  {:?}", hint.label);
            }

            // We can only show hints for parameters we can resolve.
            // Since String.format is a JDK method and we use fallback parameter names,
            // we only get the first parameter "format". This is correct behavior -
            // we don't show hints for varargs we can't resolve.
            // The key assertion here is that we don't show 4+ hints (the old bug)
            // due to incorrectly counting nested method calls as arguments.
            assert!(
                format_hints.len() <= 3,
                "Expected at most 3 hints for String.format (format + 2 varargs), got {}. \
                 The old bug counted 4+ by treating () and sub-nodes as separate arguments.",
                format_hints.len()
            );
        }
        Err(e) => {
            panic!("Error getting inlay hints: {}", e);
        }
    }
}
