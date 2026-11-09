// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Unit Tests for Diagnostic Conversion
//!
//! Tests the conversion of parse errors to LSP diagnostics.

use hyperion_lsp::diagnostics::parse_error_to_diagnostic;
use hyperion_parser::ParseError;
use rowan::{TextRange, TextSize};
use tower_lsp::lsp_types::*;

#[test]
fn test_parse_error_to_diagnostic_simple() {
    let source = "class Foo {";
    let error = ParseError::new(
        "Expected '}'",
        TextRange::new(TextSize::from(10), TextSize::from(11)),
    );

    let diagnostic = parse_error_to_diagnostic(&error, source);

    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostic.message, "Expected '}'");
    assert_eq!(diagnostic.source, Some("hyperion-parser".into()));
    assert_eq!(diagnostic.range.start.line, 0);
    assert_eq!(diagnostic.range.start.character, 10);
}

#[test]
fn test_parse_error_to_diagnostic_multiline() {
    let source = "class Foo {\nvoid bar(\n}";
    // Error at offset 21 = the '\n' at end of "void bar("
    let error = ParseError::new(
        "Expected ')'",
        TextRange::new(TextSize::from(21), TextSize::from(22)),
    );

    let diagnostic = parse_error_to_diagnostic(&error, source);

    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostic.message, "Expected ')'");
    // Should be on line 1 (the line with "void bar(")
    // The error points to the newline at the END of line 1, not the beginning of line 2
    // This is more accurate: it shows the error near the unclosed '(' where ')' should be
    assert_eq!(diagnostic.range.start.line, 1);
    assert_eq!(diagnostic.range.start.character, 9); // After the '('
}

#[test]
fn test_parse_error_to_diagnostic_unicode() {
    let source = "class 世界 {";
    let error = ParseError::new(
        "Invalid identifier",
        TextRange::new(TextSize::from(6), TextSize::from(12)),
    );

    let diagnostic = parse_error_to_diagnostic(&error, source);

    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(0, 6), Position::new(0, 8))
    );
}

mod diagnostic_properties {
    use super::*;

    #[test]
    fn test_diagnostic_severity_error() {
        let source = "test";
        let error = ParseError::new(
            "Error",
            TextRange::new(TextSize::from(0), TextSize::from(1)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn test_diagnostic_source() {
        let source = "test";
        let error = ParseError::new(
            "Error",
            TextRange::new(TextSize::from(0), TextSize::from(1)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert_eq!(diagnostic.source, Some("hyperion-parser".into()));
    }

    #[test]
    fn test_diagnostic_message_preservation() {
        let source = "test";
        let messages = vec![
            "Expected ';'",
            "Unexpected token",
            "Missing closing brace",
            "Invalid syntax",
        ];

        for msg in messages {
            let error = ParseError::new(msg, TextRange::new(TextSize::from(0), TextSize::from(1)));
            let diagnostic = parse_error_to_diagnostic(&error, source);

            assert_eq!(diagnostic.message, msg);
        }
    }

    #[test]
    fn test_diagnostic_no_code() {
        let source = "test";
        let error = ParseError::new(
            "Error",
            TextRange::new(TextSize::from(0), TextSize::from(1)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert_eq!(diagnostic.code, None);
    }

    #[test]
    fn test_diagnostic_no_related_information() {
        let source = "test";
        let error = ParseError::new(
            "Error",
            TextRange::new(TextSize::from(0), TextSize::from(1)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert_eq!(diagnostic.related_information, None);
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn test_error_at_start_of_file() {
        let source = "class";
        let error = ParseError::new(
            "Expected identifier",
            TextRange::new(TextSize::from(0), TextSize::from(5)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert_eq!(diagnostic.range.start, Position::new(0, 0));
    }

    #[test]
    fn test_error_at_end_of_file() {
        let source = "class Foo";
        let error = ParseError::new(
            "Expected '{'",
            TextRange::new(TextSize::from(9), TextSize::from(9)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        // Should handle end-of-file position
        assert_eq!(diagnostic.range.start, Position::new(0, 9));
        assert_eq!(diagnostic.range.end, Position::new(0, 9));
    }

    #[test]
    fn test_error_in_empty_file() {
        let source = "";
        let error = ParseError::new(
            "Unexpected EOF",
            TextRange::new(TextSize::from(0), TextSize::from(0)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert_eq!(diagnostic.range.start, Position::new(0, 0));
        assert_eq!(diagnostic.range.end, Position::new(0, 0));
    }

    #[test]
    fn test_error_spanning_multiple_lines() {
        let source = "class Foo {\n  int x = \n}";
        let error = ParseError::new(
            "Invalid expression",
            TextRange::new(TextSize::from(12), TextSize::from(23)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        assert!(diagnostic.range.end.line > diagnostic.range.start.line);
    }

    #[test]
    fn test_multiple_errors_same_line() {
        let source = "class Foo { int x int y }";

        let error1 = ParseError::new(
            "Expected ';'",
            TextRange::new(TextSize::from(17), TextSize::from(18)),
        );
        let error2 = ParseError::new(
            "Expected ';'",
            TextRange::new(TextSize::from(23), TextSize::from(24)),
        );

        let diag1 = parse_error_to_diagnostic(&error1, source);
        let diag2 = parse_error_to_diagnostic(&error2, source);

        assert_eq!(diag1.range.start.line, diag2.range.start.line);
        assert!(diag2.range.start.character > diag1.range.start.character);
    }

    #[test]
    fn test_error_in_line_with_only_whitespace() {
        let source = "class Foo {\n    \n}";
        let error = ParseError::new(
            "Expected member",
            TextRange::new(TextSize::from(12), TextSize::from(16)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        // Should handle whitespace-only lines
        assert_eq!(diagnostic.range.start.line, 1);
    }

    #[test]
    fn test_zero_length_error() {
        let source = "class Foo {|}";
        let error = ParseError::new(
            "Unexpected '|'",
            TextRange::new(TextSize::from(11), TextSize::from(11)),
        );
        let diagnostic = parse_error_to_diagnostic(&error, source);

        // Zero-length range should still be valid
        assert_eq!(diagnostic.range.start, diagnostic.range.end);
    }
}

/// Parse errors should NOT suppress semantic diagnostics.
/// The type checker runs on Rowan's CST which is always complete even with
/// parse errors, so semantic analysis should still produce useful results.
mod parse_error_semantic_coexistence {
    use hyperion_lsp::diagnostic_engine::{collect_diagnostics_for_file, DiagnosticOptions};
    use hyperion_lsp::semantic_host::SemanticHost;
    use hyperion_semantics::db::{Database, FileId, Language, SourceDatabase};
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;

    /// File with a parse error AND a valid class body — both parse errors
    /// and semantic diagnostics should appear.
    #[tokio::test]
    async fn test_semantic_diagnostics_with_parse_errors() {
        let mut db = Database::default();
        let file_id = FileId::new(0);

        // Missing closing brace causes a parse error, but the class body has
        // enough structure for the type checker to find the undefined type.
        let source = r"
package com.example;

public class Foo {
    public void test() {
        UndefinedType x = null;
    }
";

        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);

        let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
        let host = SemanticHost::with_database(db, Arc::clone(&workspace_index));

        let options = DiagnosticOptions::default();

        let diagnostics = collect_diagnostics_for_file(
            file_id, "java", source, &host, None, true, // workspace_ready = true
            options,
        )
        .await;

        // Should have at least one parse error
        let parse_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.source.as_deref() == Some("hyperion-parser"))
            .collect();
        assert!(
            !parse_errors.is_empty(),
            "Expected at least one parse error from the trailing garbage"
        );

        // Should ALSO have semantic diagnostics (from the type checker)
        // The total count should be more than just the parse errors
        assert!(
            diagnostics.len() > parse_errors.len(),
            "Expected semantic diagnostics alongside parse errors, \
             but only got {} parse error(s) and {} total",
            parse_errors.len(),
            diagnostics.len()
        );
    }

    /// Baseline: no parse errors, type errors appear. Regression guard.
    #[tokio::test]
    async fn test_semantic_diagnostics_without_parse_errors() {
        let mut db = Database::default();
        let file_id = FileId::new(0);

        let source = r"
package com.example;

public class Foo {
    public void test() {
        UndefinedType x = null;
    }
}
";

        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);

        let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
        let host = SemanticHost::with_database(db, Arc::clone(&workspace_index));

        let options = DiagnosticOptions::default();

        let diagnostics =
            collect_diagnostics_for_file(file_id, "java", source, &host, None, true, options).await;

        // Should have no parse errors
        let parse_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.source.as_deref() == Some("hyperion-parser"))
            .collect();
        assert!(
            parse_errors.is_empty(),
            "Expected no parse errors, got {}",
            parse_errors.len()
        );

        // Should still have diagnostics from the type checker
        assert!(
            !diagnostics.is_empty(),
            "Expected at least one semantic diagnostic for UndefinedType"
        );
    }

    #[tokio::test]
    async fn nullability_runs_when_type_checking_is_disabled() {
        let mut db = Database::default();
        let file_id = FileId::new(0);
        let source = r"
class Test {
    void test() {
        String value = null;
        value.length();
    }
}
";

        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);

        let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
        let host = SemanticHost::with_database(db, Arc::clone(&workspace_index));
        let options = DiagnosticOptions {
            run_type_check: false,
            run_nullability: true,
            run_inspections: false,
            run_security: false,
            ..DiagnosticOptions::default()
        };

        let diagnostics =
            collect_diagnostics_for_file(file_id, "java", source, &host, None, true, options).await;

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source.as_deref() == Some("hyperion-nullability")
                && diagnostic.message.contains("value")
        }));
        assert!(diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.source.as_deref() != Some("hyperion-type-checker") }));
    }
}
