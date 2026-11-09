// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Type reference extraction - handles type usages in declarations and inheritance

use crate::{import_resolver::ImportResolver, FileId, SymbolLocation, TextRange};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use lsp_types::SymbolKind;
use tracing::debug;

use super::Usage;

/// Extract type reference from CLASS_TYPE nodes
///
/// This captures type usages in:
/// - Field declarations: `private User user;`
/// - Local variable declarations: `User user = ...;`
/// - Method parameters: `void process(User user)`
/// - Method return types: `User getUser()`
/// - Generic type arguments: `List<User>`
pub fn extract_type_reference(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    owner_fqn: Option<&str>,
    usages: &mut Vec<Usage>,
) {
    // Skip if this is inside a NEW_EXPR - that's handled by extract_constructor_call
    if let Some(parent) = node.parent() {
        if parent.kind() == SyntaxKind::NEW_EXPR {
            return;
        }
    }

    // Extract the type name from CLASS_TYPE
    // CLASS_TYPE may contain: QUALIFIED_NAME (with IDENTs inside), TYPE_ARGS
    let mut name_parts = Vec::new();
    let mut first_ident_range: Option<rowan::TextRange> = None;
    let mut last_ident_range: Option<rowan::TextRange> = None;

    // The class name might be in a QUALIFIED_NAME child
    for elem in node.children() {
        if elem.kind() == SyntaxKind::QUALIFIED_NAME {
            // Extract IDENTs from QUALIFIED_NAME
            for token in elem.children_with_tokens() {
                if let rowan::NodeOrToken::Token(t) = token {
                    if t.kind().is_identifier() {
                        if first_ident_range.is_none() {
                            first_ident_range = Some(t.text_range());
                        }
                        last_ident_range = Some(t.text_range());
                        name_parts.push(t.text().to_string());
                    }
                }
            }
        }
    }

    // If no QUALIFIED_NAME, check for direct IDENT tokens
    if name_parts.is_empty() {
        for elem in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = elem {
                if token.kind().is_identifier() {
                    if first_ident_range.is_none() {
                        first_ident_range = Some(token.text_range());
                    }
                    last_ident_range = Some(token.text_range());
                    name_parts.push(token.text().to_string());
                }
            }
        }
    }

    // Fallback: handle nested qualified names that aren't direct children of CLASS_TYPE.
    if name_parts.len() <= 1 {
        if let Some(qualified) = node
            .descendants()
            .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
        {
            let mut qualified_parts = Vec::new();
            let mut qualified_first: Option<rowan::TextRange> = None;
            let mut qualified_last: Option<rowan::TextRange> = None;

            for token in qualified.children_with_tokens() {
                if let rowan::NodeOrToken::Token(t) = token {
                    if t.kind().is_identifier() {
                        if qualified_first.is_none() {
                            qualified_first = Some(t.text_range());
                        }
                        qualified_last = Some(t.text_range());
                        qualified_parts.push(t.text().to_string());
                    }
                }
            }

            if qualified_parts.len() > name_parts.len() {
                name_parts = qualified_parts;
                first_ident_range = qualified_first.or(first_ident_range);
                last_ident_range = qualified_last.or(last_ident_range);
            }
        }
    }

    if name_parts.is_empty() {
        return;
    }

    // Use the first identifier as the simple class name for resolution
    let simple_name = &name_parts[0];
    let qualified_name = name_parts.join(".");
    let resolution = if qualified_name.contains('.') {
        resolver.resolve_type_identity_in_owner(&qualified_name, owner_fqn)
    } else {
        resolver.resolve_type_identity_in_owner(simple_name, owner_fqn)
    };
    let crate::TypeNameResolution::Exact(fqn) = resolution else {
        return;
    };

    let range = if qualified_name.contains('.') {
        last_ident_range.unwrap_or(node.text_range())
    } else {
        first_ident_range.unwrap_or(node.text_range())
    };
    let symbol_name = if qualified_name.contains('.') {
        name_parts
            .last()
            .cloned()
            .unwrap_or_else(|| simple_name.clone())
    } else {
        simple_name.clone()
    };

    debug!("Extracted type reference: {} -> {}", qualified_name, fqn);

    let location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: TextRange::new(range.start().into(), range.end().into()),
        kind: SymbolKind::CLASS,
        name: symbol_name.into(),
        fqn: fqn.clone().into(),
        container_name: None,
        documentation: None,
    };

    usages.push(Usage { fqn, location });
}
