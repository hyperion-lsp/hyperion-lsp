// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Workspace Resolution
//!
//! Handles cross-file and same-file symbol resolution via workspace index

use crate::navigation::call_site_analysis::{count_arguments, find_method_call_for_token};
use crate::navigation::context::NavigationContext;
use crate::navigation::import_resolver::{extract_class_name, extract_package_from_root};
use crate::navigation_util::{find_scope_at_position, offset_to_position};
use hyperion_parser::SyntaxKind;
use hyperion_semantics::db::SemanticDatabase;
use hyperion_semantics::resolved_name::ResolvedName;
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::symbol::Type;
use hyperion_workspace::DeclarationNamespace;
use rowan::{SyntaxNode, SyntaxToken, TextSize};
use tower_lsp::lsp_types::{Location, Range, Url};
use tracing::debug;

use super::NavigationProvider;

/// Whether cached file semantics resolved the cursor, could not speak to it,
/// or authoritatively rejected a fully described call.
///
/// `RejectedCall` must remain distinct from `Unresolved`: continuing with a
/// name-only resolver after overload applicability or specificity failed would
/// fabricate a navigation target.
pub(crate) enum FileSemanticsNavigation {
    Resolved(Location),
    RejectedCall,
    RejectedBinding,
    Unresolved,
}

impl FileSemanticsNavigation {
    fn from_location_result(result: Result<Option<Location>, String>) -> Result<Self, String> {
        result.map(|location| location.map_or(Self::Unresolved, Self::Resolved))
    }
}

impl NavigationProvider<'_> {
    pub(crate) fn resolve_workspace_lookup(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        qualified_name: &str,
    ) -> Result<Option<Location>, String> {
        if let Ok(path) = ctx.uri.to_file_path() {
            if let Some(workspace_file_id) = self.workspace_index.get_file_id(&path) {
                let method_call_node = find_method_call_for_token(token);
                debug!(
                    "Attempting workspace-wide resolution for '{}'",
                    qualified_name
                );

                // Try method call with receiver resolution first (handles inheritance)
                if method_call_node.is_some() {
                    if let Ok(Some(location)) =
                        self.resolve_method_with_receiver(ctx, token, qualified_name)
                    {
                        debug!("Resolved method call with receiver (possibly inherited)");
                        return Ok(Some(location));
                    }
                }

                if method_call_node.is_some() && !qualified_name.contains('.') {
                    if let Some(class_fqn) = extract_class_name(&ctx.parse_tree.syntax()) {
                        if let Some(package) = extract_package_from_root(&ctx.parse_tree.syntax()) {
                            let full_class_fqn = format!("{package}.{class_fqn}");
                            debug!(
                                "Trying canonical unqualified method lookup: {}.{}",
                                full_class_fqn, qualified_name
                            );

                            if let Some(symbol_loc) = self.resolve_method_with_type_matching(
                                ctx.file_id,
                                &full_class_fqn,
                                qualified_name,
                                token,
                                &ctx.parse_tree.syntax(),
                                ctx.offset,
                            ) {
                                debug!("Found unqualified method via canonical resolution");
                                return self.symbol_location_to_lsp_location(&symbol_loc);
                            }
                            return Ok(None);
                        }
                    }
                }

                // Resolve simple type spellings through the exact lexical-owner/import
                // pipeline. A whole-file search by simple declaration name is not a
                // semantic identity: sibling types may legally contain same-named
                // nested declarations.
                if !self.is_method_call_at_token(token)
                    && !qualified_name.contains('.')
                    && qualified_name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase())
                {
                    // `expression.new Inner()` names a member type through the
                    // explicit enclosing-instance expression. It is not a
                    // lexically visible simple type in the caller, so resolve
                    // that syntax-bound owner before the ordinary lexical and
                    // import pipeline. The inner lookup still requires one
                    // exact indexed `Outer.Inner` identity.
                    if let Some(location) = self.resolve_inner_class_lookup(
                        ctx,
                        qualified_name,
                        token,
                        workspace_file_id,
                    )? {
                        return Ok(Some(location));
                    }
                    let lexical_owners = self.lexical_type_owners_at_offset(
                        &ctx.parse_tree.syntax(),
                        token.text_range().start(),
                    );
                    let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
                        .workspace_index
                        .resolve_type_name(workspace_file_id, &lexical_owners, qualified_name)
                    else {
                        debug!("Exact type binding for '{}' is unavailable", qualified_name);
                        return Ok(None);
                    };
                    if let Some(definition) = self.workspace_index.find_symbol_definition(
                        &hyperion_workspace::SymbolTarget::new(fqn, DeclarationNamespace::Type),
                    ) {
                        return self.symbol_location_to_lsp_location(&definition.location);
                    }
                    return Ok(None);
                }

                // Resolve simple names to FQNs via imports BEFORE workspace lookup
                // This enables navigation to same-package classes without explicit imports
                let resolved_name = if qualified_name.contains('.') {
                    // Already qualified - use as-is
                    qualified_name.to_string()
                } else {
                    // Simple name - try to resolve via imports (includes same-package resolution)
                    use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
                    if let Ok(fqn) = resolve_class_fqn_via_imports(
                        self.workspace_index.as_ref(),
                        workspace_file_id,
                        qualified_name,
                    ) {
                        debug!("Resolved '{}' to FQN '{}' via imports", qualified_name, fqn);
                        fqn
                    } else if qualified_name
                        .chars()
                        .next()
                        .is_some_and(|character| character.is_ascii_uppercase())
                    {
                        if let Some(location) = self.resolve_inner_class_lookup(
                            ctx,
                            qualified_name,
                            token,
                            workspace_file_id,
                        )? {
                            return Ok(Some(location));
                        }
                        debug!(
                            "Exact type binding for '{}' is missing or ambiguous",
                            qualified_name
                        );
                        return Ok(None);
                    } else {
                        debug!(
                            "Could not resolve '{}' via imports, using as-is",
                            qualified_name
                        );
                        qualified_name.to_string()
                    }
                };

                // Only use workspace lookup for qualified names or type references (uppercase first char).
                // Simple lowercase identifiers like "baz" could be local variables/parameters
                // that should be resolved via same-file resolution with proper scope handling.
                // This prevents the workspace from incorrectly returning a field when a parameter
                // with the same name shadows it in the local scope.
                let is_likely_local_variable = !resolved_name.contains('.')
                    && resolved_name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_lowercase());

                if is_likely_local_variable {
                    debug!(
                        "Skipping workspace lookup for '{}' - likely local variable",
                        resolved_name
                    );
                } else {
                    match self
                        .query_engine
                        .goto_definition(&resolved_name, workspace_file_id)
                    {
                        Ok(locations) if locations.len() == 1 => {
                            debug!(
                                "Found {} definition(s) via workspace index",
                                locations.len()
                            );

                            let symbol_loc = &locations[0];
                            debug!("Cross-file symbol located");
                            return self.symbol_location_to_lsp_location(symbol_loc);
                        }
                        Ok(_) => {
                            debug!("Workspace lookup returned 0 locations");
                        }
                        Err(err) => {
                            debug!("Workspace lookup failed for '{}': {}", resolved_name, err);
                        }
                    }
                }

                // Try direct symbol lookup with the resolved FQN
                if resolved_name.contains('.') {
                    if let Some(symbol_loc) = self.workspace_index.find_symbol(&resolved_name) {
                        debug!("Found qualified symbol via direct index lookup");
                        return self.symbol_location_to_lsp_location(&symbol_loc);
                    }
                }

                if let Some(location) =
                    self.resolve_inner_class_lookup(ctx, qualified_name, token, workspace_file_id)?
                {
                    return Ok(Some(location));
                }
            }
        }

        Ok(None)
    }

    /// Resolve a name at the cursor position using Salsa-cached `FileSemantics`.
    ///
    /// This queries the pre-computed name resolution from the semantic model:
    /// 1. `db.snapshot().file_semantics(file_id)` returns cached `Arc<FileSemantics>`
    /// 2. `file_sem.resolve_at(token.text_range())` looks up the resolution
    /// 3. Uses the `ResolvedName` FQN to find the source location via workspace index
    ///
    /// Returns `None` if the token isn't resolved or has no navigable location.
    /// Falls through to existing resolution strategies.
    pub(crate) fn resolve_via_file_semantics(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<FileSemanticsNavigation, String> {
        let identifier_name = token.text().to_string();

        let snapshot = self.txn.snapshot();
        // Salsa's file semantics uses the workspace-wide index handle. A scoped
        // request must not inherit a binding elected from another module.
        let file_sem = if self.workspace_index.has_source_scope() {
            snapshot.file_semantics_at_workspace_revision(
                ctx.file_id,
                self.workspace_index.current_workspace_revision(),
            )
        } else {
            snapshot.file_semantics(ctx.file_id)
        };

        let resolved = match file_sem.resolve_at(token.text_range()) {
            Some(r) => r,
            None => return Ok(FileSemanticsNavigation::Unresolved),
        };

        match resolved {
            ResolvedName::Type { fqn, .. } => {
                let lexical_owners = self.lexical_type_owners_at_offset(
                    &ctx.parse_tree.syntax(),
                    token.text_range().start(),
                );
                let written_name = token
                    .parent_ancestors()
                    .find(|node| node.kind() == SyntaxKind::CLASS_TYPE)
                    .and_then(|class_type| {
                        let qualified_name = class_type
                            .children()
                            .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)?;
                        let last_identifier = qualified_name
                            .descendants_with_tokens()
                            .filter_map(|child| child.into_token())
                            .filter(|candidate| candidate.kind() == SyntaxKind::IDENT)
                            .last()?;
                        if last_identifier.text_range() != token.text_range() {
                            return None;
                        }
                        Some(qualified_name.text().to_string())
                    })
                    .unwrap_or_else(|| identifier_name.clone());
                let canonical_resolution = self.workspace_index.resolve_type_name(
                    ctx.file_id,
                    &lexical_owners,
                    written_name.trim(),
                );
                let qualified_inner = self
                    .extract_outer_class_from_new_expr(token)
                    .and_then(|outer| {
                        crate::navigation::import_resolver::resolve_class_fqn_via_imports(
                            self.workspace_index.as_ref(),
                            ctx.file_id,
                            &outer,
                        )
                        .ok()
                    })
                    .map(|outer| format!("{outer}.{identifier_name}"))
                    .filter(|candidate| {
                        self.workspace_index
                            .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                                candidate.as_str(),
                                hyperion_workspace::DeclarationNamespace::Type,
                            ))
                            .is_some()
                    });
                let canonical_fqn = match (canonical_resolution, qualified_inner) {
                    (hyperion_workspace::TypeNameResolution::Exact(canonical), _) => canonical,
                    (
                        hyperion_workspace::TypeNameResolution::Missing
                        | hyperion_workspace::TypeNameResolution::Ambiguous(_),
                        Some(qualified_inner),
                    ) => qualified_inner,
                    (
                        hyperion_workspace::TypeNameResolution::Missing
                        | hyperion_workspace::TypeNameResolution::Ambiguous(_),
                        None,
                    ) => {
                        debug!(
                            "NAV: rejected non-canonical cached type binding '{}' -> '{}'",
                            identifier_name, fqn
                        );
                        return Ok(FileSemanticsNavigation::RejectedBinding);
                    }
                };

                debug!(
                    "NAV: file_semantics resolved '{}' as type: {}",
                    identifier_name, canonical_fqn
                );
                if let Some(symbol_loc) = self.workspace_index.find_symbol(&canonical_fqn) {
                    self.workspace_index.record_symbol_usage(&canonical_fqn);
                    return FileSemanticsNavigation::from_location_result(
                        self.symbol_location_to_lsp_location(&symbol_loc),
                    );
                }
                // Try goto_definition via query engine for FQN
                if let Ok(path) = ctx.uri.to_file_path() {
                    if let Some(workspace_file_id) = self.workspace_index.get_file_id(&path) {
                        if let Ok(locations) = self
                            .query_engine
                            .goto_definition(&canonical_fqn, workspace_file_id)
                        {
                            if let [location] = locations.as_slice() {
                                self.workspace_index.record_symbol_usage(&canonical_fqn);
                                return FileSemanticsNavigation::from_location_result(
                                    self.symbol_location_to_lsp_location(location),
                                );
                            }
                        }
                    }
                }
            }
            ResolvedName::Field {
                owner_fqn, name, ..
            } if !owner_fqn.is_empty() => {
                let field_fqn = format!("{owner_fqn}.{name}");
                debug!(
                    "NAV: file_semantics resolved '{}' as field: {}",
                    identifier_name, field_fqn
                );
                if let Some(symbol_loc) = self.find_value_with_inheritance(owner_fqn, name) {
                    self.workspace_index
                        .record_symbol_usage(symbol_loc.fqn.as_ref());
                    return FileSemanticsNavigation::from_location_result(
                        self.symbol_location_to_lsp_location(&symbol_loc),
                    );
                }
            }
            ResolvedName::Method {
                owner_fqn,
                name,
                location,
                ..
            } => {
                let method_call = find_method_call_for_token(token);

                // If semantic model provided an exact location, prefer it to avoid
                // overload collisions in name-based index lookups.
                // For call sites, keep arg-aware overload selection first.
                if method_call.is_none() && !has_explicit_member_receiver(token, ctx.source_text) {
                    if let Some(loc) = location.as_ref() {
                        if let Some(path) = self.workspace_index.get_file_path(loc.file_id) {
                            if let Ok(uri) = Url::from_file_path(&path) {
                                let source = if loc.file_id == ctx.file_id {
                                    ctx.source_text
                                } else {
                                    let Some(revision) =
                                        self.workspace_index.current_file_revision(loc.file_id)
                                    else {
                                        return Ok(FileSemanticsNavigation::Unresolved);
                                    };
                                    let Some(definition) = self
                                        .workspace_index
                                        .find_symbol_definition_at_revision_range(
                                            revision,
                                            self.workspace_index.current_workspace_revision(),
                                            loc.range,
                                            DeclarationNamespace::Callable,
                                        )
                                    else {
                                        return Ok(FileSemanticsNavigation::Unresolved);
                                    };
                                    return FileSemanticsNavigation::from_location_result(
                                        self.symbol_location_to_lsp_location(&definition.location),
                                    );
                                };
                                let Some(range) = crate::navigation_util::byte_range_to_lsp_range(
                                    source,
                                    loc.range.start,
                                    loc.range.end,
                                ) else {
                                    return Ok(FileSemanticsNavigation::Unresolved);
                                };
                                return Ok(FileSemanticsNavigation::Resolved(Location {
                                    uri,
                                    range,
                                }));
                            }
                        }
                    }
                }

                if owner_fqn.is_empty() {
                    return Ok(FileSemanticsNavigation::Unresolved);
                }

                if method_call.is_some() {
                    if let Some(symbol_loc) = self.resolve_method_with_type_matching(
                        ctx.file_id,
                        owner_fqn,
                        name,
                        token,
                        &ctx.parse_tree.syntax(),
                        ctx.offset,
                    ) {
                        self.workspace_index
                            .record_symbol_usage(symbol_loc.fqn.as_ref());
                        return FileSemanticsNavigation::from_location_result(
                            self.symbol_location_to_lsp_location(&symbol_loc),
                        );
                    }
                    // FileSemantics supplied the owner and the AST supplied the call;
                    // a canonical miss is not permission to choose by arity.
                    return Ok(FileSemanticsNavigation::RejectedCall);
                }

                let method_fqn = format!("{owner_fqn}.{name}");
                debug!(
                    "NAV: file_semantics resolved '{}' as method: {}",
                    identifier_name, method_fqn
                );
                if let Some(symbol_loc) = self.workspace_index.find_symbol(&method_fqn) {
                    self.workspace_index.record_symbol_usage(&method_fqn);
                    return FileSemanticsNavigation::from_location_result(
                        self.symbol_location_to_lsp_location(&symbol_loc),
                    );
                }
                // Fallback: inherited method might be on a parent class
                if let Some(symbol_loc) = self.find_callable_with_inheritance(owner_fqn, name) {
                    return FileSemanticsNavigation::from_location_result(
                        self.symbol_location_to_lsp_location(&symbol_loc),
                    );
                }
            }
            ResolvedName::EnumConstant { enum_fqn, name, .. } if !enum_fqn.is_empty() => {
                let constant_fqn = format!("{enum_fqn}.{name}");
                debug!(
                    "NAV: file_semantics resolved '{}' as enum constant: {}",
                    identifier_name, constant_fqn
                );
                if let Some(symbol_loc) = self.workspace_index.find_symbol(&constant_fqn) {
                    self.workspace_index.record_symbol_usage(&constant_fqn);
                    return FileSemanticsNavigation::from_location_result(
                        self.symbol_location_to_lsp_location(&symbol_loc),
                    );
                }
            }
            ResolvedName::LocalVariable {
                location: Some(ref loc),
                ..
            } if !has_explicit_member_receiver(token, ctx.source_text) => {
                debug!(
                    "NAV: file_semantics resolved '{}' as local variable/parameter",
                    identifier_name
                );
                // Navigate to the variable/parameter declaration within the same file
                if loc.file_id != ctx.file_id {
                    return Ok(FileSemanticsNavigation::Unresolved);
                }
                let file_path = self.workspace_index.get_file_path(loc.file_id);
                if let Some(path) = file_path {
                    if let Ok(uri) = Url::from_file_path(&path) {
                        let Some(range) = crate::navigation_util::byte_range_to_lsp_range(
                            ctx.source_text,
                            loc.range.start,
                            loc.range.end,
                        ) else {
                            return Ok(FileSemanticsNavigation::Unresolved);
                        };
                        return Ok(FileSemanticsNavigation::Resolved(Location { uri, range }));
                    }
                }
            }
            ResolvedName::LocalVariable { .. } => {
                // A bare-name lexical binding is not authoritative on the
                // right side of `.`, where selection happens in the
                // receiver's member namespace. Continue with the structured
                // qualified-access resolver for `this.name`, `super.name`,
                // and `obj.name`.
            }
            ResolvedName::Rejected(reason) => {
                debug!(
                    "NAV: FileSemantics authoritatively rejected '{}': {:?}",
                    identifier_name, reason
                );
                return Ok(FileSemanticsNavigation::RejectedBinding);
            }
            // TypeParameter, External, LocalVariable without location — not navigable,
            // fall through to existing strategies
            _ => {}
        }

        Ok(FileSemanticsNavigation::Unresolved)
    }

    /// Resolve method calls with receivers using inheritance
    /// Handles patterns like: `userDao.save(user)` where `save()` is defined in a parent class
    pub(crate) fn resolve_method_with_receiver(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        method_name: &str,
    ) -> Result<Option<Location>, String> {
        debug!("Checking for method call with receiver: {}", method_name);
        let Some(method_call) = find_method_call_for_token(token) else {
            return Ok(None);
        };
        let Some(receiver) = self.extract_method_receiver(&method_call) else {
            return Ok(None);
        };
        debug!("Found receiver: {}", receiver.text());

        let receiver_type = self
            .txn
            .file_semantics(ctx.file_id)
            .expression_type_at(receiver.text_range())
            .cloned()
            .or_else(|| self.resolve_receiver_type(ctx, &receiver));
        let Some(receiver_type) = receiver_type else {
            return Ok(None);
        };
        let Some(receiver_fqn) = self.type_to_fqn(&receiver_type, ctx) else {
            return Ok(None);
        };

        let symbol = self.resolve_method_with_type_matching(
            ctx.file_id,
            &receiver_fqn,
            method_name,
            token,
            &ctx.parse_tree.syntax(),
            ctx.offset,
        );
        symbol.map_or(Ok(None), |location| {
            self.symbol_location_to_lsp_location(&location)
        })
    }

    /// Extract receiver from METHOD_CALL_EXPR
    /// For `obj.method()`, returns the `obj` node
    fn extract_method_receiver(
        &self,
        method_call: &SyntaxNode<hyperion_parser::JavaLanguage>,
    ) -> Option<SyntaxNode<hyperion_parser::JavaLanguage>> {
        // METHOD_CALL_EXPR children: [receiver, DOT, IDENT, ARG_LIST]
        // We want the first child that's not DOT, IDENT, or ARG_LIST
        for child in method_call.children() {
            match child.kind() {
                SyntaxKind::DOT | SyntaxKind::ARG_LIST => continue,
                SyntaxKind::IDENT => continue, // This is the method name
                _ => return Some(child),
            }
        }
        None
    }

    /// Resolve the type of a receiver expression by name
    fn resolve_receiver_type_by_name(
        &self,
        ctx: &NavigationContext,
        receiver_text: &str,
    ) -> Option<Type> {
        debug!("Resolving type for receiver: {}", receiver_text);

        // Bind the receiver at the request position. A name found in a sibling
        // type or a later local declaration is not a receiver identity.
        let builder = ResolverBuilder::with_workspace(ctx.file_id, ctx.workspace_index.clone());
        let resolver = builder.build_from_syntax(&ctx.parse_tree.syntax());
        let offset = u32::try_from(ctx.offset).ok()?;
        let scope = resolver.find_scope_at_offset(offset)?;
        let symbol = resolver.resolve_in_scope_at_offset(receiver_text, scope, offset)?;
        debug!(
            "Resolved receiver at exact position: {} (type: {:?})",
            symbol.name, symbol.ty
        );
        Some(symbol.ty)
    }

    /// Resolve the type of a receiver expression
    fn resolve_receiver_type(
        &self,
        ctx: &NavigationContext,
        receiver: &SyntaxNode<hyperion_parser::JavaLanguage>,
    ) -> Option<Type> {
        let receiver_text = receiver.text().to_string();
        self.resolve_receiver_type_by_name(ctx, &receiver_text)
    }

    /// Convert a Type to its fully qualified name
    pub(crate) fn type_to_fqn(&self, ty: &Type, ctx: &NavigationContext) -> Option<String> {
        if let Type::Class(class_type) = ty {
            // If package is present, use it
            if let Some(package) = &class_type.package {
                return Some(format!("{}.{}", package, class_type.name));
            }

            // Otherwise, try to resolve using imports or current package
            if let Some(package) = extract_package_from_root(&ctx.parse_tree.syntax()) {
                // Check if it's in the same package
                let fqn = format!("{}.{}", package, class_type.name);
                if self.workspace_index.find_symbol(&fqn).is_some() {
                    return Some(fqn);
                }
            }

            // Last resort: just the class name
            Some(class_type.name.to_string())
        } else {
            debug!("Cannot convert type to FQN: {:?}", ty);
            None
        }
    }

    /// Convert workspace SymbolLocation to LSP Location
    pub(crate) fn symbol_location_to_lsp_location(
        &self,
        symbol_loc: &hyperion_workspace::SymbolLocation,
    ) -> Result<Option<Location>, String> {
        crate::navigation_util::symbol_location_to_lsp_location(
            &self.workspace_index,
            symbol_loc,
            Some(&self.source_providers),
        )
    }

    pub(crate) fn resolve_same_file_symbol(
        &self,
        ctx: &NavigationContext,
        source_text: &str,
        offset: u32,
        qualified_name: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        debug!("Falling back to same-file resolution");
        debug!(
            "NAV: Falling back to same-file resolution for '{}'",
            qualified_name
        );

        let builder = ResolverBuilder::with_workspace(ctx.file_id, ctx.workspace_index.clone());
        let resolver = builder.build_from_syntax(&ctx.parse_tree.syntax());

        let simple_name = qualified_name
            .split('.')
            .next_back()
            .unwrap_or(qualified_name);

        let scope_at_position = find_scope_at_position(&ctx.parse_tree.syntax(), offset, &resolver);

        let symbol = scope_at_position.and_then(|scope_id| {
            debug!(
                "NAV: Found scope {:?} at offset {}, resolving '{}' from that exact position",
                scope_id, offset, simple_name
            );
            resolver.resolve_in_scope_at_offset(simple_name, scope_id, offset)
        });
        let Some(symbol) = symbol else {
            debug!(
                "NAV: Symbol '{}' is not visible at offset {}",
                simple_name, offset
            );
            if let Some(location) = self.resolve_anonymous_class_method_symbol(
                &ctx.parse_tree.syntax(),
                source_text,
                offset,
                simple_name,
                uri,
            ) {
                return Ok(Some(location));
            }
            return Ok(None);
        };

        debug!("Resolved symbol: {} (type: {:?})", symbol.name, symbol.ty);

        let symbol_data = if let Some(s) = resolver.symbol_table().get_symbol(symbol.symbol_id) {
            s
        } else {
            debug!("Symbol data not found for ID {:?}", symbol.symbol_id);
            return Ok(None);
        };

        let start_offset = symbol_data.location.range.start as usize;
        let end_offset = symbol_data.location.range.end as usize;

        let def_position_start = offset_to_position(source_text, start_offset);
        let def_position_end = offset_to_position(source_text, end_offset);

        debug!(
            "Found definition at offset {}..{} (line {}, col {})",
            start_offset, end_offset, def_position_start.line, def_position_start.character
        );

        Ok(Some(Location {
            uri: uri.clone(),
            range: Range {
                start: def_position_start,
                end: def_position_end,
            },
        }))
    }

    pub(crate) fn method_decl_name_token(
        method_decl: &SyntaxNode<hyperion_parser::JavaLanguage>,
    ) -> Option<SyntaxToken<hyperion_parser::JavaLanguage>> {
        let params_start = method_decl
            .children()
            .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
            .map(|params| params.text_range().start());

        method_decl
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .filter(|tok| params_start.is_none_or(|start| tok.text_range().start() < start))
            .last()
    }

    pub(crate) fn method_decl_has_override_annotation(
        method_decl: &SyntaxNode<hyperion_parser::JavaLanguage>,
    ) -> bool {
        let Some(parent) = method_decl.parent() else {
            return false;
        };
        let mut prev_modifier_list = None;
        for child in parent.children() {
            if child == *method_decl {
                break;
            }
            prev_modifier_list = if child.kind() == SyntaxKind::MODIFIER_LIST {
                Some(child)
            } else {
                None
            };
        }
        let Some(prev_sibling) = prev_modifier_list else {
            return false;
        };

        prev_sibling
            .children()
            .filter(|n| n.kind() == SyntaxKind::ANNOTATION)
            .any(|annotation_node| {
                annotation_node
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::QUALIFIED_NAME)
                    .flat_map(|qname| qname.children_with_tokens())
                    .filter_map(|child| match child {
                        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t),
                        _ => None,
                    })
                    .any(|ident| ident.text() == "Override")
            })
    }

    pub(crate) fn method_decl_param_count(
        method_decl: &SyntaxNode<hyperion_parser::JavaLanguage>,
    ) -> usize {
        method_decl
            .children()
            .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
            .map_or(0, |params| {
                params
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::PARAM)
                    .count()
            })
    }

    pub(crate) fn resolve_anonymous_class_method_symbol(
        &self,
        root: &SyntaxNode<hyperion_parser::JavaLanguage>,
        source_text: &str,
        offset: u32,
        method_name: &str,
        uri: &Url,
    ) -> Option<Location> {
        let token = root
            .token_at_offset(TextSize::from(offset))
            .right_biased()
            .or_else(|| root.token_at_offset(TextSize::from(offset)).left_biased())?;

        let anonymous_new_expr = token.parent_ancestors().find(|node| {
            node.kind() == SyntaxKind::NEW_EXPR
                && node
                    .descendants()
                    .any(|child| child.kind() == SyntaxKind::METHOD_DECL)
        })?;

        let call_arg_count = find_method_call_for_token(&token)
            .and_then(|method_call| {
                method_call
                    .children()
                    .find(|child| child.kind() == SyntaxKind::ARG_LIST)
            })
            .map_or(0, |arg_list| count_arguments(&arg_list));

        let mut matches = Vec::new();
        for method_decl in anonymous_new_expr
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
        {
            let Some(name_token) = Self::method_decl_name_token(&method_decl) else {
                continue;
            };
            if name_token.text() != method_name {
                continue;
            }
            matches.push((method_decl, name_token));
        }

        if matches.is_empty() {
            return None;
        }

        let matching_arity = matches
            .iter()
            .filter(|(method_decl, _)| Self::method_decl_param_count(method_decl) == call_arg_count)
            .collect::<Vec<_>>();
        let [selected] = matching_arity.as_slice() else {
            return None;
        };
        let name_token = &selected.1;
        let token_range = name_token.text_range();
        let start_pos = offset_to_position(source_text, token_range.start().into());
        let end_pos = offset_to_position(source_text, token_range.end().into());

        Some(Location {
            uri: uri.clone(),
            range: Range {
                start: start_pos,
                end: end_pos,
            },
        })
    }
}

fn has_explicit_member_receiver(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    source_text: &str,
) -> bool {
    let mut previous = token.prev_token();
    while let Some(candidate) = previous {
        if !candidate.kind().is_trivia() {
            if matches!(candidate.kind(), SyntaxKind::DOT | SyntaxKind::COLON_COLON) {
                return true;
            }
            break;
        }
        previous = candidate.prev_token();
    }

    // Some recovered expression shapes do not expose punctuation through
    // `prev_token()` even though the source has an explicit selector. This
    // byte-level check is safe because `.` and `::` are ASCII Java tokens and
    // byte offsets are what rowan ranges carry.
    let before = source_text
        .as_bytes()
        .get(..usize::from(token.text_range().start()))
        .unwrap_or_default();
    let mut end = before.len();
    while end > 0 && before[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    before.get(end.wrapping_sub(1)) == Some(&b'.')
        || (end >= 2 && before.get(end - 2..end) == Some(b"::"))
}
