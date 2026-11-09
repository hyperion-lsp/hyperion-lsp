// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Diagnostic Conversion Utilities
//!
//! This module provides utilities for converting parse errors and semantic errors
//! into LSP Diagnostic messages that can be displayed in the editor.

use crate::position_utils::{offset_to_position, text_range_to_lsp_range};
use hyperion_parser::ParseError;
use hyperion_semantics::nullability::{
    DiagnosticKind as NullDiagKind, Nullability, NullabilityDiagnostic,
};
use hyperion_semantics::{SemanticError, SemanticErrorKind};
use serde_json::json;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

/// Converts a ParseError to an LSP Diagnostic.
///
/// This function extracts position information from the parse error's TextRange
/// and creates a properly formatted diagnostic message that can be sent to the client.
///
/// # Arguments
///
/// * `error` - The parse error to convert
/// * `source_text` - The source text of the document (needed to convert byte offsets to line/column)
///
/// # Returns
///
/// An LSP Diagnostic with appropriate range, severity, and message
pub fn parse_error_to_diagnostic(error: &ParseError, source_text: &str) -> Diagnostic {
    let range = text_range_to_lsp_range(error.range, source_text);

    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("hyperion-parser".to_string()),
        message: error.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Converts a SemanticError to an LSP Diagnostic.
///
/// This function handles semantic/type errors such as type mismatches,
/// undefined symbols, and null assignments to primitives.
///
/// # Arguments
///
/// * `error` - The semantic error to convert
/// * `source_text` - The source text of the document (needed to convert byte offsets to line/column)
///
/// # Returns
///
/// An LSP Diagnostic with appropriate range, severity, and message
pub fn semantic_error_to_diagnostic(error: &SemanticError, source_text: &str) -> Diagnostic {
    let range = text_range_to_lsp_range(error.range, source_text);

    // Determine severity based on error kind
    let severity = match &error.kind {
        // All type errors are currently hard errors
        SemanticErrorKind::DeclarationTypeMismatch { .. }
        | SemanticErrorKind::AssignmentTypeMismatch { .. }
        | SemanticErrorKind::NullToPrimitive { .. }
        | SemanticErrorKind::UndefinedSymbol { .. }
        | SemanticErrorKind::UnresolvedReference { .. }
        | SemanticErrorKind::KotlinTypeMismatch { .. }
        | SemanticErrorKind::KotlinArgumentCount { .. }
        | SemanticErrorKind::ValReassignment { .. }
        | SemanticErrorKind::MethodNotFound { .. }
        | SemanticErrorKind::WrongArgumentCount { .. }
        | SemanticErrorKind::MethodArgumentTypeMismatch { .. }
        | SemanticErrorKind::ConstructorNotApplicable { .. }
        | SemanticErrorKind::AbstractInstantiation { .. }
        | SemanticErrorKind::AbstractMethodNotImplemented { .. }
        | SemanticErrorKind::UnreportedException { .. }
        | SemanticErrorKind::NonExhaustiveSwitch { .. }
        | SemanticErrorKind::DuplicateVariable { .. }
        | SemanticErrorKind::StaticContextReference { .. }
        | SemanticErrorKind::FinalAssignment { .. }
        | SemanticErrorKind::ReturnValueMismatch { .. }
        | SemanticErrorKind::ConditionTypeMismatch { .. }
        | SemanticErrorKind::ForEachNotApplicable { .. }
        | SemanticErrorKind::PrimitiveTypeArgument { .. }
        | SemanticErrorKind::FieldNotFound { .. }
        | SemanticErrorKind::InaccessibleMember { .. }
        | SemanticErrorKind::MissingReturn
        | SemanticErrorKind::UnreachableStatement
        | SemanticErrorKind::ReturnTypeMismatch { .. }
        | SemanticErrorKind::NonExhaustiveWhen { .. }
        | SemanticErrorKind::NullableTypeMismatch { .. }
        | SemanticErrorKind::ExtensionFunctionNotInScope { .. }
        | SemanticErrorKind::PatternTypeMismatch { .. }
        | SemanticErrorKind::PatternComponentCountMismatch { .. }
        | SemanticErrorKind::UnknownRecordComponent { .. }
        | SemanticErrorKind::PatternComponentTypeMismatch { .. } => DiagnosticSeverity::ERROR,
    };

    // Generate diagnostic code for filtering/categorization
    let code = Some(NumberOrString::String(error.kind_name().to_string()));

    let data = match &error.kind {
        SemanticErrorKind::UndefinedSymbol {
            suggested_symbol: Some(suggested),
            ..
        } => Some(json!({ "suggested_symbol": suggested })),
        _ => None,
    };

    Diagnostic {
        range,
        severity: Some(severity),
        code,
        code_description: None,
        source: Some("hyperion-type-checker".to_string()),
        message: error.message(),
        related_information: None,
        tags: None,
        data,
    }
}

/// Creates a dependency resolution diagnostic for display in build files.
///
/// This diagnostic warns users when dependencies cannot be resolved,
/// providing actionable information about what's missing and how to fix it.
///
/// # Arguments
///
/// * `coordinates` - Dependency coordinates (e.g., "org.junit:junit:4.12")
/// * `reason` - Human-readable explanation of why the dependency is missing
/// * `line` - Line number in the build file (0-indexed)
///
/// # Returns
///
/// An LSP Diagnostic with warning severity
pub fn missing_dependency_diagnostic(coordinates: &str, reason: &str, line: u32) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: 999, // End of line - client will clamp
            },
        },
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String("missing-dependency".to_string())),
        code_description: None,
        source: Some("hyperion-build".to_string()),
        message: format!("Dependency not resolved: {coordinates}\n{reason}"),
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Converts a NullabilityDiagnostic to an LSP Diagnostic.
///
/// Nullability diagnostics include potential null dereferences, redundant checks,
/// and dead code warnings from dataflow analysis.
///
/// # Arguments
///
/// * `diag` - The nullability diagnostic to convert
/// * `source_text` - The source text of the document (needed to convert byte offsets to line/column)
///
/// # Returns
///
/// An LSP Diagnostic with appropriate range, severity, and message
pub fn nullability_diagnostic_to_lsp(
    diag: &NullabilityDiagnostic,
    source_text: &str,
    unknown_severity: crate::diagnostic_engine::NullabilityUnknownSeverity,
) -> Option<Diagnostic> {
    use crate::diagnostic_engine::NullabilityUnknownSeverity;

    let range = Range {
        start: offset_to_position(source_text, diag.location.start as usize),
        end: offset_to_position(source_text, diag.location.end as usize),
    };

    let is_unknown = diag.nullability == Nullability::Unknown;

    let (severity, code) = match diag.kind {
        NullDiagKind::PotentialNullDereference | NullDiagKind::NullReturnFromNonNull => {
            if is_unknown {
                let severity = match unknown_severity {
                    NullabilityUnknownSeverity::Warning => DiagnosticSeverity::WARNING,
                    NullabilityUnknownSeverity::Info => DiagnosticSeverity::INFORMATION,
                    NullabilityUnknownSeverity::Hint => DiagnosticSeverity::HINT,
                    NullabilityUnknownSeverity::Off => return None,
                };
                let code = match diag.kind {
                    NullDiagKind::PotentialNullDereference => "potential-null-dereference",
                    _ => "null-return-from-nonnull",
                };
                (severity, code)
            } else {
                let code = match diag.kind {
                    NullDiagKind::PotentialNullDereference => "potential-null-dereference",
                    _ => "null-return-from-nonnull",
                };
                (DiagnosticSeverity::WARNING, code)
            }
        }
        NullDiagKind::NullPassedToNonNull => (DiagnosticSeverity::WARNING, "null-to-nonnull"),
        NullDiagKind::RedundantNullCheck => {
            (DiagnosticSeverity::INFORMATION, "redundant-null-check")
        }
        NullDiagKind::DeadCode => (DiagnosticSeverity::HINT, "dead-code-after-null-check"),
    };

    let message = match diag.kind {
        NullDiagKind::PotentialNullDereference => {
            format!(
                "Variable '{}' may be null when dereferenced",
                diag.variable_name
            )
        }
        NullDiagKind::NullPassedToNonNull => {
            format!(
                "Passing potentially null '{}' to a non-null parameter",
                diag.variable_name
            )
        }
        NullDiagKind::NullReturnFromNonNull => {
            format!(
                "Returning potentially null '{}' from @NonNull method",
                diag.variable_name
            )
        }
        NullDiagKind::RedundantNullCheck => {
            format!(
                "Redundant null check: '{}' is already known to be non-null",
                diag.variable_name
            )
        }
        NullDiagKind::DeadCode => "Unreachable code after null check".to_string(),
    };

    Some(Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_string())),
        code_description: None,
        source: Some("hyperion-nullability".to_string()),
        message,
        related_information: None,
        tags: None,
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::symbol::{PrimitiveType, Type};
    use rowan::{TextRange, TextSize};

    #[test]
    fn test_parse_error_to_diagnostic() {
        let text = "class Foo {";
        let error = ParseError::new(
            "Expected '}'",
            TextRange::new(TextSize::from(10), TextSize::from(11)),
        );

        let diagnostic = parse_error_to_diagnostic(&error, text);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.message, "Expected '}'");
        assert_eq!(diagnostic.source, Some("hyperion-parser".to_string()));
        assert_eq!(diagnostic.range.start, Position::new(0, 10));
        assert_eq!(diagnostic.range.end, Position::new(0, 11));
    }

    #[test]
    fn test_missing_dependency_diagnostic() {
        let coordinates = "org.junit.jupiter:junit-jupiter-api:5.13.4";
        let reason =
            "Not found in local Maven repository. Run 'mvn dependency:resolve' to download.";

        let diagnostic = missing_dependency_diagnostic(coordinates, reason, 0);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(diagnostic.source, Some("hyperion-build".to_string()));
        assert_eq!(
            diagnostic.code,
            Some(NumberOrString::String("missing-dependency".to_string()))
        );
        assert!(diagnostic.message.contains(coordinates));
        assert!(diagnostic.message.contains(reason));
        assert_eq!(diagnostic.range.start, Position::new(0, 0));
    }

    #[test]
    fn test_missing_dependency_diagnostic_multiline_message() {
        let coordinates = "com.example:foo:1.0.0";
        let reason = "No version specified";

        let diagnostic = missing_dependency_diagnostic(coordinates, reason, 5);

        assert_eq!(diagnostic.range.start.line, 5);
        assert!(diagnostic.message.contains("Dependency not resolved"));
        assert!(diagnostic.message.contains(coordinates));
        assert!(diagnostic.message.contains(reason));
    }

    #[test]
    fn test_semantic_error_declaration_type_mismatch() {
        let text = "String foo = 1;";
        let error = SemanticError::new(
            TextRange::new(TextSize::from(0), TextSize::from(15)),
            SemanticErrorKind::DeclarationTypeMismatch {
                declared_type: Type::Class(hyperion_semantics::symbol::ClassType::new(
                    "String".to_string(),
                    Some("java.lang".to_string()),
                )),
                actual_type: Type::Primitive(PrimitiveType::Int),
                variable_name: "foo".to_string(),
            },
        );

        let diagnostic = semantic_error_to_diagnostic(&error, text);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.source, Some("hyperion-type-checker".to_string()));
        assert_eq!(
            diagnostic.code,
            Some(NumberOrString::String(
                "type-mismatch-declaration".to_string()
            ))
        );
        assert!(diagnostic
            .message
            .contains("incompatible types: int cannot be converted to"));
    }

    #[test]
    fn test_semantic_error_assignment_type_mismatch() {
        let text = "foo = \"Hey\";";
        let error = SemanticError::new(
            TextRange::new(TextSize::from(0), TextSize::from(12)),
            SemanticErrorKind::AssignmentTypeMismatch {
                variable_type: Type::Primitive(PrimitiveType::Int),
                assigned_type: Type::Class(hyperion_semantics::symbol::ClassType::new(
                    "String".to_string(),
                    Some("java.lang".to_string()),
                )),
                variable_name: "foo".to_string(),
            },
        );

        let diagnostic = semantic_error_to_diagnostic(&error, text);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.source, Some("hyperion-type-checker".to_string()));
        assert_eq!(
            diagnostic.code,
            Some(NumberOrString::String(
                "type-mismatch-assignment".to_string()
            ))
        );
        assert!(diagnostic.message.contains("incompatible types:"));
    }

    #[test]
    fn test_semantic_error_null_to_primitive() {
        let text = "int x = null;";
        let error = SemanticError::new(
            TextRange::new(TextSize::from(0), TextSize::from(13)),
            SemanticErrorKind::NullToPrimitive {
                primitive_type: Type::Primitive(PrimitiveType::Int),
                variable_name: "x".to_string(),
            },
        );

        let diagnostic = semantic_error_to_diagnostic(&error, text);

        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diagnostic
            .message
            .contains("cannot assign null to primitive type"));
    }
}
