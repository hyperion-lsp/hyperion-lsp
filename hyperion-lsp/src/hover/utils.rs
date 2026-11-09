// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Utility functions for hover

use hyperion_parser::{syntax::SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, SyntaxToken, TextSize};

/// Find token at a specific offset in the syntax tree
pub fn find_token_at_offset(
    root: &SyntaxNode,
    offset: u32,
) -> Option<SyntaxToken<hyperion_parser::JavaLanguage>> {
    let token_offset = root.token_at_offset(TextSize::from(offset));

    let right = token_offset.clone().right_biased();
    let left = token_offset.left_biased();

    match (left, right) {
        (Some(l), Some(r)) => {
            let offset_size = TextSize::from(offset);
            let l_contains = l.text_range().contains(offset_size);
            let r_contains = r.text_range().contains(offset_size);

            // Cursor can sit exactly on the boundary between an identifier and
            // following punctuation (e.g. `methodName<|>(...)`). Prefer the
            // identifier token in that case for richer hover/navigation.
            let at_boundary = l.text_range().end() == offset_size
                && r.text_range().start() == offset_size
                && l.kind().is_identifier()
                && !r.kind().is_identifier();
            if at_boundary {
                return Some(l);
            }

            if l_contains && !r_contains {
                return Some(l);
            }
            if r_contains && !l_contains {
                return Some(r);
            }
            if l.kind().is_identifier() && !r.kind().is_identifier() {
                Some(l)
            } else if r.kind().is_identifier() {
                Some(r)
            } else if l.kind().is_identifier() {
                Some(l)
            } else {
                Some(r)
            }
        }
        (Some(l), None) => Some(l),
        (None, Some(r)) => Some(r),
        (None, None) => None,
    }
}

/// Find an ancestor node of a specific kind
pub fn find_ancestor(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    kind: SyntaxKind,
) -> Option<SyntaxNode> {
    token.parent_ancestors().find(|n| n.kind() == kind)
}

/// Find an identifier token with a specific name in a node
/// Returns the token if found
pub fn find_identifier_in_node(
    node: &SyntaxNode,
    name: &str,
) -> Option<SyntaxToken<hyperion_parser::JavaLanguage>> {
    for child in node.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = child {
            if token.kind().is_identifier() && token.text() == name {
                return Some(token);
            }
        }
    }
    None
}
