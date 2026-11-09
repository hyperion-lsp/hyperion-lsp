// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation Provider for LSP Features
//!
//! This module implements goto-definition and find-references functionality for the Hyperion LSP.
//! It uses the semantic analysis infrastructure to resolve symbols and find their definitions
//! and references throughout the codebase.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{FileId, ParseDatabase, SourceDatabase};
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::TypeInferenceEngine;
use hyperion_workspace::{
    DeclarationNamespace, DefinitionSourceAuthority, FileRevisionToken, QueryEngine,
    ResolvedDefinition, SymbolTarget, TypeNameResolution, WorkspaceIndex,
};
use rowan::{NodeOrToken, SyntaxToken, TextSize};
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Position, Range, Url};
use tracing::debug;

use crate::semantic_host::ReadTxn;

use crate::navigation::argument_inference::{
    infer_argument_types, infer_expression_type_for_navigation,
};
use crate::navigation::call_site_analysis::find_method_call_for_token;
use crate::navigation::context::NavigationContext;
use crate::navigation::import_resolver::{
    enclosing_callable_shadows_static_import, extract_class_name, extract_package_from_root,
    resolve_class_fqn_via_imports, resolve_static_invocation_via_imports,
    resolve_static_lambda_invocation_via_imports, resolve_static_member_via_imports,
};
use crate::navigation::parse_method_chain;
use crate::navigation::ChainSegment;
use crate::navigation_util::{offset_to_position, position_to_offset};

/// Provides navigation features like goto-definition and find-references
///
/// Request-scoped: it borrows the caller's read transaction, which is the only
/// source of Salsa snapshots, so every query in one navigation request sees one
/// consistent revision and no request can create a snapshot outside the gate.
pub struct NavigationProvider<'txn> {
    pub(crate) txn: &'txn ReadTxn,
    pub(crate) workspace_index: Arc<WorkspaceIndex>,
    pub(crate) query_engine: Arc<QueryEngine>,
    pub(crate) source_providers: Arc<crate::source_provider::SourceProviderChain>,
}

/// One workspace declaration resolved from a source position through the
/// navigation pipeline. `definition` carries the exact workspace and file
/// revision proof; `location` is only its LSP presentation.
#[derive(Clone, Debug)]
pub(crate) struct ResolvedNavigationTarget {
    pub(crate) request_revision: FileRevisionToken,
    /// Keep module authority attached when a caller retains a scoped result.
    scope_refresh: Option<hyperion_workspace::ExternalSourceRefreshTicket>,
    /// Presentation location when attached/decompiled source is available.
    /// Exact external identities remain useful to semantic consumers even
    /// when a source provider cannot publish a navigable range.
    pub(crate) location: Option<Location>,
    pub(crate) definition: ResolvedDefinition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeclarationBinding {
    /// A declaration position denotes that declaration itself. This is the
    /// canonical identity used by references, rename, and call hierarchy.
    ExactIdentity,
    /// Follow declaration-specific navigation, such as an `@Override` method
    /// name leading to the overridden declaration.
    FollowNavigation,
}

impl<'txn> NavigationProvider<'txn> {
    /// Resolve array element type (String[] -> String)
    fn resolve_array_element_type(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        // Check if token is part of an array type (e.g., User in "User[]")
        let mut current = token.parent();

        while let Some(node) = current {
            if node.kind() == SyntaxKind::ARRAY_TYPE {
                debug!("NAV: Token is part of ARRAY_TYPE");

                // Find the CLASS_TYPE or PRIMITIVE_TYPE child (element type)
                for child in node.children() {
                    if child.kind() == SyntaxKind::CLASS_TYPE {
                        let element_type = child.text().to_string().trim().to_string();
                        debug!("NAV: Array element type: {}", element_type);

                        // Try to resolve the element type
                        if let Some(symbol_loc) = self.workspace_index.find_symbol(&element_type) {
                            debug!("NAV: OK Found array element type: {}", element_type);
                            self.workspace_index.record_symbol_usage(&element_type);
                            return self.symbol_location_to_lsp_location(&symbol_loc);
                        }

                        // Try import resolution
                        return self.resolve_workspace_lookup(ctx, token, &element_type);
                    }

                    if child.kind() == SyntaxKind::PRIMITIVE_TYPE {
                        // Primitive types don't have definitions to navigate to
                        debug!("NAV: Array element is primitive type, no navigation target");
                        return Ok(None);
                    }
                }

                return Ok(None);
            }

            // Move up the tree
            current = node.parent();
        }

        Ok(None)
    }

    fn resolve_qualified_type_name(
        &self,
        _ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        let Some(parent) = token.parent() else {
            return Ok(None);
        };

        if parent.kind() != SyntaxKind::QUALIFIED_NAME {
            return Ok(None);
        }

        debug!("NAV: Token is part of a QUALIFIED_NAME");
        let qualified_name = parent.text().to_string().trim().to_string();
        debug!("NAV: Full qualified name: {}", qualified_name);

        if let Some(symbol_loc) = self.workspace_index.find_symbol(&qualified_name) {
            debug!("NAV: OK Found qualified type: {}", qualified_name);
            // Record usage for completion ranking
            self.workspace_index.record_symbol_usage(&qualified_name);
            return self.symbol_location_to_lsp_location(&symbol_loc);
        }
        debug!(
            "NAV: FAIL Qualified type '{}' not found in workspace",
            qualified_name
        );

        Ok(None)
    }

    fn resolve_method_chain_entry(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        offset: u32,
    ) -> Result<Option<Location>, String> {
        debug!(
            "resolve_method_chain_entry: token='{}' (kind: {:?})",
            token.text(),
            token.kind()
        );
        let chain_result = parse_method_chain(token);
        if let Some((chain, target_idx)) = chain_result {
            debug!(
                "parse_method_chain: {} segments, target_idx={}",
                chain.len(),
                target_idx
            );
            if target_idx > 0 {
                match self.resolve_chain(
                    ctx.file_id,
                    &chain,
                    target_idx,
                    offset,
                    &ctx.parse_tree.syntax(),
                    ctx.source_text,
                ) {
                    Ok(Some(location)) => {
                        debug!("Resolved method chain to: {:?}", location);
                        return Ok(Some(location));
                    }
                    Ok(None) => {
                        #[cfg(debug_assertions)]
                        debug!("NAV: resolve_chain returned None");
                    }
                    Err(_e) => {
                        #[cfg(debug_assertions)]
                        debug!("NAV: resolve_chain error: {}", _e);
                    }
                }
            } else if target_idx == 0 && chain[0].is_method_call {
                debug!("NAV: Single-segment method call: {}", chain[0].name);

                // A single-segment chain only means the chain parser found no
                // *named* receiver. The call may still be qualified by an
                // expression that has no name to segment on -- `(switch (code)
                // { .. }).toUpperCase()`, `((Foo) bar).baz()`, `arr[0].len()`.
                // Such a call is not unqualified, and resolving it against the
                // enclosing class below would go looking for
                // `Main.toUpperCase`. Ask the receiver expression's type first;
                // this returns `Ok(None)` immediately for a genuinely
                // unqualified call, which has no DOT.
                if let Some(location) =
                    self.resolve_member_via_receiver_expression(ctx, token, offset)?
                {
                    debug!("NAV: OK Resolved member on receiver expression");
                    return Ok(Some(location));
                }

                let enclosing_class =
                    self.find_enclosing_class(&ctx.parse_tree.syntax(), TextSize::from(offset));

                if let Some(class_fqn) = enclosing_class.as_deref() {
                    debug!("NAV: Enclosing class: {}", class_fqn);
                    debug!("NAV: Looking for method: {}.{}", class_fqn, chain[0].name);

                    let symbol_loc = self.resolve_method_with_type_matching(
                        ctx.file_id,
                        class_fqn,
                        &chain[0].name,
                        token,
                        &ctx.parse_tree.syntax(),
                        offset as usize,
                    );

                    if let Some(symbol_loc) = symbol_loc {
                        debug!("NAV: OK Found method (with inheritance support)");
                        return self.symbol_location_to_lsp_location(&symbol_loc);
                    } else {
                        debug!("NAV: FAIL Method not found in workspace (checked inheritance)");
                    }
                }

                let arguments = find_method_call_for_token(token).and_then(|call| {
                    call.children()
                        .find(|child| child.kind() == SyntaxKind::ARG_LIST)
                });
                let argument_types = arguments.as_ref().map(|arguments| {
                    infer_argument_types(
                        arguments,
                        ctx.file_id,
                        &self.workspace_index,
                        &ctx.parse_tree.syntax(),
                        offset,
                    )
                    .into_iter()
                    .map(|inferred| inferred.ty)
                    .collect::<Vec<_>>()
                });
                if let Some(argument_types) = argument_types {
                    let has_lambda = arguments.as_ref().is_some_and(|args| {
                        args.children()
                            .any(|child| child.kind() == SyntaxKind::LAMBDA_EXPR)
                    });
                    let member_shadows_import = enclosing_class.as_deref().is_some_and(|owner| {
                        if has_lambda {
                            crate::navigation::overload_resolver::enclosing_overload_may_apply_with_arguments(
                                &self.workspace_index,
                                owner,
                                &chain[0].name,
                                &argument_types,
                                arguments.as_ref().expect("lambda has argument list"),
                                ctx.file_id,
                                &ctx.parse_tree.syntax(),
                            )
                        } else {
                            enclosing_callable_shadows_static_import(
                                &self.workspace_index,
                                ctx.file_id,
                                owner,
                                &chain[0].name,
                                &argument_types,
                            )
                        }
                    });
                    if !member_shadows_import {
                        let symbol_loc = if has_lambda {
                            resolve_static_lambda_invocation_via_imports(
                                &self.workspace_index,
                                ctx.file_id,
                                &chain[0].name,
                                &argument_types,
                                arguments.as_ref().expect("lambda has argument list"),
                                &ctx.parse_tree.syntax(),
                            )
                        } else {
                            resolve_static_invocation_via_imports(
                                self.workspace_index.as_ref(),
                                ctx.file_id,
                                &chain[0].name,
                                &argument_types,
                            )
                        };
                        if let Some(symbol_loc) = symbol_loc {
                            debug!("NAV: OK Found exact static import overload");
                            return self.symbol_location_to_lsp_location(&symbol_loc);
                        }
                    }
                }
            } else if target_idx == 0 && !chain[0].is_method_call {
                debug!("NAV: Single-segment field access: {}", chain[0].name);

                let enclosing_class = self.find_enclosing_class(
                    &ctx.parse_tree.syntax(),
                    TextSize::from(ctx.offset as u32),
                );
                let enclosing_value_shadows_import =
                    enclosing_class.as_deref().is_some_and(|class_fqn| {
                        self.workspace_index
                            .get_imports(ctx.file_id)
                            .is_some_and(|imports| {
                                hyperion_workspace::import_resolver::ImportResolver::new(
                                    &imports,
                                    &self.workspace_index,
                                )
                                .has_value_member_named(class_fqn, &chain[0].name)
                            })
                    });

                if let Some(class_fqn) = enclosing_class {
                    debug!("NAV: Enclosing class: {}", class_fqn);
                    debug!("NAV: Looking for field: {}.{}", class_fqn, chain[0].name);

                    if let Some(symbol_loc) =
                        self.find_value_with_inheritance(&class_fqn, &chain[0].name)
                    {
                        debug!("NAV: OK Found field (with inheritance support)");
                        return self.symbol_location_to_lsp_location(&symbol_loc);
                    }
                    debug!("NAV: FAIL Field not found in workspace (checked inheritance)");
                }

                if !enclosing_value_shadows_import {
                    if let Some(symbol_loc) = resolve_static_member_via_imports(
                        self.workspace_index.as_ref(),
                        ctx.file_id,
                        &chain[0].name,
                        DeclarationNamespace::Value,
                    ) {
                        debug!("NAV: Resolved field via static import: {}", symbol_loc.fqn);
                        return self.symbol_location_to_lsp_location(&symbol_loc);
                    }
                }
            }
        }

        if crate::navigation::ast_queries::is_followed_by_paren(token)
            || token_has_method_call_ancestor(token)
            || find_method_call_for_token(token).is_some()
        {
            let engine = self.create_type_inference_engine(ctx.file_id);
            let builder = ResolverBuilder::with_workspace(ctx.file_id, ctx.workspace_index.clone());
            let resolver = builder.build_from_syntax(&ctx.parse_tree.syntax());
            let mut inferred = hyperion_semantics::symbol::Type::Unknown;

            if let Some(receiver_expr) = self.find_receiver_expression_for_member(token) {
                inferred = engine.infer_expression(&receiver_expr, &resolver, None);
                if matches!(inferred, hyperion_semantics::symbol::Type::Unknown) {
                    let fallback = infer_expression_type_for_navigation(
                        &receiver_expr,
                        ctx.file_id,
                        &ctx.workspace_index,
                        &ctx.parse_tree.syntax(),
                        offset,
                    );
                    inferred = fallback.ty;

                    if matches!(inferred, hyperion_semantics::symbol::Type::Unknown) {
                        if let Some(branch_name) = extract_ternary_branch_name(&receiver_expr) {
                            let scope = resolver.find_scope_at_offset(offset);
                            let symbol = scope.and_then(|id| {
                                resolver.resolve_in_scope_at_offset(&branch_name, id, offset)
                            });
                            if let Some(resolved) = symbol {
                                inferred = resolved.ty;
                            }
                        }
                    }
                }
            }

            if !matches!(inferred, hyperion_semantics::symbol::Type::Unknown) {
                if find_method_call_for_token(token).is_some() {
                    let Some(receiver_fqn) = self.type_to_fqn(&inferred, ctx) else {
                        return Ok(None);
                    };
                    let matched = self.resolve_method_with_type_matching(
                        ctx.file_id,
                        &receiver_fqn,
                        token.text(),
                        token,
                        &ctx.parse_tree.syntax(),
                        offset as usize,
                    );
                    return matched.map_or(Ok(None), |location| {
                        self.symbol_location_to_lsp_location(&location)
                    });
                }
                return self.resolve_method_on_inferred_type(&inferred, token.text(), ctx.file_id);
            }
        }

        if let Some(field_access) = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
        {
            let text = field_access.text().to_string();
            let mut segments = Vec::new();
            for raw in text.split('.') {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let name = trimmed.split('(').next().unwrap_or(trimmed).trim();
                if name.is_empty() {
                    continue;
                }
                segments.push(ChainSegment {
                    name: name.to_string(),
                    is_method_call: trimmed.contains('('),
                    cast_type: None,
                    call_range: None,
                });
            }
            if let Some(target_idx) = segments.iter().rposition(|seg| seg.name == token.text()) {
                if crate::navigation::ast_queries::is_followed_by_paren(token)
                    || token_has_method_call_ancestor(token)
                {
                    segments[target_idx].is_method_call = true;
                }
                if target_idx > 0 {
                    if let Ok(Some(location)) = self.resolve_chain(
                        ctx.file_id,
                        &segments,
                        target_idx,
                        offset,
                        &ctx.parse_tree.syntax(),
                        ctx.source_text,
                    ) {
                        return Ok(Some(location));
                    }
                }
            }
        }

        Ok(None)
    }

    fn find_receiver_expression_for_member(
        &self,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Option<SyntaxNode> {
        let mut current = token.parent();

        while let Some(node) = current {
            if node.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                for child in node.children() {
                    if is_receiver_expression_kind(child.kind()) {
                        return Some(child);
                    }
                }
            }

            current = node.parent();
        }

        if let Some(parent) = token.parent() {
            if let Some(container) = parent.parent() {
                let children: Vec<_> = container.children_with_tokens().collect();
                let token_pos = children
                    .iter()
                    .position(|child| matches!(child, NodeOrToken::Token(t) if t == token))?;

                for idx in (0..token_pos).rev() {
                    match &children[idx] {
                        NodeOrToken::Node(node) if is_receiver_expression_kind(node.kind()) => {
                            return Some(node.clone());
                        }
                        NodeOrToken::Token(tok)
                            if tok.kind() == SyntaxKind::DOT
                                || tok.kind() == SyntaxKind::WHITESPACE =>
                        {
                            continue;
                        }
                        NodeOrToken::Node(node) if node.kind() == SyntaxKind::METHOD_CALL_EXPR => {
                            continue;
                        }
                        _ => break,
                    }
                }
            }
        }

        let mut prev_token = token.prev_token();
        while let Some(tok) = prev_token.clone() {
            if tok.kind() == SyntaxKind::DOT {
                if let Some(receiver_node) = self.find_receiver_expression_before_dot(&tok, token) {
                    return Some(receiver_node);
                }
                break;
            }

            if tok.kind() == SyntaxKind::WHITESPACE {
                prev_token = tok.prev_token();
                continue;
            }

            prev_token = tok.prev_token();
        }

        let mut prev = token.prev_token();
        while let Some(tok) = prev.clone() {
            if tok.kind() == SyntaxKind::R_PAREN {
                if let Some(parent) = tok.parent() {
                    if parent.kind() == SyntaxKind::PAREN_EXPR {
                        return Some(parent);
                    }
                }
            }
            prev = tok.prev_token();
        }

        None
    }

    fn find_receiver_expression_before_dot(
        &self,
        dot_token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Option<SyntaxNode> {
        let dot_offset: TextSize = dot_token.text_range().start();
        let root = token.parent_ancestors().last()?;

        let mut best: Option<SyntaxNode> = None;
        for node in root.descendants() {
            if !is_receiver_expression_kind(node.kind()) {
                continue;
            }

            let end = node.text_range().end();
            if end > dot_offset {
                continue;
            }

            let should_replace = best
                .as_ref()
                .is_none_or(|current| end > current.text_range().end());
            if should_replace {
                best = Some(node);
            }
        }

        best
    }

    fn resolve_super_keyword(&self, ctx: &NavigationContext) -> Result<Option<Location>, String> {
        if ctx.token.kind() != SyntaxKind::SUPER_KW {
            return Ok(None);
        }

        debug!("super keyword detected - navigating to parent class");

        let syntax = ctx.parse_tree.syntax();
        if let Some(current_class_fqn) =
            self.find_enclosing_class(&syntax, ctx.token.text_range().start())
        {
            debug!("NAV: Current class for super: {}", current_class_fqn);

            // Declaration symbols retain the source spelling of `extends`
            // (often just `Parent`) for diagnostics. Navigation needs the
            // canonical edge resolved by the workspace hierarchy instead.
            if let Some(parent_fqn) = self.workspace_index.get_parent_class(&current_class_fqn) {
                debug!("NAV: Parent class: {}", parent_fqn);

                if let Some(symbol_loc) = self.workspace_index.find_symbol(&parent_fqn) {
                    debug!("NAV: OK Found parent class");
                    return self.symbol_location_to_lsp_location(&symbol_loc);
                } else {
                    // Parent class not found in workspace - check if it's a JDK class
                    if parent_fqn.starts_with("java.")
                        || parent_fqn.starts_with("javax.")
                        || parent_fqn.starts_with("jakarta.")
                    {
                        debug!(
                            "NAV: ℹ️  Cannot navigate to JDK parent class '{}' \
                             (JDK source not indexed). \
                             Consider adding JDK sources to workspace for full navigation.",
                            parent_fqn
                        );
                    } else {
                        debug!(
                            "NAV: WARN Parent class '{}' not found in workspace index - \
                             this may indicate incomplete indexing or missing dependency",
                            parent_fqn
                        );
                    }
                }
            }
        }

        debug!("NAV: FAIL Could not resolve super keyword");
        Ok(None)
    }
    /// Creates a new NavigationProvider
    pub const fn new(
        txn: &'txn ReadTxn,
        workspace_index: Arc<WorkspaceIndex>,
        query_engine: Arc<QueryEngine>,
        source_providers: Arc<crate::source_provider::SourceProviderChain>,
    ) -> Self {
        Self {
            txn,
            workspace_index,
            query_engine,
            source_providers,
        }
    }

    /// Create a TypeInferenceEngine for expression type inference
    ///
    /// This provides a unified interface for inferring types of expressions,
    /// method calls, field access, and other complex expressions.
    ///
    /// # Arguments
    /// * `file_id` - The file context for type resolution (for import resolution)
    pub(crate) fn create_type_inference_engine(&self, file_id: FileId) -> TypeInferenceEngine {
        TypeInferenceEngine::new(self.workspace_index.clone(), file_id)
    }

    /// Go to the definition of the symbol at the given position
    ///
    /// This method first attempts cross-file navigation using the workspace index,
    /// then falls back to same-file semantic resolution if that fails.
    ///
    /// # Arguments
    ///
    /// * `file_id` - The FileId of the document
    /// * `position` - The cursor position in the document
    /// * `source_text` - The source text of the document
    /// * `uri` - The URI of the document
    ///
    /// # Returns
    ///
    /// A Location pointing to the symbol's definition, if found
    pub fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        if let Some(provider) = self.scoped_provider_for_file(file_id) {
            return provider.goto_definition(file_id, position, source_text, uri);
        }
        if !self.workspace_index.source_scope_is_current() {
            return Ok(None);
        }
        let workspace_revision = self.workspace_index.current_workspace_revision();
        let result = self.goto_definition_at_revision(file_id, position, source_text, uri)?;
        if !self
            .workspace_index
            .workspace_bindings_are_current(workspace_revision)
            || !self.workspace_index.source_scope_is_current()
        {
            debug!("Goto definition: workspace bindings changed during request");
            return Ok(None);
        }
        Ok(result)
    }

    /// Keep every lookup in this request on the caller's published classpath.
    /// The index view shares storage, while query caches belong to this scope.
    fn scoped_provider_for_file(&self, file_id: FileId) -> Option<Self> {
        if self.workspace_index.has_source_scope() {
            return None;
        }
        let workspace_index = Arc::new(self.workspace_index.scoped_view_for_file(file_id)?);
        Some(Self {
            txn: self.txn,
            query_engine: Arc::new(QueryEngine::new((*workspace_index).clone())),
            workspace_index,
            source_providers: Arc::clone(&self.source_providers),
        })
    }

    /// Resolve a source position through the canonical navigation pipeline and
    /// bind its returned declaration range back to one exact indexed identity.
    ///
    /// This is the shared boundary for features which need more than an LSP
    /// location (references, rename, hover, and refactorings). Both the
    /// workspace election and the declaration's source revision are captured
    /// atomically enough to fail closed if either changes during resolution.
    pub(crate) fn resolve_workspace_target_at_position(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
        expected_namespace: Option<DeclarationNamespace>,
    ) -> Result<Option<ResolvedNavigationTarget>, String> {
        self.resolve_workspace_target_at_position_with_binding(
            file_id,
            position,
            source_text,
            uri,
            expected_namespace,
            DeclarationBinding::ExactIdentity,
        )
    }

    /// Resolve the destination of declaration-specific navigation while still
    /// binding the result to an exact, revision-checked workspace identity.
    ///
    /// This is intentionally separate from [`Self::resolve_workspace_target_at_position`]:
    /// an override declaration's canonical identity is the overriding method,
    /// while its navigation relationship points at the overridden method.
    pub(crate) fn resolve_related_workspace_target_at_position(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
        expected_namespace: Option<DeclarationNamespace>,
    ) -> Result<Option<ResolvedNavigationTarget>, String> {
        self.resolve_workspace_target_at_position_with_binding(
            file_id,
            position,
            source_text,
            uri,
            expected_namespace,
            DeclarationBinding::FollowNavigation,
        )
    }

    fn resolve_workspace_target_at_position_with_binding(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
        expected_namespace: Option<DeclarationNamespace>,
        declaration_binding: DeclarationBinding,
    ) -> Result<Option<ResolvedNavigationTarget>, String> {
        if let Some(provider) = self.scoped_provider_for_file(file_id) {
            return provider.resolve_workspace_target_at_position_with_binding(
                file_id,
                position,
                source_text,
                uri,
                expected_namespace,
                declaration_binding,
            );
        }
        if !self.workspace_index.source_scope_is_current() {
            return Ok(None);
        }
        let workspace_revision = self.workspace_index.current_workspace_revision();
        let Some(request_revision) = self.workspace_index.current_file_revision(file_id) else {
            return Ok(None);
        };
        let Some(request_source) = self
            .workspace_index
            .get_indexed_revision_content(request_revision)
        else {
            return Ok(None);
        };
        if request_source.content.as_str() != source_text
            || uri.to_file_path().ok().as_ref() != Some(&request_source.file_path)
        {
            debug!("Symbol resolution: request text has no exact indexed revision");
            return Ok(None);
        }
        // Declaration identifiers already carry an exact, revision-bound
        // workspace identity. Bind that range before constructing whole-file
        // semantics or running any presentation-oriented navigation fallback.
        // Besides making declarations cheap in large files, this keeps every
        // consumer on the same namespace-aware identity election.
        if declaration_binding == DeclarationBinding::ExactIdentity {
            if let Some(target) = self.resolve_indexed_declaration_at_position(
                file_id,
                position,
                source_text,
                request_revision,
                workspace_revision,
                expected_namespace,
            ) {
                return Ok(Some(target));
            }
        }
        if expected_namespace.is_none_or(|namespace| namespace == DeclarationNamespace::Type) {
            if let Some(definition) = self.resolve_external_type_definition_at_position(
                file_id,
                position,
                source_text,
                workspace_revision,
            ) {
                let location = self
                    .symbol_location_to_lsp_location(&definition.location)
                    .ok()
                    .flatten();
                if !self
                    .workspace_index
                    .workspace_bindings_are_current(workspace_revision)
                    || self.workspace_index.current_file_revision(file_id) != Some(request_revision)
                {
                    debug!("Symbol resolution: request changed during external type binding");
                    return Ok(None);
                }
                let target = ResolvedNavigationTarget {
                    request_revision,
                    scope_refresh: self.workspace_index.source_scope_refresh_ticket(),
                    location,
                    definition,
                };
                return Ok(self.workspace_target_is_current(&target).then_some(target));
            }
        }
        let Some(location) =
            self.goto_definition_at_revision(file_id, position, source_text, uri)?
        else {
            return Ok(None);
        };
        if !self
            .workspace_index
            .workspace_bindings_are_current(workspace_revision)
        {
            debug!("Symbol resolution: workspace bindings changed during navigation");
            return Ok(None);
        }

        let Ok(definition_path) = location.uri.to_file_path() else {
            return Ok(None);
        };
        let Some(definition_file_id) = self.workspace_index.get_file_id(&definition_path) else {
            return Ok(None);
        };
        if self.workspace_index.is_external_file(definition_file_id) {
            // External navigation locations are ranges in attached/decompiled
            // source, not indexed bytecode declaration ranges. For nominal
            // types, rebind the spelling from the exact request revision
            // through the canonical contextual type resolver. Callable and
            // value identities still fail closed rather than being guessed
            // from presentation coordinates.
            let definition = expected_namespace
                .is_none_or(|namespace| namespace == DeclarationNamespace::Type)
                .then(|| {
                    self.resolve_external_type_definition_at_position(
                        file_id,
                        position,
                        source_text,
                        workspace_revision,
                    )
                })
                .flatten();
            let Some(definition) = definition else {
                return Ok(None);
            };
            if !self
                .workspace_index
                .workspace_bindings_are_current(workspace_revision)
                || self.workspace_index.current_file_revision(file_id) != Some(request_revision)
            {
                debug!("Symbol resolution: request changed during external type binding");
                return Ok(None);
            }
            let target = ResolvedNavigationTarget {
                request_revision,
                scope_refresh: self.workspace_index.source_scope_refresh_ticket(),
                location: Some(location),
                definition,
            };
            return Ok(self.workspace_target_is_current(&target).then_some(target));
        }
        let Some(definition_revision) = self
            .workspace_index
            .current_file_revision(definition_file_id)
        else {
            return Ok(None);
        };
        let Some(indexed) = self
            .workspace_index
            .get_indexed_revision_content(definition_revision)
        else {
            return Ok(None);
        };
        if indexed.file_path != definition_path {
            return Ok(None);
        }
        let start = position_to_offset(&indexed.content, location.range.start)?;
        let end = position_to_offset(&indexed.content, location.range.end)?;
        let declaration_range = hyperion_workspace::TextRange::new(start, end);

        let namespaces = [
            DeclarationNamespace::Type,
            DeclarationNamespace::Constructor,
            DeclarationNamespace::Callable,
            DeclarationNamespace::Value,
        ];
        let mut definitions = namespaces
            .into_iter()
            .filter(|namespace| expected_namespace.is_none_or(|expected| expected == *namespace))
            .filter_map(|namespace| {
                self.workspace_index
                    .find_symbol_definition_at_revision_range(
                        definition_revision,
                        workspace_revision,
                        declaration_range,
                        namespace,
                    )
            })
            .collect::<Vec<_>>();
        definitions.sort_unstable_by(|left, right| {
            left.target
                .fqn
                .cmp(&right.target.fqn)
                .then((left.target.namespace as u8).cmp(&(right.target.namespace as u8)))
        });
        definitions.dedup_by(|left, right| left.target == right.target);
        if definitions.is_empty() {
            // A Java call to `getNote()` lands on the Kotlin property `note`:
            // its identity is the property, whatever namespace the call asked for.
            if let Some(origin) = self
                .workspace_index
                .find_jvm_origin_definition_at_revision_range(
                    definition_revision,
                    workspace_revision,
                    declaration_range,
                )
            {
                definitions.push(origin);
            }
        }
        let [definition] = definitions.as_slice() else {
            debug!(
                "Symbol resolution: navigation range mapped to {} indexed declarations",
                definitions.len()
            );
            return Ok(None);
        };
        if !self
            .workspace_index
            .workspace_bindings_are_current(workspace_revision)
            || self.workspace_index.current_file_revision(file_id) != Some(request_revision)
            || self
                .workspace_index
                .current_file_revision(definition_file_id)
                != Some(definition_revision)
        {
            debug!("Symbol resolution: declaration changed during identity binding");
            return Ok(None);
        }

        let target = ResolvedNavigationTarget {
            request_revision,
            scope_refresh: self.workspace_index.source_scope_refresh_ticket(),
            location: Some(location),
            definition: definition.clone(),
        };
        Ok(self.workspace_target_is_current(&target).then_some(target))
    }

    /// Bind an identifier whose source range is itself an indexed declaration.
    ///
    /// Usage ranges are never accepted here: the lookup is restricted to the
    /// exact request revision's declaration table and one semantic namespace.
    /// Ambiguous ranges fail closed and continue through the ordinary resolver.
    fn resolve_indexed_declaration_at_position(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        request_revision: FileRevisionToken,
        workspace_revision: hyperion_workspace::WorkspaceRevision,
        expected_namespace: Option<DeclarationNamespace>,
    ) -> Option<ResolvedNavigationTarget> {
        let parse = self
            .workspace_index
            .cached_java_parse(file_id, source_text)?;
        let root = SyntaxNode::new_root(parse.as_ref().clone());
        let offset = position_to_offset(source_text, position).ok()?;
        let at_offset = root.token_at_offset(TextSize::from(offset));
        let token = [at_offset.clone().right_biased(), at_offset.left_biased()]
            .into_iter()
            .flatten()
            .find(|candidate| candidate.kind().is_identifier())?;
        let token_range = token.text_range();
        let declaration_range = hyperion_workspace::TextRange::new(
            u32::from(token_range.start()),
            u32::from(token_range.end()),
        );

        let namespaces = [
            DeclarationNamespace::Type,
            DeclarationNamespace::Constructor,
            DeclarationNamespace::Callable,
            DeclarationNamespace::Value,
        ];
        let mut definitions = namespaces
            .into_iter()
            .filter(|namespace| expected_namespace.is_none_or(|expected| expected == *namespace))
            .filter_map(|namespace| {
                self.workspace_index
                    .find_symbol_definition_at_revision_range(
                        request_revision,
                        workspace_revision,
                        declaration_range,
                        namespace,
                    )
            })
            .collect::<Vec<_>>();
        definitions.sort_unstable_by(|left, right| {
            left.target
                .fqn
                .cmp(&right.target.fqn)
                .then((left.target.namespace as u8).cmp(&(right.target.namespace as u8)))
        });
        definitions.dedup_by(|left, right| left.target == right.target);
        let [definition] = definitions.as_slice() else {
            return None;
        };
        let location = self
            .symbol_location_to_lsp_location(&definition.location)
            .ok()
            .flatten();
        let target = ResolvedNavigationTarget {
            request_revision,
            scope_refresh: self.workspace_index.source_scope_refresh_ticket(),
            location,
            definition: definition.clone(),
        };
        self.workspace_target_is_current(&target).then_some(target)
    }

    /// Bind a nominal external definition without relying on its decompiled
    /// source range. The cached parse and import table both belong to the exact
    /// indexed request revision checked by the caller.
    fn resolve_external_type_definition_at_position(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        workspace_revision: hyperion_workspace::WorkspaceRevision,
    ) -> Option<ResolvedDefinition> {
        let parse = self
            .workspace_index
            .cached_java_parse(file_id, source_text)?;
        let root = SyntaxNode::new_root(parse.as_ref().clone());
        let offset = position_to_offset(source_text, position).ok()?;
        let at_offset = root.token_at_offset(TextSize::from(offset));
        let token = [at_offset.clone().right_biased(), at_offset.left_biased()]
            .into_iter()
            .flatten()
            .find(|token| token.kind().is_identifier())?;
        let (written, lexical_anchor) = if let Some(type_node) = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::CLASS_TYPE)
        {
            let written = hyperion_types::extract_type_structure(&type_node)
                .ok()?
                .base;
            if hyperion_types::enclosing_type_parameter_names(&type_node)
                .iter()
                .any(|parameter| parameter == &written)
            {
                return None;
            }
            (written, type_node)
        } else {
            let qualified_name = token.parent().filter(|parent| {
                parent.kind() == SyntaxKind::QUALIFIED_NAME
                    && parent.parent().is_some_and(|node| {
                        matches!(node.kind(), SyntaxKind::ANNOTATION | SyntaxKind::NEW_EXPR)
                    })
            })?;
            let written = qualified_name
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|candidate| candidate.kind().is_identifier())
                .map(|candidate| candidate.text().to_owned())
                .collect::<Vec<_>>()
                .join(".");
            if written.is_empty() {
                return None;
            }
            (written, qualified_name)
        };
        let lexical_owners = self.lexical_type_owners_at_offset(&root, token.text_range().start());
        let TypeNameResolution::Exact(fqn) =
            self.workspace_index
                .resolve_type_name(file_id, &lexical_owners, &written)
        else {
            return None;
        };
        debug!(
            "Resolved external type '{}' from {:?}",
            fqn,
            lexical_anchor.kind()
        );
        let definition = self
            .workspace_index
            .find_symbol_definition(&SymbolTarget::new(fqn, DeclarationNamespace::Type))?;
        (self
            .workspace_index
            .workspace_bindings_agree(definition.workspace_revision, workspace_revision)
            && definition.source == DefinitionSourceAuthority::External)
            .then_some(definition)
    }

    /// Revalidate a previously resolved target immediately before a feature
    /// publishes bytes or metadata derived from it.
    pub(crate) fn workspace_target_is_current(&self, target: &ResolvedNavigationTarget) -> bool {
        target
            .scope_refresh
            .as_ref()
            .is_none_or(hyperion_workspace::ExternalSourceRefreshTicket::is_committed)
            && self.workspace_index.source_scope_is_current()
            && self
                .workspace_index
                .workspace_bindings_are_current(target.definition.workspace_revision)
            && self
                .workspace_index
                .current_file_revision(target.request_revision.file_id())
                == Some(target.request_revision)
            && match target.definition.source {
                DefinitionSourceAuthority::Workspace(revision) => {
                    self.workspace_index
                        .current_file_revision(revision.file_id())
                        == Some(revision)
                }
                DefinitionSourceAuthority::External => true,
            }
    }

    fn goto_definition_at_revision(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        // One snapshot for the entire goto-definition request: all Salsa
        // queries are memoized within the caller's read transaction.
        let snapshot = self.txn.snapshot();

        let language = snapshot.file_language(file_id);

        // Unified language dispatch — Kotlin, Groovy (incl. Gradle), Scala
        let ctx = crate::language_handler::LanguageHandlerContext {
            txn: self.txn,
            workspace_index: &self.workspace_index,
            source_providers: &self.source_providers,
            query_engine: None,
        };
        if let Some(handler) = ctx.handler_for(language) {
            return handler.goto_definition(file_id, position, source_text, uri);
        }

        // Use the single snapshot for parsing (no additional lock)
        let parse_tree = snapshot.parse_file(file_id);

        // debug!("NAV: Got parse tree, has {} errors", parse_tree.errors.len());

        // Convert LSP position to offset
        let offset = position_to_offset(source_text, position)?;
        // debug!("NAV: Converted position to offset {}", offset);

        // Check if cursor is in comment or string - return None early
        if super::ast_queries::is_in_comment_or_string_java(&parse_tree.syntax(), offset) {
            debug!("NAV: Cursor in comment/string, skipping goto-definition");
            return Ok(None);
        }

        // Find the token at the position
        // For method references, we might need to check both left and right tokens
        let token_offset = parse_tree.syntax().token_at_offset(TextSize::from(offset));

        // Try both left and right biased tokens to handle cursor on punctuation
        // Example: cursor on dot in `foo.bar` - right_biased gets `bar`, we want `foo`
        let mut token = {
            let right = token_offset.clone().right_biased();
            let left = token_offset.left_biased();

            match (left, right) {
                (Some(l), Some(r)) => {
                    // If both are identifiers, prefer left (navigating FROM foo in foo.bar)
                    if l.kind().is_identifier() && r.kind().is_identifier() {
                        l
                    } else if l.kind().is_identifier() {
                        l
                    } else if r.kind().is_identifier() {
                        r
                    } else {
                        // Neither is identifier, use right as before
                        r
                    }
                }
                (Some(l), None) => l,
                (None, Some(r)) => r,
                (None, None) => return Err("No token at position".into()),
            }
        };

        let offset_usize = offset as usize;

        let context = crate::navigation::context::NavigationContext::new(
            file_id,
            position,
            uri,
            source_text,
            &parse_tree,
            offset_usize,
            token.clone(),
            self.workspace_index.clone(),
        );

        // Debug: print the tree structure around the token
        #[cfg(debug_assertions)]
        if let Some(parent) = token.parent() {
            debug!("NAV: Parent children (all):");
            for child in parent.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Node(n) => {
                        debug!("NAV:   - node {:?}: {}", n.kind(), n.text());
                    }
                    rowan::NodeOrToken::Token(t) => {
                        debug!("NAV:   - token {:?}: {}", t.kind(), t.text());
                    }
                }
            }
        }

        debug!(
            "Token at position: {:?} (kind: {:?}) at offset {:?}",
            token.text(),
            token.kind(),
            token.text_range().start()
        );

        if let Some(location) = self.resolve_super_keyword(&context)? {
            return Ok(Some(location));
        }

        if let Some(location) = self.handle_method_reference(&context, &mut token)? {
            return Ok(Some(location));
        }
        if let Some(location) = self.resolve_constructor_reference_at_offset(&context)? {
            return Ok(Some(location));
        }

        // Only handle identifiers (including contextual keywords like 'record', 'var', 'yield')
        if !token.kind().is_identifier() {
            return Ok(None);
        }

        // A method declaration name is never an expression use, so it must not
        // fall through to the expression-oriented resolvers below (which would
        // happily resolve it to a same-named field). @Override methods navigate
        // to the super declaration; everything else self-navigates.
        if let Some(method_decl) = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
        {
            if let Some(name_tok) = Self::method_decl_name_token(&method_decl) {
                if name_tok.text_range() == token.text_range() {
                    if Self::method_decl_has_override_annotation(&method_decl) {
                        if let Some(location) = self.resolve_override_method(&context, &token)? {
                            return Ok(Some(location));
                        }
                    }

                    let token_range = name_tok.text_range();
                    let start = offset_to_position(source_text, token_range.start().into());
                    let end = offset_to_position(source_text, token_range.end().into());
                    return Ok(Some(Location {
                        uri: uri.clone(),
                        range: Range { start, end },
                    }));
                }
            }
        }

        // Primary resolution: FileSemantics (Salsa-cached semantic model)
        // Handles types, fields, methods, enum constants, and local variables.
        // Shared with diagnostics to prevent resolution divergence.
        match self.resolve_via_file_semantics(&context, &token)? {
            super::workspace_resolver::FileSemanticsNavigation::Resolved(location) => {
                debug!("NAV: OK Resolved via FileSemantics");
                return Ok(Some(location));
            }
            super::workspace_resolver::FileSemanticsNavigation::RejectedCall
            | super::workspace_resolver::FileSemanticsNavigation::RejectedBinding => {
                debug!("NAV: FileSemantics authoritatively rejected the binding");
                return Ok(None);
            }
            super::workspace_resolver::FileSemanticsNavigation::Unresolved => {}
        }

        // Specialized handlers for patterns FileSemantics doesn't cover yet
        if let Some(location) = self.resolve_annotation_reference(&context, &token)? {
            return Ok(Some(location));
        }

        if let Some(location) = self.resolve_override_method(&context, &token)? {
            return Ok(Some(location));
        }

        if let Some(location) = self.resolve_inheritance_clause(&context, &token)? {
            return Ok(Some(location));
        }

        if let Some(location) = self.resolve_array_element_type(&context, &token)? {
            return Ok(Some(location));
        }

        if let Some(location) = self.resolve_qualified_type_name(&context, &token)? {
            return Ok(Some(location));
        }

        if let Some(location) = self.resolve_method_chain_entry(&context, &token, offset)? {
            return Ok(Some(location));
        }

        // Parsed invocations have already passed through receiver-aware,
        // argument-aware call resolution above. Falling through to the global
        // qualified-member/name lookup would resurrect an inapplicable or
        // ambiguous method merely because it has a similarly named
        // declaration. Qualified non-call member access continues below.
        if find_method_call_for_token(&token).is_some() {
            if let Some(location) = self.resolve_anonymous_class_method_symbol(
                &parse_tree.syntax(),
                source_text,
                offset,
                token.text(),
                uri,
            ) {
                return Ok(Some(location));
            }
            return Ok(None);
        }

        if let Some(location) = self.resolve_qualified_member_access(&context, &token, offset)? {
            return Ok(Some(location));
        }

        // Extract qualified name based on context (method call, type reference, etc.)
        let qualified_name = self.extract_name_in_context(&token);
        debug!(
            "Looking for definition of identifier: {} (context-aware)",
            qualified_name
        );

        if let Some(location) = self.resolve_workspace_lookup(&context, &token, &qualified_name)? {
            debug!(
                "NAV: OK Resolved via workspace lookup for '{}'",
                qualified_name
            );
            // Record usage for completion ranking
            self.workspace_index.record_symbol_usage(&qualified_name);
            return Ok(Some(location));
        }

        if let Some(location) = self.resolve_inherited_member(&context, &qualified_name)? {
            debug!("NAV: OK Resolved via inherited member");
            // Record usage for completion ranking
            self.workspace_index.record_symbol_usage(&qualified_name);
            return Ok(Some(location));
        }

        self.resolve_same_file_symbol(&context, source_text, offset, &qualified_name, uri)
    }

    /// Extract a name from a token based on its syntactic context
    ///
    /// Handles different contexts:
    /// - METHOD_CALL_EXPR: Returns simple method name (for goto method definition)
    /// - CLASS_TYPE: Returns qualified type name (e.g., `java.util.List`)
    /// - Otherwise: Returns simple identifier
    pub fn extract_name_in_context(
        &self,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> String {
        let mut current = token.parent();

        while let Some(node) = current {
            match node.kind() {
                // Method call: foo() or obj.foo()
                SyntaxKind::METHOD_CALL_EXPR => {
                    debug!("Context: method call");
                    // Just return the simple method name
                    return token.text().to_string();
                }

                // Field access: obj.field or EnumType.CONSTANT
                SyntaxKind::FIELD_ACCESS_EXPR => {
                    debug!("Context: field access");
                    // Extract all identifiers separated by DOT to create qualified name
                    // This handles enum constants like ResourceType.UNKNOWN
                    let mut parts = Vec::new();

                    for child in node.descendants_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = child {
                            if t.kind().is_identifier() {
                                parts.push(t.text().to_string());
                            }
                        }
                    }

                    if parts.len() >= 2 {
                        let qualified = parts.join(".");
                        debug!("Extracted qualified field access: {}", qualified);
                        return qualified;
                    }

                    break;
                }

                // Qualified type: com.example.Foo
                SyntaxKind::CLASS_TYPE => {
                    debug!("Context: class type");
                    // Extract all IDENT tokens separated by DOT
                    let mut parts = Vec::new();

                    for child in node.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = child {
                            if t.kind().is_identifier() {
                                parts.push(t.text().to_string());
                            }
                        }
                    }

                    if !parts.is_empty() {
                        let qualified = parts.join(".");
                        debug!("Extracted qualified type: {}", qualified);
                        return qualified;
                    }

                    break;
                }

                _ => {}
            }

            current = node.parent();
        }

        // Fall back to simple identifier
        debug!("Context: simple identifier");
        token.text().to_string()
    }

    /// Resolve qualified member access using type inference
    ///
    /// Resolves obj.member by:
    /// 1. Finding the type of 'obj'
    /// 2. Constructing the FQN: class_fqn.member
    /// 3. Querying the workspace for the member
    pub(crate) fn resolve_class_name_for_static_access(
        &self,
        file_id: FileId,
        class_name: &str,
        root: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<hyperion_semantics::symbol::Type, String> {
        use hyperion_parser::SyntaxKind;
        use hyperion_semantics::symbol::Type;

        // Try to resolve via imports first
        match resolve_class_fqn_via_imports(self.workspace_index.as_ref(), file_id, class_name) {
            Ok(class_fqn) => {
                debug!(
                    "RESOLVE: OK Resolved '{}' as class via imports to '{}'",
                    class_name, class_fqn
                );
                // Parse the FQN to get package and name
                if let Some(last_dot) = class_fqn.rfind('.') {
                    let (pkg, name) = class_fqn.split_at(last_dot);
                    return Ok(Type::Class(hyperion_semantics::symbol::ClassType {
                        name: name[1..].to_string(),
                        package: Some(pkg.to_string()),
                        type_arguments: Vec::new(),
                        is_diamond: false,
                    }));
                }
                return Ok(Type::Class(hyperion_semantics::symbol::ClassType {
                    name: class_fqn,
                    package: None,
                    type_arguments: Vec::new(),
                    is_diamond: false,
                }));
            }
            Err(_) => {
                // Not in imports, check if it's the current class
                debug!(
                    "RESOLVE: Not in imports, checking if '{}' is current class",
                    class_name
                );
            }
        }

        // Check if this is the current file's class
        if let Some(current_class) = root.children().find(|n| n.kind() == SyntaxKind::CLASS_DECL) {
            if let Some(current_class_name) = extract_class_name(&current_class) {
                if current_class_name == class_name {
                    debug!("RESOLVE: OK '{}' matches current class", class_name);
                    // Get the package
                    if let Some(package) = extract_package_from_root(root) {
                        return Ok(Type::Class(hyperion_semantics::symbol::ClassType {
                            name: class_name.to_string(),
                            package: Some(package),
                            type_arguments: Vec::new(),
                            is_diamond: false,
                        }));
                    }
                    return Ok(Type::Class(hyperion_semantics::symbol::ClassType {
                        name: class_name.to_string(),
                        package: None,
                        type_arguments: Vec::new(),
                        is_diamond: false,
                    }));
                }
            }
        }

        Err(format!("Could not resolve '{class_name}' as class name"))
    }

    /// Find the FQN of the enclosing type declaration at a given offset.
    pub(crate) fn find_enclosing_class(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: rowan::TextSize,
    ) -> Option<String> {
        let node = root.token_at_offset(offset).right_biased()?;
        let mut current = node.parent();

        while let Some(node) = current {
            if node.kind().is_type_declaration() {
                if let Some(class_name) = extract_class_name(&node) {
                    if let Some(package) = extract_package_from_root(root) {
                        return Some(format!("{package}.{class_name}"));
                    }
                    return Some(class_name);
                }
            }
            current = node.parent();
        }

        None
    }

    /// Return enclosing source type identities nearest-first for exact lexical
    /// type-name resolution. The declaring type itself is excluded while the
    /// cursor is in its header because member types declared in its body are
    /// not in scope there.
    pub(crate) fn lexical_type_owners_at_offset(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: rowan::TextSize,
    ) -> Vec<String> {
        let Some(token) = root.token_at_offset(offset).right_biased() else {
            return Vec::new();
        };
        let type_nodes = token
            .parent_ancestors()
            .filter(|node| node.kind().is_type_declaration())
            .collect::<Vec<_>>();
        if type_nodes.is_empty() {
            return Vec::new();
        }

        let innermost_is_header = type_nodes[0]
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::L_BRACE)
            .is_none_or(|brace| offset < brace.text_range().start());
        let names = type_nodes
            .iter()
            .rev()
            .filter_map(extract_class_name)
            .collect::<Vec<_>>();
        let package = extract_package_from_root(root);
        let mut prefix = package.unwrap_or_default();
        let mut owners = Vec::with_capacity(names.len());
        for name in names {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(&name);
            owners.push(prefix.clone());
        }
        owners.reverse();
        if innermost_is_header {
            owners.remove(0);
        }
        owners
    }

    /// Find the fully qualified name of the enclosing class at a given offset.
    /// Properly handles nested classes by building the complete FQN path.
    ///
    /// For example, in `com.example.Outer.Inner`, if cursor is in Inner,
    /// this returns "com.example.Outer.Inner" (for static nested classes)
    /// or "com.example.Outer$Inner" depending on the context.
    pub(crate) fn find_enclosing_class_fqn_at_offset(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
    ) -> Option<String> {
        use rowan::TextSize;

        // Find the token at the given offset
        let token = root
            .token_at_offset(TextSize::from(offset))
            .right_biased()?;
        let mut current = token.parent();

        // Collect all enclosing type declaration names (innermost first)
        let mut class_names = Vec::new();

        while let Some(node) = current {
            if node.kind().is_type_declaration() {
                if let Some(class_name) = extract_class_name(&node) {
                    class_names.push(class_name);
                }
            }
            current = node.parent();
        }

        if class_names.is_empty() {
            return None;
        }

        // Reverse to get outermost first
        class_names.reverse();

        // Extract package from root
        let package = extract_package_from_root(root);

        // Build the FQN: package.OuterClass.InnerClass
        // Note: Using dot notation for workspace lookup (static nested classes)
        let class_path = class_names.join(".");

        if let Some(pkg) = package {
            Some(format!("{}.{}", pkg, class_path))
        } else {
            Some(class_path)
        }
    }

    /// Find the CONSTRUCTOR_DECL node that contains the given token
    ///
    /// Used in Phase 1 of constructor reference matching to extract parameter count
    /// from the constructor definition.
    pub(crate) fn find_constructor_node_at_position(
        &self,
        _root: &hyperion_parser::syntax::SyntaxNode,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Option<hyperion_parser::syntax::SyntaxNode> {
        use hyperion_parser::SyntaxKind;

        // Walk up from the token to find CONSTRUCTOR_DECL
        let mut current = token.parent();

        while let Some(node) = current {
            if node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
                return Some(node);
            }
            current = node.parent();
        }

        None
    }

    /// Extract parameter types from a constructor definition
    ///
    /// Phase 2 implementation: Builds type signature for constructor definitions
    /// to match with constructor call signatures from usage extraction.
    ///
    /// Returns a comma-separated string of parameter types, e.g., "String,int"
    pub(crate) fn extract_constructor_parameter_types(
        &self,
        constructor_node: &hyperion_parser::syntax::SyntaxNode,
    ) -> String {
        use hyperion_parser::SyntaxKind;

        // Find PARAM_LIST node
        for child in constructor_node.descendants() {
            if child.kind() == SyntaxKind::PARAM_LIST {
                return self.extract_types_from_param_list(&child);
            }
        }

        String::new()
    }

    /// Extract parameter types from a PARAM_LIST node
    fn extract_types_from_param_list(
        &self,
        param_list: &hyperion_parser::syntax::SyntaxNode,
    ) -> String {
        use hyperion_parser::SyntaxKind;

        let mut param_types = Vec::new();

        // Walk through PARAM nodes and extract type information
        for param_node in param_list
            .children()
            .filter(|child| child.kind() == SyntaxKind::PARAM)
        {
            if let Some(param_type) = self.extract_param_type(&param_node) {
                param_types.push(param_type);
            }
        }

        param_types.join(",")
    }

    /// Extract the type from a single PARAM node
    ///
    /// Handles:
    /// - Primitive types: int, long, boolean, etc.
    /// - Class types: String, User, etc.
    /// - Array types: int[], String[][], etc.
    /// - Varargs: String... (treated as String[])
    fn extract_param_type(
        &self,
        param_node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<String> {
        use hyperion_parser::SyntaxKind;

        let mut type_name = None;
        let mut array_dimensions = 0;

        // PARAM structure: TYPE nodes followed by IDENT (parameter name)
        // We want to extract the type and count array dimensions
        for child in param_node.children() {
            match child.kind() {
                SyntaxKind::PRIMITIVE_TYPE => {
                    // Extract primitive type keyword (int, long, etc.)
                    for token in child.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = token {
                            if matches!(
                                t.kind(),
                                SyntaxKind::INT_KW
                                    | SyntaxKind::LONG_KW
                                    | SyntaxKind::FLOAT_KW
                                    | SyntaxKind::DOUBLE_KW
                                    | SyntaxKind::BOOLEAN_KW
                                    | SyntaxKind::CHAR_KW
                                    | SyntaxKind::BYTE_KW
                                    | SyntaxKind::SHORT_KW
                            ) {
                                type_name = Some(t.text().to_string());
                            }
                        }
                    }
                }
                SyntaxKind::CLASS_TYPE => {
                    // Extract class name (simple name, not FQN)
                    // CLASS_TYPE may contain QUALIFIED_NAME or direct IDENTs
                    let mut identifiers = Vec::new();

                    for elem in child.descendants_with_tokens() {
                        if let rowan::NodeOrToken::Token(token) = elem {
                            if token.kind().is_identifier() {
                                identifiers.push(token.text().to_string());
                            }
                        }
                    }

                    if !identifiers.is_empty() {
                        // Use the last identifier (simple name)
                        type_name = Some(identifiers.last().unwrap().clone());
                    }
                }
                SyntaxKind::ARRAY_TYPE => {
                    // Count array dimensions
                    // ARRAY_TYPE nodes may be nested or siblings
                    array_dimensions += 1;
                }
                _ => {}
            }
        }

        // Check for array brackets in tokens (C-style array declaration)
        for token in param_node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(t) = token {
                if t.kind() == SyntaxKind::L_BRACKET {
                    array_dimensions += 1;
                }
            }
        }

        // Build the type string with array dimensions
        type_name.map(|base_type| {
            if array_dimensions > 0 {
                format!("{}{}", base_type, "[]".repeat(array_dimensions))
            } else {
                base_type
            }
        })
    }

    /// Extract the outer class name from a qualified inner class constructor
    /// Pattern: new Outer().new Inner() or new Outer.StaticNested()
    /// Returns the outer class name (e.g., "Outer") if detected
    pub(crate) fn extract_outer_class_from_new_expr(
        &self,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Option<String> {
        use hyperion_parser::SyntaxKind;

        // Walk up to find NEW_EXPR
        let mut current = token.parent();
        let mut new_expr_node = None;

        while let Some(node) = current.clone() {
            if node.kind() == SyntaxKind::NEW_EXPR {
                new_expr_node = Some(node);
                break;
            }
            current = node.parent();
        }

        let new_expr = new_expr_node?;

        // Check for qualified inner class constructor pattern: new Outer().new Inner()
        // Look at preceding tokens in the token stream to find if there's a DOT before this NEW_EXPR
        let new_kw_token = new_expr
            .children_with_tokens()
            .find_map(|child| match child {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::NEW_KW => Some(t),
                _ => None,
            });

        if let Some(new_kw) = new_kw_token {
            debug!("NAV: Found NEW_KW token");
            // Get the token just before the NEW keyword
            if let Some(prev_token) = new_kw.prev_token() {
                debug!(
                    "NAV: prev_token before new: {:?} '{}'",
                    prev_token.kind(),
                    prev_token.text()
                );
                if prev_token.kind() == SyntaxKind::DOT {
                    // Found DOT before NEW - this is a qualified inner class constructor!
                    // Now find the expression before the DOT
                    if let Some(before_dot) = prev_token.prev_token() {
                        // Walk back to find the complete expression before the DOT
                        // This could be a method call (), identifier, etc.
                        let mut expr_tokens = vec![before_dot.clone()];
                        let mut current = before_dot.prev_token();

                        while let Some(tok) = current {
                            // Stop at certain boundaries
                            if matches!(tok.kind(), SyntaxKind::SEMICOLON | SyntaxKind::EQ) {
                                break;
                            }
                            expr_tokens.insert(0, tok.clone());
                            current = tok.prev_token();
                            if expr_tokens.len() > 20 {
                                break; // Safety limit
                            }
                        }

                        // Now extract the outer class name from the tokens
                        // Pattern: ... new Outer() .
                        // We need to find the identifier after 'new'
                        for (i, tok) in expr_tokens.iter().enumerate() {
                            if tok.kind() == SyntaxKind::NEW_KW {
                                // Look for the next identifier
                                for next_tok in &expr_tokens[i + 1..] {
                                    if next_tok.kind() == SyntaxKind::IDENT {
                                        let outer_class_name = next_tok.text().to_string();
                                        debug!(
                                            "NAV: Found qualified inner constructor: {}.{}",
                                            outer_class_name,
                                            token.text()
                                        );
                                        return Some(outer_class_name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Also check if the token itself is part of a QUALIFIED_NAME within NEW_EXPR
        // Pattern: new Outer.Inner() (static nested class)
        // In this case, the QUALIFIED_NAME contains "Outer.Inner"
        if let Some(parent) = token.parent() {
            if parent.kind() == SyntaxKind::QUALIFIED_NAME {
                // Extract all identifiers from the qualified name
                let mut parts = Vec::new();
                for child in parent.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = child {
                        if t.kind() == SyntaxKind::IDENT {
                            parts.push(t.text().to_string());
                        }
                    }
                }

                // If we have at least 2 parts and the last one is our token,
                // then the preceding parts form the outer class
                if parts.len() >= 2 {
                    let token_text = token.text().to_string();
                    if parts.last() == Some(&token_text) {
                        // Return all but the last part (the outer class)
                        let outer = parts[..parts.len() - 1].join(".");
                        debug!(
                            "NAV: Found static nested class constructor: {}.{}",
                            outer, token_text
                        );
                        return Some(outer);
                    }
                }
            }
        }

        None
    }

    /// Check if a token is followed by a method call expression (has parentheses after it)
    /// Returns false for constructor calls (new Foo())
    pub(crate) fn is_method_call_at_token(
        &self,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> bool {
        use hyperion_parser::SyntaxKind;

        // First, check if this token is part of a NEW_EXPR (constructor call)
        // In that case, it's not a method call even if followed by parentheses
        let mut current = token.parent();
        while let Some(node) = current {
            if node.kind() == SyntaxKind::NEW_EXPR {
                // This is a constructor call like `new Foo()` or `new Outer().new Inner()`
                return false;
            }
            // Don't go too far up
            if matches!(
                node.kind(),
                SyntaxKind::EXPR_STMT
                    | SyntaxKind::LOCAL_VAR_DECL
                    | SyntaxKind::BLOCK_STMT
                    | SyntaxKind::METHOD_DECL
            ) {
                break;
            }
            current = node.parent();
        }

        // Check if the next sibling is a METHOD_CALL_EXPR or L_PAREN
        if let Some(parent) = token.parent() {
            let children: Vec<_> = parent.children_with_tokens().collect();

            // Find the token's position
            for (idx, child) in children.iter().enumerate() {
                if let rowan::NodeOrToken::Token(t) = child {
                    if t == token {
                        // Check next sibling
                        if idx + 1 < children.len() {
                            match &children[idx + 1] {
                                rowan::NodeOrToken::Node(n)
                                    if n.kind() == SyntaxKind::METHOD_CALL_EXPR =>
                                {
                                    return true;
                                }
                                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::L_PAREN => {
                                    return true;
                                }
                                _ => {}
                            }
                        }
                        break;
                    }
                }
            }
        }
        false
    }

    /// Look up a value declaration with inheritance support.
    pub(crate) fn find_value_with_inheritance(
        &self,
        class_fqn: &str,
        member_name: &str,
    ) -> Option<hyperion_workspace::SymbolLocation> {
        self.workspace_index
            .find_value_definition_with_inheritance(class_fqn, member_name)
            .map(|definition| definition.location)
    }

    /// Look up a callable declaration with inheritance support. This is a
    /// deliberately descriptorless recovery path: exact call sites use the
    /// overload resolver, while syntax-degraded callers retain deterministic
    /// name lookup within each canonical owner.
    pub(crate) fn find_callable_with_inheritance(
        &self,
        class_fqn: &str,
        member_name: &str,
    ) -> Option<hyperion_workspace::SymbolLocation> {
        let candidates = self
            .workspace_index
            .find_effective_methods_by_name(class_fqn, member_name);
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        self.workspace_index
            .find_symbol_definition(&SymbolTarget::new(
                Arc::clone(&candidate.fqn),
                DeclarationNamespace::Callable,
            ))
            .map(|definition| definition.location)
    }

    /// Resolve a method call using type-based overload resolution.
    ///
    /// This adapter owns syntax-specific argument inference; candidate collection,
    /// inheritance, conversions, and most-specific selection stay in semantics.
    pub(crate) fn resolve_method_with_type_matching(
        &self,
        file_id: FileId,
        class_fqn: &str,
        method_name: &str,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        syntax_root: &hyperion_parser::syntax::SyntaxNode,
        offset: usize,
    ) -> Option<hyperion_workspace::SymbolLocation> {
        // Find ARG_LIST
        let method_call = find_method_call_for_token(token)?;
        let arg_list = method_call
            .children()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)?;

        let arg_types = infer_argument_types(
            &arg_list,
            file_id,
            &self.workspace_index,
            syntax_root,
            offset as u32,
        );

        debug!(
            "NAV: Inferred {} argument types: {:?}",
            arg_types.len(),
            arg_types
                .iter()
                .map(|t| format!("{:?}", t.ty))
                .collect::<Vec<_>>()
        );

        // Preserve the parameterized receiver at the semantic call boundary.
        // Reducing Stream<String> to its owner name erases filter's Predicate
        // contract before the candidate-specific lambda inference can use it.
        let receiver_type = method_call
            .children()
            .find(|node| is_receiver_expression_kind(node.kind()))
            .map(|receiver| {
                let engine = self.create_type_inference_engine(file_id);
                let resolver =
                    ResolverBuilder::with_workspace(file_id, self.workspace_index.clone())
                        .build_from_syntax(syntax_root);
                engine.infer_expression(&receiver, &resolver, None)
            });
        let matched = crate::navigation::overload_resolver::resolve_overload_with_arguments(
            &self.workspace_index,
            class_fqn,
            receiver_type.as_ref(),
            method_name,
            &arg_types,
            &arg_list,
            file_id,
            syntax_root,
        );
        let Some(matched) = matched else {
            return self.kotlin_method_by_arity(class_fqn, method_name, arg_types.len());
        };
        debug!(
            "NAV: Canonical resolution selected {} with score {:?}",
            matched.declaration.fqn, matched.score
        );
        Some(matched.definition.location)
    }

    /// A Java call to a Kotlin function cannot go through Java overload
    /// resolution: the declaration's parameter types are Kotlin-spelled
    /// (`Int`, `List<String>`) and carry no JVM descriptor. When every method
    /// of that name on the receiver is a Kotlin function, the call's argument
    /// count identifies it if exactly one takes that many parameters (Java
    /// callers see no default arguments unless `@JvmOverloads` expanded them,
    /// which the file facade / static forwarders cover separately).
    fn kotlin_method_by_arity(
        &self,
        class_fqn: &str,
        method_name: &str,
        argument_count: usize,
    ) -> Option<hyperion_workspace::SymbolLocation> {
        let candidates = self
            .workspace_index
            .find_methods_by_name_with_inheritance(class_fqn, method_name);
        let is_kotlin = |symbol: &hyperion_workspace::Symbol| {
            symbol.kind == lsp_types::SymbolKind::FUNCTION
                // compiler-generated members of a Kotlin class (`copy`, `componentN`)
                || symbol
                    .container
                    .as_deref()
                    .and_then(|owner| self.workspace_index.find_symbol(owner))
                    .and_then(|location| self.workspace_index.get_file_path(location.file_id))
                    .is_some_and(|path| {
                        matches!(path.extension().and_then(|ext| ext.to_str()), Some("kt" | "kts"))
                    })
        };
        if candidates.is_empty() || candidates.iter().any(|symbol| !is_kotlin(symbol)) {
            return None;
        }
        let mut matching = candidates
            .iter()
            .filter(|symbol| symbol.parameters.len() == argument_count);
        let symbol = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        self.workspace_index
            .find_symbol_definition(&SymbolTarget::for_symbol(symbol))
            .map(|definition| definition.location)
    }
}

const fn is_receiver_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NAME_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::LITERAL_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
    )
}

fn token_has_method_call_ancestor(token: &SyntaxToken<hyperion_parser::JavaLanguage>) -> bool {
    token
        .parent_ancestors()
        .any(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
}

fn extract_ternary_branch_name(node: &SyntaxNode) -> Option<String> {
    let ternary = if node.kind() == SyntaxKind::TERNARY_EXPR {
        Some(node.clone())
    } else {
        node.descendants()
            .find(|child| child.kind() == SyntaxKind::TERNARY_EXPR)
    }?;

    let mut exprs = Vec::new();
    for child in ternary.children() {
        if is_receiver_expression_kind(child.kind()) {
            exprs.push(child);
        }
    }

    let branch = exprs.get(1).or_else(|| exprs.get(2))?;
    extract_first_identifier_in_node(branch)
}

fn extract_first_identifier_in_node(node: &SyntaxNode) -> Option<String> {
    for child in node.descendants_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::db::Language;

    #[test]
    fn external_constructor_types_bind_without_using_attached_source_coordinates() {
        let temp = tempfile::tempdir().unwrap();
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let symbols = hyperion_workspace::symbol_extractor::SymbolExtractor::extract(
            "package fixture; public class Library {}",
        )
        .unwrap()
        .symbols;
        index.add_external_symbols_batch(symbols);
        let host = SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&index),
            Arc::new(QueryEngine::new(index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        for (name, source) in [
            ("Simple", "import fixture.Library; class Simple { Object create() { return new Library(); } }"),
            ("Qualified", "class Qualified { Object create() { return new fixture.Library(); } }"),
        ] {
            let path = temp.path().join(format!("{name}.java"));
            index.index_file_with_source(&path, source).unwrap();
            let file_id = index.get_file_id(&path).unwrap();
            let offset = source.rfind("Library").unwrap();
            let target = provider.resolve_external_type_definition_at_position(
                file_id, Position::new(0, offset as u32), source, index.current_workspace_revision(),
            ).expect("constructor type retains its bytecode declaration identity");
            assert_eq!(target.location.fqn.as_ref(), "fixture.Library");
            assert_eq!(target.source, DefinitionSourceAuthority::External);
        }
    }

    #[test]
    fn retained_scoped_target_survives_pending_refresh_until_replacement_commits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Target.java");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, "package p; class Target {}")
            .unwrap();
        let file_id = index.get_file_id(&path).unwrap();
        let ticket = index.begin_external_source_refresh("gradle:scope-proof");
        let _refresh = index
            .replace_external_sources_for_refresh(&ticket, &[])
            .unwrap();
        assert!(index.publish_source_scopes_for_refresh(
            &ticket,
            vec![hyperion_workspace::SourceScopeInput {
                requester_roots: vec![temp.path().to_path_buf()],
                visible_source_roots: vec![temp.path().to_path_buf()],
                visible_external_archives: vec![],
            }],
        ));
        let scoped = index.scoped_view_for_file(file_id).unwrap();
        let target = ResolvedNavigationTarget {
            request_revision: index.current_file_revision(file_id).unwrap(),
            scope_refresh: scoped.source_scope_refresh_ticket(),
            location: None,
            definition: scoped
                .find_symbol_definition(&SymbolTarget::new("p.Target", DeclarationNamespace::Type))
                .unwrap(),
        };
        let host = SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&index),
            Arc::new(QueryEngine::new((*index).clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        assert!(provider.workspace_target_is_current(&target));
        let revision = index.current_workspace_revision();
        let replacement = index.begin_external_source_refresh("gradle:scope-proof");
        assert_eq!(index.current_workspace_revision(), revision);
        assert!(scoped.source_scope_is_current());
        assert!(provider.workspace_target_is_current(&target));
        let _refresh = index
            .replace_external_sources_for_refresh(&replacement, &[])
            .unwrap();
        assert!(!scoped.source_scope_is_current());
        assert!(!provider.workspace_target_is_current(&target));
    }

    /// Restoring the cached JDK image publishes ~350k symbols in
    /// owner-complete chunks, each advancing the workspace generation. A
    /// resolved answer that never depended on those chunks must survive them,
    /// or every navigation request for the whole restore answers "not found".
    #[test]
    fn resolved_workspace_target_survives_external_symbol_batch_publication() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let path = temp.path().join("Target.java");
        let source = "package p; class Target {}";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("first source publication");
        let file_id = index.get_file_id(&path).expect("indexed file");
        let request_revision = index
            .current_file_revision(file_id)
            .expect("current file revision");
        let definition = index
            .find_symbol_definition(&SymbolTarget::new("p.Target", DeclarationNamespace::Type))
            .expect("type definition");
        let host = SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&index),
            Arc::new(QueryEngine::new(index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let resolved = ResolvedNavigationTarget {
            request_revision,
            scope_refresh: None,
            location: Some(Location::new(
                Url::from_file_path(&path).expect("file URI"),
                Range::default(),
            )),
            definition,
        };

        assert!(provider.workspace_target_is_current(&resolved));
        for chunk in 0..4 {
            let symbols = hyperion_workspace::symbol_extractor::SymbolExtractor::extract(&format!(
                "package jdk{chunk}; public class Cached {{}}"
            ))
            .expect("synthetic external chunk")
            .symbols;
            index.add_external_symbols_batch(symbols);
            assert!(
                provider.workspace_target_is_current(&resolved),
                "external chunk {chunk} must not retire an unrelated answer"
            );
        }
    }

    /// The same invariant end to end: definitions keep resolving while a
    /// background thread publishes external chunks, as the JDK restore does.
    #[test]
    fn definitions_resolve_while_external_symbol_chunks_are_published() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let service_path = temp.path().join("Service.java");
        let client_path = temp.path().join("Client.java");
        let service_source = "package p;\npublic class Service {\n    void run() {}\n}\n";
        let client_source =
            "package p;\npublic class Client {\n    void call(Service service) {\n        service.run();\n    }\n}\n";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&service_path, service_source)
            .expect("service publication");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("client publication");
        let service_id = index.get_file_id(&service_path).expect("indexed service");
        let client_id = index.get_file_id(&client_path).expect("indexed client");
        let host = SemanticHost::new(Arc::clone(&index));
        host.with_db(|db| {
            db.set_file_text(service_id, Arc::new(service_source.to_string()));
            db.set_file_language(service_id, Language::Java);
            db.set_file_text(client_id, Arc::new(client_source.to_string()));
            db.set_file_language(client_id, Language::Java);
        });
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&index),
            Arc::new(QueryEngine::new(index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let uri = Url::from_file_path(&client_path).expect("client URI");
        // `Service` in the parameter list: a workspace type answer that owes
        // nothing to the external symbols arriving underneath it.
        let position = Position::new(2, 14);
        assert!(provider
            .goto_definition(client_id, position, client_source, &uri)
            .expect("navigation succeeds")
            .is_some());

        let publishing = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let publisher = {
            let index = Arc::clone(&index);
            let publishing = Arc::clone(&publishing);
            std::thread::spawn(move || {
                let mut chunk = 0u32;
                while publishing.load(std::sync::atomic::Ordering::Relaxed) {
                    let symbols =
                        hyperion_workspace::symbol_extractor::SymbolExtractor::extract(&format!(
                            "package jdk.c{chunk}; public class Cached {{ void member() {{}} }}"
                        ))
                        .expect("synthetic external chunk")
                        .symbols;
                    index.add_external_symbols_batch(symbols);
                    chunk += 1;
                }
                chunk
            })
        };

        let mut empty = 0;
        for _ in 0..200 {
            if provider
                .goto_definition(client_id, position, client_source, &uri)
                .expect("navigation succeeds")
                .is_none()
            {
                empty += 1;
            }
        }
        publishing.store(false, std::sync::atomic::Ordering::Relaxed);
        let chunks = publisher.join().expect("publisher thread");
        assert!(chunks > 0, "the publisher must have advanced the index");
        assert_eq!(
            empty, 0,
            "concurrent external publication must not empty definition answers"
        );
    }

    #[test]
    fn resolved_workspace_target_expires_after_same_text_republication() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let path = temp.path().join("Target.java");
        let source = "package p; class Target {}";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("first source publication");
        let file_id = index.get_file_id(&path).expect("indexed file");
        let request_revision = index
            .current_file_revision(file_id)
            .expect("current file revision");
        let definition = index
            .find_symbol_definition(&SymbolTarget::new("p.Target", DeclarationNamespace::Type))
            .expect("type definition");
        let host = SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&index),
            Arc::new(QueryEngine::new(index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let resolved = ResolvedNavigationTarget {
            request_revision,
            scope_refresh: None,
            location: Some(Location::new(
                Url::from_file_path(&path).expect("file URI"),
                Range::default(),
            )),
            definition,
        };

        assert!(provider.workspace_target_is_current(&resolved));
        index
            .index_file_with_source(&path, source)
            .expect("same-text second publication");
        assert!(!provider.workspace_target_is_current(&resolved));
    }

    #[test]
    fn override_declaration_identity_is_distinct_from_its_navigation_relationship() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let base_path = temp.path().join("Base.java");
        let child_path = temp.path().join("Child.java");
        let base_source = "package p;\npublic interface Base {\n    void close();\n}\n";
        let child_source = "package p;\npublic interface Child extends Base {\n    @Override\n    default void close() {}\n}\n";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&base_path, base_source)
            .expect("base source publication");
        index
            .index_file_with_source(&child_path, child_source)
            .expect("child source publication");
        let base_id = index.get_file_id(&base_path).expect("indexed base file");
        let child_id = index.get_file_id(&child_path).expect("indexed child file");
        let host = SemanticHost::new(Arc::clone(&index));
        host.with_db(|db| {
            db.set_file_text(base_id, Arc::new(base_source.to_string()));
            db.set_file_language(base_id, Language::Java);
            db.set_file_text(child_id, Arc::new(child_source.to_string()));
            db.set_file_language(child_id, Language::Java);
        });
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&index),
            Arc::new(QueryEngine::new(index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let position = Position::new(3, 18);
        let uri = Url::from_file_path(&child_path).expect("child URI");

        let exact = provider
            .resolve_workspace_target_at_position(
                child_id,
                position,
                child_source,
                &uri,
                Some(DeclarationNamespace::Callable),
            )
            .expect("exact resolution")
            .expect("exact target");
        let related = provider
            .resolve_related_workspace_target_at_position(
                child_id,
                position,
                child_source,
                &uri,
                Some(DeclarationNamespace::Callable),
            )
            .expect("related resolution")
            .expect("related target");

        assert_eq!(exact.definition.target.fqn.as_ref(), "p.Child.close()");
        assert_eq!(related.definition.target.fqn.as_ref(), "p.Base.close()");
    }
}
