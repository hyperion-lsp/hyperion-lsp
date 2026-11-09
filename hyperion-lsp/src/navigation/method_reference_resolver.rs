// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method Reference Resolution
//!
//! Handles Java method reference syntax (User::new, list::add, etc.)

use crate::navigation::context::NavigationContext;
use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
use crate::navigation::type_finder::{find_method_in_ast, find_variable_type};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use rowan::{SyntaxToken, TextSize};
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Url};
use tracing::debug;

/// Resolve the type of a field access chain like "config.settings" or "System.out"
///
/// This walks the chain of field accesses:
/// 1. Start with first part - check if it's a variable and get its type
/// 2. For each subsequent part, look up the field in the class and get its type
///
/// Returns the FQN of the final type in the chain, or None if resolution fails.
fn resolve_field_chain_type(
    receiver: &str,
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    workspace_index: &Arc<WorkspaceIndex>,
    file_id: FileId,
) -> Option<String> {
    let parts: Vec<&str> = receiver.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    // Handle well-known static field patterns first (common case)
    match receiver {
        "System.out" | "System.err" => return Some("java.io.PrintStream".to_string()),
        "System.in" => return Some("java.io.InputStream".to_string()),
        _ => {}
    }

    // Start with first part - might be a variable or class name
    let first_part = parts[0];
    let mut current_type = find_variable_type(token, first_part)
        .and_then(|t| resolve_class_fqn_via_imports(workspace_index.as_ref(), file_id, &t).ok())
        .or_else(|| {
            // Maybe it's a class name directly
            resolve_class_fqn_via_imports(workspace_index.as_ref(), file_id, first_part).ok()
        })?;

    // Walk remaining parts of the chain through the canonical field election
    //. Reading only the declared members of each owner broke the
    // chain at the first inherited field, and treated a name inherited
    // ambiguously from two branches as if one of them had won.
    for field_name in &parts[1..] {
        let definition =
            workspace_index.find_effective_field_definition_by_name(&current_type, field_name)?;
        let field_type = definition.symbol.return_type.as_deref()?;

        // Resolve the declared type in the field's source context. The method
        // reference can live in a file with unrelated imports for the same
        // simple type spelling.
        current_type = resolve_class_fqn_via_imports(
            workspace_index.as_ref(),
            definition.location.file_id,
            field_type,
        )
        .ok()?;
    }

    Some(current_type)
}

use super::NavigationProvider;

impl NavigationProvider<'_> {
    pub(crate) fn handle_method_reference(
        &self,
        ctx: &NavigationContext,
        token: &mut SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        let token_offset = ctx
            .parse_tree
            .syntax()
            .token_at_offset(TextSize::from(ctx.offset as u32));
        let mut current_token = token.clone();
        let original_token = token.clone();
        let mut tried_left = false;

        let resolve_if_method_ref = |token: &SyntaxToken<hyperion_parser::JavaLanguage>| {
            let mut node = token.parent();
            while let Some(parent) = node {
                match parent.kind() {
                    SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR => {
                        debug!("Detected method reference expression");
                        return Some(self.resolve_method_reference(
                            ctx.file_id,
                            token,
                            &parent,
                            ctx.uri,
                            ctx.source_text,
                            &ctx.parse_tree.syntax(),
                        ));
                    }
                    _ => {
                        node = parent.parent();
                    }
                }
            }
            None
        };

        'method_ref_check: loop {
            if let Some(result) = resolve_if_method_ref(&current_token) {
                return result;
            }

            if !tried_left {
                if let Some(left_token) = token_offset.clone().left_biased() {
                    if left_token.kind() == SyntaxKind::IDENT {
                        current_token = left_token;
                        tried_left = true;
                        continue 'method_ref_check;
                    }
                    break;
                }
            }
            break;
        }

        if let Some(location) =
            self.resolve_constructor_reference_from_tokens(ctx, &current_token)?
        {
            return Ok(Some(location));
        }

        if current_token.kind() != SyntaxKind::IDENT {
            current_token = original_token;
        }

        *token = current_token;
        Ok(None)
    }
    pub(crate) fn resolve_method_reference(
        &self,
        _file_id: FileId,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        parent: &hyperion_parser::syntax::SyntaxNode,
        uri: &Url,
        source_text: &str,
        ast_root: &SyntaxNode,
    ) -> Result<Option<Location>, String> {
        // debug!("RESOLVE_METHOD_REF: Starting resolution for token: {}", token.text());
        debug!("Resolving method reference at token: {}", token.text());

        // For method references, we need to:
        // 1. Find the receiver (left side of ::)
        // 2. Find the method/constructor name (right side of ::)
        // 3. Resolve the receiver type
        // 4. Find the method in that type

        let mut receiver_text = None;
        let mut method_name = None;
        let mut is_constructor = parent.kind() == SyntaxKind::CONSTRUCTOR_REF_EXPR;

        // Parse METHOD_REF_EXPR / CONSTRUCTOR_REF_EXPR directly.
        // The current CST stores the receiver and the right-hand symbol inside `parent`.
        // Examples:
        // - String::toUpperCase
        // - user::getName
        // - System.out::println
        // - String[]::new
        let mut seen_colon_colon = false;
        let mut receiver_parts = Vec::new();
        for child in parent.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(t) => {
                    if !seen_colon_colon {
                        if t.kind() == SyntaxKind::COLON_COLON {
                            seen_colon_colon = true;
                            continue;
                        }
                        if t.kind() != SyntaxKind::WHITESPACE {
                            receiver_parts.push(t.text().to_string());
                        }
                    } else if t.kind() == SyntaxKind::IDENT {
                        method_name = Some(t.text().to_string());
                    } else if t.kind() == SyntaxKind::NEW_KW
                        || (t.kind().is_identifier() && t.text() == "new")
                    {
                        is_constructor = true;
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    if !seen_colon_colon {
                        let text = n.text().to_string();
                        if !text.trim().is_empty() {
                            receiver_parts.push(text);
                        }
                    }
                }
            }
        }

        if !receiver_parts.is_empty() {
            receiver_text = Some(receiver_parts.join("").trim().to_string());
        }

        if method_name.is_none() && token.text() == "new" {
            is_constructor = true;
        }

        debug!(
            "Extracted - receiver: {:?}, method: {:?}, is_constructor: {}",
            receiver_text, method_name, is_constructor
        );

        if receiver_text.is_none() {
            let text_source = parent.text().to_string();
            if let Some((left, _)) = text_source.split_once("::") {
                let trimmed = left.trim();
                if !trimmed.is_empty() {
                    receiver_text = Some(trimmed.to_string());
                }
            }
        }

        let receiver = match receiver_text {
            Some(r) => r,
            None => {
                // debug!("RESOLVE_METHOD_REF: FAIL No receiver found");
                return Err("No receiver found in method reference".to_string());
            }
        };
        // debug!("RESOLVE_METHOD_REF: Using receiver: {}", receiver);
        debug!("Method reference receiver: {}", receiver);

        // Try to resolve using workspace index
        if let Ok(path) = uri.to_file_path() {
            if let Some(workspace_file_id) = self.workspace_index.get_file_id(&path) {
                // debug!("RESOLVE_METHOD_REF: Got workspace_file_id: {:?}", workspace_file_id);

                // Special handling for 'this' keyword
                if receiver == "this" {
                    // For this::method, search for the method in the current file's AST
                    if let Some(ref method) = method_name {
                        // debug!("RESOLVE_METHOD_REF: Looking for method '{}' in current class (this::)", method);
                        debug!("Looking for method '{}' in current class (this::)", method);

                        // Search the current file's AST for the method
                        // Use the AST root passed from goto_definition (avoids deadlock)
                        if let Some(method_location) =
                            find_method_in_ast(ast_root, method, source_text, uri)
                        {
                            // debug!("RESOLVE_METHOD_REF: Found this::{} in current file", method);
                            return Ok(Some(method_location));
                        }
                        // debug!("RESOLVE_METHOD_REF: Could not find this::{} in current file", method);
                    }
                    // If we didn't find the method, continue with normal flow
                }

                // First check if receiver is a variable - if so, get its type
                let type_to_resolve = if let Some(var_type) = find_variable_type(token, &receiver) {
                    debug!("Found variable '{}' with type: {}", receiver, var_type);
                    var_type
                } else if receiver.contains('.') {
                    // Receiver contains a dot - could be field access chain (e.g., config.settings)
                    // Try to resolve the chain to get the final type
                    if let Some(resolved_type) = resolve_field_chain_type(
                        &receiver,
                        token,
                        &self.workspace_index,
                        workspace_file_id,
                    ) {
                        debug!(
                            "Resolved field chain '{}' to type: {}",
                            receiver, resolved_type
                        );
                        resolved_type
                    } else {
                        debug!(
                            "Field chain resolution failed for '{}'; trying it as one qualified type",
                            receiver
                        );
                        receiver.clone()
                    }
                } else {
                    // Simple name - treat as class/type name directly
                    debug!("No variable found, treating '{}' as type name", receiver);
                    receiver.clone()
                };
                // Try to resolve the type as a class name
                match resolve_class_fqn_via_imports(
                    self.workspace_index.as_ref(),
                    workspace_file_id,
                    &type_to_resolve,
                ) {
                    Ok(fqn) => {
                        debug!("Resolved receiver '{}' to FQN: {}", receiver, fqn);

                        // Now find the method/constructor in that class
                        if is_constructor {
                            if let Some(location) = self.workspace_index.find_symbol(&fqn) {
                                return self.symbol_location_to_lsp_location(&location);
                            }
                        } else if let Some(method) = method_name {
                            debug!(
                                "Looking for method '{}' in receiver class '{}'",
                                method, fqn
                            );
                            if let Some(location) =
                                self.find_callable_with_inheritance(&fqn, &method)
                            {
                                return self.symbol_location_to_lsp_location(&location);
                            }
                        }
                    }
                    Err(e) => {
                        // debug!("RESOLVE_METHOD_REF: Failed to resolve receiver: {}", e);
                        debug!("Failed to resolve receiver: {}", e);
                    }
                }
            }
        }

        // debug!("RESOLVE_METHOD_REF: FAIL Could not resolve method reference");
        debug!("Could not resolve method reference");
        Ok(None)
    }

    pub(crate) fn resolve_constructor_reference_at_offset(
        &self,
        ctx: &NavigationContext,
    ) -> Result<Option<Location>, String> {
        if !self.is_constructor_reference_at_offset(ctx) {
            return Ok(None);
        }

        if let Some(receiver) = self.extract_constructor_receiver_from_ast(ctx) {
            return self.resolve_constructor_reference_receiver(ctx, receiver);
        }

        Ok(None)
    }

    fn resolve_constructor_reference_from_tokens(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        debug!(
            "NAV: Constructor ref check token='{}' kind={:?}",
            token.text(),
            token.kind()
        );
        if token.text() != "new" {
            if !self.is_constructor_reference_at_offset(ctx) {
                return Ok(None);
            }
            if let Some(receiver) = self.extract_constructor_receiver_from_ast(ctx) {
                debug!(
                    "NAV: Constructor ref receiver (text fallback): {}",
                    receiver
                );
                return self.resolve_constructor_reference_receiver(ctx, receiver);
            }
            return Ok(None);
        }

        let mut current = token.prev_token();
        let mut steps = 0usize;
        let colon_colon = loop {
            match current {
                Some(tok) if tok.kind() == SyntaxKind::COLON_COLON => break Some(tok),
                Some(tok) => {
                    current = tok.prev_token();
                }
                None => break None,
            }
            steps += 1;
            if steps > 64 {
                break None;
            }
        };

        let colon_colon = match colon_colon {
            Some(tok) => tok,
            None => return Ok(None),
        };

        let mut parts = Vec::new();
        let mut current = colon_colon.prev_token();
        let mut steps = 0usize;
        while let Some(tok) = current {
            match tok.kind() {
                SyntaxKind::IDENT
                | SyntaxKind::DOT
                | SyntaxKind::L_BRACKET
                | SyntaxKind::R_BRACKET => {
                    parts.push(tok.text().to_string());
                }
                _ => break,
            }
            current = tok.prev_token();
            steps += 1;
            if steps > 64 {
                break;
            }
        }

        parts.reverse();
        let mut receiver = parts.join("");
        if receiver.is_empty() {
            if let Some(fallback) = self.extract_constructor_receiver_from_ast(ctx) {
                receiver = fallback;
            } else {
                return Ok(None);
            }
        }
        debug!("NAV: Constructor ref receiver (tokens): {}", receiver);
        receiver = receiver.trim().to_string();
        while receiver.ends_with("[]") {
            receiver.truncate(receiver.len().saturating_sub(2));
        }

        self.resolve_constructor_reference_receiver(ctx, receiver)
    }

    fn extract_constructor_receiver_from_ast(&self, ctx: &NavigationContext) -> Option<String> {
        use hyperion_parser::SyntaxKind;

        let syntax = ctx.parse_tree.syntax();
        let offset = rowan::TextSize::from(ctx.offset as u32);
        let token = match syntax.token_at_offset(offset) {
            rowan::TokenAtOffset::Single(t) => t,
            rowan::TokenAtOffset::Between(l, r) => {
                if l.kind().is_identifier() {
                    l
                } else {
                    r
                }
            }
            rowan::TokenAtOffset::None => return None,
        };

        // Walk up to find the CONSTRUCTOR_REF_EXPR or METHOD_REF_EXPR node
        let ref_node = token.parent_ancestors().find(|node| {
            matches!(
                node.kind(),
                SyntaxKind::CONSTRUCTOR_REF_EXPR | SyntaxKind::METHOD_REF_EXPR
            )
        })?;

        // Collect identifier text before the :: token to get the receiver name
        let mut receiver_parts = Vec::new();
        for child in ref_node.children_with_tokens() {
            match &child {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::COLON_COLON => break,
                rowan::NodeOrToken::Token(t) if t.kind().is_identifier() => {
                    receiver_parts.push(t.text().to_string());
                }
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::DOT => {
                    receiver_parts.push(".".to_string());
                }
                rowan::NodeOrToken::Node(n) => {
                    // For nested expressions like NAME_EXPR, extract the text
                    for t in n.descendants_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = t {
                            if t.kind().is_identifier() {
                                receiver_parts.push(t.text().to_string());
                            } else if t.kind() == SyntaxKind::DOT {
                                receiver_parts.push(".".to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let receiver = receiver_parts.join("");
        if receiver.is_empty() {
            None
        } else {
            Some(receiver)
        }
    }

    fn resolve_constructor_reference_receiver(
        &self,
        ctx: &NavigationContext,
        receiver: String,
    ) -> Result<Option<Location>, String> {
        let workspace_file_id = ctx
            .uri
            .to_file_path()
            .ok()
            .and_then(|path| self.workspace_index.get_file_id(&path))
            .unwrap_or(ctx.file_id);

        if let Ok(fqn) = resolve_class_fqn_via_imports(
            self.workspace_index.as_ref(),
            workspace_file_id,
            &receiver,
        ) {
            if let Some(location) = self.workspace_index.find_symbol(&fqn) {
                return self.symbol_location_to_lsp_location(&location);
            }
        }
        Ok(None)
    }

    fn is_constructor_reference_at_offset(&self, ctx: &NavigationContext) -> bool {
        // Check if cursor is inside a METHOD_REF_EXPR with "::new" pattern
        // Uses the AST instead of text scanning to avoid false positives from
        // unrelated :: tokens elsewhere in the file.
        use hyperion_parser::SyntaxKind;

        let syntax = ctx.parse_tree.syntax();
        let offset = rowan::TextSize::from(ctx.offset as u32);
        let token = match syntax.token_at_offset(offset) {
            rowan::TokenAtOffset::Single(t) => t,
            rowan::TokenAtOffset::Between(l, r) => {
                if l.kind().is_identifier() {
                    l
                } else {
                    r
                }
            }
            rowan::TokenAtOffset::None => return false,
        };

        // Walk up to see if we're inside a CONSTRUCTOR_REF_EXPR (or METHOD_REF_EXPR with ::new)
        let mut current = token.parent();
        while let Some(node) = current {
            match node.kind() {
                // Parser emits CONSTRUCTOR_REF_EXPR specifically for ::new
                SyntaxKind::CONSTRUCTOR_REF_EXPR => return true,
                // Also check METHOD_REF_EXPR in case parser wraps it differently
                SyntaxKind::METHOD_REF_EXPR => {
                    let has_new = node.children_with_tokens().any(|child| {
                        matches!(child, rowan::NodeOrToken::Token(ref t)
                            if t.kind() == SyntaxKind::NEW_KW
                                || (t.kind().is_identifier() && t.text() == "new"))
                    });
                    return has_new;
                }
                // Don't walk past statement boundaries
                SyntaxKind::EXPR_STMT
                | SyntaxKind::LOCAL_VAR_DECL
                | SyntaxKind::BLOCK_STMT
                | SyntaxKind::METHOD_DECL
                | SyntaxKind::CLASS_DECL => return false,
                _ => {}
            }
            current = node.parent();
        }
        false
    }
}
