// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Receiver and identifier extraction helpers for chain parsing

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use tracing::debug;

use super::segment_parser::is_receiver_expr_kind;

/// Extract the full receiver from a FIELD_ACCESS_EXPR node
///
/// For `obj.member` or `foo.bar.baz`, returns the full chain before the last DOT
/// Examples:
/// - `obj.member` -> "obj"
/// - `com.example.utils.StringUtils.isEmpty` -> "com.example.utils.StringUtils"
/// - `Outer.Inner.helper` -> "Outer.Inner"
pub fn extract_receiver(node: &SyntaxNode) -> Option<String> {
    extract_full_receiver_chain(node)
}

/// Recursively extract the full qualified receiver chain from a FIELD_ACCESS_EXPR
///
/// This collects all identifiers and dots to build the complete receiver name
fn extract_full_receiver_chain(node: &SyntaxNode) -> Option<String> {
    let mut parts = Vec::new();

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::IDENT {
                    parts.push(t.text().to_string());
                } else if t.kind() == SyntaxKind::SUPER_KW {
                    parts.push("super".to_string());
                } else if t.kind() == SyntaxKind::DOT {
                    // Skip the dot - we'll add it when joining
                }
            }
            rowan::NodeOrToken::Node(n) => {
                // Recursively handle nested field access
                if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                    if let Some(chain) = extract_full_receiver_chain(&n) {
                        // This nested chain should become the receiver
                        return Some(chain);
                    }
                } else if n.kind() == SyntaxKind::SUPER_EXPR {
                    parts.push("super".to_string());
                } else if let Some(ident) = extract_first_identifier(&n) {
                    // For other node types, just get the first identifier
                    return Some(ident);
                }
            }
        }
    }

    // If we have multiple parts, it's a chain like foo.bar.baz
    // We need to return all but the last part (which is the member we're accessing)
    match parts.len().cmp(&1) {
        std::cmp::Ordering::Greater => {
            // Remove the last part (the member being accessed)
            parts.pop();
            Some(parts.join("."))
        }
        std::cmp::Ordering::Equal => Some(parts[0].clone()),
        std::cmp::Ordering::Less => None,
    }
}

/// Extract the first identifier from a node (recursively)
///
/// This function traverses the AST with a depth limit to prevent stack overflow
/// on maliciously crafted or extremely deeply nested structures.
/// 30 levels is sufficient for any reasonable Java expression while staying safe.
pub fn extract_first_identifier(node: &SyntaxNode) -> Option<String> {
    const MAX_DEPTH: u32 = 30;
    extract_first_identifier_internal(node, 0, MAX_DEPTH)
}

/// Internal helper for extract_first_identifier with depth tracking
fn extract_first_identifier_internal(
    node: &SyntaxNode,
    current_depth: u32,
    max_depth: u32,
) -> Option<String> {
    // Prevent unbounded recursion on deeply nested or cyclic structures
    if current_depth >= max_depth {
        debug!(
            "WARN: extract_first_identifier reached max depth {} - preventing potential stack overflow",
            max_depth
        );
        return None;
    }

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::IDENT {
                    return Some(t.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(n) => {
                if let Some(ident) =
                    extract_first_identifier_internal(&n, current_depth + 1, max_depth)
                {
                    return Some(ident);
                }
            }
        }
    }
    None
}

/// Extract the receiver text from a method reference
/// Handles:
/// - Simple identifiers: "String"
/// - Qualified expressions: "System.out" -> extracts "out"
/// - Array types: "String[]" -> extracts "String"
pub fn extract_receiver_text(node: &SyntaxNode) -> String {
    match node.kind() {
        // For array types like String[], extract the base type
        SyntaxKind::ARRAY_TYPE => {
            // Find the CLASS_TYPE or PRIMITIVE_TYPE child
            for child in node.children() {
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE
                ) {
                    return extract_receiver_text(&child);
                }
            }
            // Fallback: return the text without brackets
            node.text().to_string().replace("[]", "")
        }

        // For qualified expressions like System.out, extract the last identifier
        SyntaxKind::QUALIFIED_NAME | SyntaxKind::FIELD_ACCESS_EXPR => {
            // Get the last identifier in the chain
            let mut last_ident = None;
            for token in node.descendants_with_tokens() {
                if let Some(t) = token.as_token() {
                    if t.kind() == SyntaxKind::IDENT {
                        last_ident = Some(t.text().to_string());
                    }
                }
            }
            last_ident.unwrap_or_else(|| node.text().to_string())
        }

        // For simple types, extract the identifier
        SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE => {
            extract_first_identifier(node).unwrap_or_else(|| node.text().to_string())
        }

        // Default: just get the first identifier
        _ => extract_first_identifier(node).unwrap_or_else(|| node.text().to_string()),
    }
}

/// Extract cast type and base from a PAREN_EXPR containing a CAST_EXPR
/// Returns (cast_type, base_name) if this is a cast expression
pub(super) fn extract_cast_from_paren(paren_node: &SyntaxNode) -> Option<(String, String)> {
    // Look for CAST_EXPR child in PAREN_EXPR
    for child in paren_node.children() {
        if child.kind() == SyntaxKind::CAST_EXPR {
            // CAST_EXPR structure: L_PAREN, type, R_PAREN, expression
            let cast_type = extract_cast_type(&child);
            let mut base_expr = None;
            let mut past_rparen = false;

            for item in child.children_with_tokens() {
                match item {
                    rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::R_PAREN => {
                        past_rparen = true;
                    }
                    rowan::NodeOrToken::Node(n)
                        if past_rparen
                        // This is the base expression (after R_PAREN)
                        && n.kind() == SyntaxKind::NAME_EXPR =>
                    {
                        base_expr = Some(n.text().to_string().trim().to_string());
                    }
                    _ => {}
                }
            }

            if let (Some(typ), Some(base)) = (cast_type, base_expr) {
                return Some((typ, base));
            }
        }
    }
    None
}

/// Preserve the complete written type of a cast, including qualification,
/// generic arguments, and array dimensions. Treating the whole `CAST_EXPR` as
/// a type also captures the operand (`(Type[]) value`) and creates a fictitious
/// class name.
pub(super) fn extract_cast_type(cast_node: &SyntaxNode) -> Option<String> {
    (cast_node.kind() == SyntaxKind::CAST_EXPR)
        .then(|| {
            cast_node.children().find(|child| {
                matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE
                        | SyntaxKind::PRIMITIVE_TYPE
                        | SyntaxKind::ARRAY_TYPE
                        | SyntaxKind::INTERSECTION_TYPE
                )
            })
        })
        .flatten()
        .map(|type_node| type_node.text().to_string().trim().to_string())
}

/// Extract name from a simple PAREN_EXPR (not a cast)
pub(super) fn extract_name_from_paren(paren_node: &SyntaxNode) -> Option<String> {
    for child in paren_node.children() {
        if child.kind() == SyntaxKind::NAME_EXPR {
            return Some(child.text().to_string().trim().to_string());
        }
        if child.kind() == SyntaxKind::TERNARY_EXPR {
            let mut exprs = Vec::new();
            for expr in child.children() {
                if is_receiver_expr_kind(expr.kind()) {
                    exprs.push(expr);
                }
            }
            if let Some(branch) = exprs.get(1).or_else(|| exprs.get(2)) {
                if let Some(ident) = extract_first_identifier(branch) {
                    return Some(ident);
                }
            }
        }
    }
    None
}
