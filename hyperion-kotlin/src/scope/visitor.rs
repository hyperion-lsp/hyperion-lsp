// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scope graph visitor and extraction logic
//!
//! This module contains the AST visitor that builds scope graphs
//! and utility functions for extracting type and method call information.

use super::builder::ScopeGraphBuilder;
use super::graph::ScopeGraph;
use super::types::{
    ImportInfo, ImportKind, MethodCallInfo, ScopeSymbol, ScopeSymbolKind, SmartCast,
};
use crate::inference::infer_lambda_it_type;
use crate::smart_cast::{analyze_condition, SmartCastSource};
use crate::type_extractor::Type;
use crate::types::{KotlinType, Nullability};
use crate::{SyntaxKind, SyntaxNode, TypeExtractor};
use rowan::ast::AstNode;

/// Extract type annotation from token stream after variable name
/// Looks for pattern: COLON WHITESPACE* TYPE
/// Returns None if no type annotation is present
fn extract_type_annotation_from_tokens(
    tokens: &mut std::iter::Peekable<rowan::SyntaxElementChildren<crate::KotlinLanguage>>,
) -> Option<KotlinType> {
    // Skip whitespace
    while let Some(next) = tokens.peek() {
        if let Some(t) = next.as_token() {
            if t.kind() == SyntaxKind::WHITESPACE {
                tokens.next();
                continue;
            }
        }
        break;
    }

    // Check for COLON (type annotation marker)
    {
        let next = tokens.peek()?;
        {
            let t = next.as_token()?;
            if t.kind() == SyntaxKind::COLON {
                tokens.next(); // consume colon
            } else {
                return None; // No type annotation
            }
        }
    }

    // Skip whitespace after colon
    while let Some(next) = tokens.peek() {
        if let Some(t) = next.as_token() {
            if t.kind() == SyntaxKind::WHITESPACE {
                tokens.next();
                continue;
            }
        }
        break;
    }

    // Collect type tokens until we hit =, semicolon, or EOF
    let mut type_tokens = Vec::new();
    let mut depth: i32 = 0; // Track generic/parenthesis depth

    while let Some(next) = tokens.peek() {
        if let Some(t) = next.as_token() {
            match t.kind() {
                // Stop tokens when at depth 0
                SyntaxKind::EQ | SyntaxKind::SEMICOLON if depth == 0 => {
                    break;
                }
                // Skip whitespace but preserve it in generics
                SyntaxKind::WHITESPACE => {
                    if depth > 0 {
                        type_tokens.push(" ".to_string());
                    }
                    tokens.next();
                }
                // Track depth for generics
                SyntaxKind::LT => {
                    depth += 1;
                    type_tokens.push(t.text().to_string());
                    tokens.next();
                }
                SyntaxKind::GT => {
                    depth = depth.saturating_sub(1);
                    type_tokens.push(t.text().to_string());
                    tokens.next();
                }
                // Collect all other tokens
                _ => {
                    type_tokens.push(t.text().to_string());
                    tokens.next();
                }
            }
        } else {
            // Not a token (might be a node like TYPE_REF)
            // Skip nodes and continue with next element
            tokens.next();
        }
    }

    // Parse collected tokens into a Type
    if type_tokens.is_empty() {
        return None;
    }

    // Join tokens and parse
    let type_str = type_tokens.join("");

    // Simple parsing logic:
    // - If ends with ?, it's nullable
    // - If contains <>, it's generic
    // - Otherwise, it's simple

    if type_str.ends_with('?') {
        let inner = type_str.trim_end_matches('?').to_string();
        return Some(KotlinType::Class {
            fqn: inner,
            type_arguments: vec![],
            nullability: Nullability::Nullable,
        });
    }

    Some(KotlinType::Class {
        fqn: type_str,
        type_arguments: vec![],
        nullability: Nullability::NonNull,
    })
}

/// Extract import information from an IMPORT_DECL node
fn extract_import(node: &SyntaxNode, builder: &mut ScopeGraphBuilder) {
    let mut is_wildcard = false;
    let mut path_parts = Vec::new();
    let mut alias: Option<String> = None;
    let mut collecting_alias = false;

    // Collect all tokens in the import declaration
    for token in node.descendants_with_tokens() {
        if let Some(t) = token.as_token() {
            match t.kind() {
                SyntaxKind::STAR => {
                    is_wildcard = true;
                }
                SyntaxKind::IDENT => {
                    if collecting_alias {
                        // This is the alias name - DON'T add to path_parts
                        alias = Some(t.text().to_string());
                        collecting_alias = false;
                    } else {
                        // This is part of the import path
                        path_parts.push(t.text().to_string());
                    }
                }
                SyntaxKind::AS_KW => {
                    collecting_alias = true;
                }
                _ => {}
            }
        }
    }

    if path_parts.is_empty() {
        return;
    }

    let fqn = path_parts.join(".");
    let import_range = node.text_range();

    if is_wildcard {
        // import kotlin.collections.*
        builder.graph.add_import(ImportInfo {
            name: "*".to_string(),
            fqn,
            kind: ImportKind::Wildcard,
            definition_range: import_range,
        });
    } else if let Some(alias_name) = alias {
        // import kotlin.collections.List as L
        if let Some(simple_name) = path_parts.last() {
            builder.graph.add_import(ImportInfo {
                name: simple_name.clone(),
                fqn,
                kind: ImportKind::Aliased { alias: alias_name },
                definition_range: import_range,
            });
        }
    } else {
        // import kotlin.collections.List
        if let Some(simple_name) = path_parts.last() {
            builder.graph.add_import(ImportInfo {
                name: simple_name.clone(),
                fqn,
                kind: ImportKind::Explicit,
                definition_range: import_range,
            });
        }
    }
}

/// Extract method call information from initializer elements
///
/// Detects patterns:
/// - Direct method call: user.getName() -> Some(MethodCallInfo { receiver: Some("user"), method: "getName", is_chained: false })
/// - Property access: user.name -> Some(MethodCallInfo { receiver: Some("user"), method: "name", is_chained: false })
/// - Function call: listOf(...) -> Some(MethodCallInfo { receiver: None, method: "listOf", is_chained: false })
/// - Chained call: obj.getA().getB() -> Some(MethodCallInfo { receiver: Some("obj"), method: "getB", is_chained: true })
///
/// Returns None if not a method/function call or property access pattern
fn extract_method_call_info_from_elements(
    elements: &[rowan::SyntaxElement<crate::KotlinLanguage>],
) -> Option<MethodCallInfo> {
    // Strategy: Scan for pattern IDENT [DOT IDENT]* [L_PAREN | BLOCK]
    // - If we see IDENT DOT IDENT L_PAREN -> receiver.method()
    // - If we see IDENT DOT IDENT BLOCK -> receiver.method { } (trailing lambda)
    // - If we see IDENT DOT IDENT -> receiver.property
    // - If we see IDENT L_PAREN -> function()
    // - Track paren depth to handle nested calls

    let mut tokens: Vec<(SyntaxKind, String)> = Vec::new();
    let mut has_trailing_block = false;

    // First, collect all non-whitespace tokens, and check if there's a trailing BLOCK
    for elem in elements {
        if let Some(token) = elem.as_token() {
            if token.kind() != SyntaxKind::WHITESPACE {
                tokens.push((token.kind(), token.text().to_string()));
            }
        } else if let Some(node) = elem.as_node() {
            // Check for BLOCK or LAMBDA_EXPR node (trailing lambda)
            if matches!(node.kind(), SyntaxKind::BLOCK | SyntaxKind::LAMBDA_EXPR) {
                has_trailing_block = true;
            }
        }
    }

    // Strategy 0: Handle trailing lambda pattern: IDENT DOT IDENT BLOCK
    // For "numbers.filter { ... }", the tokens are [IDENT, DOT, IDENT] and has_trailing_block=true
    if has_trailing_block {
        if let Some(info) = extract_trailing_lambda_method_call(&tokens) {
            return Some(info);
        }
    }

    // Strategy 1: Check if the rightmost element is a property access (IDENT DOT IDENT, no L_PAREN)
    // This handles patterns like: "hello".uppercase().length
    // We need to prioritize property access because it appears at the END of the chain
    if let Some(info) = extract_property_access(&tokens) {
        return Some(info);
    }

    // Strategy 2: Try to find a method call pattern (with L_PAREN)
    // Find the last L_PAREN at depth 0 (outermost call)
    let mut depth: i32 = 0;
    let mut last_lparen_idx = None;
    for (i, (kind, _)) in tokens.iter().enumerate() {
        match kind {
            SyntaxKind::L_PAREN => {
                if depth == 0 {
                    last_lparen_idx = Some(i);
                }
                depth += 1;
            }
            SyntaxKind::R_PAREN => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    // If we found L_PAREN, extract method call
    if let Some(lparen_idx) = last_lparen_idx {
        return extract_method_call_with_paren(&tokens, lparen_idx);
    }

    None
}

/// Extract method call info for trailing lambda pattern: IDENT DOT IDENT (with trailing BLOCK)
fn extract_trailing_lambda_method_call(tokens: &[(SyntaxKind, String)]) -> Option<MethodCallInfo> {
    // Look for pattern: IDENT DOT IDENT at the end
    // This handles: numbers.filter { ... }
    if tokens.len() < 3 {
        return None;
    }

    // Check for pattern at the end of tokens (before the BLOCK which was not collected as a token)
    let last_idx = tokens.len() - 1;

    // Check if last token is IDENT (method name)
    if tokens[last_idx].0 != SyntaxKind::IDENT {
        return None;
    }
    let method_name = tokens[last_idx].1.clone();

    // Check if second-to-last is DOT
    if last_idx < 1 || tokens[last_idx - 1].0 != SyntaxKind::DOT {
        return None;
    }

    // Check if third-to-last is IDENT (receiver)
    if last_idx < 2 || tokens[last_idx - 2].0 != SyntaxKind::IDENT {
        return None;
    }
    let receiver_name = tokens[last_idx - 2].1.clone();

    Some(MethodCallInfo {
        receiver_name: Some(receiver_name),
        method_name,
        is_chained: false,
        receiver_type: None,
        arg_tokens: None,
    })
}

/// Extract method call info when we have a pattern with L_PAREN
fn extract_method_call_with_paren(
    tokens: &[(SyntaxKind, String)],
    lparen_idx: usize,
) -> Option<MethodCallInfo> {
    // Method name is the IDENT immediately before L_PAREN
    if lparen_idx == 0 {
        return None; // No method name
    }

    if tokens[lparen_idx - 1].0 != SyntaxKind::IDENT {
        return None; // Not an identifier before (
    }

    let method_name = tokens[lparen_idx - 1].1.clone();

    // Check if there's a receiver: look for IDENT DOT before the method name
    let mut receiver_name: Option<String> = None;
    let mut is_chained = false;

    // Try to infer receiver type for method chains
    let mut receiver_type = None;

    if lparen_idx >= 2 && tokens[lparen_idx - 2].0 == SyntaxKind::DOT {
        // We have "something.method(" pattern
        // Look for the receiver before the DOT
        if lparen_idx >= 3 {
            // Check if the pattern before the DOT is an identifier or a closing paren (chained call)
            if tokens[lparen_idx - 3].0 == SyntaxKind::IDENT {
                receiver_name = Some(tokens[lparen_idx - 3].1.clone());
            } else if tokens[lparen_idx - 3].0 == SyntaxKind::R_PAREN {
                // Chained call: something().method()
                is_chained = true;
                // Infer receiver type from the chain
                receiver_type =
                    infer_chain_receiver_type(tokens, lparen_idx - 3).map(KotlinType::from);
            }
        }
    }

    // Extract argument tokens between L_PAREN and matching R_PAREN
    let arg_tokens = extract_argument_tokens(tokens, lparen_idx);

    Some(MethodCallInfo {
        receiver_name,
        method_name,
        is_chained,
        receiver_type,
        arg_tokens: Some(arg_tokens),
    })
}

/// Extract tokens between parentheses for argument type inference
///
/// Given tokens and the index of L_PAREN, extract all tokens up to the matching R_PAREN
fn extract_argument_tokens(
    tokens: &[(SyntaxKind, String)],
    lparen_idx: usize,
) -> Vec<(SyntaxKind, String)> {
    let mut arg_tokens = Vec::new();
    let mut depth = 1; // We're already inside the first (
    let mut idx = lparen_idx + 1;

    while idx < tokens.len() && depth > 0 {
        let (kind, text) = &tokens[idx];
        match kind {
            SyntaxKind::L_PAREN => depth += 1,
            SyntaxKind::R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    break; // Found matching closing paren
                }
            }
            _ => {}
        }
        arg_tokens.push((*kind, text.clone()));
        idx += 1;
    }

    arg_tokens
}

/// Infer the receiver type for a chained call
///
/// Given tokens ending in R_PAREN (e.g., "hello".uppercase().length),
/// infer the type of the method call result before the R_PAREN.
///
/// # Arguments
///
/// * `tokens` - All tokens in the expression
/// * `rparen_idx` - Index of the R_PAREN token
///
/// # Returns
///
/// The inferred type of the receiver, or None if it cannot be determined
fn infer_chain_receiver_type(tokens: &[(SyntaxKind, String)], rparen_idx: usize) -> Option<Type> {
    use crate::type_extractor::{
        find_method_return_type as find_method_ret, resolve_free_function_return_type,
    };

    // Find the matching L_PAREN for this R_PAREN
    let mut depth = 1;
    let mut lparen_idx = None;

    for i in (0..rparen_idx).rev() {
        match tokens[i].0 {
            SyntaxKind::R_PAREN => depth += 1,
            SyntaxKind::L_PAREN => {
                depth -= 1;
                if depth == 0 {
                    lparen_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let lparen_idx = lparen_idx?;

    // Method name is before L_PAREN
    if lparen_idx == 0 || tokens[lparen_idx - 1].0 != SyntaxKind::IDENT {
        return None;
    }
    let method_name = &tokens[lparen_idx - 1].1;

    // Check if there's a receiver before the method
    if lparen_idx >= 2 && tokens[lparen_idx - 2].0 == SyntaxKind::DOT {
        // Pattern: receiver.method()
        if lparen_idx >= 3 {
            let receiver_token = &tokens[lparen_idx - 3];
            match receiver_token.0 {
                SyntaxKind::IDENT => {
                    // Variable receiver - we can't resolve without scope
                    // Return None for now
                    None
                }
                SyntaxKind::STRING_LITERAL | SyntaxKind::RAW_STRING_LITERAL => {
                    // String literal receiver
                    let receiver_type = Type::Simple("String".to_string());
                    find_method_ret(&receiver_type, method_name)
                }
                SyntaxKind::INT_LITERAL => {
                    let receiver_type = Type::Simple("Int".to_string());
                    find_method_ret(&receiver_type, method_name)
                }
                SyntaxKind::LONG_LITERAL => {
                    let receiver_type = Type::Simple("Long".to_string());
                    find_method_ret(&receiver_type, method_name)
                }
                SyntaxKind::FLOAT_LITERAL => {
                    let receiver_type = Type::Simple("Float".to_string());
                    find_method_ret(&receiver_type, method_name)
                }
                SyntaxKind::DOUBLE_LITERAL => {
                    let receiver_type = Type::Simple("Double".to_string());
                    find_method_ret(&receiver_type, method_name)
                }
                SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => {
                    let receiver_type = Type::Simple("Boolean".to_string());
                    find_method_ret(&receiver_type, method_name)
                }
                SyntaxKind::R_PAREN => {
                    // Nested chain - recursively resolve
                    let nested_type = infer_chain_receiver_type(tokens, lparen_idx - 3)?;
                    find_method_ret(&nested_type, method_name)
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        // No receiver - free function
        resolve_free_function_return_type(method_name)
    }
}

/// Extract property access pattern (IDENT DOT IDENT without L_PAREN)
fn extract_property_access(tokens: &[(SyntaxKind, String)]) -> Option<MethodCallInfo> {
    // Look for pattern: [IDENT | R_PAREN] DOT IDENT (at end of tokens, no L_PAREN after)
    // Examples:
    // - text.length -> receiver: "text", property: "length"
    // - "hello".uppercase().length -> receiver: chained, property: "length"

    if tokens.len() < 3 {
        return None;
    }

    // Check if last token is IDENT (property name)
    let last_idx = tokens.len() - 1;
    if tokens[last_idx].0 != SyntaxKind::IDENT {
        return None;
    }

    let property_name = tokens[last_idx].1.clone();

    // Check if second-to-last is DOT
    if last_idx < 1 || tokens[last_idx - 1].0 != SyntaxKind::DOT {
        return None;
    }

    // Check if third-to-last is IDENT or R_PAREN
    if last_idx < 2 {
        return None;
    }

    let receiver_token_kind = tokens[last_idx - 2].0;

    match receiver_token_kind {
        SyntaxKind::IDENT => {
            // Simple property access: text.length
            let receiver_name = tokens[last_idx - 2].1.clone();
            Some(MethodCallInfo {
                receiver_name: Some(receiver_name),
                method_name: property_name,
                is_chained: false,
                receiver_type: None,
                arg_tokens: None, // Property access has no arguments
            })
        }
        SyntaxKind::R_PAREN => {
            // Chained property access: something().length
            // Try to infer receiver type from the chain
            let receiver_type =
                infer_chain_receiver_type(tokens, last_idx - 2).map(KotlinType::from);
            Some(MethodCallInfo {
                receiver_name: None, // Will be resolved from the chain
                method_name: property_name,
                is_chained: true,
                receiver_type,
                arg_tokens: None, // Property access has no arguments
            })
        }
        _ => None,
    }
}

/// Detect smart cast from an if condition preceding a block
///
/// Looks for pattern: IF_KW L_PAREN IDENT IS_KW IDENT R_PAREN BLOCK
/// or: IF_KW L_PAREN BINARY_EXPR(IDENT IS_KW IDENT) R_PAREN BLOCK
/// Returns (variable_name, narrowed_type) if found
fn detect_smart_cast_for_block(block_node: &SyntaxNode) -> Option<(String, KotlinType)> {
    let parent = block_node.parent()?;

    // For IF_EXPR parent, look for the condition
    if parent.kind() == SyntaxKind::IF_EXPR {
        // New AST structure: IF_EXPR contains condition as an expression node
        return extract_is_check_from_if_expr(&parent);
    }

    // Fallback to old detection method for legacy AST structures
    let siblings: Vec<_> = parent.children_with_tokens().collect();
    let block_pos = siblings
        .iter()
        .position(|n| n.as_node() == Some(block_node))?;

    // Look backwards for R_PAREN (end of condition)
    let mut idx = block_pos.saturating_sub(1);
    while idx > 0 {
        if let Some(tok) = siblings[idx].as_token() {
            if tok.kind() == SyntaxKind::WHITESPACE {
                idx -= 1;
                continue;
            }
            if tok.kind() == SyntaxKind::R_PAREN {
                break;
            }
            // Not preceded by condition - not an if-then block
            return None;
        }
        return None;
    }

    // Now look backwards from R_PAREN to find: IDENT IS_KW TYPE_IDENT
    // Pattern: IF_KW L_PAREN var IS_KW Type R_PAREN
    let mut found_r_paren = false;
    let mut type_name: Option<String> = None;
    let mut found_is = false;
    let mut variable_name: Option<String> = None;
    let mut found_l_paren = false;
    let mut found_if = false;

    for i in (0..=idx).rev() {
        if let Some(tok) = siblings[i].as_token() {
            let kind = tok.kind();
            if kind == SyntaxKind::WHITESPACE {
                continue;
            }

            if !found_r_paren {
                if kind == SyntaxKind::R_PAREN {
                    found_r_paren = true;
                }
                continue;
            }

            if type_name.is_none() {
                if kind == SyntaxKind::IDENT {
                    type_name = Some(tok.text().to_string());
                }
                continue;
            }

            if !found_is {
                if kind == SyntaxKind::IS_KW {
                    found_is = true;
                }
                continue;
            }

            if variable_name.is_none() {
                if kind == SyntaxKind::IDENT {
                    variable_name = Some(tok.text().to_string());
                }
                continue;
            }

            if !found_l_paren {
                if kind == SyntaxKind::L_PAREN {
                    found_l_paren = true;
                }
                continue;
            }

            if !found_if {
                if kind == SyntaxKind::IF_KW {
                    found_if = true;
                }
                break;
            }
        }
    }

    // All components found - this is a smart cast
    if found_if && found_l_paren && found_is {
        if let (Some(var), Some(ty)) = (variable_name, type_name) {
            return Some((var, KotlinType::simple(&ty)));
        }
    }

    None
}

/// Extract is-check from an IF_EXPR node
///
/// Handles both:
/// - Direct tokens: IF_KW L_PAREN IDENT IS_KW IDENT R_PAREN
/// - Expression node: IF_KW L_PAREN BINARY_EXPR R_PAREN
fn extract_is_check_from_if_expr(if_expr: &SyntaxNode) -> Option<(String, KotlinType)> {
    // Look for BINARY_EXPR children that might contain "x is Type"
    for child in if_expr.children() {
        if child.kind() == SyntaxKind::BINARY_EXPR {
            if let Some(result) = extract_is_check_from_expr(&child) {
                return Some(result);
            }
        }
    }

    // Also try extracting from tokens directly (flat structure)
    let mut var_name: Option<String> = None;
    let mut type_name: Option<String> = None;
    let mut found_is = false;

    for elem in if_expr.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            let kind = token.kind();
            if kind == SyntaxKind::WHITESPACE
                || kind == SyntaxKind::L_PAREN
                || kind == SyntaxKind::R_PAREN
                || kind == SyntaxKind::IF_KW
            {
                continue;
            }

            if kind == SyntaxKind::IDENT {
                if var_name.is_none() {
                    var_name = Some(token.text().to_string());
                } else if found_is && type_name.is_none() {
                    type_name = Some(token.text().to_string());
                }
            } else if kind == SyntaxKind::IS_KW {
                found_is = true;
            }
        }
    }

    if found_is {
        if let (Some(var), Some(ty)) = (var_name, type_name) {
            return Some((var, KotlinType::simple(&ty)));
        }
    }

    None
}

/// Extract is-check from a BINARY_EXPR (or nested expression)
///
/// Looks for pattern: IDENT IS_KW IDENT
fn extract_is_check_from_expr(expr: &SyntaxNode) -> Option<(String, KotlinType)> {
    let mut var_name: Option<String> = None;
    let mut type_name: Option<String> = None;
    let mut found_is = false;

    // Flatten all tokens from the expression tree
    fn collect_tokens(node: &SyntaxNode, tokens: &mut Vec<(SyntaxKind, String)>) {
        for elem in node.children_with_tokens() {
            match elem {
                rowan::NodeOrToken::Token(t) => {
                    tokens.push((t.kind(), t.text().to_string()));
                }
                rowan::NodeOrToken::Node(n) => {
                    collect_tokens(&n, tokens);
                }
            }
        }
    }

    let mut tokens = Vec::new();
    collect_tokens(expr, &mut tokens);

    for (kind, text) in tokens {
        if kind == SyntaxKind::WHITESPACE {
            continue;
        }

        if kind == SyntaxKind::IDENT {
            if var_name.is_none() {
                var_name = Some(text);
            } else if found_is && type_name.is_none() {
                type_name = Some(text);
            }
        } else if kind == SyntaxKind::IS_KW {
            found_is = true;
        }
    }

    if found_is {
        if let (Some(var), Some(ty)) = (var_name, type_name) {
            return Some((var, KotlinType::simple(&ty)));
        }
    }

    None
}

/// Detect smart casts for a block, including else blocks and when entries
///
/// This enhanced version handles:
/// - if (x is Type) { ... } -> smart cast in then block
/// - if (x !is Type) { ... } else { ... } -> smart cast in else block
/// - if (x != null) { ... } -> non-null smart cast in then block
/// - when (x) { is Type -> ... } -> smart cast in branch body
///
/// Returns a list of (variable_name, narrowed_type, source) tuples
pub fn detect_smart_casts_for_block(
    block_node: &SyntaxNode,
) -> Vec<(String, KotlinType, SmartCastSource)> {
    let mut results = Vec::new();

    let parent = match block_node.parent() {
        Some(p) => p,
        None => return results,
    };

    match parent.kind() {
        SyntaxKind::IF_EXPR => {
            // Check if this is the then-block or else-block
            if let Some((analysis, is_else_block)) = analyze_if_expr_for_block(&parent, block_node)
            {
                if is_else_block {
                    // For else block, use the false_branch_casts
                    for cast in analysis.false_branch_casts {
                        results.push((cast.variable_name, cast.narrowed_type, cast.source));
                    }
                } else {
                    // For then block, use the true_branch_casts
                    for cast in analysis.true_branch_casts {
                        results.push((cast.variable_name, cast.narrowed_type, cast.source));
                    }
                }
            }
        }
        SyntaxKind::WHEN_ENTRY => {
            // Extract is-check from when entry condition
            if let Some((type_name, source)) = extract_when_entry_is_check(&parent) {
                // Find the subject variable from the parent WHEN_EXPR
                if let Some(when_expr) = parent.parent() {
                    if when_expr.kind() == SyntaxKind::WHEN_EXPR {
                        if let Some(subject) = extract_when_subject(&when_expr) {
                            results.push((subject, KotlinType::simple(&type_name), source));
                        }
                    }
                }
            }
        }
        _ => {
            // Fall back to basic detection for other cases
            if let Some((var, ty)) = detect_smart_cast_for_block(block_node) {
                results.push((var, ty, SmartCastSource::IsCheck));
            }
        }
    }

    results
}

/// Analyze an IF_EXPR and determine if a block is the then or else branch
fn analyze_if_expr_for_block(
    if_expr: &SyntaxNode,
    block_node: &SyntaxNode,
) -> Option<(crate::smart_cast::ConditionAnalysis, bool)> {
    // Find the condition node
    let condition_node = find_if_condition(if_expr)?;

    // Analyze the condition using the new smart_cast module
    let analysis = analyze_condition(&condition_node);

    // Determine if this block is the else branch
    let is_else_block = is_else_block(if_expr, block_node);

    Some((analysis, is_else_block))
}

/// Find the condition expression in an IF_EXPR
fn find_if_condition(if_expr: &SyntaxNode) -> Option<SyntaxNode> {
    // The condition is typically a BINARY_EXPR or other expression
    // between L_PAREN and R_PAREN after IF_KW
    let mut in_condition = false;

    for child in if_expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                SyntaxKind::L_PAREN => in_condition = true,
                SyntaxKind::R_PAREN => in_condition = false,
                _ => {}
            }
        } else if let Some(node) = child.as_node() {
            if in_condition {
                // This is the condition expression
                return Some(node.clone());
            }
        }
    }

    // If no node found in parentheses, look for direct child expression nodes
    for child in if_expr.children() {
        if matches!(
            child.kind(),
            SyntaxKind::BINARY_EXPR | SyntaxKind::UNARY_EXPR | SyntaxKind::PAREN_EXPR
        ) {
            return Some(child);
        }
    }

    None
}

/// Check if a block is the else branch of an if expression
fn is_else_block(if_expr: &SyntaxNode, block_node: &SyntaxNode) -> bool {
    // The else block follows the ELSE_KW token
    let mut saw_else_kw = false;

    for child in if_expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::ELSE_KW {
                saw_else_kw = true;
            }
        } else if let Some(node) = child.as_node() {
            if *node == *block_node {
                return saw_else_kw;
            }
        }
    }

    false
}

/// Extract is-check from a when entry condition
fn extract_when_entry_is_check(when_entry: &SyntaxNode) -> Option<(String, SmartCastSource)> {
    let mut found_is = false;
    let mut found_not_is = false;
    let mut type_parts = Vec::new();

    for elem in when_entry.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            match token.kind() {
                SyntaxKind::IS_KW => found_is = true,
                SyntaxKind::NOT_IS => found_not_is = true,
                SyntaxKind::IDENT if found_is || found_not_is => {
                    type_parts.push(token.text().to_string());
                }
                SyntaxKind::DOT if found_is || found_not_is => {
                    // Continue collecting qualified name
                }
                SyntaxKind::ARROW => {
                    // End of condition
                    break;
                }
                _ => {}
            }
        }
    }

    if type_parts.is_empty() {
        None
    } else {
        let type_name = type_parts.join(".");
        let source = if found_not_is {
            SmartCastSource::NotIsCheck
        } else {
            SmartCastSource::WhenBranch
        };
        Some((type_name, source))
    }
}

/// Extract the subject variable name from a when expression
fn extract_when_subject(when_expr: &SyntaxNode) -> Option<String> {
    let mut in_subject = false;

    for child in when_expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                SyntaxKind::L_PAREN => in_subject = true,
                SyntaxKind::R_PAREN => in_subject = false,
                SyntaxKind::IDENT if in_subject => {
                    return Some(token.text().to_string());
                }
                _ => {}
            }
        }
    }

    None
}

/// Check if a BLOCK node is a trailing lambda (not a function body)
///
/// A trailing lambda follows a method call pattern: receiver.method { ... }
fn is_trailing_lambda(block_node: &SyntaxNode) -> bool {
    let parent = match block_node.parent() {
        Some(p) => p,
        None => return false,
    };

    // Check if this block is preceded by a method call pattern
    let siblings: Vec<_> = parent.children_with_tokens().collect();
    let block_pos = match siblings
        .iter()
        .position(|n| n.as_node() == Some(block_node))
    {
        Some(p) => p,
        None => return false,
    };

    // Look backwards for method call pattern: IDENT DOT IDENT [WHITESPACE] BLOCK
    // or function call pattern: IDENT L_PAREN ... R_PAREN [WHITESPACE] BLOCK
    if block_pos == 0 {
        return false;
    }

    // Skip whitespace before the block
    let mut idx = block_pos - 1;
    while idx > 0 {
        if let Some(tok) = siblings[idx].as_token() {
            if tok.kind() != SyntaxKind::WHITESPACE {
                break;
            }
        }
        idx -= 1;
    }

    // Check for IDENT (method name) or R_PAREN (end of call args)
    if let Some(tok) = siblings.get(idx).and_then(|e| e.as_token()) {
        if tok.kind() == SyntaxKind::IDENT {
            // Check if preceded by DOT (method call pattern)
            if idx > 0 {
                if let Some(prev) = siblings.get(idx - 1).and_then(|e| e.as_token()) {
                    if prev.kind() == SyntaxKind::DOT {
                        return true;
                    }
                }
            }
        } else if tok.kind() == SyntaxKind::R_PAREN {
            // Method call with args: method(...) { ... }
            // Look backwards to find matching L_PAREN and then DOT IDENT pattern
            let mut paren_depth = 1;
            let mut j = idx;
            while j > 0 && paren_depth > 0 {
                j -= 1;
                if let Some(t) = siblings.get(j).and_then(|e| e.as_token()) {
                    if t.kind() == SyntaxKind::R_PAREN {
                        paren_depth += 1;
                    } else if t.kind() == SyntaxKind::L_PAREN {
                        paren_depth -= 1;
                    }
                }
            }
            // j now points to L_PAREN, check for IDENT before it
            if j > 0 {
                // Skip whitespace
                let mut k = j - 1;
                while k > 0 {
                    if let Some(t) = siblings.get(k).and_then(|e| e.as_token()) {
                        if t.kind() != SyntaxKind::WHITESPACE {
                            break;
                        }
                    }
                    k -= 1;
                }
                if let Some(t) = siblings.get(k).and_then(|e| e.as_token()) {
                    if t.kind() == SyntaxKind::IDENT {
                        // Check for DOT before the method name
                        if k > 0 {
                            if let Some(prev) = siblings.get(k - 1).and_then(|e| e.as_token()) {
                                if prev.kind() == SyntaxKind::DOT {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Infer the type of implicit 'it' from a BLOCK used as a trailing lambda
fn infer_implicit_it_type_from_block(
    block_node: &SyntaxNode,
    builder: &ScopeGraphBuilder,
) -> Option<KotlinType> {
    let parent = block_node.parent()?;
    let siblings: Vec<_> = parent.children_with_tokens().collect();
    let block_pos = siblings
        .iter()
        .position(|n| n.as_node() == Some(block_node))?;

    // Look backwards to find the method name
    let mut idx = block_pos.saturating_sub(1);

    // Skip whitespace and R_PAREN if present
    while idx > 0 {
        if let Some(tok) = siblings.get(idx).and_then(|e| e.as_token()) {
            if tok.kind() == SyntaxKind::WHITESPACE {
                idx -= 1;
                continue;
            }
            if tok.kind() == SyntaxKind::R_PAREN {
                // Skip to matching L_PAREN
                let mut depth = 1;
                while idx > 0 && depth > 0 {
                    idx -= 1;
                    if let Some(t) = siblings.get(idx).and_then(|e| e.as_token()) {
                        if t.kind() == SyntaxKind::R_PAREN {
                            depth += 1;
                        } else if t.kind() == SyntaxKind::L_PAREN {
                            depth -= 1;
                        }
                    }
                }
                idx = idx.saturating_sub(1);
                continue;
            }
            break;
        }
        idx -= 1;
    }

    // Now idx should point to method name (IDENT)
    let method_name = siblings
        .get(idx)
        .and_then(|e| e.as_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())?;

    // Check for DOT before method name
    if idx > 0
        && siblings
            .get(idx - 1)
            .and_then(|e| e.as_token())
            .is_some_and(|t| t.kind() == SyntaxKind::DOT)
    {
        // Extract receiver name (before the DOT)
        let receiver_name = if idx >= 2 {
            siblings
                .get(idx - 2)
                .and_then(|e| e.as_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
        } else {
            None
        };

        // Try to resolve receiver type using scope lookup
        let receiver_type = receiver_name.as_ref().map_or_else(
            || KotlinType::list(KotlinType::any()),
            |recv| {
                let block_pos_offset: usize = block_node.text_range().start().into();
                builder
                    .resolve_symbol(recv, block_pos_offset)
                    .and_then(|symbol| symbol.type_annotation.clone())
                    .map_or_else(
                        || KotlinType::list(KotlinType::any()),
                        |type_ann| {
                            tracing::debug!(
                                "Resolved trailing lambda receiver '{}' from scope with type: {:?}",
                                recv,
                                type_ann
                            );
                            type_ann
                        },
                    )
            },
        );

        return infer_lambda_it_type(&receiver_type, &method_name);
    }

    None
}

/// Infer the type of an implicit 'it' parameter in a lambda
///
/// This function walks up the AST from a LAMBDA_EXPR to find the parent
/// method call, then uses type inference to determine the 'it' type.
///
/// For example, in `list.filter { it > 0 }`:
/// - The lambda's parent is a CALL_EXPR with method "filter"
/// - The receiver is `list` which might be `List<Int>`
/// - We infer 'it' type as `Int`
fn infer_implicit_it_type(
    lambda_node: &SyntaxNode,
    builder: &ScopeGraphBuilder,
) -> Option<KotlinType> {
    // Walk up to find the parent CALL_EXPR
    let mut current = lambda_node.parent();
    let mut call_expr = None;

    while let Some(node) = current {
        if node.kind() == SyntaxKind::CALL_EXPR {
            call_expr = Some(node);
            break;
        }
        current = node.parent();
    }

    let call_expr = call_expr?;

    // Extract method name and receiver from CALL_EXPR
    let (method_name, receiver_type) = extract_call_context(&call_expr, builder)?;

    // Use inference engine to determine 'it' type
    infer_lambda_it_type(&receiver_type, &method_name)
}

/// Extract method name and receiver type from a CALL_EXPR node
///
/// Pattern: receiver.methodName { lambda } or receiver.methodName(args) { lambda }
fn extract_call_context(
    call_expr: &SyntaxNode,
    builder: &ScopeGraphBuilder,
) -> Option<(String, KotlinType)> {
    // First, check for MEMBER_ACCESS_EXPR child (new parser structure)
    // Pattern: CALL_EXPR { MEMBER_ACCESS_EXPR { IDENT, DOT, IDENT }, LAMBDA_EXPR }
    for child in call_expr.children() {
        if child.kind() == SyntaxKind::MEMBER_ACCESS_EXPR {
            let mut receiver_name: Option<String> = None;
            let mut method_name: Option<String> = None;

            for elem in child.children_with_tokens() {
                if let Some(token) = elem.as_token() {
                    if token.kind() == SyntaxKind::IDENT {
                        if receiver_name.is_none() {
                            receiver_name = Some(token.text().to_string());
                        } else {
                            method_name = Some(token.text().to_string());
                        }
                    }
                }
            }

            if let (Some(recv), Some(method)) = (receiver_name.as_ref(), method_name.as_ref()) {
                let call_pos: usize = call_expr.text_range().start().into();
                let receiver_type = resolve_receiver_type(recv, call_pos, builder);
                return Some((method.clone(), receiver_type));
            }
        }
    }

    // Fallback: look for direct tokens (old parser structure)
    // Pattern: IDENT DOT IDENT (receiver.method)
    let tokens: Vec<_> = call_expr.children_with_tokens().collect();
    let mut receiver_name: Option<String> = None;
    let mut method_name: Option<String> = None;

    let mut i = 0;
    while i < tokens.len() {
        if let Some(token) = tokens[i].as_token() {
            if token.kind() == SyntaxKind::IDENT {
                if receiver_name.is_none() {
                    receiver_name = Some(token.text().to_string());
                } else if i > 0 {
                    // Check if previous was DOT
                    let prev_is_dot = tokens
                        .get(i - 1)
                        .and_then(|t| t.as_token())
                        .is_some_and(|t| t.kind() == SyntaxKind::DOT);
                    if prev_is_dot {
                        method_name = Some(token.text().to_string());
                    }
                }
            }
        }
        i += 1;
    }

    // If we have receiver.method pattern
    if let (Some(recv), Some(method)) = (receiver_name.as_ref(), method_name.as_ref()) {
        // Try to resolve receiver type using scope lookup
        let call_pos: usize = call_expr.text_range().start().into();
        let receiver_type = resolve_receiver_type(recv, call_pos, builder);
        return Some((method.clone(), receiver_type));
    }

    None
}

/// Resolve receiver type using scope lookup
///
/// First tries to look up the receiver variable in the scope graph to get its actual type.
/// If it is not found, the receiver is unknown: its type is not guessed from the method name.
fn resolve_receiver_type(
    receiver_name: &str,
    call_pos: usize,
    builder: &ScopeGraphBuilder,
) -> KotlinType {
    // First, try to resolve the receiver from the scope graph
    if let Some(symbol) = builder.resolve_symbol(receiver_name, call_pos) {
        if let Some(ref type_ann) = symbol.type_annotation {
            tracing::debug!(
                "Resolved receiver '{}' from scope with type: {:?}",
                receiver_name,
                type_ann
            );
            return type_ann.clone();
        }
    }

    KotlinType::Unknown
}

/// Build scope graph from Kotlin syntax tree
#[allow(clippy::too_many_lines)] // Includes necessary nested helper functions
pub fn build_scope_graph(root: &SyntaxNode) -> ScopeGraph {
    use crate::ast::{ClassDecl, FunDecl, PropertyDecl};

    // Nested helper functions (must be declared before any statements)
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)] // Complex but cohesive block handling
    fn visit_node(node: &SyntaxNode, builder: &mut ScopeGraphBuilder) {
        match node.kind() {
            // Function declarations create new scopes
            SyntaxKind::FUNCTION_DECL => {
                if let Some(fun) = FunDecl::cast(node.clone()) {
                    visit_function(fun, builder);
                }
                return; // Already visited children in visit_function
            }

            // Class declarations create new scopes
            SyntaxKind::CLASS_DECL => {
                if let Some(class) = ClassDecl::cast(node.clone()) {
                    visit_class(class, builder);
                }
                return; // Already visited children in visit_class
            }

            // Property declarations add symbols
            SyntaxKind::PROPERTY_DECL => {
                if let Some(prop) = PropertyDecl::cast(node.clone()) {
                    visit_property(prop, builder);
                }
            }

            // Lambda expressions create new scopes and have parameters
            SyntaxKind::LAMBDA_EXPR => {
                let range = node.text_range();
                builder.enter_scope(range);

                // Check for LAMBDA_PARAMETERS child node (new parser structure)
                // Structure: LAMBDA_EXPR { L_BRACE, LAMBDA_PARAMETERS { IDENT, ..., ARROW }, body, R_BRACE }
                let lambda_params_node = node
                    .children()
                    .find(|c| c.kind() == SyntaxKind::LAMBDA_PARAMETERS);

                if let Some(params_node) = lambda_params_node {
                    // Extract lambda parameters from LAMBDA_PARAMETERS node
                    for elem in params_node.children_with_tokens() {
                        if let Some(token) = elem.as_token() {
                            if token.kind() == SyntaxKind::IDENT {
                                let name = token.text().to_string();
                                let param_range = token.text_range();
                                builder.add_symbol(ScopeSymbol {
                                    name,
                                    kind: ScopeSymbolKind::LambdaParameter,
                                    definition_range: param_range,
                                    type_annotation: None,
                                    method_call_info: None,
                                });
                            }
                        }
                    }
                } else {
                    // Fallback: check for arrow token directly (old parser structure)
                    // or implicit 'it' parameter
                    let tokens_vec: Vec<_> = node.children_with_tokens().collect();
                    let arrow_idx = tokens_vec.iter().position(|elem| {
                        elem.as_token()
                            .is_some_and(|t| t.kind() == SyntaxKind::ARROW)
                    });

                    if let Some(arrow_idx) = arrow_idx {
                        // Extract lambda parameters before arrow
                        for i in 0..arrow_idx {
                            if let Some(token) = tokens_vec[i].as_token() {
                                if token.kind() == SyntaxKind::IDENT {
                                    // Check if it's not after a dot (method call)
                                    let is_method_call = i > 0
                                        && tokens_vec[i - 1]
                                            .as_token()
                                            .is_some_and(|t| t.kind() == SyntaxKind::DOT);
                                    if !is_method_call {
                                        let name = token.text().to_string();
                                        let param_range = token.text_range();
                                        builder.add_symbol(ScopeSymbol {
                                            name,
                                            kind: ScopeSymbolKind::LambdaParameter,
                                            definition_range: param_range,
                                            type_annotation: None,
                                            method_call_info: None,
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        // No arrow means implicit 'it' parameter
                        // Try to infer the type from the parent method call context
                        if let Some(it_type) = infer_implicit_it_type(node, builder) {
                            builder.add_symbol(ScopeSymbol {
                                name: "it".to_string(),
                                kind: ScopeSymbolKind::LambdaParameter,
                                definition_range: range, // Use lambda range as definition
                                type_annotation: Some(it_type),
                                method_call_info: None,
                            });
                        } else {
                            // Fallback: add 'it' without type information
                            builder.add_symbol(ScopeSymbol {
                                name: "it".to_string(),
                                kind: ScopeSymbolKind::LambdaParameter,
                                definition_range: range,
                                type_annotation: None,
                                method_call_info: None,
                            });
                        }
                    }
                }

                // Visit children
                for child in node.children() {
                    visit_node(&child, builder);
                }

                builder.exit_scope();
                return; // Already visited children
            }

            // For expressions create new scopes for loop variables
            SyntaxKind::FOR_EXPR => {
                let range = node.text_range();
                builder.enter_scope(range);

                // Extract loop variable from FOR_LOOP_VAR child
                // Structure: FOR_EXPR { FOR_KW, L_PAREN, FOR_LOOP_VAR { IDENT, IN_KW, ... }, R_PAREN, BLOCK }
                for child in node.children() {
                    if child.kind() == SyntaxKind::FOR_LOOP_VAR {
                        // Find IDENT token in FOR_LOOP_VAR (before IN_KW)
                        for elem in child.children_with_tokens() {
                            if let Some(token) = elem.as_token() {
                                if token.kind() == SyntaxKind::IN_KW {
                                    break; // Stop before 'in' keyword
                                }
                                if token.kind() == SyntaxKind::IDENT {
                                    let name = token.text().to_string();
                                    let var_range = token.text_range();
                                    builder.add_symbol(ScopeSymbol {
                                        name,
                                        kind: ScopeSymbolKind::LoopVariable,
                                        definition_range: var_range,
                                        type_annotation: None,
                                        method_call_info: None,
                                    });
                                    break; // Only one loop variable (destructuring handled separately)
                                }
                            }
                        }
                        break;
                    }
                }

                // Visit children (including the BLOCK body)
                for child in node.children() {
                    visit_node(&child, builder);
                }

                builder.exit_scope();
                return; // Already visited children
            }

            // Block expressions create new scopes
            SyntaxKind::BLOCK => {
                let range = node.text_range();
                builder.enter_scope(range);

                // Check for smart casts from if-is conditions, else blocks, when entries, and null checks
                // Patterns: if (x is String), if (x !is Type) else, if (x != null), when (x) { is Type -> }
                for (var_name, narrowed_type, _source) in detect_smart_casts_for_block(node) {
                    builder.add_smart_cast(SmartCast {
                        variable_name: var_name,
                        narrowed_type,
                    });
                }

                // Check if this block is a function body
                // This handles both:
                // 1. Top-level functions with PARAMETER_LIST nodes
                // 2. Nested functions with raw parameter tokens
                if let Some(parent) = node.parent() {
                    let siblings: Vec<_> = parent.children_with_tokens().collect();
                    if let Some(pos) = siblings.iter().position(|n| n.as_node() == Some(node)) {
                        // First try: Look for PARAMETER_LIST node (top-level functions)
                        let mut found_params = false;
                        for i in (0..pos).rev() {
                            if let Some(sibling_node) = siblings[i].as_node() {
                                if sibling_node.kind() == SyntaxKind::PARAMETER_LIST {
                                    // Extract parameters from PARAMETER_LIST node
                                    for param in sibling_node.children() {
                                        if param.kind() == SyntaxKind::PARAMETER {
                                            visit_parameter(&param, builder);
                                        }
                                    }
                                    found_params = true;
                                    break;
                                }
                                // Stop if we hit another node
                                if sibling_node.kind() != SyntaxKind::PARAMETER_LIST {
                                    break;
                                }
                            }
                        }

                        // Second try: Look for parameter tokens (nested functions)
                        // Pattern: FUN_KW IDENT L_PAREN [param: Type] R_PAREN BLOCK
                        if !found_params {
                            // Look backwards for L_PAREN...R_PAREN sequence
                            let mut r_paren_idx = None;
                            let mut l_paren_idx = None;

                            for i in (0..pos).rev() {
                                if let Some(tok) = siblings[i].as_token() {
                                    if r_paren_idx.is_none() && tok.kind() == SyntaxKind::R_PAREN {
                                        r_paren_idx = Some(i);
                                    } else if r_paren_idx.is_some()
                                        && tok.kind() == SyntaxKind::L_PAREN
                                    {
                                        l_paren_idx = Some(i);
                                        break;
                                    }
                                }
                            }

                            // If we found parentheses, extract parameters from between them
                            if let (Some(l_idx), Some(r_idx)) = (l_paren_idx, r_paren_idx) {
                                // Look for pattern: IDENT COLON TYPE_IDENT between parens
                                let mut i = l_idx + 1;
                                while i < r_idx {
                                    // Skip whitespace and commas
                                    if let Some(tok) = siblings[i].as_token() {
                                        if tok.kind() == SyntaxKind::WHITESPACE
                                            || tok.kind() == SyntaxKind::COMMA
                                        {
                                            i += 1;
                                            continue;
                                        }

                                        // Look for parameter name (IDENT)
                                        if tok.kind() == SyntaxKind::IDENT {
                                            let param_name = tok.text().to_string();
                                            let param_range = tok.text_range();

                                            // Add parameter to scope
                                            builder.add_symbol(ScopeSymbol {
                                                name: param_name,
                                                kind: ScopeSymbolKind::Parameter,
                                                definition_range: param_range,
                                                type_annotation: None, // Could extract type after COLON if needed
                                                method_call_info: None,
                                            });

                                            // Skip to next comma or end
                                            i += 1;
                                            while i < r_idx {
                                                if let Some(t) = siblings[i].as_token() {
                                                    if t.kind() == SyntaxKind::COMMA {
                                                        break;
                                                    }
                                                }
                                                i += 1;
                                            }
                                        }
                                    } else {
                                        i += 1;
                                    }
                                    i += 1;
                                }
                            }
                        }
                    }
                }

                // Check if this block is a lambda with parameters
                // Pattern: { param -> ... } or { param1, param2 -> ... }
                let tokens_vec: Vec<_> = node.children_with_tokens().collect();
                let has_arrow = tokens_vec.iter().any(|elem| {
                    elem.as_token()
                        .is_some_and(|t| t.kind() == SyntaxKind::ARROW)
                });

                if has_arrow {
                    // Extract explicit lambda parameters before arrow
                    let Some(arrow_idx) = tokens_vec.iter().position(|elem| {
                        elem.as_token()
                            .is_some_and(|t| t.kind() == SyntaxKind::ARROW)
                    }) else {
                        // If has_arrow is true, arrow must exist - this is a logic error
                        tracing::error!("has_arrow is true but arrow not found in lambda");
                        return;
                    };

                    for i in 0..arrow_idx {
                        if let Some(token) = tokens_vec[i].as_token() {
                            if token.kind() == SyntaxKind::IDENT {
                                // Check if it's not after a dot (method call)
                                let is_method_call = i > 0
                                    && tokens_vec[i - 1]
                                        .as_token()
                                        .is_some_and(|t| t.kind() == SyntaxKind::DOT);
                                if !is_method_call {
                                    let name = token.text().to_string();
                                    let param_range = token.text_range();
                                    builder.add_symbol(ScopeSymbol {
                                        name,
                                        kind: ScopeSymbolKind::LambdaParameter,
                                        definition_range: param_range,
                                        type_annotation: None,
                                        method_call_info: None,
                                    });
                                }
                            }
                        }
                    }
                } else if is_trailing_lambda(node) {
                    // No arrow means implicit 'it' parameter for trailing lambdas
                    // Try to infer the type from the parent method call context
                    if let Some(it_type) = infer_implicit_it_type_from_block(node, builder) {
                        builder.add_symbol(ScopeSymbol {
                            name: "it".to_string(),
                            kind: ScopeSymbolKind::LambdaParameter,
                            definition_range: range, // Use block range as definition
                            type_annotation: Some(it_type),
                            method_call_info: None,
                        });
                    } else {
                        // Fallback: add 'it' without type information
                        builder.add_symbol(ScopeSymbol {
                            name: "it".to_string(),
                            kind: ScopeSymbolKind::LambdaParameter,
                            definition_range: range,
                            type_annotation: None,
                            method_call_info: None,
                        });
                    }
                }

                // Extract local variables from val/var declarations
                // Handle PROPERTY_DECL child nodes (parsed property declarations)
                for child in node.children() {
                    if child.kind() == SyntaxKind::PROPERTY_DECL {
                        if let Some(prop) = PropertyDecl::cast(child.clone()) {
                            visit_property(prop, builder);
                        }
                    }
                }

                // Also handle tokens directly in BLOCK (unparsed/flat structure)
                let mut tokens = node.children_with_tokens().peekable();
                while let Some(elem) = tokens.next() {
                    // Check for val/var keyword
                    if let Some(token) = elem.as_token() {
                        if token.kind() == SyntaxKind::VAL_KW || token.kind() == SyntaxKind::VAR_KW
                        {
                            // Skip whitespace
                            while let Some(next) = tokens.peek() {
                                if let Some(t) = next.as_token() {
                                    if t.kind() == SyntaxKind::WHITESPACE {
                                        tokens.next();
                                        continue;
                                    }
                                }
                                break;
                            }

                            // Check for destructuring: val (a, b) = ...
                            if let Some(next) = tokens.peek() {
                                if let Some(t) = next.as_token() {
                                    if t.kind() == SyntaxKind::L_PAREN {
                                        tokens.next(); // consume L_PAREN
                                                       // Extract all identifiers until R_PAREN
                                        loop {
                                            // Skip whitespace
                                            while let Some(next) = tokens.peek() {
                                                if let Some(t) = next.as_token() {
                                                    if t.kind() == SyntaxKind::WHITESPACE {
                                                        tokens.next();
                                                        continue;
                                                    }
                                                }
                                                break;
                                            }

                                            if let Some(next) = tokens.peek() {
                                                if let Some(t) = next.as_token() {
                                                    if t.kind() == SyntaxKind::R_PAREN {
                                                        tokens.next(); // consume R_PAREN
                                                        break;
                                                    } else if t.kind() == SyntaxKind::IDENT {
                                                        let name = t.text().to_string();
                                                        let var_range = t.text_range();
                                                        tokens.next(); // consume IDENT
                                                        builder.add_symbol(ScopeSymbol {
                                                            name,
                                                            kind: ScopeSymbolKind::Variable,
                                                            definition_range: var_range,
                                                            type_annotation: None,
                                                            method_call_info: None,
                                                        });
                                                        // Skip comma
                                                        while let Some(next) = tokens.peek() {
                                                            if let Some(t) = next.as_token() {
                                                                if t.kind() == SyntaxKind::COMMA
                                                                    || t.kind()
                                                                        == SyntaxKind::WHITESPACE
                                                                {
                                                                    tokens.next();
                                                                    continue;
                                                                }
                                                            }
                                                            break;
                                                        }
                                                    } else {
                                                        break;
                                                    }
                                                } else {
                                                    break;
                                                }
                                            } else {
                                                break;
                                            }
                                        }
                                        continue;
                                    }
                                }
                            }

                            // Regular variable: get variable name
                            if let Some(next) = tokens.next() {
                                if let Some(name_token) = next.as_token() {
                                    if name_token.kind() == SyntaxKind::IDENT {
                                        let name = name_token.text().to_string();
                                        let range = name_token.text_range();

                                        // Extract type annotation if present (explicit type)
                                        let mut type_annotation =
                                            extract_type_annotation_from_tokens(&mut tokens);

                                        // Method call info (extracted alongside type inference)
                                        let mut method_call_info: Option<MethodCallInfo> = None;

                                        // If no explicit type, try to infer from initializer
                                        if type_annotation.is_none() {
                                            // Look for a PROPERTY_DECL ancestor which will contain the initializer expression
                                            let mut property_decl_node = None;
                                            let mut current = name_token.parent();
                                            while let Some(node) = current {
                                                if node.kind() == SyntaxKind::PROPERTY_DECL {
                                                    property_decl_node = Some(node.clone());
                                                    break;
                                                }
                                                current = node.parent();
                                            }

                                            // Try both PROPERTY_DECL (for top-level props) and BLOCK (for local vars)
                                            let container_node =
                                                property_decl_node.or_else(|| name_token.parent());

                                            if let Some(container) = container_node {
                                                // Collect all elements after the = sign
                                                let all_children: Vec<_> =
                                                    container.children_with_tokens().collect();
                                                let mut eq_index = None;
                                                let mut name_token_index = None;

                                                // Find the index of the current variable name token
                                                for (i, elem) in all_children.iter().enumerate() {
                                                    if let Some(token) = elem.as_token() {
                                                        if token.text_range()
                                                            == name_token.text_range()
                                                        {
                                                            name_token_index = Some(i);
                                                            break;
                                                        }
                                                    }
                                                }

                                                // Find the = sign AFTER the variable name
                                                if let Some(start_idx) = name_token_index {
                                                    for (i, elem) in all_children
                                                        .iter()
                                                        .enumerate()
                                                        .skip(start_idx)
                                                    {
                                                        if let Some(token) = elem.as_token() {
                                                            if token.kind() == SyntaxKind::EQ {
                                                                eq_index = Some(i);
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }

                                                if let Some(eq_idx) = eq_index {
                                                    // Get all tokens/nodes after = until end of statement
                                                    // Statement ends at newline (after meaningful content), semicolon, or brace
                                                    let mut init_elements = Vec::new();
                                                    let mut has_meaningful_content = false;
                                                    for elem in &all_children[(eq_idx + 1)..] {
                                                        // Stop at statement terminators
                                                        if let Some(token) = elem.as_token() {
                                                            // Skip leading whitespace (before any meaningful content)
                                                            // But don't break on whitespace with newlines in raw strings
                                                            if token.kind()
                                                                == SyntaxKind::RAW_STRING_LITERAL
                                                            {
                                                                has_meaningful_content = true;
                                                                // Raw strings can contain newlines, don't stop
                                                            } else if token.kind()
                                                                == SyntaxKind::SEMICOLON
                                                            {
                                                                // Semicolon always ends statement
                                                                break;
                                                            } else if token.text().contains('\n') {
                                                                // Only break on newlines AFTER meaningful content
                                                                // Allows multi-line raw strings to work
                                                                if has_meaningful_content {
                                                                    break;
                                                                }
                                                                // Skip leading whitespace with newlines
                                                                continue;
                                                            }
                                                            // Track when we've seen meaningful content
                                                            if !token.kind().is_trivia() {
                                                                has_meaningful_content = true;
                                                            }
                                                            // VAL/VAR/FUN keywords start new statement
                                                            if matches!(
                                                                token.kind(),
                                                                SyntaxKind::VAL_KW
                                                                    | SyntaxKind::VAR_KW
                                                                    | SyntaxKind::FUN_KW
                                                            ) {
                                                                break;
                                                            }
                                                        } else {
                                                            // Nodes are always meaningful content
                                                            has_meaningful_content = true;
                                                        }
                                                        init_elements.push(elem.clone());
                                                    }

                                                    // First try: Look for expression nodes (BINARY_EXPR, etc.)
                                                    for elem in &init_elements {
                                                        if let Some(init_node) = elem.as_node() {
                                                            type_annotation = TypeExtractor::infer_from_initializer(init_node)
                                                                .map(KotlinType::from);
                                                            if type_annotation.is_some() {
                                                                break;
                                                            }
                                                        }
                                                    }

                                                    // Second try: Constructor call from tokens
                                                    if type_annotation.is_none() {
                                                        type_annotation =
                                                            TypeExtractor::infer_from_token_sequence(
                                                                &init_elements,
                                                            )
                                                            .map(KotlinType::from);
                                                    }

                                                    // Third try: Literal from first non-whitespace token
                                                    // Also handle negative literals (MINUS followed by numeric literal)
                                                    if type_annotation.is_none() {
                                                        let mut found_minus = false;
                                                        for elem in &init_elements {
                                                            if let Some(token) = elem.as_token() {
                                                                if token.kind().is_trivia() {
                                                                    continue;
                                                                }
                                                                // Handle negative literals: MINUS + INT/LONG/FLOAT/DOUBLE
                                                                if token.kind() == SyntaxKind::MINUS
                                                                {
                                                                    found_minus = true;
                                                                    continue;
                                                                }
                                                                if let Some(inferred) =
                                                                    TypeExtractor::infer_literal_type(
                                                                        token.kind(),
                                                                        token.text(),
                                                                    )
                                                                    .map(KotlinType::from)
                                                                {
                                                                    type_annotation =
                                                                        Some(inferred);
                                                                    break;
                                                                }
                                                                // If we hit a non-literal, non-minus token, stop
                                                                if !found_minus {
                                                                    break;
                                                                }
                                                                // After minus, if we didn't find a literal, stop
                                                                break;
                                                            }
                                                        }
                                                    }

                                                    // Extract method call info from init_elements
                                                    method_call_info =
                                                        extract_method_call_info_from_elements(
                                                            &init_elements,
                                                        );

                                                    // If we inferred type from token sequence but also found a method call,
                                                    // and the "type" matches the method name, check if it's a function call (not constructor)
                                                    if let (
                                                        Some(KotlinType::Class { ref fqn, .. }),
                                                        Some(ref call_info),
                                                    ) = (&type_annotation, &method_call_info)
                                                    {
                                                        if fqn == &call_info.method_name
                                                            && call_info.receiver_name.is_none()
                                                        {
                                                            // Check if this is a function call (lowercase first char) vs constructor (uppercase)
                                                            if fqn
                                                                .chars()
                                                                .next()
                                                                .is_some_and(char::is_lowercase)
                                                            {
                                                                // This is a function call, not a constructor
                                                                // e.g., listOf(...) was incorrectly inferred as KotlinType::Class { fqn: "listOf" }
                                                                type_annotation = None;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        builder.add_symbol(ScopeSymbol {
                                            name,
                                            kind: ScopeSymbolKind::Variable,
                                            definition_range: range,
                                            type_annotation,
                                            method_call_info,
                                        });
                                    }
                                }
                            }
                        } else if token.kind() == SyntaxKind::FOR_KW {
                            // Handle for-loop: for (item in items) { ... }
                            // Skip whitespace and L_PAREN
                            while let Some(next) = tokens.peek() {
                                if let Some(t) = next.as_token() {
                                    if t.kind() == SyntaxKind::WHITESPACE
                                        || t.kind() == SyntaxKind::L_PAREN
                                    {
                                        tokens.next();
                                        continue;
                                    }
                                }
                                break;
                            }

                            // Get loop variable name
                            if let Some(next) = tokens.next() {
                                if let Some(name_token) = next.as_token() {
                                    if name_token.kind() == SyntaxKind::IDENT {
                                        let name = name_token.text().to_string();
                                        let range = name_token.text_range();
                                        builder.add_symbol(ScopeSymbol {
                                            name,
                                            kind: ScopeSymbolKind::LoopVariable,
                                            definition_range: range,
                                            type_annotation: None,
                                            method_call_info: None,
                                        });
                                    }
                                }
                            }
                        } else if token.kind() == SyntaxKind::FUN_KW {
                            // Handle nested function: fun name(...) { ... }
                            // Nested functions in Kotlin are not wrapped in FUNCTION_DECL nodes,
                            // they appear as token sequences: FUN_KW IDENT PARAMETER_LIST BLOCK

                            // Skip whitespace
                            while let Some(next) = tokens.peek() {
                                if let Some(t) = next.as_token() {
                                    if t.kind() == SyntaxKind::WHITESPACE {
                                        tokens.next();
                                        continue;
                                    }
                                }
                                break;
                            }

                            // Get function name
                            let mut fun_name = None;
                            if let Some(next) = tokens.next() {
                                if let Some(name_token) = next.as_token() {
                                    if name_token.kind() == SyntaxKind::IDENT {
                                        let name = name_token.text().to_string();
                                        let range = name_token.text_range();
                                        builder.add_symbol(ScopeSymbol {
                                            name: name.clone(),
                                            kind: ScopeSymbolKind::Function,
                                            definition_range: range,
                                            type_annotation: None,
                                            method_call_info: None,
                                        });
                                        fun_name = Some(name);
                                    }
                                }
                            }

                            // Look ahead to find PARAMETER_LIST and then the function body BLOCK
                            // We need to handle parameters specially since the function body
                            // creates its own scope via normal BLOCK processing, but parameters
                            // need to be in that scope
                            if fun_name.is_some() {
                                // NOTE: Parameters are handled by the BLOCK visitor which extracts them
                                // from the preceding PARAMETER_LIST node. This architecture avoids
                                // duplicate parameter processing.
                            }
                        }
                    }
                }

                // Visit children
                for child in node.children() {
                    visit_node(&child, builder);
                }

                builder.exit_scope();
                return; // Already visited children
            }

            _ => {}
        }

        // Visit children for other nodes
        for child in node.children() {
            visit_node(&child, builder);
        }
    }

    fn visit_function(fun: FunDecl, builder: &mut ScopeGraphBuilder) {
        // Add function to current scope
        for elem in fun.syntax().children_with_tokens() {
            if elem.kind() == SyntaxKind::IDENT {
                if let Some(token) = elem.as_token() {
                    let name = token.text().to_string();
                    let range = token.text_range();
                    builder.add_symbol(ScopeSymbol {
                        name,
                        kind: ScopeSymbolKind::Function,
                        definition_range: range,
                        type_annotation: None,
                        method_call_info: None,
                    });
                    break;
                }
            }
        }

        // Enter function body scope
        let body_range = fun.syntax().text_range();
        builder.enter_scope(body_range);

        // Add parameters to function scope
        for child in fun.syntax().children() {
            if child.kind() == SyntaxKind::PARAMETER_LIST {
                for param in child.children() {
                    if param.kind() == SyntaxKind::PARAMETER {
                        visit_parameter(&param, builder);
                    }
                }
            }
        }

        // Visit function body
        for child in fun.syntax().children() {
            visit_node(&child, builder);
        }

        builder.exit_scope();
    }

    fn visit_parameter(param: &SyntaxNode, builder: &mut ScopeGraphBuilder) {
        // Extract parameter name
        let mut name = None;
        for child in param.children_with_tokens() {
            if child.kind() == SyntaxKind::IDENT && name.is_none() {
                if let Some(token) = child.as_token() {
                    name = Some((token.text().to_string(), token.text_range()));
                    break;
                }
            }
        }

        // Extract type using TypeExtractor and convert to KotlinType
        let type_annotation = TypeExtractor::extract_from_parameter(param).map(KotlinType::from);

        if let Some((param_name, range)) = name {
            builder.add_symbol(ScopeSymbol {
                name: param_name,
                kind: ScopeSymbolKind::Parameter,
                definition_range: range,
                type_annotation,
                method_call_info: None,
            });
        }
    }

    fn visit_class(class: ClassDecl, builder: &mut ScopeGraphBuilder) {
        // Add class to current scope
        for elem in class.syntax().children_with_tokens() {
            if elem.kind() == SyntaxKind::IDENT {
                if let Some(token) = elem.as_token() {
                    let name = token.text().to_string();
                    let range = token.text_range();
                    builder.add_symbol(ScopeSymbol {
                        name,
                        kind: ScopeSymbolKind::Class,
                        definition_range: range,
                        type_annotation: None,
                        method_call_info: None,
                    });
                    break;
                }
            }
        }

        // Enter class body scope
        let body_range = class.syntax().text_range();
        builder.enter_scope(body_range);

        // Visit class body
        for child in class.syntax().children() {
            visit_node(&child, builder);
        }

        builder.exit_scope();
    }

    fn visit_property(prop: PropertyDecl, builder: &mut ScopeGraphBuilder) {
        // Extract property name
        let mut name = None;
        let mut eq_index = None;
        let all_children: Vec<_> = prop.syntax().children_with_tokens().collect();

        for (i, child) in all_children.iter().enumerate() {
            if let Some(token) = child.as_token() {
                if child.kind() == SyntaxKind::IDENT && name.is_none() {
                    name = Some((token.text().to_string(), token.text_range()));
                }
                if child.kind() == SyntaxKind::EQ && eq_index.is_none() {
                    eq_index = Some(i);
                }
            }
        }

        // Extract type using TypeExtractor (includes inference from initializer)
        let type_annotation = TypeExtractor::infer_val_type(prop.syntax());

        // Extract method call info from initializer (elements after =)
        // Only extract if there's no explicit type annotation, since method call info
        // is for type inference purposes
        let method_call_info = if type_annotation.is_none() {
            eq_index.and_then(|eq_idx| {
                // Flatten elements to include tokens from nested nodes
                // BUT preserve BLOCK and LAMBDA_EXPR nodes for trailing lambda detection
                fn flatten_elements(
                    elems: &[rowan::SyntaxElement<crate::KotlinLanguage>],
                ) -> Vec<rowan::SyntaxElement<crate::KotlinLanguage>> {
                    let mut result = Vec::new();
                    for elem in elems {
                        if let Some(node) = elem.as_node() {
                            // Preserve BLOCK and LAMBDA_EXPR nodes for trailing lambda detection
                            if matches!(node.kind(), SyntaxKind::BLOCK | SyntaxKind::LAMBDA_EXPR) {
                                result.push(elem.clone());
                            } else {
                                // Recursively flatten other node's children
                                let children: Vec<_> = node.children_with_tokens().collect();
                                result.extend(flatten_elements(&children));
                            }
                        } else {
                            result.push(elem.clone());
                        }
                    }
                    result
                }

                let init_elements: Vec<_> = all_children[(eq_idx + 1)..].to_vec();
                let flattened = flatten_elements(&init_elements);
                extract_method_call_info_from_elements(&flattened)
            })
        } else {
            None
        };

        if let Some((prop_name, range)) = name {
            builder.add_symbol(ScopeSymbol {
                name: prop_name,
                kind: ScopeSymbolKind::Variable,
                definition_range: range,
                type_annotation: type_annotation.map(KotlinType::from),
                method_call_info,
            });
        }
    }

    // Main function body starts here
    let file_range = root.text_range();
    let mut builder = ScopeGraphBuilder::new(file_range);

    // First pass: Extract imports
    for child in root.children() {
        if child.kind() == SyntaxKind::IMPORT_DECL {
            extract_import(&child, &mut builder);
        }
    }

    // Traverse AST and build scopes
    visit_node(root, &mut builder);
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn print_tree(node: &SyntaxNode, indent: usize) {
        for elem in node.children_with_tokens() {
            let prefix = "  ".repeat(indent);
            match elem {
                rowan::NodeOrToken::Node(n) => {
                    eprintln!("{}Node: {:?} @ {:?}", prefix, n.kind(), n.text_range());
                    print_tree(&n, indent + 1);
                }
                rowan::NodeOrToken::Token(t) => {
                    eprintln!(
                        "{}Token: {:?} '{}' @ {:?}",
                        prefix,
                        t.kind(),
                        t.text(),
                        t.text_range()
                    );
                }
            }
        }
    }

    #[test]
    fn test_local_variable_scope() {
        let source = r"
            fun test() {
                val x = 5
                val y = x + 1
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Debug: print AST structure
        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        let scope_graph = build_scope_graph(&root);

        // Debug: print scope graph
        eprintln!("\n=== SCOPE GRAPH ===");
        eprintln!("{scope_graph:?}");

        // Find position of 'x' in 'x + 1'
        let x_usage_pos = source.find("x + 1").unwrap();
        eprintln!("\n=== Looking for 'x' at position {x_usage_pos} ===");

        // Should resolve to the 'val x = 5' declaration
        let symbol = scope_graph.resolve_symbol("x", x_usage_pos);
        eprintln!("Result: {symbol:?}");
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().name, "x");
        assert_eq!(symbol.unwrap().kind, ScopeSymbolKind::Variable);
    }

    #[test]
    fn test_function_parameter_scope() {
        let source = r"
            fun greet(name: String) {
                println(name)
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find position of 'name' in 'println(name)'
        let name_usage_pos = source.find("println(name)").unwrap() + "println(".len();

        // Should resolve to the parameter
        let symbol = scope_graph.resolve_symbol("name", name_usage_pos);
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().name, "name");
        assert_eq!(symbol.unwrap().kind, ScopeSymbolKind::Parameter);
    }

    #[test]
    fn test_nested_scopes() {
        let source = r"
            fun outer() {
                val x = 1
                fun inner() {
                    val y = x + 2
                }
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find position of 'x' in 'x + 2'
        let x_usage_pos = source.find("x + 2").unwrap();

        // Should resolve to outer scope's 'x'
        let symbol = scope_graph.resolve_symbol("x", x_usage_pos);
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().name, "x");
    }

    #[test]
    fn test_shadowing() {
        let source = r"
            fun test() {
                val x = 1
                {
                    val x = 2
                    println(x)
                }
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find position of 'x' in 'println(x)'
        let x_usage_pos = source.find("println(x)").unwrap() + "println(".len();

        // Should resolve to inner scope's 'x = 2'
        let symbol = scope_graph.resolve_symbol("x", x_usage_pos);
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().name, "x");

        // The definition should be the inner one (closer to usage)
        let inner_val_pos = source.rfind("val x = 2").unwrap();
        let def_pos: usize = symbol.unwrap().definition_range.start().into();
        assert!(def_pos > inner_val_pos - 20 && def_pos < inner_val_pos + 20);
    }

    #[test]
    fn test_for_loop_variable() {
        let source = r"
            fun test() {
                for (item in items) {
                    println(item)
                }
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find position of 'item' in 'println(item)'
        let item_usage_pos = source.find("println(item)").unwrap() + "println(".len();

        // Should resolve to loop variable
        let symbol = scope_graph.resolve_symbol("item", item_usage_pos);
        assert!(symbol.is_some());
        assert_eq!(symbol.unwrap().name, "item");
        assert_eq!(symbol.unwrap().kind, ScopeSymbolKind::LoopVariable);
    }

    #[test]
    fn test_symbols_in_scope() {
        let source = r#"
            fun test(param1: String, param2: Int) {
                val local1 = 5
                val local2 = "hello"
            }
        "#;

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find position inside function body
        let pos = source.find("local2").unwrap();

        // Should have both parameters and both locals
        let symbols = scope_graph.symbols_in_scope(pos);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();

        assert!(names.contains(&"param1"));
        assert!(names.contains(&"param2"));
        assert!(names.contains(&"local1"));
        assert!(names.contains(&"local2"));
        assert!(names.contains(&"test")); // Function itself
    }

    #[test]
    fn test_method_call_extraction() {
        let source = r#"
            class User {
                fun getName(): String = "John"
            }

            fun test() {
                val user = User()
                val name = user.getName()
                val id = listOf(1, 2, 3)
            }
        "#;

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find "name" variable
        let name_pos = source.find("val name").unwrap() + 10;
        let name_symbol = scope_graph.resolve_symbol("name", name_pos);

        assert!(name_symbol.is_some());
        let name_symbol = name_symbol.unwrap();
        assert_eq!(name_symbol.name.as_str(), "name");

        // Check method call info
        assert!(name_symbol.method_call_info.is_some());
        let method_info = name_symbol.method_call_info.as_ref().unwrap();
        assert_eq!(method_info.method_name, "getName");
        assert_eq!(method_info.receiver_name, Some("user".to_string()));
        assert!(!method_info.is_chained);

        // Find "id" variable
        let id_pos = source.find("val id").unwrap() + 10;
        let id_symbol = scope_graph.resolve_symbol("id", id_pos);

        assert!(id_symbol.is_some());
        let id_symbol = id_symbol.unwrap();
        assert_eq!(id_symbol.name.as_str(), "id");

        // Note: stdlib functions like listOf now get type inference directly,
        // so method_call_info is None but type_annotation is populated
        assert!(
            id_symbol.type_annotation.is_some(),
            "Expected type_annotation for stdlib function call"
        );
        let type_str = id_symbol.type_annotation.as_ref().unwrap().to_string();
        assert!(
            type_str.contains("List"),
            "Expected List type but got: {type_str}"
        );
    }

    #[test]
    fn test_implicit_it_parameter() {
        let source = r"
            fun test() {
                val list = listOf(1, 2, 3)
                list.filter { it > 0 }
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Debug: print the AST to understand the structure
        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        let scope_graph = build_scope_graph(&root);

        // Debug: print scope graph
        eprintln!("\n=== SCOPE GRAPH ===");
        eprintln!("{scope_graph:?}");

        // Find position inside the lambda
        let it_usage_pos = source.find("it > 0").unwrap();
        eprintln!("\nLooking for 'it' at position {it_usage_pos}");

        // Should resolve 'it' to an implicit lambda parameter
        let symbol = scope_graph.resolve_symbol("it", it_usage_pos);
        eprintln!("Result: {symbol:?}");

        assert!(
            symbol.is_some(),
            "Expected 'it' symbol to be found in lambda scope"
        );
        let symbol = symbol.unwrap();
        assert_eq!(symbol.name.as_str(), "it");
        assert_eq!(symbol.kind, ScopeSymbolKind::LambdaParameter);

        // The type should be inferred (Any for now since we use heuristics)
        // In a full implementation with scope-based receiver type lookup,
        // this would be 'Int' from List<Int>.filter
        assert!(
            symbol.type_annotation.is_some(),
            "Expected 'it' to have inferred type"
        );
        eprintln!("Inferred type: {:?}", symbol.type_annotation);
    }

    #[test]
    fn test_explicit_lambda_parameter() {
        let source = r"
            fun test() {
                val list = listOf(1, 2, 3)
                list.map { x -> x * 2 }
            }
        ";

        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let scope_graph = build_scope_graph(&root);

        // Find position inside the lambda
        let x_usage_pos = source.find("x * 2").unwrap();

        // Should resolve 'x' to an explicit lambda parameter
        let symbol = scope_graph.resolve_symbol("x", x_usage_pos);
        assert!(
            symbol.is_some(),
            "Expected 'x' symbol to be found in lambda scope"
        );
        let symbol = symbol.unwrap();
        assert_eq!(symbol.name.as_str(), "x");
        assert_eq!(symbol.kind, ScopeSymbolKind::LambdaParameter);
    }

    #[test]
    fn test_constructor_type_inference() {
        let source = r#"
class User(val name: String)
fun test() {
    val user = User("John")
}
"#;
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Debug: print AST structure
        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        let scope_graph = build_scope_graph(&root);

        // Debug: print scope graph
        eprintln!("\n=== SCOPE GRAPH ===");
        eprintln!("{scope_graph:#?}");

        // Find position of 'user' usage
        let user_pos = source.find("val user").unwrap() + 4;
        eprintln!("\nLooking for 'user' at position {user_pos}");

        let symbol = scope_graph.resolve_symbol("user", user_pos);
        eprintln!("Result: {symbol:?}");

        assert!(symbol.is_some(), "Expected 'user' symbol to be found");
        let symbol = symbol.unwrap();
        assert_eq!(symbol.name.as_str(), "user");
        assert_eq!(symbol.kind, ScopeSymbolKind::Variable);

        // The type should be inferred as User from the constructor call
        eprintln!("Type annotation: {:?}", symbol.type_annotation);
        assert!(
            symbol.type_annotation.is_some(),
            "Expected 'user' to have inferred User type from constructor"
        );

        // Check that it's User, not String (which would be wrong)
        let type_str = symbol.type_annotation.as_ref().unwrap().to_string();
        assert!(
            type_str.contains("User"),
            "Expected User type but got: {type_str}"
        );
    }

    #[test]
    fn test_literal_type_inference() {
        let source = r"
fun test() {
    val count = 5
}
";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Debug: print AST structure
        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        let scope_graph = build_scope_graph(&root);

        // Debug: print scope graph
        eprintln!("\n=== SCOPE GRAPH ===");
        eprintln!("{scope_graph:#?}");

        // Find position of 'count' usage
        let count_pos = source.find("val count").unwrap() + 4;
        eprintln!("\nLooking for 'count' at position {count_pos}");

        let symbol = scope_graph.resolve_symbol("count", count_pos);
        eprintln!("Result: {symbol:?}");

        assert!(symbol.is_some(), "Expected 'count' symbol to be found");
        let symbol = symbol.unwrap();
        assert_eq!(symbol.name.as_str(), "count");
        assert_eq!(symbol.kind, ScopeSymbolKind::Variable);

        // The type should be inferred as Int from the literal 5
        eprintln!("Type annotation: {:?}", symbol.type_annotation);
        assert!(
            symbol.type_annotation.is_some(),
            "Expected 'count' to have inferred Int type from literal 5"
        );
    }

    #[test]
    fn test_property_access_method_call_info() {
        let source = r#"
fun test() {
    val text = "hello"
    val len = text.length
}
"#;
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Debug: print AST structure
        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        let scope_graph = build_scope_graph(&root);

        // Debug: print scope graph
        eprintln!("\n=== SCOPE GRAPH ===");
        eprintln!("{scope_graph:#?}");

        // Find position of 'len' variable
        let len_pos = source.find("val len").unwrap() + 4;
        eprintln!("\nLooking for 'len' at position {len_pos}");

        let symbol = scope_graph.resolve_symbol("len", len_pos);
        eprintln!("Result: {symbol:?}");

        assert!(symbol.is_some(), "Expected 'len' symbol to be found");
        let symbol = symbol.unwrap();
        assert_eq!(symbol.name.as_str(), "len");
        assert_eq!(symbol.kind, ScopeSymbolKind::Variable);

        // Should have method_call_info for property access
        eprintln!("method_call_info: {:?}", symbol.method_call_info);
        assert!(
            symbol.method_call_info.is_some(),
            "Expected 'len' to have method_call_info for text.length"
        );

        let call_info = symbol.method_call_info.as_ref().unwrap();
        assert_eq!(call_info.receiver_name, Some("text".to_string()));
        assert_eq!(call_info.method_name, "length");
    }

    #[test]
    fn test_trailing_lambda_method_call_info() {
        let source = r"
fun test() {
    val numbers = listOf(1, 2, 3, 4, 5)
    val evens = numbers.filter { it % 2 == 0 }
}
";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Debug: print AST structure
        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        let scope_graph = build_scope_graph(&root);

        // Debug: print scope graph
        eprintln!("\n=== SCOPE GRAPH ===");
        eprintln!("{scope_graph:#?}");

        // Find "evens" variable
        let evens_pos = source.find("val evens").unwrap() + 4;
        eprintln!("\nLooking for 'evens' at position {evens_pos}");

        let symbol = scope_graph.resolve_symbol("evens", evens_pos);
        eprintln!("Result: {symbol:?}");

        assert!(symbol.is_some(), "Expected 'evens' symbol to be found");
        let symbol = symbol.unwrap();
        assert_eq!(symbol.name.as_str(), "evens");

        // Should have method_call_info for the filter call
        eprintln!("type_annotation: {:?}", symbol.type_annotation);
        eprintln!("method_call_info: {:?}", symbol.method_call_info);
        assert!(
            symbol.method_call_info.is_some(),
            "Expected 'evens' to have method_call_info for numbers.filter"
        );

        let call_info = symbol.method_call_info.as_ref().unwrap();
        assert_eq!(call_info.receiver_name, Some("numbers".to_string()));
        assert_eq!(call_info.method_name, "filter");
    }
}
