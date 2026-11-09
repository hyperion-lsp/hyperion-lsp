// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Inlay Hints Provider
//!
//! Provides inline type and parameter hints for improved code readability.
//!
//! Supported hints:
//! - Type hints for `var` declarations (Java)
//! - Type hints for `val` declarations (Kotlin)
//! - Parameter name hints for method calls

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{FileId, ParseDatabase};
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_workspace::{DeclarationNamespace, QueryEngine, WorkspaceIndex};
use std::sync::Arc;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Range, Url};
use tracing::debug;

use crate::navigation::argument_inference::{
    infer_argument_types, infer_expression_type_for_navigation, Confidence, InferredType,
};
use crate::navigation::overload_resolver::{
    receiver_owner_for_lookup, resolve_overload_with_receiver, OverloadMatch,
};
use crate::navigation::NavigationProvider;
use crate::position_utils::{offset_to_position, position_to_offset};
use crate::semantic_host::ReadTxn;

/// Provides inlay hint functionality
pub struct InlayHintsProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
    navigation_provider: NavigationProvider<'txn>,
}

impl<'txn> InlayHintsProvider<'txn> {
    /// Creates a new InlayHintsProvider
    pub fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        let navigation_provider = NavigationProvider::new(
            txn,
            Arc::clone(&workspace_index),
            Arc::new(QueryEngine::new(workspace_index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        Self {
            txn,
            workspace_index,
            navigation_provider,
        }
    }

    /// Get inlay hints for a given range in a document
    pub fn inlay_hints(
        &self,
        file_id: FileId,
        range: Range,
        source_text: &str,
    ) -> Result<Vec<InlayHint>, String> {
        debug!("Inlay hints requested for range {:?}", range);

        // Parse the file using Salsa snapshot to avoid locking.
        // try_lock_for: degrade to no hints rather than freeze if did_change
        // is currently holding the DB mutex.
        let root = {
            let snapshot = self.txn.snapshot();

            let parse_tree = snapshot.parse_file(file_id);
            parse_tree.syntax()
        };

        let mut hints = Vec::new();

        // Convert range to offsets
        let start_offset: usize = position_to_offset(source_text, range.start)
            .map_err(|e| format!("Invalid start position: {e}"))?;
        let end_offset: usize = position_to_offset(source_text, range.end)
            .map_err(|e| format!("Invalid end position: {e}"))?;

        // Build resolver ONCE for the entire file and reuse it for all hints
        // This is a critical optimization: building a resolver walks the entire AST
        // to create symbol tables and scope maps. Building it 100+ times per file
        // (once per method call for parameter hints) is extremely expensive.
        //
        // Build the file scope once. Var inference must use that populated table;
        // a fresh ResolverBuilder cannot resolve receiver variables in method-call
        // initializers such as `var user = service.getUser()`.
        let mut resolver = ResolverBuilder::with_workspace(file_id, self.workspace_index.clone())
            .build_from_syntax(&root);
        let resolver_builder = ResolverBuilder::for_var_inference(
            file_id,
            resolver.symbol_table().clone(),
            self.workspace_index.clone(),
        );
        let source_uri = self
            .workspace_index
            .get_file_path(file_id)
            .and_then(|path| Url::from_file_path(path).ok());

        // Walk the AST and collect hints
        for node in root.descendants() {
            let node_start: usize = Into::<u32>::into(node.text_range().start()) as usize;
            let node_end: usize = Into::<u32>::into(node.text_range().end()) as usize;

            // Skip nodes outside the requested range
            if node_end < start_offset || node_start > end_offset {
                continue;
            }

            match node.kind() {
                SyntaxKind::LOCAL_VAR_DECL => {
                    // Check for var declarations - uses resolver_builder for type inference
                    if let Some(hint) = self.var_type_hint(&node, &resolver_builder, source_text) {
                        hints.push(hint);
                    }
                }
                // NOTE: PROPERTY_DECL is a Kotlin-specific syntax kind not in hyperion_parser
                // Kotlin properties are handled separately via the Kotlin language path
                // SyntaxKind::PROPERTY_DECL => {
                //     // Check for Kotlin val declarations
                //     if let Some(hint) = self.kotlin_val_type_hint(&node, source_text) {
                //         hints.push(hint);
                //     }
                // }
                SyntaxKind::METHOD_CALL_EXPR => {
                    // Add parameter name hints - pass pre-built resolver for reuse
                    // This avoids rebuilding the resolver for every method call
                    if let Some(method_hints) = self.parameter_name_hints(
                        &node,
                        &root,
                        file_id,
                        &mut resolver,
                        source_text,
                        source_uri.as_ref(),
                    ) {
                        hints.extend(method_hints);
                    }
                }
                _ => {}
            }
        }

        debug!("Generated {} inlay hints", hints.len());
        Ok(hints)
    }

    /// Generate type hint for var declaration
    fn var_type_hint(
        &self,
        node: &SyntaxNode,
        resolver_builder: &hyperion_semantics::resolver::ResolverBuilder,
        source_text: &str,
    ) -> Option<InlayHint> {
        // Check if this is a var declaration
        let has_var = node.descendants_with_tokens().any(|child| {
            child
                .as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::VAR_KW)
        });

        if !has_var {
            return None;
        }

        // Infer the type using resolver builder
        let inferred_type = resolver_builder.infer_var_type(node);

        // Skip if type is unknown
        if matches!(inferred_type, hyperion_semantics::symbol::Type::Unknown) {
            return None;
        }

        // Convert type to string
        let type_string = type_to_string(&inferred_type);

        // Find position after the variable name
        // Bug fix: Find the IDENT that comes AFTER VAR_KW, not the first IDENT
        // In `var users = new ArrayList<User>();`, we want "users" not "ArrayList"
        let mut found_var = false;
        let var_name_offset = node.descendants_with_tokens().find_map(|child| {
            if let Some(t) = child.as_token() {
                if t.kind() == SyntaxKind::VAR_KW {
                    found_var = true;
                    return None; // Keep searching
                }
                if found_var && t.kind() == SyntaxKind::IDENT {
                    return Some(t.text_range().end());
                }
            }
            None
        })?;

        let position = offset_to_position(source_text, var_name_offset.into());

        Some(InlayHint {
            position,
            label: InlayHintLabel::String(format!(": {type_string}")),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: Some(true),
            data: None,
        })
    }

    /// Generate parameter name hints for method call
    fn parameter_name_hints(
        &self,
        node: &SyntaxNode,
        syntax_root: &SyntaxNode,
        file_id: FileId,
        resolver: &mut hyperion_semantics::resolver::SymbolResolver,
        source_text: &str,
        source_uri: Option<&Url>,
    ) -> Option<Vec<InlayHint>> {
        // METHOD_CALL_EXPR contains: [receiver, DOT, IDENT("method"), ARG_LIST]
        // or [NAME_EXPR("method"), ARG_LIST]. Extract the method name IDENT.
        let method_name = extract_method_name_from_call(node)?;

        debug!("Looking for parameter hints for method: {}", method_name);

        // Find the argument list
        let arg_list = node
            .children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)?;

        // Extract arguments using the same logic as call_site_analysis.rs
        // Count commas to determine argument boundaries, and collect the first expression node for each argument
        //
        // Bug fix: Track nesting depth to avoid treating commas inside lambdas, array initializers,
        // or nested method calls as argument separators.
        // Only count COMMAs at depth 0 (top level of arg list)
        let mut arguments: Vec<SyntaxNode> = Vec::new();
        let mut current_arg_first_expr: Option<SyntaxNode> = None;
        // Start at -1 because ARG_LIST includes its own L_PAREN/R_PAREN as
        // direct children. The opening L_PAREN increments depth to 0, so
        // top-level commas (between the arg list's own parens) are at depth 0.
        let mut nesting_depth: i32 = -1;

        for child in arg_list.children_with_tokens() {
            match &child {
                rowan::NodeOrToken::Token(token) => {
                    match token.kind() {
                        SyntaxKind::L_PAREN | SyntaxKind::L_BRACE | SyntaxKind::L_BRACKET => {
                            nesting_depth += 1;
                        }
                        SyntaxKind::R_PAREN | SyntaxKind::R_BRACE | SyntaxKind::R_BRACKET => {
                            nesting_depth -= 1;
                        }
                        SyntaxKind::COMMA if nesting_depth == 0 => {
                            // Only count comma as argument separator at top level
                            if let Some(expr) = current_arg_first_expr.take() {
                                arguments.push(expr);
                            }
                            // Reset for next argument
                            current_arg_first_expr = None;
                        }
                        _ => {
                            // Skip other tokens (whitespace, etc.)
                        }
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    // Check if this is an expression node
                    if is_expression_kind(node.kind()) {
                        // Record the first expression node for this argument
                        if current_arg_first_expr.is_none() {
                            current_arg_first_expr = Some(node.clone());
                        }
                    }
                }
            }
        }

        // Don't forget the last argument (no trailing comma)
        if let Some(expr) = current_arg_first_expr.take() {
            arguments.push(expr);
        }

        if arguments.is_empty() {
            return None;
        }

        debug!(
            "Found {} arguments for method {}",
            arguments.len(),
            method_name
        );

        // Resolve the method to get actual parameter names from the workspace index
        let parameter_names = self.lookup_method_parameters(
            node,
            syntax_root,
            file_id,
            &method_name,
            arguments.len(),
            &arguments,
            resolver,
            source_text,
            source_uri,
        )?;

        debug!(
            "Found {} parameter names for {}: {:?}",
            parameter_names.len(),
            method_name,
            parameter_names
        );

        // Generate hints for each argument
        let mut hints = Vec::new();
        for (i, arg) in arguments.iter().enumerate() {
            if let Some(param_name) = parameter_names.get(i) {
                // Position hint at the start of the argument expression
                let arg_start = arg.text_range().start();
                let position = offset_to_position(source_text, arg_start.into());

                debug!(
                    "Adding hint '{}:' at position {:?} for arg #{} (offset {:?})",
                    param_name, position, i, arg_start
                );

                hints.push(InlayHint {
                    position,
                    label: InlayHintLabel::String(format!("{param_name}:")),
                    kind: Some(InlayHintKind::PARAMETER),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right: Some(true),
                    data: None,
                });
            }
        }

        if hints.is_empty() {
            None
        } else {
            Some(hints)
        }
    }

    /// Look up parameter names for a method using workspace index
    fn lookup_method_parameters(
        &self,
        method_call_node: &SyntaxNode,
        syntax_root: &SyntaxNode,
        file_id: FileId,
        method_name: &str,
        arg_count: usize,
        arguments: &[SyntaxNode],
        resolver: &mut hyperion_semantics::resolver::SymbolResolver,
        source_text: &str,
        source_uri: Option<&Url>,
    ) -> Option<Vec<String>> {
        let arg_types: Vec<InferredType> = arguments
            .iter()
            .map(|arg| {
                let offset: u32 = arg.text_range().start().into();
                infer_expression_type_for_navigation(
                    arg,
                    file_id,
                    &self.workspace_index,
                    syntax_root,
                    offset,
                )
            })
            .collect();

        // Try to get receiver type and resolve actual method parameters
        let receiver_type =
            self.infer_method_receiver(method_call_node, syntax_root, file_id, resolver);
        debug!(
            "Receiver type inference for '{}': {:?}",
            method_name, receiver_type
        );

        if let Some(ref receiver_type) = receiver_type {
            let class_fqn = self.type_to_fqn(receiver_type)?;
            debug!(
                "Looking up method in workspace: {}.{}({} args)",
                class_fqn, method_name, arg_count
            );

            // Look up method in workspace index
            if let Some(param_names) = self.get_method_param_names(
                &class_fqn,
                Some(receiver_type),
                method_name,
                arg_count,
                &arg_types,
            ) {
                debug!("✓ Found workspace method parameters: {:?}", param_names);
                return Some(param_names);
            }
            if self.method_projection_unavailable(&class_fqn, receiver_type, method_name) {
                debug!(
                    "✗ Receiver-relative projection unavailable for {}.{}",
                    class_fqn, method_name
                );
                return None;
            }
            debug!("✗ Method not found in workspace index");
        } else {
            debug!(
                "✗ Failed to infer receiver type for method '{}'",
                method_name
            );
        }

        // Complex receivers and unqualified calls use the same exact callable
        // identity as navigation instead of searching every same-named method
        // in the workspace. Keep this as the fallback so the common
        // receiver-relative path above remains allocation-light.
        if let Some(uri) = source_uri {
            let name_range = method_name_range_from_call(method_call_node)?;
            let position = offset_to_position(source_text, name_range.start().into());
            if let Ok(Some(target)) = self
                .navigation_provider
                .resolve_workspace_target_at_position(
                    file_id,
                    position,
                    source_text,
                    uri,
                    Some(DeclarationNamespace::Callable),
                )
            {
                let names =
                    Self::parameter_names_for_call(&target.definition.symbol.parameters, arg_count);
                if self
                    .navigation_provider
                    .workspace_target_is_current(&target)
                {
                    if names.is_some() {
                        return names;
                    }
                } else {
                    return None;
                }
            }
        }

        // Bytecode may omit parameter names. Only apply curated names after a
        // receiver was actually identified as a well-known external API; a
        // bare method spelling is not semantic evidence.
        let receiver_fqn = receiver_type.as_ref().and_then(|ty| self.type_to_fqn(ty))?;
        if self.is_known_fallback_owner(&receiver_fqn) {
            debug!("Using curated parameter names for {receiver_fqn}.{method_name}");
            self.fallback_parameter_names(method_name, arg_count)
        } else {
            None
        }
    }

    /// Infer the receiver type of a method call
    ///
    /// This method reuses the pre-built resolver passed from inlay_hints(),
    /// avoiding the expensive AST walk that would occur if we created a new resolver here.
    ///
    /// Bug fix: Now handles METHOD_CALL_EXPR as receivers (method chains like builder pattern)
    fn infer_method_receiver(
        &self,
        method_call_node: &SyntaxNode,
        syntax_root: &SyntaxNode,
        file_id: FileId,
        resolver: &mut hyperion_semantics::resolver::SymbolResolver,
    ) -> Option<hyperion_semantics::symbol::Type> {
        // Java postfix expressions are nested by the parser: in
        // `factory.choose(1).accept("x")`, the outer call owns the complete inner
        // `factory.choose(1)` call as its first expression child. Looking among the
        // parent's siblings misses that receiver and makes the global name fallback
        // accidentally responsible for ordinary calls.
        let receiver = method_call_node.children().find(|node| {
            matches!(
                node.kind(),
                SyntaxKind::NAME_EXPR
                    | SyntaxKind::QUALIFIED_NAME
                    | SyntaxKind::METHOD_CALL_EXPR
                    | SyntaxKind::FIELD_ACCESS_EXPR
            )
        })?;
        debug!("Found receiver node: {:?}", receiver.kind());

        // Find scope at method call position using the pre-built resolver
        let offset = method_call_node.text_range().start().into();
        let scope = resolver.find_scope_at_offset(offset)?;

        match receiver.kind() {
            SyntaxKind::NAME_EXPR | SyntaxKind::QUALIFIED_NAME => {
                // Simple variable or qualified name - resolve directly
                let receiver_text = receiver.text().to_string();
                debug!("Resolving simple receiver: {}", receiver_text);
                if let Some(resolved) = resolver.resolve_in_scope(&receiver_text, scope) {
                    return Some(resolved.ty);
                }
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                // Method call as receiver - need to resolve its return type
                // For `user.getName().toUpperCase()`, we need to get String (return of getName)
                debug!("Resolving method call receiver");

                // Recursively get the receiver type of this method call
                if let Some(inner_receiver_type) =
                    self.infer_method_receiver(&receiver, syntax_root, file_id, resolver)
                {
                    let inner_receiver_fqn = self.type_to_fqn(&inner_receiver_type)?;
                    // Extract method name directly from the METHOD_CALL_EXPR
                    let inner_method_name = extract_method_name_from_call(&receiver);

                    if let Some(method_name) = inner_method_name {
                        debug!(
                            "Looking up return type of {}.{}",
                            inner_receiver_fqn, method_name
                        );
                        let arg_list = receiver
                            .children()
                            .find(|child| child.kind() == SyntaxKind::ARG_LIST)?;
                        let call_offset = receiver.text_range().start().into();
                        let arg_types = infer_argument_types(
                            &arg_list,
                            file_id,
                            &self.workspace_index,
                            syntax_root,
                            call_offset,
                        );

                        // Resolve the inner call with the same canonical Java conversion
                        // rules used by navigation, hover, and the type checker. A chained
                        // receiver's type must come from the selected declaration, not the
                        // first declaration sharing its name.
                        let resolved = resolve_overload_with_receiver(
                            &self.workspace_index,
                            &inner_receiver_fqn,
                            Some(&inner_receiver_type),
                            &method_name,
                            &arg_types,
                        );
                        if let Some(matched) = resolved {
                            debug!("Found projected return type: {}", matched.return_type);
                            return Some(matched.return_type);
                        }
                        let resolved = {
                            // In incomplete code every argument can be unknown. Preserve
                            // a deliberately degraded arity-only fallback only for that
                            // case; a known incompatible call must not acquire a receiver.
                            if arg_types
                                .iter()
                                .all(|ty| ty.confidence == Confidence::Unknown)
                            {
                                self.workspace_index.find_method_by_args_with_inheritance(
                                    &inner_receiver_fqn,
                                    &method_name,
                                    arg_types.len(),
                                )
                            } else {
                                None
                            }
                        };

                        if let Some(return_type) =
                            resolved.and_then(|method| method.return_type.clone())
                        {
                            debug!("Found return type: {}", return_type);
                            return hyperion_semantics::type_parser::parse_type_string(
                                &return_type,
                            );
                        }
                    }
                }
            }
            SyntaxKind::FIELD_ACCESS_EXPR => {
                // Field access as receiver - resolve the field type
                debug!("Resolving field access receiver");
                // For now, just try to extract and resolve the text
                let receiver_text = receiver.text().to_string();
                if let Some(resolved) = resolver.resolve_in_scope(&receiver_text, scope) {
                    return Some(resolved.ty);
                }
            }
            _ => {}
        }

        None
    }

    /// Convert Type to fully qualified name
    fn type_to_fqn(&self, typ: &hyperion_semantics::symbol::Type) -> Option<String> {
        use hyperion_semantics::symbol::Type;
        match typ {
            Type::Class(class_type) => {
                if let Some(ref pkg) = class_type.package {
                    Some(format!("{}.{}", pkg, class_type.name))
                } else {
                    Some(class_type.name.to_string())
                }
            }
            _ => None,
        }
    }

    /// Get method parameter names from workspace index
    fn get_method_param_names(
        &self,
        class_fqn: &str,
        receiver_type: Option<&hyperion_semantics::symbol::Type>,
        method_name: &str,
        arg_count: usize,
        arg_types: &[InferredType],
    ) -> Option<Vec<String>> {
        let has_type_signal = arg_types
            .iter()
            .any(|ty| ty.confidence != Confidence::Unknown);

        if has_type_signal {
            let matched = resolve_overload_with_receiver(
                &self.workspace_index,
                class_fqn,
                receiver_type,
                method_name,
                arg_types,
            )?;
            if let Some(param_names) = Self::parameter_names_from_match(&matched, arg_count) {
                debug!(
                    "Resolved {}.{} via type-based matching (score: {:?})",
                    class_fqn, method_name, matched.score
                );
                return Some(param_names);
            }
            return None;
        }

        // Fallback: use arg-count-based lookup when type inference has no useful signal.
        let method = self.workspace_index.find_method_by_args_with_inheritance(
            class_fqn,
            method_name,
            arg_count,
        )?;
        // `parameter_names_from_symbol` filters out malformed entries with `filter_map`,
        // so the returned vec can be shorter than `method.parameters.len()`. If the
        // length doesn't match `arg_count`, returning the names would misalign hints
        // (param 0 → arg 0, but param 2 → arg 1, etc.). Mirror the type-matched
        // path's guard above and emit no hints rather than wrong ones.
        let param_names = Self::parameter_names_from_symbol(&method.parameters);
        if param_names.len() == arg_count {
            Some(param_names)
        } else {
            None
        }
    }

    fn parameter_names_from_symbol<P: AsRef<str>>(params: &[P]) -> Vec<String> {
        params
            .iter()
            .filter_map(|param| {
                let parts: Vec<&str> = param.as_ref().split_whitespace().collect();
                parts.last().map(|s| (*s).to_string())
            })
            .collect()
    }

    fn parameter_names_from_match(
        matched: &OverloadMatch,
        argument_count: usize,
    ) -> Option<Vec<String>> {
        let names = Self::parameter_names_from_symbol(&matched.parameter_declarations);
        let is_variadic = matched
            .parameter_declarations
            .last()
            .is_some_and(|parameter| parameter.contains("...") || parameter.starts_with("vararg "));
        if !is_variadic {
            return (names.len() == argument_count).then_some(names);
        }
        let fixed_count = names.len().saturating_sub(1);
        if argument_count < fixed_count {
            return None;
        }
        let variadic_name = names.last()?.clone();
        Some(
            (0..argument_count)
                .map(|index| {
                    names
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| variadic_name.clone())
                })
                .collect(),
        )
    }

    fn parameter_names_for_call<P: AsRef<str>>(
        parameters: &[P],
        argument_count: usize,
    ) -> Option<Vec<String>> {
        let names = Self::parameter_names_from_symbol(parameters);
        let is_variadic = parameters.last().is_some_and(|parameter| {
            let parameter = parameter.as_ref();
            parameter.contains("...") || parameter.starts_with("vararg ")
        });
        if !is_variadic {
            return (names.len() == argument_count).then_some(names);
        }
        let fixed_count = names.len().saturating_sub(1);
        if argument_count < fixed_count {
            return None;
        }
        let variadic_name = names.last()?.clone();
        Some(
            (0..argument_count)
                .map(|index| {
                    names
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| variadic_name.clone())
                })
                .collect(),
        )
    }

    fn is_known_fallback_owner(&self, fqn: &str) -> bool {
        fqn.starts_with("java.")
            || fqn.starts_with("javax.")
            || fqn.starts_with("org.junit.")
            || fqn.starts_with("org.mockito.")
    }

    fn method_projection_unavailable(
        &self,
        class_fqn: &str,
        receiver_type: &hyperion_semantics::symbol::Type,
        method_name: &str,
    ) -> bool {
        let Some(owner) =
            receiver_owner_for_lookup(&self.workspace_index, class_fqn, receiver_type)
        else {
            return false;
        };
        let arguments = match receiver_type {
            hyperion_semantics::symbol::Type::Class(class) => class
                .type_arguments
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let candidates = self
            .workspace_index
            .find_projected_effective_methods_by_name(&owner, &arguments, method_name);
        !candidates.is_empty()
            && candidates
                .iter()
                .any(|candidate| candidate.projection.is_none())
    }

    /// Fallback parameter names for common methods
    ///
    /// JDK classes are typically compiled without the `-parameters` flag, so their
    /// parameter names aren't available in bytecode. This provides human-readable
    /// fallbacks for the most commonly used methods.
    fn fallback_parameter_names(&self, method_name: &str, arg_count: usize) -> Option<Vec<String>> {
        let params: Vec<String> = match (method_name, arg_count) {
            // ========== Testing frameworks ==========
            ("assertEquals" | "assertEqual", 2) => vec!["expected", "actual"],
            ("assertEquals" | "assertEqual", 3) => vec!["message", "expected", "actual"],
            ("assertNotEquals", 2) => vec!["unexpected", "actual"],
            ("assertNotEquals", 3) => vec!["message", "unexpected", "actual"],
            ("assertTrue" | "assertFalse", 1) => vec!["condition"],
            ("assertTrue" | "assertFalse", 2) => vec!["message", "condition"],
            ("assertNull" | "assertNotNull", 1) => vec!["object"],
            ("assertNull" | "assertNotNull", 2) => vec!["message", "object"],
            ("assertSame" | "assertNotSame", 2) => vec!["expected", "actual"],
            ("assertThrows", 2) => vec!["expectedType", "executable"],
            ("fail", 1) => vec!["message"],
            ("when", 1) => vec!["methodCall"],
            ("thenReturn", 1) => vec!["value"],
            ("verify", 1) => vec!["mock"],
            ("verify", 2) => vec!["mock", "mode"],
            ("given", 1) => vec!["methodCall"],
            ("willReturn", 1) => vec!["value"],
            ("assertThat", 2) => vec!["actual", "matcher"],

            // ========== String methods ==========
            ("substring", 1) => vec!["beginIndex"],
            ("substring", 2) => vec!["beginIndex", "endIndex"],
            ("indexOf", 1) => vec!["str"],
            ("indexOf", 2) => vec!["str", "fromIndex"],
            ("lastIndexOf", 1) => vec!["str"],
            ("lastIndexOf", 2) => vec!["str", "fromIndex"],
            ("replace", 2) => vec!["target", "replacement"],
            ("replaceAll" | "replaceFirst", 2) => vec!["regex", "replacement"],
            ("split", 1) => vec!["regex"],
            ("split", 2) => vec!["regex", "limit"],
            ("startsWith", 1) => vec!["prefix"],
            ("startsWith", 2) => vec!["prefix", "offset"],
            ("endsWith", 1) => vec!["suffix"],
            ("equals" | "equalsIgnoreCase", 1) => vec!["other"],
            ("compareTo" | "compareToIgnoreCase", 1) => vec!["other"],
            ("contains", 1) => vec!["s"],
            ("concat", 1) => vec!["str"],
            ("matches", 1) => vec!["regex"],
            ("regionMatches", 4) => vec!["toffset", "other", "ooffset", "len"],
            ("regionMatches", 5) => vec!["ignoreCase", "toffset", "other", "ooffset", "len"],
            ("charAt", 1) => vec!["index"],
            ("codePointAt", 1) => vec!["index"],
            ("getBytes", 1) => vec!["charset"],
            ("format", _) => vec!["format", "args"],
            ("join", _) => vec!["delimiter", "elements"],
            ("repeat", 1) => vec!["count"],
            ("strip" | "stripLeading" | "stripTrailing" | "trim", 0) => vec![],
            ("toUpperCase" | "toLowerCase", 1) => vec!["locale"],
            ("valueOf", 1) => vec!["value"],
            ("copyValueOf", 1) => vec!["data"],
            ("copyValueOf", 3) => vec!["data", "offset", "count"],

            // ========== Collection methods ==========
            ("put", 2) => vec!["key", "value"],
            ("putIfAbsent", 2) => vec!["key", "value"],
            ("putAll", 1) => vec!["map"],
            ("get", 1) => vec!["key"],
            ("getOrDefault", 2) => vec!["key", "defaultValue"],
            ("remove", 1) => vec!["key"],
            ("remove", 2) => vec!["key", "value"],
            ("containsKey", 1) => vec!["key"],
            ("containsValue", 1) => vec!["value"],
            ("compute", 2) => vec!["key", "remappingFunction"],
            ("computeIfAbsent", 2) => vec!["key", "mappingFunction"],
            ("computeIfPresent", 2) => vec!["key", "remappingFunction"],
            ("merge", 3) => vec!["key", "value", "remappingFunction"],
            ("add", 1) => vec!["element"],
            ("add", 2) => vec!["index", "element"],
            ("addAll", 1) => vec!["collection"],
            ("addAll", 2) => vec!["index", "collection"],
            ("set", 2) => vec!["index", "element"],
            ("subList", 2) => vec!["fromIndex", "toIndex"],
            ("toArray", 1) => vec!["array"],
            ("sort", 1) => vec!["comparator"],
            ("binarySearch", 2) => vec!["list", "key"],
            ("binarySearch", 3) => vec!["list", "key", "comparator"],
            ("copy", 2) => vec!["dest", "src"],
            ("fill", 2) => vec!["list", "obj"],
            ("nCopies", 2) => vec!["n", "obj"],
            ("reverse" | "shuffle", 1) => vec!["list"],
            ("singleton" | "singletonList" | "singletonMap", 1) => vec!["obj"],
            ("emptyList" | "emptySet" | "emptyMap", 0) => vec![],

            // ========== Stream methods ==========
            ("map" | "flatMap", 1) => vec!["mapper"],
            ("filter", 1) => vec!["predicate"],
            ("forEach" | "forEachOrdered", 1) => vec!["action"],
            ("reduce", 1) => vec!["accumulator"],
            ("reduce", 2) => vec!["identity", "accumulator"],
            ("reduce", 3) => vec!["identity", "accumulator", "combiner"],
            ("collect", 1) => vec!["collector"],
            ("collect", 3) => vec!["supplier", "accumulator", "combiner"],
            ("anyMatch" | "allMatch" | "noneMatch", 1) => vec!["predicate"],
            ("findFirst" | "findAny", 0) => vec![],
            ("limit" | "skip", 1) => vec!["n"],
            ("sorted", 1) => vec!["comparator"],
            ("distinct" | "sequential" | "parallel", 0) => vec![],
            ("peek", 1) => vec!["action"],
            ("takeWhile" | "dropWhile", 1) => vec!["predicate"],
            ("flatMapToInt" | "flatMapToLong" | "flatMapToDouble", 1) => vec!["mapper"],
            ("mapToInt" | "mapToLong" | "mapToDouble", 1) => vec!["mapper"],

            // ========== Optional methods ==========
            ("of" | "ofNullable", 1) => vec!["value"],
            ("orElse", 1) => vec!["other"],
            ("orElseGet", 1) => vec!["supplier"],
            ("orElseThrow", 1) => vec!["exceptionSupplier"],
            ("ifPresent", 1) => vec!["action"],
            ("ifPresentOrElse", 2) => vec!["action", "emptyAction"],
            ("or", 1) => vec!["supplier"],

            // ========== I/O methods ==========
            ("read", 1) => vec!["buffer"],
            ("read", 3) => vec!["buffer", "offset", "length"],
            ("write", 1) => vec!["b"],
            ("write", 3) => vec!["buffer", "offset", "length"],
            ("readLine", 0) => vec![],
            ("println" | "print", 1) => vec!["message"],
            ("printf", _) => vec!["format", "args"],
            ("readAllBytes" | "readAllLines", 1) => vec!["path"],
            ("writeString", 2) => vec!["path", "content"],
            ("writeString", 3) => vec!["path", "content", "options"],
            ("newBufferedReader" | "newBufferedWriter", 1) => vec!["path"],
            ("newInputStream" | "newOutputStream", 1) => vec!["path"],
            ("move", 2) => vec!["source", "target"],
            ("delete" | "deleteIfExists", 1) => vec!["path"],
            ("exists" | "notExists", 1) => vec!["path"],
            ("createFile" | "createDirectory" | "createDirectories", 1) => vec!["path"],

            // ========== Math methods ==========
            (
                "abs" | "sqrt" | "cbrt" | "ceil" | "floor" | "round" | "sin" | "cos" | "tan"
                | "log" | "log10" | "exp",
                1,
            ) => vec!["a"],
            ("max" | "min" | "pow" | "atan2" | "hypot", 2) => vec!["a", "b"],
            ("random", 0) => vec![],
            ("toRadians" | "toDegrees", 1) => vec!["angdeg"],
            ("addExact" | "subtractExact" | "multiplyExact", 2) => vec!["x", "y"],

            // ========== Object methods ==========
            ("hashCode", 0) => vec![],
            ("toString", 0) => vec![],
            ("getClass", 0) => vec![],
            ("clone", 0) => vec![],
            ("wait", 1) => vec!["timeout"],
            ("wait", 2) => vec!["timeout", "nanos"],
            ("notify" | "notifyAll", 0) => vec![],

            // ========== Reflection/Class methods ==========
            ("forName", 1) => vec!["className"],
            ("newInstance", 0) => vec![],
            ("isInstance", 1) => vec!["obj"],
            ("isAssignableFrom", 1) => vec!["cls"],
            ("cast", 1) => vec!["obj"],
            ("getMethod", _) => vec!["name", "parameterTypes"],
            ("getDeclaredMethod", _) => vec!["name", "parameterTypes"],
            ("getField" | "getDeclaredField", 1) => vec!["name"],
            ("getConstructor" | "getDeclaredConstructor", _) => vec!["parameterTypes"],
            ("invoke", _) => vec!["obj", "args"],

            // ========== Concurrent methods ==========
            ("submit", 1) => vec!["task"],
            ("execute", 1) => vec!["command"],
            ("schedule", 3) => vec!["command", "delay", "unit"],
            ("scheduleAtFixedRate" | "scheduleWithFixedDelay", 4) => {
                vec!["command", "initialDelay", "period", "unit"]
            }
            ("await", 2) => vec!["timeout", "unit"],
            ("tryLock", 2) => vec!["time", "unit"],
            ("offer", 1) => vec!["element"],
            ("offer", 3) => vec!["element", "timeout", "unit"],
            ("poll", 2) => vec!["timeout", "unit"],
            ("take" | "peek", 0) => vec![],

            // ========== Date/Time methods (java.time) ==========
            ("of", 2) => vec!["year", "month"],
            ("of", 3) => vec!["year", "month", "dayOfMonth"],
            ("of", 5) => vec!["year", "month", "dayOfMonth", "hour", "minute"],
            ("of", 6) => vec!["year", "month", "dayOfMonth", "hour", "minute", "second"],
            ("parse", 1) => vec!["text"],
            ("parse", 2) => vec!["text", "formatter"],
            ("plusDays" | "minusDays", 1) => vec!["days"],
            ("plusMonths" | "minusMonths", 1) => vec!["months"],
            ("plusYears" | "minusYears", 1) => vec!["years"],
            ("plusHours" | "minusHours", 1) => vec!["hours"],
            ("plusMinutes" | "minusMinutes", 1) => vec!["minutes"],
            ("plusSeconds" | "minusSeconds", 1) => vec!["seconds"],
            ("withDayOfMonth", 1) => vec!["dayOfMonth"],
            ("withMonth", 1) => vec!["month"],
            ("withYear", 1) => vec!["year"],
            ("isBefore" | "isAfter" | "isEqual", 1) => vec!["other"],
            ("between", 2) => vec!["startInclusive", "endExclusive"],
            ("until", 2) => vec!["endExclusive", "unit"],

            // ========== Logging methods ==========
            ("debug" | "info" | "warn" | "error" | "trace", 1) => vec!["message"],
            ("debug" | "info" | "warn" | "error" | "trace", 2) => vec!["format", "arg"],
            ("debug" | "info" | "warn" | "error" | "trace", 3) => vec!["format", "arg1", "arg2"],

            // ========== Builder pattern ==========
            ("builder", 0) => vec![],
            ("build", 0) => vec![],
            ("with", 1) => vec!["value"],

            // ========== Misc common methods ==========
            ("sleep", 1) => vec!["millis"],
            ("sleep", 2) => vec!["millis", "nanos"],
            ("currentTimeMillis" | "nanoTime", 0) => vec![],
            ("getProperty", 1) => vec!["key"],
            ("getProperty", 2) => vec!["key", "defaultValue"],
            ("setProperty", 2) => vec!["key", "value"],
            ("getenv", 1) => vec!["name"],
            ("exit", 1) => vec!["status"],
            ("arraycopy", 5) => vec!["src", "srcPos", "dest", "destPos", "length"],
            ("asList", _) => vec!["elements"],
            ("requireNonNull", 1) => vec!["obj"],
            ("requireNonNull", 2) => vec!["obj", "message"],
            ("checkIndex", 2) => vec!["index", "length"],
            ("checkFromToIndex", 3) => vec!["fromIndex", "toIndex", "length"],

            // No match — don't generate generic arg0/arg1 hints.
            // Showing meaningless names is worse than showing nothing.
            (_, _) => return None,
        }
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        // Pad or truncate to match argument count
        let mut result = params;
        if result.len() < arg_count {
            for i in result.len()..arg_count {
                result.push(format!("arg{i}"));
            }
        } else if result.len() > arg_count {
            result.truncate(arg_count);
        }

        Some(result)
    }
}

/// Check if a SyntaxKind represents an expression node.
/// This is used to identify argument boundaries when parsing method calls.
const fn is_expression_kind(kind: SyntaxKind) -> bool {
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
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
    )
}

/// Convert a Type to a display string
fn type_to_string(typ: &hyperion_semantics::symbol::Type) -> String {
    use hyperion_semantics::symbol::{PrimitiveType, Type};

    match typ {
        Type::Primitive(p) => match p {
            PrimitiveType::Boolean => "boolean",
            PrimitiveType::Byte => "byte",
            PrimitiveType::Char => "char",
            PrimitiveType::Short => "short",
            PrimitiveType::Int => "int",
            PrimitiveType::Long => "long",
            PrimitiveType::Float => "float",
            PrimitiveType::Double => "double",
        }
        .to_string(),
        Type::Class(c) => {
            if c.type_arguments.is_empty() {
                c.name.to_string()
            } else {
                format!(
                    "{}<{}>",
                    c.name,
                    c.type_arguments
                        .iter()
                        .map(type_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Type::Array {
            element_type,
            dimensions,
        } => {
            let mut result = type_to_string(element_type);
            for _ in 0..*dimensions {
                result.push_str("[]");
            }
            result
        }
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::TypeVariable { name, .. } => name.clone(),
        Type::Wildcard { .. } => "?".to_string(),
        Type::Unknown => "?".to_string(),
        Type::Intersection { types } => {
            // Display as Type1 & Type2 & ...
            types
                .iter()
                .map(type_to_string)
                .collect::<Vec<_>>()
                .join(" & ")
        }
        Type::Union { types } => {
            // Display as Type1 | Type2 | ...
            types
                .iter()
                .map(type_to_string)
                .collect::<Vec<_>>()
                .join(" | ")
        }
    }
}

/// Extract method name from a METHOD_CALL_EXPR node.
///
/// With nested expression trees, METHOD_CALL_EXPR contains:
///   [receiver, DOT, IDENT("method"), ARG_LIST]  — for `receiver.method()`
///   [NAME_EXPR("method"), ARG_LIST]              — for `method()`
fn extract_method_name_from_call(node: &SyntaxNode) -> Option<String> {
    method_name_token_from_call(node).map(|token| token.text().to_string())
}

fn method_name_range_from_call(node: &SyntaxNode) -> Option<rowan::TextRange> {
    method_name_token_from_call(node).map(|token| token.text_range())
}

fn method_name_token_from_call(node: &SyntaxNode) -> Option<hyperion_parser::SyntaxToken> {
    // Look for a direct IDENT token child (the method name after DOT)
    let mut last_ident = None;
    for child in node.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                last_ident = Some(t.clone());
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                break;
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::NAME_EXPR => {
                // For simple calls, extract the IDENT from NAME_EXPR
                for elem in n.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = elem {
                        if t.kind() == SyntaxKind::IDENT {
                            last_ident = Some(t);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    last_ident
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::call_resolution::resolve_call;
    use hyperion_semantics::type_parser::parse_type_string;

    #[test]
    fn expanded_java_and_kotlin_varargs_map_each_actual_argument_to_a_name() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Collector.kt"),
                "package p\nclass Collector { fun collect(prefix: String, vararg values: String) {} }",
            )
            .expect("Kotlin vararg fixture should index");
        let string = parse_type_string("String").expect("String type");

        let omitted = resolve_call(
            &index,
            "p.Collector",
            None,
            "collect",
            std::slice::from_ref(&string),
        )
        .expect("omitted vararg call should resolve");
        assert_eq!(
            InlayHintsProvider::parameter_names_from_match(&omitted, 1),
            Some(vec!["prefix".to_string()])
        );

        let expanded = resolve_call(
            &index,
            "p.Collector",
            None,
            "collect",
            &[string.clone(), string.clone(), string],
        )
        .expect("expanded vararg call should resolve");
        assert_eq!(
            InlayHintsProvider::parameter_names_from_match(&expanded, 3),
            Some(vec![
                "prefix".to_string(),
                "values".to_string(),
                "values".to_string(),
            ])
        );

        index
            .index_file_with_source(
                &temp.path().join("p/JavaCollector.java"),
                "package p; class JavaCollector { void collect(String prefix, String... values) {} }",
            )
            .expect("Java vararg fixture should index");
        let java_expanded = resolve_call(
            &index,
            "p.JavaCollector",
            None,
            "collect",
            &[
                parse_type_string("java.lang.String").unwrap(),
                parse_type_string("java.lang.String").unwrap(),
                parse_type_string("java.lang.String").unwrap(),
            ],
        )
        .expect("expanded Java vararg call should resolve");
        assert_eq!(
            InlayHintsProvider::parameter_names_from_match(&java_expanded, 3),
            Some(vec![
                "prefix".to_string(),
                "values".to_string(),
                "values".to_string(),
            ])
        );
    }
}
