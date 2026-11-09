// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Qualified member access detection (obj.member patterns)

use hyperion_parser::syntax::SyntaxKind;
use rowan::SyntaxToken;
use tracing::debug;

use super::extraction::{extract_name_from_paren, extract_receiver};

/// Detect if the token is part of a qualified member access (obj.member) or method chain
///
/// Returns (receiver, member) if qualified access is detected.
/// For chains like a.b().c(), returns the receiver and immediate member for the cursor position.
pub fn detect_qualified_access(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
) -> Option<(String, String)> {
    let member_name = token.text().to_string();
    debug!(
        "DETECT: Checking qualified access for token '{}'",
        member_name
    );

    // Check if the token is an identifier followed by METHOD_CALL_EXPR
    // and preceded by DOT and a receiver
    // Pattern: receiver . member ()
    //                      ^token

    let parent = token.parent()?;
    debug!("DETECT: Token parent: {:?}", parent.kind());

    // Look for pattern in siblings: NAME_EXPR DOT IDENT(member) METHOD_CALL_EXPR
    // or: IDENT(receiver) DOT IDENT(member) METHOD_CALL_EXPR
    // or: NAME_EXPR DOT IDENT DOT IDENT(member) for qualified names

    let children: Vec<_> = parent.children_with_tokens().collect();

    // Find our token's position
    let token_pos = children
        .iter()
        .position(|child| matches!(child, rowan::NodeOrToken::Token(t) if t == token))?;

    debug!("DETECT: Token at position {}", token_pos);

    // Look backwards from token to find the receiver parts
    // We should find: ... receiver_parts DOT(at token_pos-1) member_token
    if token_pos == 0 {
        debug!("DETECT: Token at position 0, no receiver before it");
        return None;
    }

    // Check if there's a DOT before our token
    if token_pos < 1 {
        return None;
    }

    let prev_pos = token_pos - 1;
    if let rowan::NodeOrToken::Token(t) = &children[prev_pos] {
        if t.kind() != SyntaxKind::DOT {
            debug!("DETECT: No DOT before token");
            return None;
        }
    } else {
        debug!("DETECT: Previous element is not a DOT token");
        return None;
    }

    // Collect receiver parts by looking at all children before the DOT that precedes our token
    // We need to collect qualified name segments like: Config.Limits or com.example.StringUtils
    let mut receiver_parts = Vec::new();
    let mut expecting_ident_or_name_expr = true;
    let mut i = prev_pos as isize - 1;

    // Work backwards from before the final DOT, collecting identifier parts
    while i >= 0 {
        let idx = i as usize;
        if idx >= children.len() {
            break;
        }

        match &children[idx] {
            rowan::NodeOrToken::Token(t) => {
                match t.kind() {
                    SyntaxKind::IDENT => {
                        if expecting_ident_or_name_expr {
                            receiver_parts.push(t.text().to_string());
                            expecting_ident_or_name_expr = false;
                        } else {
                            break; // We have an IDENT but we're expecting a DOT
                        }
                    }
                    SyntaxKind::SUPER_KW => {
                        if expecting_ident_or_name_expr {
                            receiver_parts.push("super".to_string());
                            break;
                        } else {
                            break;
                        }
                    }
                    SyntaxKind::THIS_KW => {
                        if expecting_ident_or_name_expr {
                            receiver_parts.push("this".to_string());
                            break;
                        } else {
                            break;
                        }
                    }
                    SyntaxKind::DOT => {
                        if expecting_ident_or_name_expr {
                            break; // Two DOTs in a row doesn't make sense
                        }
                        expecting_ident_or_name_expr = true;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => {
                        // Any other token breaks the pattern
                        break;
                    }
                }
            }
            rowan::NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::NAME_EXPR && expecting_ident_or_name_expr {
                    // NAME_EXPR is the start of the chain
                    let name = n.text().to_string().trim().to_string();
                    receiver_parts.push(name);
                    break; // NAME_EXPR is the start, we're done
                } else if n.kind() == SyntaxKind::PAREN_EXPR && expecting_ident_or_name_expr {
                    if let Some(name) = extract_name_from_paren(n) {
                        receiver_parts.push(name);
                        break;
                    }
                } else if n.kind() == SyntaxKind::NEW_EXPR && expecting_ident_or_name_expr {
                    // NEW_EXPR like "new QueryBuilder()" - extract the class name
                    // NEW_EXPR contains: NEW_KW, CLASS_TYPE (or array type), METHOD_CALL_EXPR
                    for child in n.children() {
                        if child.kind() == SyntaxKind::CLASS_TYPE {
                            // Extract the class name from CLASS_TYPE
                            let class_name = child.text().to_string().trim().to_string();
                            receiver_parts.push(class_name);
                            break;
                        }
                    }
                    break; // NEW_EXPR is the start, we're done
                } else if n.kind() == SyntaxKind::SUPER_EXPR && expecting_ident_or_name_expr {
                    receiver_parts.push("super".to_string());
                    break;
                } else if n.kind() == SyntaxKind::THIS_EXPR && expecting_ident_or_name_expr {
                    receiver_parts.push("this".to_string());
                    break;
                } else if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR && expecting_ident_or_name_expr
                {
                    // FIELD_ACCESS_EXPR is the full receiver chain
                    // e.g., Color.RED in Color.RED.getRed()
                    // Collect all identifiers from it to form the qualified name
                    let text = n.text().to_string();
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        receiver_parts.push(trimmed.to_string());
                    }
                    break;
                } else if n.kind() == SyntaxKind::METHOD_CALL_EXPR {
                    // Skip METHOD_CALL_EXPR nodes (e.g., method chain receivers)
                } else {
                    break; // Unknown node type, stop
                }
            }
        }
        i -= 1;
    }

    if !receiver_parts.is_empty() {
        receiver_parts.reverse(); // We collected backwards, so reverse to get correct order
        let receiver_name = receiver_parts.join(".");
        debug!("DETECT: Found qualified access pattern!");
        debug!(
            "DETECT: Receiver: Some({}), Member: {}",
            receiver_name, member_name
        );
        return Some((receiver_name, member_name));
    }

    // Fallback: Also try the old method for FIELD_ACCESS_EXPR (in case some parsers create it)
    let mut current = token.parent();
    let mut depth = 0;

    while let Some(node) = current {
        if node.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
            debug!("DETECT: Found FIELD_ACCESS_EXPR (old path)");
            if let Some(receiver) = extract_receiver(&node) {
                return Some((receiver, member_name));
            }
        }

        current = node.parent();
        depth += 1;
        if depth > 10 {
            break;
        }
    }

    debug!("DETECT: No qualified access pattern found");
    None
}
