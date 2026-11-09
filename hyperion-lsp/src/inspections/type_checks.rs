// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type-related inspections: raw type usage and narrowing conversions.

use crate::position_utils::text_range_to_lsp_range;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

/// Check for raw type usage (generics without type arguments)
/// e.g., `List list = new ArrayList();` instead of `List<String> list = new ArrayList<>();`
pub(crate) fn check_raw_type_usage(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Common generic types that should have type arguments
    let generic_types = [
        "List",
        "Set",
        "Map",
        "Collection",
        "Iterable",
        "ArrayList",
        "LinkedList",
        "HashSet",
        "TreeSet",
        "HashMap",
        "TreeMap",
        "LinkedHashMap",
        "Optional",
        "Stream",
        "Supplier",
        "Consumer",
        "Function",
        "Predicate",
        "Comparable",
        "Comparator",
        "Iterator",
        "Queue",
        "Deque",
        "Stack",
    ];

    for node in syntax.descendants() {
        // Check variable declarations
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL || node.kind() == SyntaxKind::FIELD_DECL {
            for child in node.children() {
                if child.kind() == SyntaxKind::CLASS_TYPE {
                    let type_text = child.text().to_string();
                    let type_name = type_text.split('<').next().unwrap_or(&type_text).trim();

                    // Check if this is a generic type without type arguments
                    if generic_types.contains(&type_name) && !type_text.contains('<') {
                        let range = child.text_range();
                        let lsp_range = text_range_to_lsp_range(range, source_text);

                        diagnostics.push(Diagnostic {
                            range: lsp_range,
                            severity: Some(DiagnosticSeverity::WARNING),
                            code: Some(NumberOrString::String("raw_type".to_string())),
                            code_description: None,
                            source: Some("hyperion-lint".to_string()),
                            message: format!("Raw use of parameterized type '{type_name}'. Consider using generic type arguments."),
                            related_information: None,
                            tags: None,
                            data: None,
                        });
                    }
                }
            }
        }
    }

    diagnostics
}
