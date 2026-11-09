// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hover Information Provider
//!
//! This module implements hover functionality for the Hyperion LSP.
//! It provides information about symbols when the user hovers over them in the editor.

use super::documentation::extract_documentation;
use super::extractors::{
    extract_field_info_from_node, extract_method_signature_from_node,
    extract_parameter_info_from_node, extract_variable_info_from_node,
};
use super::resolution::{
    is_field_name_token, is_method_name_token, is_type_name_token, resolve_identifier_reference,
};
use super::utils::find_token_at_offset;
use crate::navigation::{
    lambda_inference, parse_method_chain, NavigationProvider, ResolvedNavigationTarget,
};
use crate::position_utils::{offset_to_position, position_to_offset};
use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_workspace::{DeclarationNamespace, QueryEngine, WorkspaceIndex};
use rowan::TextSize;
use std::sync::Arc;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Url};
use tracing::{debug, warn};

use crate::semantic_host::ReadTxn;
use crate::source_provider::SourceProviderChain;

/// Provides hover information for symbols.
///
/// Request-scoped: it borrows the request's read transaction.
pub struct HoverProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
    source_providers: Arc<SourceProviderChain>,
    navigation_provider: NavigationProvider<'txn>,
}

impl<'txn> HoverProvider<'txn> {
    /// Creates a new HoverProvider
    pub fn new(
        txn: &'txn ReadTxn,
        workspace_index: Arc<WorkspaceIndex>,
        source_providers: Arc<SourceProviderChain>,
    ) -> Self {
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        Self::with_query_engine(txn, workspace_index, query_engine, source_providers)
    }

    /// Creates a hover provider sharing the server's query engine. Tests may
    /// use [`Self::new`], which constructs an equivalent lightweight handle
    /// over the same clone-shared workspace index.
    pub fn with_query_engine(
        txn: &'txn ReadTxn,
        workspace_index: Arc<WorkspaceIndex>,
        query_engine: Arc<QueryEngine>,
        source_providers: Arc<SourceProviderChain>,
    ) -> Self {
        let navigation_provider = NavigationProvider::new(
            txn,
            Arc::clone(&workspace_index),
            query_engine,
            Arc::clone(&source_providers),
        );
        Self {
            txn,
            workspace_index,
            source_providers,
            navigation_provider,
        }
    }

    /// Creates a Markdown hover response.
    const fn create_hover(&self, content: String) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }
    }

    fn resolve_navigation_target(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<ResolvedNavigationTarget>, String> {
        let chain_target_is_callable = parse_method_chain(token)
            .and_then(|(chain, target)| chain.get(target).map(|segment| segment.is_method_call))
            .unwrap_or(false);
        let method_reference_target = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::METHOD_REF_EXPR)
            .and_then(|reference| {
                reference
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|candidate| candidate.kind().is_identifier())
                    .last()
            })
            .is_some_and(|selector| selector.text_range() == token.text_range());
        let method_declaration_target = token.parent().is_some_and(|parent| {
            parent.kind() == SyntaxKind::METHOD_DECL && is_method_name_token(&parent, token)
        });
        let Some(path) = self.workspace_index.get_file_path(file_id) else {
            return Ok(None);
        };
        let Ok(uri) = Url::from_file_path(path) else {
            return Ok(None);
        };
        self.navigation_provider
            .resolve_workspace_target_at_position(
                file_id,
                position,
                source_text,
                &uri,
                (chain_target_is_callable || method_reference_target || method_declaration_target)
                    .then_some(DeclarationNamespace::Callable),
            )
    }

    /// Get hover information for a position in a document
    ///
    /// # Arguments
    ///
    /// * `file_id` - The FileId of the document
    /// * `position` - The cursor position in the document
    /// * `source_text` - The source text of the document (needed to convert position to offset)
    ///
    /// # Returns
    ///
    /// An Option containing Hover information if a symbol is found at the position
    pub fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        debug!(
            "Computing hover for file {:?} at position {:?}",
            file_id, position
        );

        // One snapshot for the entire hover request: all Salsa queries are
        // memoized within the request's read transaction.
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
            return handler.hover(file_id, position, source_text);
        }

        // Convert LSP position to byte offset
        let offset = match position_to_offset(source_text, position) {
            Ok(offset) => {
                // Debug: show position details
                debug!(
                    "HOVER: Position line={} char={} -> offset={}, source_len={}",
                    position.line,
                    position.character,
                    offset,
                    source_text.len()
                );
                // Show context around the offset
                let start = offset.saturating_sub(20);
                let end = (offset + 20).min(source_text.len());
                if let Some(context) = source_text.get(start..end) {
                    let marker_pos = offset - start;
                    debug!(
                        "HOVER: Context: '{}' (cursor at pos {})",
                        context.replace('\n', "\\n").replace('\t', "\\t"),
                        marker_pos
                    );
                }
                offset
            }
            Err(e) => {
                warn!("Invalid position for hover: {}", e);
                return Ok(None);
            }
        };

        // Use the single snapshot for parsing (no additional lock)
        let parse_tree = snapshot.parse_file(file_id);

        let syntax = parse_tree.syntax();

        // Check if offset is within u32 bounds (parser limitation)
        let offset_u32 = if let Ok(o) = u32::try_from(offset) {
            o
        } else {
            warn!("Hover offset {} exceeds u32::MAX, file too large", offset);
            return Ok(None);
        };

        // Check if cursor is in comment or string - return None early
        if crate::navigation::ast_queries::is_in_comment_or_string_java(&syntax, offset_u32) {
            debug!("HOVER: Cursor in comment/string, skipping hover");
            return Ok(None);
        }

        // Find the token at the offset
        let mut token = find_token_at_offset(&syntax, offset_u32);

        if let Some(ref tok) = token {
            if let Some(local_var_decl) = tok
                .parent_ancestors()
                .find(|node| node.kind() == SyntaxKind::LOCAL_VAR_DECL)
            {
                let offset_size = TextSize::from(offset_u32);
                let mut has_covering_ident = false;
                if let Some(covering_ident) = local_var_decl
                    .descendants_with_tokens()
                    .filter_map(|n| n.into_token())
                    .find(|t| t.kind().is_identifier() && t.text_range().contains(offset_size))
                {
                    has_covering_ident = true;
                    token = Some(covering_ident);
                }

                // Preserve the exact identifier under cursor (e.g. lambda params) instead of
                // force-jumping to a later method call token in the same local declaration.
                if !has_covering_ident {
                    if let Some(method_call) = local_var_decl.descendants().find(|node| {
                        node.kind() == SyntaxKind::METHOD_CALL_EXPR
                            && node.text_range().contains(TextSize::from(offset_u32))
                    }) {
                        token = find_token_at_offset(&method_call, offset_u32);
                    } else {
                        let is_method_call_token =
                            |t: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>| {
                                if let Some(parent) = t.parent() {
                                    if parent.kind() == SyntaxKind::METHOD_CALL_EXPR {
                                        return true;
                                    }
                                    if let Some(grandparent) = parent.parent() {
                                        return grandparent
                                            .children()
                                            .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR);
                                    }
                                }
                                false
                            };

                        if let Some(next_ident) = local_var_decl
                            .descendants_with_tokens()
                            .filter_map(|n| n.into_token())
                            .filter(|t| t.kind().is_identifier())
                            .find(|t| {
                                t.text_range().start() >= offset_size && is_method_call_token(t)
                            })
                        {
                            token = Some(next_ident);
                        }
                    }
                }
            }
        }

        if let Some(token) = token {
            let token_text = token.text();
            let kind = token.kind();

            debug!(
                "Found token at offset {}: {:?} = '{}' (token range: {:?})",
                offset,
                kind,
                token_text,
                token.text_range()
            );

            // First check if we're hovering on a declaration NAME specifically
            // Only show declaration info if hovering on the actual declaration identifier
            use hyperion_parser::SyntaxKind;

            // Check if this token is the name of a declaration
            if kind == SyntaxKind::IDENT {
                if let Some(parent) = token.parent() {
                    // Check if parent is a declaration node and this is its name
                    let is_declaration_name = match parent.kind() {
                        SyntaxKind::METHOD_DECL => {
                            // For methods, the name is the first IDENT after the return type
                            is_method_name_token(&parent, &token)
                        }
                        SyntaxKind::FIELD_DECL => {
                            // For fields, check if this is a field name
                            is_field_name_token(&parent, &token)
                        }
                        kind if kind.is_type_declaration() => {
                            // For class/interface/enum/etc, name is first IDENT after keyword
                            is_type_name_token(&parent, &token)
                        }
                        _ => false,
                    };

                    if is_declaration_name {
                        if let Some(mut context_info) =
                            self.infer_from_context_with_node(&parent, source_text)
                        {
                            if parent.kind() == SyntaxKind::METHOD_DECL
                                && NavigationProvider::method_decl_has_override_annotation(&parent)
                            {
                                let related_target = self
                                    .workspace_index
                                    .get_file_path(file_id)
                                    .and_then(|path| Url::from_file_path(path).ok())
                                    .map(|uri| {
                                        self.navigation_provider
                                            .resolve_related_workspace_target_at_position(
                                                file_id,
                                                position,
                                                source_text,
                                                &uri,
                                                Some(DeclarationNamespace::Callable),
                                            )
                                    })
                                    .transpose()?
                                    .flatten();
                                if let Some(target) = related_target.filter(|target| {
                                    target.definition.target.namespace
                                        == DeclarationNamespace::Callable
                                        && self
                                            .navigation_provider
                                            .workspace_target_is_current(target)
                                }) {
                                    let token_range = token.text_range();
                                    let target_range = target.definition.location.range;
                                    let resolves_to_self = target.definition.location.file_id
                                        == file_id
                                        && target_range.start == u32::from(token_range.start())
                                        && target_range.end == u32::from(token_range.end());
                                    if !resolves_to_self {
                                        context_info.push_str(&format!(
                                            "\n\n**Overrides:** `{}`",
                                            target.definition.target.fqn
                                        ));
                                    }
                                }
                            }
                            let hover = self.create_hover(context_info);
                            debug!("Returning declaration context information");
                            return Ok(Some(hover));
                        }
                    }
                }
            }

            let is_lambda_parameter =
                lambda_inference::identifier_is_lambda_parameter_reference(&token);
            let is_type_position = token
                .parent_ancestors()
                .any(|node| node.kind() == SyntaxKind::CLASS_TYPE);
            let is_lexical_type_parameter = is_type_position
                && token.parent().is_some_and(|parent| {
                    hyperion_types::enclosing_type_parameter_names(&parent)
                        .iter()
                        .any(|name| name == token.text())
                });
            let file_semantics = kind
                .is_identifier()
                .then(|| snapshot.file_semantics(file_id));
            let has_lexical_semantic_identity = file_semantics
                .as_ref()
                .and_then(|semantics| semantics.resolve_at(token.text_range()))
                .is_some_and(|resolved| {
                    matches!(
                        resolved,
                        hyperion_semantics::resolved_name::ResolvedName::LocalVariable { .. }
                            | hyperion_semantics::resolved_name::ResolvedName::TypeParameter { .. }
                    )
                })
                || is_lexical_type_parameter;
            let canonical_navigation_target =
                if kind.is_identifier() && !is_lambda_parameter && !has_lexical_semantic_identity {
                    self.resolve_navigation_target(file_id, position, source_text, &token)?
                } else {
                    None
                };
            let canonical_callable_target = canonical_navigation_target.as_ref().filter(|target| {
                matches!(
                    target.definition.target.namespace,
                    DeclarationNamespace::Callable | DeclarationNamespace::Constructor
                )
            });
            let rejected_workspace_type = canonical_navigation_target.is_none()
                && file_semantics
                    .as_ref()
                    .and_then(|semantics| semantics.resolve_at(token.text_range()))
                    .and_then(|resolved| match resolved {
                        hyperion_semantics::resolved_name::ResolvedName::Type { fqn, .. } => {
                            self.workspace_index.find_symbol(fqn)
                        }
                        _ => None,
                    })
                    .is_some_and(|location| {
                        !self.workspace_index.is_external_file(location.file_id)
                    });
            let rejected_semantic_binding = file_semantics
                .as_ref()
                .and_then(|semantics| semantics.resolve_at(token.text_range()))
                .is_some_and(|resolved| {
                    matches!(
                        resolved,
                        hyperion_semantics::resolved_name::ResolvedName::Rejected(_)
                    )
                });
            if rejected_semantic_binding {
                debug!(
                    "HOVER: FileSemantics authoritatively rejected '{}'",
                    token.text()
                );
                return Ok(None);
            }
            if rejected_workspace_type {
                debug!(
                    "HOVER: canonical navigation rejected cached workspace type binding for '{}'",
                    token.text()
                );
                return Ok(None);
            }
            let is_annotation_name = token.parent().is_some_and(|parent| {
                parent.kind() == SyntaxKind::QUALIFIED_NAME
                    && parent
                        .parent()
                        .is_some_and(|node| node.kind() == SyntaxKind::ANNOTATION)
            });
            if canonical_navigation_target.is_none()
                && !has_lexical_semantic_identity
                && (is_type_position || is_annotation_name)
            {
                debug!(
                    "HOVER: canonical type resolution rejected '{}'",
                    token.text()
                );
                return Ok(None);
            }

            // Workspace types and values already carry complete, revision-bound
            // declaration metadata. Publish that exact identity immediately rather
            // than consulting any presentation fallback keyed by simple name.
            if let Some(target) = canonical_navigation_target.as_ref().filter(|target| {
                matches!(
                    target.definition.target.namespace,
                    DeclarationNamespace::Type | DeclarationNamespace::Value
                )
            }) {
                if !self.navigation_provider.workspace_target_is_current(target) {
                    return Ok(None);
                }
                let mut hover_text =
                    super::resolution::format_workspace_definition_hover(&target.definition);
                if token
                    .parent_ancestors()
                    .any(|ancestor| ancestor.kind() == SyntaxKind::IMPORT_DECL)
                {
                    hover_text.push_str("\n\n**Import statement**");
                }
                return Ok(Some(self.create_hover(hover_text)));
            }

            // Primary resolution path: FileSemantics (Salsa-cached name resolution)
            // Handles types, methods, fields, enum constants, locals, type params, externals
            if let Some(file_semantics) = file_semantics.as_deref() {
                if let Some(hover_text) = self.resolve_hover_via_file_semantics(
                    &token,
                    snapshot,
                    file_semantics,
                    canonical_callable_target,
                ) {
                    if canonical_callable_target.is_some_and(|target| {
                        !self.navigation_provider.workspace_target_is_current(target)
                    }) {
                        return Ok(None);
                    }
                    let hover = self.create_hover(hover_text);
                    debug!("HOVER: Resolved via FileSemantics");
                    return Ok(Some(hover));
                }
            }

            // The semantic graph does not yet model every type-parameter use
            // in declaration headers. Lexical scope is nevertheless exact:
            // enclosing declarations and static boundaries uniquely identify
            // this spelling without consulting workspace names.
            if is_lexical_type_parameter {
                let hover_text = format!("```java\n{}\n```\n\n**Type parameter**", token.text());
                return Ok(Some(self.create_hover(hover_text)));
            }

            // Fallback: lambda parameter type inference can be expensive on large files,
            // so run it only if the fast FileSemantics path didn't resolve this token.
            if kind == SyntaxKind::IDENT {
                let param_name = token.text();

                // Try to infer lambda parameter type (with workspace lookup for custom methods)
                if let Some(inferred_type) =
                    lambda_inference::infer_lambda_parameter_type_with_workspace_and_file(
                        &syntax,
                        param_name,
                        offset_u32,
                        Some(self.workspace_index.clone()),
                        file_id,
                    )
                {
                    debug!("HOVER: Inferred lambda parameter type: {:?}", inferred_type);

                    // Format the type information
                    let type_str = format!("{inferred_type}");

                    let hover_text = format!(
                        "```java\n{type_str} {param_name}\n```\n\n**Lambda parameter** (inferred type)"
                    );

                    let hover = self.create_hover(hover_text);
                    debug!("HOVER: Returning lambda parameter type information");
                    return Ok(Some(hover));
                }
            }

            // Try to resolve identifier references (parameters, local vars, fields, method calls)
            // Allow contextual keywords (record, var, yield) as identifiers
            if kind.is_identifier() {
                // Check for Lombok-generated fields BEFORE normal identifier resolution
                // This ensures we provide hover info for fields that don't exist in the AST
                let identifier = token.text();

                // Start from token's parent to find enclosing class
                if let Some(token_parent) = token.parent() {
                    if let Some(enclosing_class) =
                        crate::navigation::ast_queries::find_enclosing_class(&token_parent)
                    {
                        let lombok_registry = super::lombok::LombokRegistry::new();
                        if let Some(lombok_field) =
                            lombok_registry.resolve_lombok_field(identifier, &enclosing_class)
                        {
                            debug!(
                                "Resolved '{}' as Lombok-generated field: {}",
                                identifier, lombok_field.type_fqn
                            );

                            // Generate hover for Lombok field
                            let static_modifier = if lombok_field.is_static {
                                "static "
                            } else {
                                ""
                            };

                            let hover_text = format!(
                                "```java\n{}{} {}\n```\n\n{}",
                                static_modifier,
                                lombok_field.type_fqn,
                                lombok_field.name,
                                lombok_field.documentation
                            );

                            let hover = self.create_hover(hover_text);
                            debug!("Returning Lombok field information");
                            return Ok(Some(hover));
                        }
                    }
                }

                if let Some(resolved_info) = resolve_identifier_reference(
                    &token,
                    &syntax,
                    source_text,
                    file_id,
                    offset_u32,
                    &self.workspace_index,
                    canonical_callable_target.map(|target| &target.definition),
                ) {
                    if canonical_callable_target.is_some_and(|target| {
                        !self.navigation_provider.workspace_target_is_current(target)
                    }) {
                        return Ok(None);
                    }
                    let hover = self.create_hover(resolved_info);
                    debug!("Returning resolved identifier information");
                    return Ok(Some(hover));
                }

                // Special check for import statements - must be done early to avoid fallback wrapping
                if let Some(parent) = token.parent() {
                    if parent.kind() == SyntaxKind::QUALIFIED_NAME {
                        if let Some(grandparent) = parent.parent() {
                            if grandparent.kind() == SyntaxKind::IMPORT_DECL {
                                // This is an identifier in an import statement
                                // Use infer_from_context which will handle the lookup
                                if let Some(import_info) =
                                    self.infer_from_context(&token, &syntax, source_text)
                                {
                                    let hover = self.create_hover(import_info);
                                    debug!("Returning import statement information");
                                    return Ok(Some(hover));
                                }
                            }
                        }
                    }
                }
            }

            // Unresolved identifiers receive syntax-only context. Never attach
            // metadata from another declaration merely because its name matches.
            let context_info = self.infer_from_context(&token, &syntax, source_text);
            let kind_label = match kind {
                kind if kind.is_identifier() => "Identifier",
                SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => "String literal",
                SyntaxKind::INT_LITERAL | SyntaxKind::LONG_LITERAL => "Integer literal",
                SyntaxKind::FLOAT_LITERAL | SyntaxKind::DOUBLE_LITERAL => "Float literal",
                SyntaxKind::CHAR_LITERAL => "Character literal",
                SyntaxKind::BOOLEAN_LITERAL => "Boolean literal",
                SyntaxKind::NULL_LITERAL => "null",
                _ => "Symbol",
            };
            let basic_info = if let Some(ctx) = context_info {
                format!("**{kind_label}**\n\n{ctx}\n\n`{token_text}`")
            } else {
                format!("**{kind_label}**\n\n`{token_text}`")
            };

            let hover = self.create_hover(basic_info);

            debug!("Returning basic hover information");
            Ok(Some(hover))
        } else {
            debug!("No token found at offset {}", offset);
            Ok(None)
        }
    }

    /// Try to resolve hover information using Salsa-cached FileSemantics.
    ///
    /// Returns formatted hover text if the token resolves to a known symbol.
    /// Returns None to fall through to existing hover logic for cases
    /// FileSemantics doesn't cover (method chains, Lombok, Javadoc).
    fn resolve_hover_via_file_semantics(
        &self,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        snapshot: &salsa::Snapshot<Database>,
        file_sem: &hyperion_semantics::file_semantics::FileSemantics,
        canonical_callable: Option<&ResolvedNavigationTarget>,
    ) -> Option<String> {
        use hyperion_semantics::resolved_name::ResolvedName;

        let resolved = match file_sem.resolve_at(token.text_range()) {
            Some(r) => r,
            None => {
                // No name resolution.
                // For method-name identifiers, keep walking normal hover fallbacks so we can
                // still show method signatures instead of only return-type summaries.
                if token.kind().is_identifier()
                    && token
                        .parent_ancestors()
                        .any(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
                {
                    return None;
                }

                // For non-method contexts, expression-type hover is a useful fallback.
                return self.hover_from_expression_type(file_sem, token);
            }
        };

        // Sanity check: if the resolved name doesn't match the token text,
        // the semantic walker mapped the wrong range. Fall through to existing logic.
        let token_text = token.text();

        // FileSemantics can sometimes classify unresolved enum-like constants as
        // external types (e.g. `UNKNOWN` -> `com.example.UNKNOWN`). Do not publish
        // that spelling as an identity. Indexed enum constants have already been
        // handled by the canonical navigation target above.
        if let ResolvedName::External { .. } = resolved {
            let enum_context = token.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    SyntaxKind::NAME_EXPR
                        | SyntaxKind::CASE_CLAUSE
                        | SyntaxKind::LOCAL_VAR_DECL
                        | SyntaxKind::FIELD_ACCESS_EXPR
                )
            }) || token
                .parent_ancestors()
                .any(|node| node.kind() == SyntaxKind::FIELD_ACCESS_EXPR);
            let enum_like_name = !token_text.is_empty()
                && token_text
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
            if enum_context && enum_like_name {
                debug!(
                    "HOVER: Deferring external FileSemantics result for enum-like token '{}'",
                    token_text
                );
                return None;
            }
        }

        let name_matches = match resolved {
            ResolvedName::Type { fqn, .. } => {
                fqn.ends_with(token_text) || fqn.rsplit('.').next() == Some(token_text)
            }
            ResolvedName::Method { name, .. } => name == token_text,
            ResolvedName::Field { name, .. } => name == token_text,
            ResolvedName::EnumConstant { name, .. } => name == token_text,
            ResolvedName::LocalVariable { name, .. } => name == token_text,
            ResolvedName::TypeParameter { name, .. } => name == token_text,
            ResolvedName::External { fqn, .. } => {
                fqn.ends_with(token_text) || fqn.rsplit('.').next() == Some(token_text)
            }
            ResolvedName::Package { .. } => false,
            ResolvedName::Rejected(_) => false,
        };
        if !name_matches {
            debug!(
                "HOVER: FileSemantics name mismatch for token '{}', falling through",
                token_text
            );
            return None;
        }

        match resolved {
            ResolvedName::Type { fqn, .. } => {
                let simple_name = fqn.rsplit('.').next().unwrap_or(fqn);
                let mut result = format!("```java\n{simple_name}\n```\n\n");

                // FileSemantics supplied an FQN. Only exact metadata is safe here;
                // a same-simple-name symbol belongs to a different identity.
                let location = self.workspace_index.find_symbol(fqn);

                if let Some(loc) = location.as_ref() {
                    if let Some(file_path) = self.workspace_index.get_file_path(loc.file_id) {
                        let path_str = file_path.to_string_lossy();
                        if let Some(validated) = self
                            .workspace_index
                            .get_validated_file_contents(loc.file_id)
                        {
                            let offset: usize = loc.range.start as usize;
                            let position = crate::position_utils::offset_to_position(
                                &validated.content,
                                offset,
                            );
                            let line_number = position.line + 1;
                            result.push_str(&format!(
                                "**{}** defined in `{path_str}:{line_number}`",
                                Self::format_symbol_kind(loc.kind)
                            ));
                        } else {
                            result.push_str(&format!(
                                "**{}** defined in `{path_str}`",
                                Self::format_symbol_kind(loc.kind)
                            ));
                        }
                    } else if self.workspace_index.is_external_file(loc.file_id) {
                        let source_desc = self.format_external_source(loc.file_id);
                        result.push_str(&format!(
                            "**{}** defined in {source_desc}",
                            Self::format_symbol_kind(loc.kind)
                        ));
                    }
                } else {
                    result.push_str(&format!("`{fqn}`"));
                }
                Some(result)
            }
            ResolvedName::Method {
                owner_fqn,
                name,
                return_type,
                modifier_prefix,
                ..
            } => {
                if let Some(target) = canonical_callable {
                    return Some(super::resolution::format_workspace_definition_hover(
                        &target.definition,
                    ));
                }
                let return_str = format!("{return_type}");
                let owner_simple = owner_fqn.rsplit('.').next().unwrap_or(owner_fqn);
                Some(format!(
                    "```java\n{modifier_prefix}{return_str} {name}()\n```\n\n**Method** in `{owner_simple}`"
                ))
            }
            ResolvedName::Field {
                owner_fqn,
                name,
                field_type,
                modifier_prefix,
                ..
            } => {
                let type_str = format!("{field_type}");
                let owner_simple = owner_fqn.rsplit('.').next().unwrap_or(owner_fqn);
                Some(format!(
                    "```java\n{modifier_prefix}{type_str} {name}\n```\n\n**Field** in `{owner_simple}`"
                ))
            }
            ResolvedName::EnumConstant { enum_fqn, name, .. } => {
                let enum_simple = enum_fqn.rsplit('.').next().unwrap_or(enum_fqn);
                Some(format!(
                    "```java\n{enum_simple}.{name}\n```\n\n**enum constant**"
                ))
            }
            ResolvedName::LocalVariable {
                name,
                var_type,
                is_parameter,
                location,
                ..
            } => {
                let type_str = format!("{var_type}");
                if !*is_parameter
                    && location
                        .as_ref()
                        .is_some_and(|loc| self.is_pattern_variable_location(snapshot, loc, name))
                {
                    return Some(format!(
                        "```java\nfinal {type_str} {name}\n```\n\n**Pattern variable**"
                    ));
                }

                let label = if *is_parameter {
                    "Parameter"
                } else {
                    "Local variable"
                };
                Some(format!("```java\n{type_str} {name}\n```\n\n**{label}**"))
            }
            ResolvedName::TypeParameter { name, bounds } => {
                if bounds.is_empty() {
                    Some(format!("```java\n{name}\n```\n\n**Type parameter**"))
                } else {
                    let bounds_str: Vec<String> = bounds.iter().map(|b| format!("{b}")).collect();
                    Some(format!(
                        "```java\n{name} extends {}\n```\n\n**Type parameter**",
                        bounds_str.join(" & ")
                    ))
                }
            }
            ResolvedName::External { fqn } => {
                let simple_name = fqn.rsplit('.').next().unwrap_or(fqn);
                Some(format!(
                    "```java\n{simple_name}\n```\n\n`{fqn}` (external type)"
                ))
            }
            ResolvedName::Package { .. } => None,
            ResolvedName::Rejected(_) => None,
        }
    }

    fn is_pattern_variable_location(
        &self,
        snapshot: &salsa::Snapshot<Database>,
        location: &hyperion_semantics::symbol::SourceLocation,
        name: &str,
    ) -> bool {
        let parse_tree = snapshot.parse_file(location.file_id);
        let syntax = parse_tree.syntax();
        let Some(token) = find_token_at_offset(&syntax, location.range.start) else {
            return false;
        };

        token.kind().is_identifier()
            && token.text() == name
            && token.parent_ancestors().any(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::TYPE_PATTERN
                        | SyntaxKind::RECORD_PATTERN
                        | SyntaxKind::PATTERN_INSTANCEOF_EXPR
                )
            })
    }

    /// Try to produce hover text from cached expression types in FileSemantics.
    ///
    /// Walks up from the token to find a parent expression node (METHOD_CALL_EXPR,
    /// FIELD_ACCESS_EXPR, NEW_EXPR, CAST_EXPR) and returns the inferred return type.
    fn hover_from_expression_type(
        &self,
        file_sem: &hyperion_semantics::file_semantics::FileSemantics,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Option<String> {
        use hyperion_parser::SyntaxKind;

        let node = token.parent()?;

        // Walk up to find the nearest expression node that has a cached type
        let mut current = node;
        loop {
            match current.kind() {
                SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::CAST_EXPR => {
                    if let Some(ty) = file_sem.expression_type_at(current.text_range()) {
                        let type_str = format!("{ty}");
                        let kind_label = match current.kind() {
                            SyntaxKind::METHOD_CALL_EXPR => "Method call returns",
                            SyntaxKind::FIELD_ACCESS_EXPR => "Field access type",
                            SyntaxKind::NEW_EXPR => "Constructor creates",
                            SyntaxKind::CAST_EXPR => "Cast to",
                            _ => "Expression type",
                        };
                        return Some(format!("```java\n{type_str}\n```\n\n**{kind_label}**"));
                    }
                    break;
                }
                _ => {}
            }
            current = current.parent()?;
        }
        None
    }

    /// Infer information from a declaration node directly
    fn infer_from_context_with_node(&self, node: &SyntaxNode, source_text: &str) -> Option<String> {
        use hyperion_parser::SyntaxKind;

        match node.kind() {
            SyntaxKind::CLASS_DECL => {
                // Find the class name from the node
                let name = node.children_with_tokens().find_map(|child| {
                    if let rowan::NodeOrToken::Token(token) = child {
                        if token.kind() == SyntaxKind::IDENT {
                            return Some(token.text().to_string());
                        }
                    }
                    None
                })?;

                let mut result = String::new();
                if let Some(doc) = extract_documentation(node) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }
                result.push_str(&format!("```java\nclass {name}\n```"));
                Some(result)
            }
            SyntaxKind::INTERFACE_DECL => {
                // Find the interface name
                let name = node.children_with_tokens().find_map(|child| {
                    if let rowan::NodeOrToken::Token(token) = child {
                        if token.kind() == SyntaxKind::IDENT {
                            return Some(token.text().to_string());
                        }
                    }
                    None
                })?;

                let mut result = String::new();
                if let Some(doc) = extract_documentation(node) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }
                result.push_str(&format!("```java\ninterface {name}\n```"));
                Some(result)
            }
            SyntaxKind::METHOD_DECL => {
                let mut result = String::new();

                if let Some(doc) = extract_documentation(node) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }

                if let Some(signature) = extract_method_signature_from_node(node) {
                    let offset: usize = node.text_range().start().into();
                    let position = offset_to_position(source_text, offset);
                    let line_number = position.line + 1;
                    result.push_str(&format!("```java\n{signature}\n```\n\n"));

                    result.push_str(&format!("**Method declaration** (line {line_number})"));
                } else {
                    result.push_str("**Method declaration**");
                }

                Some(result)
            }
            SyntaxKind::FIELD_DECL => {
                let mut result = String::new();

                if let Some(doc) = extract_documentation(node) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }

                if let Some(field_info) = extract_field_info_from_node(node) {
                    result.push_str(&format!(
                        "```java\n{field_info}\n```\n\n**Field declaration**"
                    ));
                } else {
                    result.push_str("**Field declaration**");
                }
                if let Some(relationship) = super::jpa::relationship_markdown(node) {
                    result.push_str("\n\n");
                    result.push_str(&relationship);
                }

                Some(result)
            }
            _ => None,
        }
    }

    /// Infer information about a token from its syntactic context
    fn infer_from_context(
        &self,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        _syntax: &SyntaxNode,
        source_text: &str,
    ) -> Option<String> {
        use hyperion_parser::SyntaxKind;

        // Check the parent node to understand context
        let parent = token.parent()?;
        let parent_kind = parent.kind();

        if parent_kind == SyntaxKind::QUALIFIED_NAME
            && parent
                .parent()
                .is_some_and(|ancestor| ancestor.kind() == SyntaxKind::IMPORT_DECL)
        {
            let type_name = token.text();
            return Some(format!(
                "```java\n{type_name}\n```\n\n**Import statement** (not found in workspace)"
            ));
        }

        match parent_kind {
            SyntaxKind::CLASS_DECL => {
                let name = token.text();
                let mut result = String::new();

                // Add documentation if available
                if let Some(doc) = extract_documentation(&parent) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }

                result.push_str(&format!("```java\nclass {name}\n```"));
                Some(result)
            }
            SyntaxKind::INTERFACE_DECL => {
                let name = token.text();
                let mut result = String::new();

                // Add documentation if available
                if let Some(doc) = extract_documentation(&parent) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }

                result.push_str(&format!("```java\ninterface {name}\n```"));
                Some(result)
            }
            SyntaxKind::METHOD_DECL => {
                let mut result = String::new();

                // Add documentation if available
                if let Some(doc) = extract_documentation(&parent) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }

                // Extract method signature
                if let Some(signature) = extract_method_signature_from_node(&parent) {
                    // Calculate line number from node's text range
                    let offset: usize = parent.text_range().start().into();
                    let position = offset_to_position(source_text, offset);
                    let line_number = position.line + 1; // LSP is 0-indexed, display as 1-indexed

                    result.push_str(&format!(
                        "```java\n{signature}\n```\n\n**Method declaration** (line {line_number})"
                    ));
                } else {
                    result.push_str("**Method declaration**");
                }

                Some(result)
            }
            SyntaxKind::FIELD_DECL => {
                let mut result = String::new();

                // Add documentation if available
                if let Some(doc) = extract_documentation(&parent) {
                    result.push_str(&doc);
                    result.push_str("\n\n");
                }

                // Extract field type and name
                if let Some(field_info) = extract_field_info_from_node(&parent) {
                    result.push_str(&format!(
                        "```java\n{field_info}\n```\n\n**Field declaration**"
                    ));
                } else {
                    result.push_str("**Field declaration**");
                }
                if let Some(relationship) = super::jpa::relationship_markdown(&parent) {
                    result.push_str("\n\n");
                    result.push_str(&relationship);
                }

                Some(result)
            }
            SyntaxKind::LOCAL_VAR_DECL => {
                // Extract variable type and name
                if let Some(var_info) = extract_variable_info_from_node(&parent) {
                    Some(format!("```java\n{var_info}\n```\n\n**Local variable**"))
                } else {
                    Some("**Local variable**".to_string())
                }
            }
            SyntaxKind::PARAM => {
                // Extract parameter type and name
                if let Some(param_info) = extract_parameter_info_from_node(&parent) {
                    Some(format!("```java\n{param_info}\n```\n\n**Parameter**"))
                } else {
                    Some("**Parameter**".to_string())
                }
            }
            SyntaxKind::METHOD_CALL_EXPR => Some("**Method call**".to_string()),
            SyntaxKind::FIELD_ACCESS_EXPR => Some("**Field access**".to_string()),
            SyntaxKind::CLASS_TYPE => {
                let type_name = token.text();
                Some(format!("```java\n{type_name}\n```\n\n**Type reference**"))
            }
            // Note: IMPORT_DECL case removed - it's handled above in the QUALIFIED_NAME check
            // since the parent of an IDENT in an import is always QUALIFIED_NAME, not IMPORT_DECL directly
            _ => None,
        }
    }

    /// Format an external source name for display in hover tooltips.
    fn format_external_source(&self, file_id: FileId) -> String {
        let Some(identity) = self.workspace_index.get_external_source_identity(file_id) else {
            return "external library".to_string();
        };
        if identity.virtual_name() == Some("jdk") {
            return "**JDK standard library**".to_string();
        }
        if let Some(name) = identity.virtual_name() {
            return format!("`{name}`");
        }
        let Some(path) = self.workspace_index.get_external_source_path(file_id) else {
            return "external library".to_string();
        };
        let file_name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        format!("`{file_name}`")
    }

    /// Format LSP SymbolKind for user-friendly display
    const fn format_symbol_kind(kind: tower_lsp::lsp_types::SymbolKind) -> &'static str {
        use tower_lsp::lsp_types::SymbolKind;
        match kind {
            SymbolKind::FILE => "File",
            SymbolKind::MODULE => "Module",
            SymbolKind::NAMESPACE => "Namespace",
            SymbolKind::PACKAGE => "Package",
            SymbolKind::CLASS => "Class",
            SymbolKind::METHOD => "Method",
            SymbolKind::PROPERTY => "Property",
            SymbolKind::FIELD => "Field",
            SymbolKind::CONSTRUCTOR => "Constructor",
            SymbolKind::ENUM => "Enum",
            SymbolKind::INTERFACE => "Interface",
            SymbolKind::FUNCTION => "Function",
            SymbolKind::VARIABLE => "Variable",
            SymbolKind::CONSTANT => "Constant",
            SymbolKind::STRING => "String",
            SymbolKind::NUMBER => "Number",
            SymbolKind::BOOLEAN => "Boolean",
            SymbolKind::ARRAY => "Array",
            SymbolKind::OBJECT => "Object",
            SymbolKind::KEY => "Key",
            SymbolKind::NULL => "Null",
            SymbolKind::ENUM_MEMBER => "Enum Member",
            SymbolKind::STRUCT => "Struct",
            SymbolKind::EVENT => "Event",
            SymbolKind::OPERATOR => "Operator",
            SymbolKind::TYPE_PARAMETER => "Type Parameter",
            _ => "Symbol",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::db::{Database, FileId, Language, SourceDatabase};

    #[test]
    fn test_hover_empty_file() {
        let mut db = Database::new();
        let file_id = FileId::new(0);
        db.set_file_text(file_id, Arc::new(String::new()));
        db.set_file_language(file_id, Language::Java);

        let workspace_index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            std::path::PathBuf::from("/tmp"),
        ));
        let host =
            crate::semantic_host::SemanticHost::with_database(db, Arc::clone(&workspace_index));
        let txn = host.read_now();
        let source_providers = Arc::new(crate::source_provider::SourceProviderChain::new());
        let provider = HoverProvider::new(&txn, workspace_index, source_providers);

        let result = provider.hover(file_id, Position::new(0, 0), "");

        // Should not crash, might return None
        assert!(result.is_ok());
    }

    #[test]
    fn test_hover_simple_class() {
        let mut db = Database::new();
        let file_id = FileId::new(0);
        let source = "class Foo { }";
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);

        let workspace_index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            std::path::PathBuf::from("/tmp"),
        ));
        let host =
            crate::semantic_host::SemanticHost::with_database(db, Arc::clone(&workspace_index));
        let txn = host.read_now();
        let source_providers = Arc::new(crate::source_provider::SourceProviderChain::new());
        let provider = HoverProvider::new(&txn, workspace_index, source_providers);

        // Hover over "class" keyword
        let result = provider.hover(file_id, Position::new(0, 2), source);
        assert!(result.is_ok());

        // Hover over "Foo" identifier
        let result = provider.hover(file_id, Position::new(0, 6), source);
        assert!(result.is_ok());

        if let Ok(Some(hover)) = result {
            // Should have some content
            if let HoverContents::Markup(markup) = hover.contents {
                assert!(markup.value.contains("Foo"));
            }
        }
    }
}
