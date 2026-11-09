// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Documentation extraction and formatting for hover
//!
//! Functions that extract JavaDoc comments and format them for display

use hyperion_parser::{syntax::SyntaxKind, SyntaxNode};

/// Extract documentation comment from a declaration node
pub fn extract_documentation(node: &SyntaxNode) -> Option<String> {
    // Look for DOC_COMMENT tokens immediately preceding this node
    // JavaDoc comments appear before the declaration
    let parent = node.parent()?;

    // Search through siblings to find doc comments immediately before this node
    let mut doc_comments = Vec::new();
    let mut found_current = false;

    for sibling in parent.children_with_tokens() {
        if let rowan::NodeOrToken::Node(sibling_node) = &sibling {
            if sibling_node == node {
                found_current = true;
                break;
            }
            // If we encounter another non-trivial node (like another method/field),
            // clear accumulated doc comments since they belong to that node
            if matches!(
                sibling_node.kind(),
                SyntaxKind::METHOD_DECL
                    | SyntaxKind::FIELD_DECL
                    | SyntaxKind::CONSTRUCTOR_DECL
                    | SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
            ) {
                doc_comments.clear();
            }
        }

        if let rowan::NodeOrToken::Token(token) = sibling {
            if token.kind() == SyntaxKind::DOC_COMMENT {
                doc_comments.push(token.text().to_string());
            }
        }
    }

    if !found_current || doc_comments.is_empty() {
        return None;
    }

    // Join all doc comments and clean them up
    let full_doc = doc_comments.join("\n");
    Some(format_javadoc(&full_doc))
}

/// Format a JavaDoc comment for display in hover
pub fn format_javadoc(doc: &str) -> String {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for line in doc.lines() {
        let trimmed = line.trim();

        // Remove JavaDoc markers
        let cleaned = if trimmed.starts_with("/**") {
            trimmed.strip_prefix("/**").unwrap_or(trimmed).trim()
        } else if trimmed.starts_with("*/") {
            continue; // Skip closing marker
        } else if trimmed.starts_with('*') {
            trimmed.strip_prefix("*").unwrap_or(trimmed).trim()
        } else {
            trimmed
        };

        // Handle @tags
        if cleaned.starts_with("@param ") || cleaned.starts_with("@return ") {
            if !in_code_block {
                lines.push(String::new()); // Add blank line before tags
            }
            lines.push(format!("**{cleaned}**"));
        } else if cleaned.starts_with("@throws ") || cleaned.starts_with("@exception ") {
            if !in_code_block {
                lines.push(String::new());
            }
            lines.push(format!("**{cleaned}**"));
        } else if cleaned.starts_with("@deprecated") {
            if !in_code_block {
                lines.push(String::new());
            }
            lines.push(format!("**Warning: {cleaned}**"));
        } else if cleaned.starts_with('@') {
            // Other tags
            if !in_code_block {
                lines.push(String::new());
            }
            lines.push(format!("*{cleaned}*"));
        } else if !cleaned.is_empty() {
            lines.push(cleaned.to_string());
        }

        // Track code blocks
        if cleaned.contains("```") {
            in_code_block = !in_code_block;
        }
    }

    lines.join("\n")
}
