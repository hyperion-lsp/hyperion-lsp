// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Chain segment parsing from syntax tokens

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::SyntaxToken;
use tracing::debug;

use super::extraction::{extract_cast_from_paren, extract_cast_type, extract_name_from_paren};
use super::ChainSegment;

/// Parse a method chain from a syntax token
///
/// For example, `user.getProfile().getName()` would be parsed as:
/// ["user" (field), "getProfile" (method), "getName" (method)]
///
/// Returns (chain, target_index) where target_index is the index of the cursor position
pub fn parse_method_chain(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
) -> Option<(Vec<ChainSegment>, usize)> {
    let target_name = token.text().to_string();
    if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
        eprintln!(
            "DEBUG SEGMENT_PARSER: token kind={:?} text='{}'",
            token.kind(),
            target_name
        );
    }
    debug!(
        "CHAIN: Parsing chain for target '{}' (kind: {:?})",
        target_name,
        token.kind()
    );

    let parent = token.parent()?;
    let mut chain_parent = parent.clone();
    if parent.kind() != SyntaxKind::FIELD_ACCESS_EXPR {
        if let Some(ancestor) = parent
            .ancestors()
            .find(|node| node.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
        {
            chain_parent = ancestor;
        }
    }
    let parent_kind = chain_parent.kind();
    debug!("CHAIN: Parent: {:?}", parent_kind);

    // Special handling for METHOD_REF_EXPR (method references like User::getName)
    // Structure: NAME_EXPR(User) COLON_COLON METHOD_REF_EXPR(getName)
    // Grandparent contains all three elements as siblings
    if parent_kind == SyntaxKind::METHOD_REF_EXPR {
        debug!("CHAIN: Detected METHOD_REF_EXPR (method reference)");

        if let Some(grandparent) = parent.parent() {
            // Extract receiver (the class/type before ::)
            let mut receiver = None;
            let mut found_colon_colon = false;

            for child in grandparent.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::COLON_COLON => {
                        found_colon_colon = true;
                    }
                    rowan::NodeOrToken::Node(n)
                        if !found_colon_colon
                            && (n.kind() == SyntaxKind::NAME_EXPR
                                || n.kind() == SyntaxKind::CLASS_TYPE) =>
                    {
                        receiver = Some(n.text().to_string().trim().to_string());
                    }
                    _ => {}
                }
            }

            if let Some(receiver_name) = receiver {
                // Return a 2-segment chain: [receiver, method]
                let method_name = token.text().to_string();
                debug!(
                    "CHAIN: Method reference pattern: {}::{}",
                    receiver_name, method_name
                );

                let segments = vec![
                    ChainSegment {
                        name: receiver_name,
                        is_method_call: false,
                        cast_type: None,
                        call_range: None,
                    },
                    ChainSegment {
                        name: method_name,
                        is_method_call: true, // Method references refer to methods
                        cast_type: None,
                        call_range: None,
                    },
                ];

                debug!(
                    "CHAIN: Parsed method reference: {} segments, target at index 1",
                    segments.len()
                );
                return Some((segments, 1)); // Target is at index 1 (the method)
            }
        }
    }

    // With nested trees, METHOD_CALL_EXPR directly contains: receiver + DOT + IDENT + ARG_LIST
    // Handle this when the IDENT's parent is METHOD_CALL_EXPR and contains a DOT.
    if parent.kind() == SyntaxKind::METHOD_CALL_EXPR {
        let has_dot = parent
            .children_with_tokens()
            .any(|c| c.kind() == SyntaxKind::DOT);
        if has_dot {
            if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
                eprintln!("DEBUG SEGMENT_PARSER: using parent METHOD_CALL_EXPR");
            }
            debug!("CHAIN: Detected METHOD_CALL_EXPR parent with DOT (nested tree)");
            let segments = parse_nested_method_call_chain(&parent, token);
            if let Some((segs, idx)) = segments {
                if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
                    let rendered = segs
                        .iter()
                        .map(|s| format!("{}{}", s.name, if s.is_method_call { "()" } else { "" }))
                        .collect::<Vec<_>>()
                        .join(".");
                    eprintln!("DEBUG SEGMENT_PARSER: nested chain={rendered} target_index={idx}");
                }
                debug!(
                    "CHAIN: Returning {} segments with target at index {}",
                    segs.len(),
                    idx
                );
                return Some((segs, idx));
            }
            if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
                eprintln!("DEBUG SEGMENT_PARSER: nested parse returned None");
            }
        }
    } else if let Some(method_call) = parent
        .ancestors()
        .find(|n| n.kind() == SyntaxKind::METHOD_CALL_EXPR)
    {
        // Common shape: IDENT → NAME_EXPR → METHOD_CALL_EXPR.
        // When the cursor is inside the IDENT token, the immediate parent is NAME_EXPR,
        // so we need to walk up to the METHOD_CALL_EXPR to parse the nested chain.
        let has_dot = method_call
            .children_with_tokens()
            .any(|c| c.kind() == SyntaxKind::DOT);
        if has_dot {
            if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
                eprintln!("DEBUG SEGMENT_PARSER: using ancestor METHOD_CALL_EXPR");
            }
            debug!("CHAIN: Detected METHOD_CALL_EXPR ancestor with DOT (nested tree)");
            let segments = parse_nested_method_call_chain(&method_call, token);
            if let Some((segs, idx)) = segments {
                if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
                    let rendered = segs
                        .iter()
                        .map(|s| format!("{}{}", s.name, if s.is_method_call { "()" } else { "" }))
                        .collect::<Vec<_>>()
                        .join(".");
                    eprintln!("DEBUG SEGMENT_PARSER: nested chain={rendered} target_index={idx}");
                }
                debug!(
                    "CHAIN: Returning {} segments with target at index {}",
                    segs.len(),
                    idx
                );
                return Some((segs, idx));
            }
            if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
                eprintln!("DEBUG SEGMENT_PARSER: nested parse returned None");
            }
        }
    }

    // Special handling for FIELD_ACCESS_EXPR parent
    // For `user.getName()`, the structure is:
    // FIELD_ACCESS_EXPR(user.getName) + METHOD_CALL_EXPR(())
    // where FIELD_ACCESS_EXPR contains: receiver + DOT + method_name
    if chain_parent.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
        debug!("CHAIN: Detected FIELD_ACCESS_EXPR parent");

        // Check if this is followed by METHOD_CALL_EXPR
        let is_method_call = chain_parent.parent().is_some_and(|gp| {
            gp.children()
                .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
        });

        debug!("CHAIN: is_method_call={}", is_method_call);

        // Extract the chain by parsing the FIELD_ACCESS_EXPR structure
        let mut segments = Vec::new();
        let mut target_idx = None;

        // First, collect all children to determine which IDENT is the method name
        let children: Vec<_> = chain_parent.children_with_tokens().collect();

        // Find the last IDENT token - that's the method name in a FIELD_ACCESS_EXPR
        let last_ident_idx = children.iter().rposition(
            |child| matches!(child, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT),
        );

        // Collect all parts of the field access chain
        for (idx, child) in children.iter().enumerate() {
            match child {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                    let name = t.text().to_string();
                    debug!("CHAIN: Found IDENT '{}'", name);

                    // Check if this is the target token
                    if *t == *token {
                        target_idx = Some(segments.len());
                        debug!("CHAIN: This is the target token");
                    }

                    // Only the LAST identifier is the method being called.
                    // Earlier identifiers (receivers) are not method calls.
                    let is_last_ident = Some(idx) == last_ident_idx;
                    segments.push(ChainSegment {
                        name,
                        is_method_call: is_last_ident && is_method_call,
                        cast_type: None,
                        call_range: None,
                    });
                }
                rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::NAME_EXPR => {
                    // Receiver expression
                    let name = n.text().to_string().trim().to_string();
                    debug!("CHAIN: Found NAME_EXPR '{}'", name);

                    // Check if the target token is inside this NAME_EXPR
                    let target_in_this_node = n
                        .descendants_with_tokens()
                        .any(|desc| matches!(&desc, rowan::NodeOrToken::Token(t) if t == token));

                    if name.contains('.') || name.contains('(') {
                        // Set target_idx as we go: when the part name equals the target
                        // token's text, record this segment as the target. Without this,
                        // a dotted NAME_EXPR receiver (e.g. `com.example.Utils`) would
                        // emit several segments but leave target_idx unset, the fallback
                        // at the bottom only triggers when target_idx is already Some,
                        // and the second fallback that calls `position()` on direct
                        // children misses the descendant token entirely. Net effect:
                        // goto-definition silently returned None for the inner name.
                        let target_text = if target_in_this_node {
                            Some(token.text().to_string())
                        } else {
                            None
                        };
                        for raw_part in name.split('.') {
                            let trimmed = raw_part.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            let part_name = trimmed.split('(').next().unwrap_or(trimmed).trim();
                            if part_name.is_empty() {
                                continue;
                            }
                            if target_idx.is_none() && target_text.as_deref() == Some(part_name) {
                                target_idx = Some(segments.len());
                                debug!(
                                    "CHAIN: Target token in dotted NAME_EXPR, target_idx={}",
                                    segments.len()
                                );
                            }
                            segments.push(ChainSegment {
                                name: part_name.to_string(),
                                is_method_call: trimmed.contains('('),
                                cast_type: None,
                                call_range: None,
                            });
                        }
                    } else {
                        // Set target_idx if the target token is in this NAME_EXPR
                        if target_in_this_node {
                            target_idx = Some(segments.len());
                            debug!(
                                "CHAIN: Target token is in NAME_EXPR, target_idx={}",
                                segments.len()
                            );
                        }
                        segments.push(ChainSegment {
                            name,
                            is_method_call: false,
                            cast_type: None,
                            call_range: None,
                        });
                    }
                }
                rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR => {
                    // Nested field access - recursively parse it
                    debug!("CHAIN: Found nested FIELD_ACCESS_EXPR");
                    let nested_is_call = n.parent().is_some_and(|p| {
                        p.children()
                            .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
                    });
                    // Collect identifiers from the nested FIELD_ACCESS_EXPR, and
                    // remember the index of the one that *is* the target token so the
                    // caller can return a (segments, target_idx) tuple. Without this,
                    // when the cursor lands on an inner identifier of a nested
                    // FIELD_ACCESS_EXPR receiver, target_idx would never be set and
                    // both downstream fallbacks would miss it (same failure mode as
                    // the dotted-NAME_EXPR branch above).
                    let mut nested_names = Vec::new();
                    let mut nested_target_idx: Option<usize> = None;
                    for sub_child in n.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = sub_child {
                            if t.kind() == SyntaxKind::IDENT {
                                if nested_target_idx.is_none() && &t == token {
                                    nested_target_idx = Some(nested_names.len());
                                }
                                let name = t.text().to_string();
                                debug!("CHAIN: Found IDENT in nested '{}'", name);
                                nested_names.push(name);
                            }
                        }
                    }
                    let segments_base = segments.len();
                    let last_idx = nested_names.len().saturating_sub(1);
                    for (idx, name) in nested_names.into_iter().enumerate() {
                        segments.push(ChainSegment {
                            name,
                            is_method_call: nested_is_call && idx == last_idx,
                            cast_type: None,
                            call_range: None,
                        });
                    }
                    if let Some(rel) = nested_target_idx {
                        if target_idx.is_none() {
                            target_idx = Some(segments_base + rel);
                            debug!(
                                "CHAIN: Target token in nested FIELD_ACCESS_EXPR, target_idx={}",
                                segments_base + rel
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(idx) = target_idx {
            if segments.len() == 1 {
                let text = parent.text().to_string();
                if text.contains('.') {
                    let mut rebuilt = Vec::new();
                    for raw_part in text.split('.') {
                        let trimmed = raw_part.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let name = trimmed.split('(').next().unwrap_or(trimmed).trim();
                        if name.is_empty() {
                            continue;
                        }
                        let is_method = trimmed.contains('(');
                        rebuilt.push(ChainSegment {
                            name: name.to_string(),
                            is_method_call: is_method,
                            cast_type: None,
                            call_range: None,
                        });
                    }
                    if let Some(target_idx) =
                        rebuilt.iter().rposition(|seg| seg.name == target_name)
                    {
                        if is_method_call {
                            rebuilt[target_idx].is_method_call = true;
                        }
                        debug!(
                            "CHAIN: Rebuilt {} segments from text, target at index {}",
                            rebuilt.len(),
                            target_idx
                        );
                        return Some((rebuilt, target_idx));
                    }
                }
            }

            debug!(
                "CHAIN: Returning {} segments with target at index {}",
                segments.len(),
                idx
            );
            return Some((segments, idx));
        }
        debug!("CHAIN: Warning: target token not found in segments");
    }

    // Check if this is a simple method call or a receiver in a method call chain.
    // With nested trees:
    //   `helper()`           -> METHOD_CALL_EXPR -> [NAME_EXPR("helper"), ARG_LIST]
    //   `baz.isSomething()`  -> METHOD_CALL_EXPR -> [NAME_EXPR("baz"), DOT, IDENT, ARG_LIST]
    let mut is_simple_method_call = false;
    if parent.kind() == SyntaxKind::NAME_EXPR {
        if let Some(gp) = parent.parent() {
            if gp.kind() == SyntaxKind::METHOD_CALL_EXPR {
                // NAME_EXPR is a child of METHOD_CALL_EXPR. Check if a DOT follows it.
                let parent_range = parent.text_range();
                let mut found_name = false;
                let mut dot_follows = false;
                for child in gp.children_with_tokens() {
                    if found_name {
                        if child.kind() == SyntaxKind::WHITESPACE {
                            continue;
                        }
                        dot_follows = child.kind() == SyntaxKind::DOT;
                        break;
                    }
                    if let rowan::NodeOrToken::Node(n) = &child {
                        if n.text_range() == parent_range {
                            found_name = true;
                        }
                    }
                }
                if dot_follows {
                    // NAME_EXPR is the receiver (followed by DOT).
                    // Return None to allow proper scope-based resolution for
                    // local variables and parameters that may shadow fields.
                    debug!("CHAIN: NAME_EXPR is receiver (followed by DOT), returning None for proper scope resolution");
                    return None;
                } else {
                    // NAME_EXPR is the method name in a simple call.
                    is_simple_method_call = true;
                    debug!(
                        "CHAIN: Detected simple method call (NAME_EXPR child of METHOD_CALL_EXPR)"
                    );
                }
            } else if gp.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                // NAME_EXPR is a child of FIELD_ACCESS_EXPR (receiver of field access)
                // Return None to let scope-based resolution handle it.
                debug!("CHAIN: NAME_EXPR is receiver in FIELD_ACCESS_EXPR, returning None for proper scope resolution");
                return None;
            }
        }
    }

    let children: Vec<_> = parent.children_with_tokens().collect();

    // Find the target token's position
    let target_pos = children
        .iter()
        .position(|child| matches!(child, rowan::NodeOrToken::Token(t) if t == token))?;

    // Work backwards from target to find chain start
    let mut chain_start = target_pos;
    let mut i = target_pos as isize - 1;

    while i >= 0 {
        let idx = i as usize;
        if idx >= children.len() {
            break;
        }
        match &children[idx] {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::DOT => {
                if i > 0 {
                    // Check if there's an IDENT, SUPER_KW, or METHOD_CALL_EXPR before the DOT
                    // Skip any whitespace to find the actual element
                    let mut prev_idx = (i - 1) as usize;
                    while prev_idx < children.len() {
                        if let rowan::NodeOrToken::Token(t) = &children[prev_idx] {
                            if t.kind() == SyntaxKind::WHITESPACE {
                                if prev_idx == 0 {
                                    break;
                                }
                                prev_idx -= 1;
                                continue;
                            }
                        }
                        // Found non-whitespace element
                        break;
                    }

                    if prev_idx >= children.len() {
                        break;
                    }
                    match &children[prev_idx] {
                        rowan::NodeOrToken::Token(tok)
                            if tok.kind() == SyntaxKind::IDENT
                                || tok.kind() == SyntaxKind::SUPER_KW =>
                        {
                            debug!(
                                "CHAIN: Found IDENT/SUPER token at {}, updating chain_start",
                                prev_idx
                            );
                            chain_start = prev_idx;
                            i = prev_idx as isize - 1;
                        }
                        rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::SUPER_EXPR => {
                            debug!(
                                "CHAIN: Found SUPER_EXPR node at {}, updating chain_start",
                                prev_idx
                            );
                            chain_start = prev_idx;
                            i = prev_idx as isize - 1;
                        }
                        rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::METHOD_CALL_EXPR => {
                            // METHOD_CALL_EXPR found before DOT
                            // Need to find the IDENT that precedes this METHOD_CALL_EXPR
                            // Walk backwards from the METHOD_CALL_EXPR to find the IDENT
                            debug!("CHAIN: Found METHOD_CALL_EXPR at {}, searching for preceding IDENT", prev_idx);

                            // The IDENT should be just before the METHOD_CALL_EXPR
                            if i >= 2 {
                                let ident_idx = (i - 2) as usize;
                                if ident_idx < children.len() {
                                    if let rowan::NodeOrToken::Token(tok) = &children[ident_idx] {
                                        if tok.kind() == SyntaxKind::IDENT {
                                            debug!(
                                                "CHAIN: Found IDENT '{}' at {}",
                                                tok.text(),
                                                ident_idx
                                            );
                                            chain_start = ident_idx;
                                            i = ident_idx as isize - 1;
                                            continue;
                                        }
                                    }
                                }
                            }

                            // If we couldn't find IDENT at i-2, keep searching backwards
                            // This handles cases where the structure is different
                            debug!("CHAIN: IDENT not at expected position, continuing backwards search");
                            i = prev_idx as isize - 1;
                        }
                        rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::NAME_EXPR => {
                            chain_start = prev_idx;
                            break; // NAME_EXPR marks the start
                        }
                        rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::LITERAL_EXPR => {
                            // Literal expression like "hello" or 42
                            debug!("CHAIN: Found literal expression at {}", prev_idx);
                            chain_start = prev_idx;
                            break;
                        }
                        rowan::NodeOrToken::Node(n)
                            if n.kind() == SyntaxKind::ARRAY_ACCESS_EXPR =>
                        {
                            // Array access like types[0].method()
                            // Continue backwards to find the base expression
                            debug!(
                                "CHAIN: Found ARRAY_ACCESS_EXPR at {}, searching for base",
                                prev_idx
                            );
                            // Look for the base before the array access
                            if i >= 2 {
                                let base_idx = (i - 2) as usize;
                                if base_idx < children.len() {
                                    if let rowan::NodeOrToken::Node(base_node) = &children[base_idx]
                                    {
                                        if base_node.kind() == SyntaxKind::NAME_EXPR {
                                            debug!("CHAIN: Found NAME_EXPR base at {}", base_idx);
                                            chain_start = base_idx;
                                            break;
                                        }
                                    }
                                }
                            }
                            // If we can't find the base, treat the array access as the start
                            chain_start = prev_idx;
                            break;
                        }
                        rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::PAREN_EXPR => {
                            // Could be a cast expression: ((Type) expr).method()
                            debug!("CHAIN: Found PAREN_EXPR at {}, checking for cast", prev_idx);
                            // Mark that we have a parenthesized expression
                            // We'll handle cast extraction during segment collection
                            chain_start = prev_idx;
                            break;
                        }
                        rowan::NodeOrToken::Node(n)
                            if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR =>
                        {
                            // FIELD_ACCESS_EXPR is an empty marker - continue searching backwards
                            // This handles chains like System.out.println where there's a FIELD_ACCESS_EXPR
                            // between "out" and the second DOT
                            debug!(
                                "CHAIN: Found FIELD_ACCESS_EXPR marker at {}, continuing search",
                                prev_idx
                            );
                            i = prev_idx as isize - 1;
                        }
                        _ => break,
                    }
                } else {
                    break;
                }
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::METHOD_CALL_EXPR => {
                // Skip METHOD_CALL_EXPR
                i -= 1;
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::NAME_EXPR => {
                // NAME_EXPR marks the start of a chain in flat structure
                // This handles: NAME_EXPR.METHOD_CALL_EXPR.DOT.IDENT.METHOD_CALL_EXPR
                // e.g., assertThatThrownBy().hasMessage()
                debug!("CHAIN: Found NAME_EXPR at {}, setting as chain_start", idx);
                chain_start = idx;
                break;
            }
            _ => {
                i -= 1;
            }
        }
    }

    // Now collect the chain segments from chain_start
    let mut segments = Vec::new();
    let mut target_idx = None;
    let mut i = chain_start;

    while i < children.len() {
        match &children[i] {
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::SUPER_EXPR {
                    segments.push(ChainSegment {
                        name: "super".to_string(),
                        is_method_call: false,
                        cast_type: None,
                        call_range: None,
                    });
                    i += 1;
                } else if node.kind() == SyntaxKind::NAME_EXPR {
                    let name = node.text().to_string().trim().to_string();
                    // Check if next child is METHOD_CALL_EXPR to determine if this is a method call
                    let is_method_call = i + 1 < children.len()
                        && matches!(&children[i + 1], rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::METHOD_CALL_EXPR);
                    debug!(
                        "CHAIN: Segment {}: NAME_EXPR '{}' (method: {})",
                        segments.len(),
                        name,
                        is_method_call
                    );
                    segments.push(ChainSegment {
                        name,
                        is_method_call,
                        cast_type: None,
                        call_range: None,
                    });
                    i += 1;
                } else if node.kind() == SyntaxKind::LITERAL_EXPR {
                    // Literal expression like "hello".toUpperCase() or (1).toString()
                    debug!("CHAIN: Segment {}: LITERAL_EXPR", segments.len());

                    // Determine the type of the literal by examining its token
                    let literal_type = node
                        .children_with_tokens()
                        .find_map(|child| {
                            if let rowan::NodeOrToken::Token(t) = child {
                                match t.kind() {
                                    SyntaxKind::STRING_LITERAL => Some("java.lang.String"),
                                    SyntaxKind::INT_LITERAL => Some("java.lang.Integer"),
                                    SyntaxKind::FLOAT_LITERAL => Some("java.lang.Double"),
                                    SyntaxKind::BOOLEAN_LITERAL => Some("java.lang.Boolean"),
                                    SyntaxKind::NULL_LITERAL => Some("java.lang.Object"),
                                    SyntaxKind::CHAR_LITERAL => Some("java.lang.Character"),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        })
                        .unwrap_or("java.lang.Object");

                    segments.push(ChainSegment {
                        name: literal_type.to_string(),
                        is_method_call: false,
                        cast_type: None,
                        call_range: None,
                    });
                    i += 1;
                } else if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                    i += 1; // Skip method call expr
                } else if node.kind() == SyntaxKind::ARRAY_ACCESS_EXPR {
                    // Skip array access expressions like [0] in types[0].method()
                    // The type system will handle the array element type inference
                    debug!(
                        "CHAIN: Skipping ARRAY_ACCESS_EXPR at segment {}",
                        segments.len()
                    );
                    i += 1;
                } else if node.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                    // FIELD_ACCESS_EXPR is an empty marker in our parser - skip it
                    // This handles chains like System.out.println
                    debug!(
                        "CHAIN: Skipping FIELD_ACCESS_EXPR marker at segment {}",
                        segments.len()
                    );
                    i += 1;
                } else if node.kind() == SyntaxKind::PAREN_EXPR {
                    // Check if this is a cast expression: ((Type) expr)
                    debug!(
                        "CHAIN: Found PAREN_EXPR at segment {}, checking for cast",
                        segments.len()
                    );
                    if let Some((cast_type, base_name)) = extract_cast_from_paren(node) {
                        debug!(
                            "CHAIN: Extracted cast: type='{}', base='{}'",
                            cast_type, base_name
                        );
                        segments.push(ChainSegment {
                            name: base_name,
                            is_method_call: false,
                            cast_type: Some(cast_type),
                            call_range: None,
                        });
                    } else {
                        // Just a parenthesized expression, not a cast
                        debug!("CHAIN: Not a cast, treating as regular expr");
                        // Try to extract the expression inside
                        if let Some(inner_name) = extract_name_from_paren(node) {
                            segments.push(ChainSegment {
                                name: inner_name,
                                is_method_call: false,
                                cast_type: None,
                                call_range: None,
                            });
                        }
                    }
                    i += 1;
                } else {
                    break; // End of chain
                }
            }
            rowan::NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::SUPER_KW {
                    // Handle super keyword as a special receiver
                    debug!("CHAIN: Segment {}: SUPER_KW", segments.len());
                    segments.push(ChainSegment {
                        name: "super".to_string(),
                        is_method_call: false,
                        cast_type: None,
                        call_range: None,
                    });
                    i += 1;
                } else if tok.kind() == SyntaxKind::IDENT {
                    let name = tok.text().to_string();

                    // Check if it's a method call:
                    // 1. Next sibling is METHOD_CALL_EXPR (for chained calls)
                    // 2. Simple method call detected earlier (parent is NAME_EXPR in METHOD_CALL_EXPR)
                    let is_method_call = (i + 1 < children.len()
                        && matches!(&children[i + 1], rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::METHOD_CALL_EXPR))
                        || (is_simple_method_call && tok == token);

                    debug!(
                        "CHAIN: Segment {}: IDENT '{}' (method: {})",
                        segments.len(),
                        name,
                        is_method_call
                    );

                    if tok == token {
                        target_idx = Some(segments.len());
                        debug!("CHAIN: This is the target segment");
                    }

                    segments.push(ChainSegment {
                        name,
                        is_method_call,
                        cast_type: None,
                        call_range: None,
                    });
                    i += 1;
                } else if tok.kind() == SyntaxKind::DOT {
                    i += 1; // Continue chain
                } else if tok.kind() == SyntaxKind::WHITESPACE {
                    i += 1; // Skip whitespace
                } else {
                    break; // End of chain
                }
            }
        }
    }

    if segments.is_empty() || target_idx.is_none() {
        if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
            let kinds = children
                .iter()
                .map(|c| format!("{:?}", c.kind()))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "DEBUG SEGMENT_PARSER: failed segments_len={} target_idx={:?} children=[{}]",
                segments.len(),
                target_idx,
                kinds
            );
        }
        debug!(
            "CHAIN: Failed - segments: {}, target: {:?}",
            segments.len(),
            target_idx
        );
        return None;
    }

    let target_index = target_idx.expect("verified Some above");
    debug!(
        "CHAIN: Parsed {} segments, target at index {}",
        segments.len(),
        target_index
    );

    if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
        let rendered = segments
            .iter()
            .map(|s| format!("{}{}", s.name, if s.is_method_call { "()" } else { "" }))
            .collect::<Vec<_>>()
            .join(".");
        eprintln!("DEBUG SEGMENT_PARSER: chain={rendered} target_index={target_index}");
    }

    Some((segments, target_index))
}

pub(super) const fn is_receiver_expr_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
    )
}

/// Parse a method call chain from a nested METHOD_CALL_EXPR tree.
///
/// With nested expression trees, `user.getProfile().getName()` produces:
/// ```text
/// METHOD_CALL_EXPR                    <- outer (.getName())
///   METHOD_CALL_EXPR                  <- inner (.getProfile())
///     NAME_EXPR("user")
///     DOT
///     IDENT("getProfile")
///     ARG_LIST
///   DOT
///   IDENT("getName")
///   ARG_LIST
/// ```
///
/// This function recursively walks the nested METHOD_CALL_EXPR/FIELD_ACCESS_EXPR
/// tree to build a flat chain of segments.
fn parse_nested_method_call_chain(
    node: &SyntaxNode,
    target_token: &SyntaxToken<hyperion_parser::JavaLanguage>,
) -> Option<(Vec<ChainSegment>, usize)> {
    let mut segments = Vec::new();
    let mut target_idx = None;
    collect_chain_segments(node, target_token, &mut segments, &mut target_idx);
    target_idx.map(|idx| (segments, idx))
}

fn token_is_target(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    target_token: &SyntaxToken<hyperion_parser::JavaLanguage>,
) -> bool {
    token.kind() == target_token.kind() && token.text_range() == target_token.text_range()
}

fn collect_chain_segments(
    node: &SyntaxNode,
    target_token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    segments: &mut Vec<ChainSegment>,
    target_idx: &mut Option<usize>,
) {
    let is_method_call = node.kind() == SyntaxKind::METHOD_CALL_EXPR;

    for child in node.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Node(n) => {
                match n.kind() {
                    SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::FIELD_ACCESS_EXPR => {
                        // Recurse into nested call/access
                        collect_chain_segments(n, target_token, segments, target_idx);
                    }
                    SyntaxKind::ARRAY_ACCESS_EXPR => {
                        // For chains like `types[0].equals(...)`, we only want the receiver
                        // expression (`types`) as part of the chain. The index expression
                        // should not contribute chain segments.
                        if let Some(receiver) =
                            n.children().find(|c| is_receiver_expr_kind(c.kind()))
                        {
                            collect_chain_segments(&receiver, target_token, segments, target_idx);
                        }
                    }
                    SyntaxKind::NAME_EXPR => {
                        // Receiver or simple method name.
                        // If parent is METHOD_CALL_EXPR with no DOT, this NAME_EXPR
                        // is the method name itself (simple call like `getUser()`).
                        // If parent has a DOT, this is the receiver.
                        let name = n.text().to_string().trim().to_string();
                        let contains_target = n.descendants_with_tokens().any(|desc| {
                            matches!(&desc, rowan::NodeOrToken::Token(t) if token_is_target(t, target_token))
                        });
                        if contains_target {
                            *target_idx = Some(segments.len());
                        }
                        // Simple call: METHOD_CALL_EXPR has no DOT -> NAME_EXPR is the method
                        let is_simple_call = is_method_call
                            && !node
                                .children_with_tokens()
                                .any(|c| c.kind() == SyntaxKind::DOT);
                        segments.push(ChainSegment {
                            name,
                            is_method_call: is_simple_call,
                            cast_type: None,
                            call_range: is_simple_call.then(|| node.text_range()),
                        });
                    }
                    SyntaxKind::THIS_EXPR => {
                        segments.push(ChainSegment {
                            name: "this".to_string(),
                            is_method_call: false,
                            cast_type: None,
                            call_range: None,
                        });
                    }
                    SyntaxKind::SUPER_EXPR => {
                        segments.push(ChainSegment {
                            name: "super".to_string(),
                            is_method_call: false,
                            cast_type: None,
                            call_range: None,
                        });
                    }
                    SyntaxKind::CAST_EXPR => {
                        // Extract the cast type and recurse into the inner expression
                        let cast_type = extract_cast_type(n);
                        // The inner expression is the last non-type child
                        let children: Vec<_> = n.children().collect();
                        for inner in children.iter().rev() {
                            if matches!(
                                inner.kind(),
                                SyntaxKind::NAME_EXPR
                                    | SyntaxKind::METHOD_CALL_EXPR
                                    | SyntaxKind::FIELD_ACCESS_EXPR
                                    | SyntaxKind::PAREN_EXPR
                                    | SyntaxKind::THIS_EXPR
                                    | SyntaxKind::NEW_EXPR
                                    | SyntaxKind::CAST_EXPR
                            ) {
                                collect_chain_segments(inner, target_token, segments, target_idx);
                                break;
                            }
                        }
                        // Annotate the last segment with the cast type
                        if let (Some(last), Some(cast_type)) = (segments.last_mut(), cast_type) {
                            last.cast_type = Some(cast_type);
                        }
                    }
                    SyntaxKind::NEW_EXPR => {
                        // new Foo() -- extract the type name
                        let type_name = n
                            .children()
                            .find(|c| c.kind() == SyntaxKind::CLASS_TYPE)
                            .map_or_else(
                                || n.text().to_string().trim().to_string(),
                                |c| c.text().to_string().trim().to_string(),
                            );
                        segments.push(ChainSegment {
                            name: type_name,
                            is_method_call: true, // new expression is like a constructor call
                            cast_type: None,
                            call_range: None,
                        });
                    }
                    SyntaxKind::PAREN_EXPR => {
                        // Unwrap parentheses -- find the inner expression
                        for inner in n.children() {
                            if matches!(
                                inner.kind(),
                                SyntaxKind::NAME_EXPR
                                    | SyntaxKind::METHOD_CALL_EXPR
                                    | SyntaxKind::FIELD_ACCESS_EXPR
                                    | SyntaxKind::PAREN_EXPR
                                    | SyntaxKind::THIS_EXPR
                                    | SyntaxKind::NEW_EXPR
                                    | SyntaxKind::CAST_EXPR
                                    | SyntaxKind::BINARY_EXPR
                                    | SyntaxKind::UNARY_EXPR
                                    | SyntaxKind::TERNARY_EXPR
                            ) {
                                collect_chain_segments(&inner, target_token, segments, target_idx);
                                break;
                            }
                        }
                    }
                    SyntaxKind::ARG_LIST => {
                        // Skip argument lists
                    }
                    _ => {}
                }
            }
            rowan::NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::IDENT {
                    let name = t.text().to_string();
                    if token_is_target(t, target_token) {
                        *target_idx = Some(segments.len());
                    }
                    segments.push(ChainSegment {
                        name,
                        is_method_call,
                        cast_type: None,
                        call_range: is_method_call.then(|| node.text_range()),
                    });
                }
                // DOT, WHITESPACE, etc. are skipped
            }
        }
    }
}
