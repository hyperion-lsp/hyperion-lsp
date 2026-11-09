// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Utility functions and shared types for the security scanner.

use crate::config::ScanConfig;
use crate::finding::{Finding, Location, Severity};
use crate::logging_context::LoggingContext;
use crate::sinks::SinkRegistry;
use crate::sources::SourceRegistry;
use crate::taint::{TaintState, TrustLevel};

use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::syntax::{AstNode, SourceFile, SyntaxKind, SyntaxNode, TypeDecl};
use rowan::TextRange;
use std::path::Path;

/// Return the declared parameter names of a Java lambda.
pub fn lambda_parameter_names(lambda: &SyntaxNode) -> Vec<String> {
    if lambda.kind() != SyntaxKind::LAMBDA_EXPR {
        return Vec::new();
    }

    lambda
        .children()
        .filter(|child| child.kind() == SyntaxKind::PARAM)
        .filter_map(|param| {
            param
                .descendants_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|token| token.kind().is_identifier())
                .last()
                .map(|token| token.text().to_string())
        })
        .collect()
}

/// Check if a SyntaxKind is an expression kind
pub const fn is_expr_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::PATTERN_INSTANCEOF_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
            | SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::SWITCH_EXPR
    )
}

/// Check if a SyntaxKind is a statement kind
pub const fn is_stmt_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BLOCK_STMT
            | SyntaxKind::EXPR_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::DO_WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOREACH_STMT
            | SyntaxKind::SWITCH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::TRY_WITH_RESOURCES_STMT
            | SyntaxKind::SYNCHRONIZED_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::THROW_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::ASSERT_STMT
            | SyntaxKind::EMPTY_STMT
            | SyntaxKind::LABELED_STMT
            | SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION
            | SyntaxKind::LOCAL_VAR_DECL
            | SyntaxKind::YIELD_STMT
    )
}

pub fn looks_like_identifier_expr(text: &str) -> bool {
    let lowered = text.trim().to_lowercase();
    if lowered.is_empty() {
        return false;
    }
    if lowered.contains('"') || lowered.contains('\'') {
        return false;
    }
    if lowered.contains(' ') || lowered.contains('\n') || lowered.contains('\r') {
        return false;
    }
    lowered.ends_with("id")
        || lowered.ends_with("id)")
        || lowered.contains("getid")
        || lowered.contains("uuid")
        || lowered.contains("guid")
}

pub const fn lower_severity(severity: Severity) -> Severity {
    match severity {
        Severity::Critical => Severity::High,
        Severity::High => Severity::Medium,
        Severity::Medium => Severity::Low,
        Severity::Low | Severity::Info => Severity::Info,
    }
}

pub const fn confidence_for_taint(taint_state: &TaintState) -> f32 {
    match taint_state.trust_level() {
        Some(TrustLevel::Untrusted) => 0.9,
        Some(TrustLevel::SemiTrusted) => 0.7,
        Some(TrustLevel::Derived) => 0.5,
        None => 0.6,
    }
}

/// Helper function to get location from a node without capturing self
pub fn location_from_node(node: &SyntaxNode, source: &str, file: &Path) -> Location {
    let range = node.text_range();
    let start_offset: usize = range.start().into();
    let end_offset: usize = range.end().into();

    let (start_line, start_col) = offset_to_line_col(source, start_offset);
    let (end_line, end_col) = offset_to_line_col(source, end_offset);

    Location {
        file: file.to_path_buf(),
        line: start_line,
        column: start_col,
        end_line,
        end_column: end_col,
    }
}

/// Helper function to get code snippet from a node
pub fn code_snippet_from_node(node: &SyntaxNode, _source: &str) -> String {
    node.text().to_string()
}

pub fn is_suppressed(finding: &Finding, source: &str) -> bool {
    let vuln_token = finding.vuln_type.as_str();
    let cwe_token = finding.vuln_type.cwe_id().to_lowercase();
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return false;
    }

    let line_index = finding.source_location.line.saturating_sub(1) as usize;

    let check_line = |line: &str, allow_next_line: bool| -> bool {
        let lower = line.to_lowercase();
        if lower.contains("hyperion-security: ignore-next-line") {
            return allow_next_line;
        }
        if !lower.contains("hyperion-security: ignore") {
            return false;
        }
        let after = lower
            .split("hyperion-security: ignore")
            .nth(1)
            .unwrap_or("");
        let after = after.trim();
        if after.is_empty() {
            return true;
        }
        after.contains(vuln_token) || after.contains(&cwe_token)
    };

    if line_index < lines.len() && check_line(lines[line_index], false) {
        return true;
    }
    if line_index > 0 && check_line(lines[line_index - 1], false) {
        return true;
    }
    if line_index > 0 && check_line(lines[line_index - 1], true) {
        return true;
    }

    false
}

pub fn offset_to_line_col(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    let mut skip_next_lf = false;

    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if skip_next_lf {
            skip_next_lf = false;
            if ch == '\n' {
                continue;
            }
        }
        if ch == '\r' {
            if source.as_bytes().get(idx + 1) == Some(&b'\n') {
                skip_next_lf = true;
            }
            line += 1;
            col = 1;
            continue;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
            continue;
        }

        col += 1;
    }

    (line, col)
}

pub fn method_called_outside_range(source: &str, name: &str, range: TextRange) -> bool {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    let bytes = source.as_bytes();

    let is_ident_char = |byte: u8| (byte as char).is_ascii_alphanumeric() || byte == b'_';

    for (idx, _) in source.match_indices(name) {
        if idx >= start && idx < end {
            continue;
        }

        if idx > 0 && is_ident_char(bytes[idx - 1]) {
            continue;
        }

        let mut cursor = idx + name.len();
        if cursor < bytes.len() && is_ident_char(bytes[cursor]) {
            continue;
        }

        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        if cursor < bytes.len() && bytes[cursor] == b'(' {
            return true;
        }
    }

    false
}

pub fn collect_dead_code_ranges(source_file: &SourceFile, source: &str) -> Vec<(u32, u32)> {
    let mut dead_ranges = Vec::new();

    for type_decl in source_file.types() {
        let TypeDecl::Class(class_decl) = type_decl else {
            continue;
        };

        for method in class_decl.methods() {
            let range = method.syntax().text_range();
            let (start_line, _) = offset_to_line_col(source, range.start().into());
            let (end_line, _) = offset_to_line_col(source, range.end().into());

            // The shared lookup finds the modifier list beside the method, where the parser
            // places it, and reads the keywords inside their wrapper nodes.
            let is_private = declaration_modifiers(method.syntax()).is_private();

            if !is_private || method.is_abstract() {
                continue;
            }

            let Some(name) = method.name() else {
                continue;
            };
            // Java invokes serialization hooks reflectively. They are deliberately
            // private and normally have no source-level call site, so treating them
            // as dead code would hide findings in attacker-controlled callbacks.
            if matches!(
                name.text(),
                "readObject"
                    | "readObjectNoData"
                    | "readResolve"
                    | "writeObject"
                    | "writeReplace"
                    | "validateObject"
            ) {
                continue;
            }
            if method_called_outside_range(source, name.text(), range) {
                continue;
            }
            dead_ranges.push((start_line, end_line));
        }
    }

    dead_ranges
}

/// Scan statistics for error tracking
#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    /// Total files scanned
    pub files_scanned: usize,
    /// Files skipped due to cache
    pub files_cached: usize,
    /// Files that had parse errors
    pub parse_errors: usize,
    /// Files that timed out
    pub timeouts: usize,
    /// Total vulnerabilities found
    pub total_findings: usize,
    /// Findings per rule (rule_id or fallback to vuln type)
    pub findings_by_rule: std::collections::BTreeMap<String, usize>,
}

/// Security scanner
pub struct SecurityScanner {
    pub(crate) config: ScanConfig,
    pub(crate) source_registry: SourceRegistry,
    pub(crate) sink_registry: SinkRegistry,
    pub(crate) sanitizer_registry: crate::sanitizers::SanitizerRegistry,
    pub(crate) logging_context: std::sync::RwLock<LoggingContext>,
}
