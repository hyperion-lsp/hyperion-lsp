// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP Integration for Security Scanner
//!
//! This module provides integration between the hyperion-security scanner
//! and the Language Server Protocol (LSP). It converts security findings
//! to LSP diagnostics, provides code actions for fixes, and generates
//! hover information showing taint flow.

use crate::finding::{Finding, FlowStep, Location, Severity, VulnType};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity, Hover, HoverContents, Location as LspLocation, MarkedString,
    NumberOrString, Position, Range, TextEdit, Url, WorkspaceEdit,
};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Type alias for source text provider callback
type SourceProvider<'a> = Option<&'a dyn Fn(&std::path::Path) -> Option<String>>;

/// Converts a Finding to an LSP Diagnostic
///
/// This function creates a rich diagnostic with:
/// - Appropriate severity level
/// - Diagnostic code (CWE ID)
/// - Related information for data flow steps
/// - Source attribution
pub fn finding_to_diagnostic(
    finding: &Finding,
    source_text: &str,
    source_provider: SourceProvider<'_>,
) -> Diagnostic {
    let range = location_to_range(&finding.source_location, source_text);
    let severity = severity_to_diagnostic_severity(finding.severity);

    // Create related information for data flow
    let related_information = if finding.dataflow.is_empty() {
        None
    } else {
        Some(
            finding
                .dataflow
                .iter()
                .filter_map(|step| flow_step_to_related_info(step, source_provider))
                .collect(),
        )
    };

    // Create a detailed message showing the full issue
    let mut message = format!("{}\n", finding.message);
    if let Some(sink_loc) = &finding.sink_location {
        let _ = write!(message, "\nSink: {sink_loc}\n");
    }
    if !finding.dataflow.is_empty() {
        let _ = write!(message, "\nData flow ({} steps):", finding.dataflow.len());
        for (i, step) in finding.dataflow.iter().enumerate() {
            let _ = write!(
                message,
                "\n  {}. {} ({}:{})",
                i + 1,
                step.description,
                step.location.line,
                step.location.column
            );
        }
    }
    let _ = write!(message, "\n\nRemediation: {}", finding.remediation);

    Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(
            finding.vuln_type.cwe_id().to_string(),
        )),
        code_description: None,
        source: Some("hyperion-security".to_string()),
        message,
        related_information,
        tags: None,
        data: None,
    }
}

/// Converts severity to LSP diagnostic severity.
///
/// Never `ERROR`: that is reserved for compile errors, and an editor shows a
/// security finding (even a critical one) next to them as noise otherwise. The
/// finding's own severity stays in the message and the distinct
/// `hyperion-security` source.
const fn severity_to_diagnostic_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Info | Severity::Low => DiagnosticSeverity::INFORMATION,
        Severity::Medium | Severity::High | Severity::Critical => DiagnosticSeverity::WARNING,
    }
}

/// Converts a Location to an LSP Range
fn location_to_range(location: &Location, source_text: &str) -> Range {
    if source_text.is_empty() {
        return location_to_range_fallback(location);
    }

    Range {
        start: location_to_position(location.line, location.column, source_text),
        end: location_to_position(location.end_line, location.end_column, source_text),
    }
}

fn location_to_range_from_file(location: &Location) -> Range {
    hyperion_workspace::read_source_file(&location.file).map_or_else(
        |_| location_to_range_fallback(location),
        |source_text| location_to_range(location, &source_text),
    )
}

const fn location_to_range_fallback(location: &Location) -> Range {
    Range {
        start: Position {
            line: location.line.saturating_sub(1), // LSP is 0-indexed
            character: location.column.saturating_sub(1),
        },
        end: Position {
            line: location.end_line.saturating_sub(1),
            character: location.end_column.saturating_sub(1),
        },
    }
}

fn location_to_position(line: u32, column: u32, source_text: &str) -> Position {
    let requested_line = line.saturating_sub(1) as usize;

    let mut found_line: Option<&str> = None;
    let mut last_line: Option<(usize, &str)> = None;
    for (idx, current_line) in source_text.lines().enumerate() {
        last_line = Some((idx, current_line));
        if idx == requested_line {
            found_line = Some(current_line);
            break;
        }
    }

    let (line_index, line_text) = if let Some(line_text) = found_line {
        (requested_line, line_text)
    } else if let Some((idx, line_text)) = last_line {
        (idx, line_text)
    } else {
        return Position::new(0, 0);
    };

    let target_chars = column.saturating_sub(1) as usize;
    let mut utf16_count = 0u32;

    for ch in line_text.chars().take(target_chars) {
        utf16_count = utf16_count.saturating_add(u32::try_from(ch.len_utf16()).unwrap_or(u32::MAX));
    }

    // A position past u32::MAX cannot be represented; clamp it rather than wrap.
    Position::new(u32::try_from(line_index).unwrap_or(u32::MAX), utf16_count)
}

/// Converts a FlowStep to related diagnostic information
fn flow_step_to_related_info(
    step: &FlowStep,
    source_provider: SourceProvider<'_>,
) -> Option<DiagnosticRelatedInformation> {
    let uri = Url::from_file_path(&step.location.file).ok()?;
    let range = source_provider
        .and_then(|provider| provider(&step.location.file))
        .map_or_else(
            || location_to_range_from_file(&step.location),
            |source_text| location_to_range(&step.location, &source_text),
        );

    Some(DiagnosticRelatedInformation {
        location: LspLocation { uri, range },
        message: step.description.clone(),
    })
}

/// Generates code actions for a security finding
///
/// This function creates actionable quick fixes that can be applied
/// to remediate security issues. The specific actions depend on the
/// vulnerability type.
pub fn finding_to_code_actions(
    finding: &Finding,
    uri: &Url,
    source_text: &str,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let range = location_to_range(&finding.source_location, source_text);

    match finding.vuln_type {
        VulnType::SqlInjection => {
            // Suggest using PreparedStatement
            if let Some(action) = create_prepared_statement_action(finding, uri, range, source_text)
            {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
        VulnType::CommandInjection => {
            // Suggest using ProcessBuilder with validation
            actions.push(CodeActionOrCommand::CodeAction(
                create_process_builder_action(finding, uri, range),
            ));
        }
        VulnType::PathTraversal => {
            // Suggest path validation
            actions.push(CodeActionOrCommand::CodeAction(
                create_path_validation_action(finding, uri, range),
            ));
        }
        VulnType::Xss => {
            // Suggest HTML encoding
            actions.push(CodeActionOrCommand::CodeAction(create_html_encode_action(
                finding, uri, range,
            )));
        }
        VulnType::InsecureRandom => {
            // Suggest SecureRandom
            if let Some(action) = create_secure_random_action(finding, uri, range, source_text) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
        VulnType::WeakCrypto => {
            // Suggest stronger algorithms
            if let Some(action) = create_strong_crypto_action(finding, uri, range, source_text) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
        _ => {
            // For other types, create a generic "Learn more" action
            actions.push(CodeActionOrCommand::CodeAction(create_learn_more_action(
                finding, uri, range,
            )));
        }
    }

    actions
}

/// Creates a code action to use PreparedStatement instead of string concatenation
fn create_prepared_statement_action(
    finding: &Finding,
    uri: &Url,
    range: Range,
    _source_text: &str,
) -> Option<CodeAction> {
    // Extract the current statement for context
    let snippet = &finding.code_snippet;

    // Create a template for PreparedStatement
    let new_code = if snippet.contains("Statement") && snippet.contains("executeQuery") {
        "// Use PreparedStatement instead:\n\
         PreparedStatement stmt = connection.prepareStatement(\"SELECT * FROM users WHERE id = ?\");\n\
         stmt.setString(1, userId); // Use parameterized query\n\
         ResultSet rs = stmt.executeQuery();"
            .to_string()
    } else {
        return None;
    };

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: new_code,
        }],
    );

    Some(CodeAction {
        title: "Use PreparedStatement with parameterized queries".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// Creates a code action for ProcessBuilder usage
fn create_process_builder_action(_finding: &Finding, uri: &Url, range: Range) -> CodeAction {
    let new_code = "// Use ProcessBuilder with explicit arguments:\n\
         ProcessBuilder pb = new ProcessBuilder(\"command\", arg1, arg2);\n\
         // Validate arguments against allowlist before use\n\
         Process p = pb.start();"
        .to_string();

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: new_code,
        }],
    );

    CodeAction {
        title: "Use ProcessBuilder with explicit arguments".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    }
}

/// Creates a code action for path validation
fn create_path_validation_action(_finding: &Finding, uri: &Url, range: Range) -> CodeAction {
    let new_code = "// Validate and canonicalize path:\n\
         Path basePath = Paths.get(\"/safe/base/path\");\n\
         Path userPath = Paths.get(userInput).normalize();\n\
         Path resolvedPath = basePath.resolve(userPath);\n\
         if (!resolvedPath.startsWith(basePath)) {\n    \
             throw new SecurityException(\"Path traversal attempt\");\n\
         }"
    .to_string();

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: new_code,
        }],
    );

    CodeAction {
        title: "Add path validation against traversal".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    }
}

/// Creates a code action for HTML encoding
fn create_html_encode_action(_finding: &Finding, uri: &Url, range: Range) -> CodeAction {
    let new_code = "// Use OWASP Java Encoder:\nimport org.owasp.encoder.Encode;\n...\n\
         String safe = Encode.forHtml(userInput);"
        .to_string();

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: new_code,
        }],
    );

    CodeAction {
        title: "HTML-encode output using OWASP Encoder".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    }
}

/// Creates a code action to use SecureRandom
fn create_secure_random_action(
    finding: &Finding,
    uri: &Url,
    range: Range,
    _source_text: &str,
) -> Option<CodeAction> {
    let snippet = &finding.code_snippet;

    // Try to detect if it's using Math.random() or Random
    let new_code = if snippet.contains("Math.random()") {
        "// Use SecureRandom instead:\nimport java.security.SecureRandom;\n...\n\
         SecureRandom random = new SecureRandom();\n\
         int value = random.nextInt();"
            .to_string()
    } else if snippet.contains("new Random()") {
        "// Use SecureRandom for security-sensitive operations:\n\
         import java.security.SecureRandom;\n...\n\
         SecureRandom random = new SecureRandom();"
            .to_string()
    } else {
        return None;
    };

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: new_code,
        }],
    );

    Some(CodeAction {
        title: "Use SecureRandom for cryptographic operations".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// Creates a code action to use stronger cryptography
fn create_strong_crypto_action(
    finding: &Finding,
    uri: &Url,
    range: Range,
    _source_text: &str,
) -> Option<CodeAction> {
    let snippet = &finding.code_snippet;

    let new_code = if snippet.contains("DES") || snippet.contains("MD5") {
        "// Use strong cryptography:\n\
         // Replace DES with AES-256-GCM\n\
         // Replace MD5 with SHA-256 or SHA-3\n\
         Cipher cipher = Cipher.getInstance(\"AES/GCM/NoPadding\");\n\
         MessageDigest digest = MessageDigest.getInstance(\"SHA-256\");"
            .to_string()
    } else {
        return None;
    };

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: new_code,
        }],
    );

    Some(CodeAction {
        title: "Use strong cryptographic algorithms".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

/// Creates a generic "Learn more" action
fn create_learn_more_action(finding: &Finding, _uri: &Url, _range: Range) -> CodeAction {
    CodeAction {
        title: format!(
            "Learn more about {} ({})",
            finding.vuln_type.description(),
            finding.vuln_type.cwe_id()
        ),
        kind: Some(CodeActionKind::EMPTY),
        diagnostics: None,
        edit: None,
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    }
}

/// Creates hover information showing taint flow for a security finding
///
/// This displays:
/// - The vulnerability type and severity
/// - The taint source
/// - Data flow path (if available)
/// - Remediation advice
pub fn finding_to_hover(finding: &Finding, source_text: &str) -> Hover {
    let mut content = String::new();

    // Header with severity badge
    let _ = write!(
        content,
        "**Security Issue: {}**\n\n",
        finding.vuln_type.description()
    );
    let _ = write!(content, "**Severity:** {} | ", finding.severity);
    let _ = write!(content, "**CWE:** {}\n\n", finding.vuln_type.cwe_id());
    let _ = write!(
        content,
        "**Confidence:** {}%\n\n",
        (finding.confidence * 100.0).round()
    );

    // Issue description
    let _ = write!(content, "**Issue:** {}\n\n", finding.message);

    // Data flow visualization
    if !finding.dataflow.is_empty() {
        content.push_str("**Taint Flow:**\n\n");
        for (i, step) in finding.dataflow.iter().enumerate() {
            let arrow = if i == 0 {
                "🔴" // Source
            } else if i == finding.dataflow.len() - 1 {
                "🎯" // Sink
            } else {
                "➡️" // Intermediate
            };
            let _ = writeln!(
                content,
                "{}. {} `{}` at line {}",
                i + 1,
                arrow,
                step.description,
                step.location.line
            );
            if !step.code_snippet.is_empty() && step.code_snippet.len() < 80 {
                let _ = write!(
                    content,
                    "   ```java\n   {}\n   ```\n",
                    step.code_snippet.trim()
                );
            }
        }
        content.push('\n');
    }

    // Remediation
    let _ = writeln!(content, "**Remediation:** {}", finding.remediation);

    Hover {
        contents: HoverContents::Scalar(MarkedString::String(content)),
        range: Some(location_to_range(&finding.source_location, source_text)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_finding_to_diagnostic() {
        let finding = Finding::new(
            VulnType::SqlInjection,
            "User input flows to SQL query".to_string(),
            Location {
                file: PathBuf::from("/test/file.java"),
                line: 10,
                column: 5,
                end_line: 10,
                end_column: 30,
            },
            "stmt.executeQuery(\"SELECT * FROM users WHERE id = \" + userId)".to_string(),
        );

        let source_text = "class Test {}\n";
        let diagnostic = finding_to_diagnostic(&finding, source_text, None);

        assert_eq!(finding.severity, Severity::Critical);
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(diagnostic.source, Some("hyperion-security".to_string()));
        assert_eq!(
            diagnostic.code,
            Some(NumberOrString::String("CWE-89".to_string()))
        );
        assert!(diagnostic.message.contains("User input flows to SQL query"));
        assert!(diagnostic.message.contains("PreparedStatement"));
    }

    #[test]
    fn test_severity_conversion() {
        assert_eq!(
            severity_to_diagnostic_severity(Severity::Critical),
            DiagnosticSeverity::WARNING
        );
        assert_eq!(
            severity_to_diagnostic_severity(Severity::High),
            DiagnosticSeverity::WARNING
        );
        assert_eq!(
            severity_to_diagnostic_severity(Severity::Medium),
            DiagnosticSeverity::WARNING
        );
        assert_eq!(
            severity_to_diagnostic_severity(Severity::Low),
            DiagnosticSeverity::INFORMATION
        );
        assert_eq!(
            severity_to_diagnostic_severity(Severity::Info),
            DiagnosticSeverity::INFORMATION
        );
    }

    #[test]
    fn test_location_to_range() {
        let location = Location {
            file: PathBuf::from("/test.java"),
            line: 1,
            column: 3,
            end_line: 1,
            end_column: 5,
        };
        let source_text = "ab👋cd\n";

        let range = location_to_range(&location, source_text);

        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 2);
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, 5);
    }

    #[test]
    fn test_finding_to_hover() {
        let mut finding = Finding::new(
            VulnType::SqlInjection,
            "User input flows to SQL query".to_string(),
            Location {
                file: PathBuf::from("/test/file.java"),
                line: 10,
                column: 5,
                end_line: 10,
                end_column: 30,
            },
            "stmt.executeQuery(query)".to_string(),
        );

        finding.dataflow.push(FlowStep {
            location: Location {
                file: PathBuf::from("/test/file.java"),
                line: 5,
                column: 10,
                end_line: 5,
                end_column: 20,
            },
            description: "Source: HTTP parameter".to_string(),
            code_snippet: "request.getParameter(\"id\")".to_string(),
        });

        finding.dataflow.push(FlowStep {
            location: Location {
                file: PathBuf::from("/test/file.java"),
                line: 10,
                column: 5,
                end_line: 10,
                end_column: 30,
            },
            description: "Sink: SQL query".to_string(),
            code_snippet: "stmt.executeQuery(query)".to_string(),
        });

        let source_text = "class Test {}\n";
        let hover = finding_to_hover(&finding, source_text);

        if let HoverContents::Scalar(MarkedString::String(content)) = hover.contents {
            assert!(content.contains("SQL Injection"));
            assert!(content.contains("CRITICAL"));
            assert!(content.contains("CWE-89"));
            assert!(content.contains("Taint Flow"));
            assert!(content.contains("Source: HTTP parameter"));
            assert!(content.contains("PreparedStatement"));
        } else {
            panic!("Expected scalar string hover content");
        }
    }

    #[test]
    fn test_related_info_uses_source_provider() {
        let mut finding = Finding::new(
            VulnType::SqlInjection,
            "User input flows to SQL query".to_string(),
            Location {
                file: PathBuf::from("/test/file.java"),
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 2,
            },
            "stmt.executeQuery(query)".to_string(),
        );

        finding.dataflow.push(FlowStep {
            location: Location {
                file: PathBuf::from("/test/file.java"),
                line: 1,
                column: 3,
                end_line: 1,
                end_column: 4,
            },
            description: "Source: HTTP parameter".to_string(),
            code_snippet: "request.getParameter(\"id\")".to_string(),
        });

        let source_text = "a👋b\n";
        let source_provider = |_path: &std::path::Path| Some(source_text.to_string());
        let diagnostic = finding_to_diagnostic(&finding, source_text, Some(&source_provider));
        let related = diagnostic.related_information.expect("related info");
        let range = related[0].location.range;

        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 3);
    }

    #[test]
    fn test_code_actions_for_sql_injection() {
        let finding = Finding::new(
            VulnType::SqlInjection,
            "User input flows to SQL query".to_string(),
            Location {
                file: PathBuf::from("/test/file.java"),
                line: 10,
                column: 5,
                end_line: 10,
                end_column: 30,
            },
            "Statement stmt = conn.createStatement();\nstmt.executeQuery(query)".to_string(),
        );

        let uri = Url::parse("file:///test/file.java").unwrap();
        let source_text = "Statement stmt = conn.createStatement();\nstmt.executeQuery(query)\n";
        let actions = finding_to_code_actions(&finding, &uri, source_text);

        assert!(!actions.is_empty());

        if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
            assert!(action.title.contains("PreparedStatement"));
            assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
            assert_eq!(action.is_preferred, Some(true));
        } else {
            panic!("Expected CodeAction");
        }
    }

    #[test]
    fn test_code_actions_for_insecure_random() {
        let finding = Finding::new(
            VulnType::InsecureRandom,
            "Using insecure random for security-sensitive operation".to_string(),
            Location {
                file: PathBuf::from("/test/file.java"),
                line: 10,
                column: 5,
                end_line: 10,
                end_column: 30,
            },
            "double value = Math.random();".to_string(),
        );

        let uri = Url::parse("file:///test/file.java").unwrap();
        let source_text = "double value = Math.random();\n";
        let actions = finding_to_code_actions(&finding, &uri, source_text);

        assert!(!actions.is_empty());

        if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
            assert!(action.title.contains("SecureRandom"));
            assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        } else {
            panic!("Expected CodeAction");
        }
    }
}
