// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Inheritance Resolution
//!
//! Handles resolution of inherited members, @Override navigation, and inheritance clauses

use crate::navigation::call_site_analysis::find_method_call_for_token;
use crate::navigation::context::NavigationContext;
use crate::navigation::import_resolver::{extract_class_name, extract_package_from_root};
use crate::navigation_util::position_to_offset;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{ParseDatabase, SemanticDatabase};
use hyperion_semantics::resolved_name::ResolvedName;
use hyperion_workspace::SymbolTarget;
use rowan::{SyntaxToken, TextSize};
use tower_lsp::lsp_types::{Location, Position, SymbolKind};
use tracing::debug;

use super::NavigationProvider;

/// Extract the fully qualified class name including all enclosing classes and interfaces
/// For a nested class like Outer.Inner, returns ["Outer", "Inner"]
/// Also works for nested interfaces since they can contain types too
fn extract_enclosing_class_path(node: &SyntaxNode) -> Vec<String> {
    let mut class_names = Vec::new();
    let mut current = Some(node.clone());

    while let Some(n) = current {
        if n.kind().is_type_declaration() {
            if let Some(name) = extract_class_name(&n) {
                class_names.push(name);
            }
        }
        current = n.parent();
    }

    // Reverse because we collected from innermost to outermost
    class_names.reverse();
    class_names
}

impl NavigationProvider<'_> {
    /// Resolve concrete overrides for the method declaration or call at the
    /// cursor. Returning `None` means the cursor is not a resolvable method, or
    /// the method has no indexed implementations; the request handler may then
    /// preserve the normal goto-definition fallback.
    pub(crate) fn method_implementation_locations(
        &self,
        file_id: hyperion_semantics::db::FileId,
        position: Position,
        source_text: &str,
        definition: &Location,
    ) -> Result<Option<Vec<Location>>, String> {
        let workspace_revision = self.workspace_index.current_workspace_revision();
        let result = self.method_implementation_locations_at_revision(
            file_id,
            position,
            source_text,
            definition,
            workspace_revision,
        )?;
        if !self
            .workspace_index
            .workspace_bindings_are_current(workspace_revision)
        {
            debug!("Goto implementation: workspace bindings changed during request");
            return Ok(None);
        }
        Ok(result)
    }

    fn method_implementation_locations_at_revision(
        &self,
        file_id: hyperion_semantics::db::FileId,
        position: Position,
        source_text: &str,
        definition: &Location,
        workspace_revision: hyperion_workspace::WorkspaceRevision,
    ) -> Result<Option<Vec<Location>>, String> {
        let offset = position_to_offset(source_text, position)
            .map_err(|error| format!("Failed to convert position: {error}"))?;
        let offset = u32::try_from(offset)
            .map_err(|_| "Method implementation offset exceeds u32".to_string())?;

        let snapshot = self.txn.snapshot();
        let parse_tree = snapshot.parse_file(file_id);
        let Some(token) = parse_tree
            .syntax()
            .token_at_offset(TextSize::from(offset))
            .right_biased()
        else {
            return Ok(None);
        };
        let declaration_method = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .and_then(|method| Self::method_decl_name_token(&method))
            .filter(|name| name.text_range() == token.text_range())
            .and_then(|name| {
                self.workspace_index.find_symbol_in_file(
                    file_id,
                    name.text(),
                    SymbolKind::METHOD,
                    Some(name.text_range().start().into()),
                )
            });

        let is_method_reference = token
            .parent_ancestors()
            .any(|node| node.kind() == SyntaxKind::METHOD_REF_EXPR);
        if declaration_method.is_none()
            && find_method_call_for_token(&token).is_none()
            && !is_method_reference
        {
            return Ok(None);
        }

        let method_symbol = declaration_method
            .or_else(|| {
                let file_semantics = snapshot.file_semantics(file_id);
                let ResolvedName::Method {
                    owner_fqn,
                    name,
                    location,
                    ..
                } = file_semantics.resolve_at(token.text_range())?
                else {
                    return None;
                };
                if owner_fqn.is_empty() {
                    return None;
                }

                location
                    .as_ref()
                    .and_then(|location| {
                        self.workspace_index.find_symbol_in_file(
                            location.file_id,
                            name,
                            SymbolKind::METHOD,
                            Some(location.range.start),
                        )
                    })
                    .or_else(|| {
                        let methods = self.workspace_index.find_methods_by_name(owner_fqn, name);
                        let [method] = methods.as_slice() else {
                            return None;
                        };
                        Some(method.clone())
                    })
            })
            .or_else(|| {
                let path = definition.uri.to_file_path().ok()?;
                let definition_file_id = self.workspace_index.get_file_id(&path)?;
                let definition_offset = if definition_file_id == file_id {
                    position_to_offset(source_text, definition.range.start).ok()?
                } else {
                    let revision = self
                        .workspace_index
                        .current_file_revision(definition_file_id)?;
                    let indexed = self
                        .workspace_index
                        .get_indexed_revision_content(revision)?;
                    (indexed.file_path == path)
                        .then(|| position_to_offset(&indexed.content, definition.range.start).ok())
                        .flatten()?
                };
                let definition_offset = u32::try_from(definition_offset).ok()?;
                self.workspace_index.find_symbol_in_file(
                    definition_file_id,
                    token.text(),
                    SymbolKind::METHOD,
                    Some(definition_offset),
                )
            });
        debug!(
            "Goto implementation resolved method token '{}' to {:?}",
            token.text(),
            method_symbol.as_ref().map(|symbol| symbol.fqn.as_ref())
        );
        let Some(method_symbol) = method_symbol else {
            return Ok(None);
        };
        let implementations = match self
            .workspace_index
            .find_current_overriding_method_definitions(
                &SymbolTarget::for_symbol(&method_symbol),
                workspace_revision,
            ) {
            Ok(implementations) => implementations,
            Err(error) => {
                debug!("Goto implementation unavailable: {error}");
                return Ok(None);
            }
        };
        debug!(
            "Goto implementation found {} exact overrides for {}",
            implementations.len(),
            method_symbol.fqn,
        );
        let mut implementations: Vec<_> = implementations
            .into_iter()
            .filter(|definition| !definition.symbol.modifiers.is_abstract)
            .map(|definition| definition.location)
            .collect();
        debug!(
            "Goto implementation retained {} concrete overrides for {}",
            implementations.len(),
            method_symbol.fqn
        );
        implementations.sort_unstable_by(|left, right| {
            left.fqn
                .cmp(&right.fqn)
                .then(left.file_id.as_u32().cmp(&right.file_id.as_u32()))
                .then(left.range.start.cmp(&right.range.start))
        });
        implementations.dedup_by(|left, right| {
            left.fqn == right.fqn && left.file_id == right.file_id && left.range == right.range
        });

        let mut locations = Vec::with_capacity(implementations.len());
        for implementation in implementations {
            if let Some(location) = self.symbol_location_to_lsp_location(&implementation)? {
                locations.push(location);
            }
        }
        Ok((!locations.is_empty()).then_some(locations))
    }

    pub(crate) fn resolve_override_method(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        let Some(method_decl) = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
        else {
            return Ok(None);
        };

        if !Self::method_decl_has_override_annotation(&method_decl) {
            return Ok(None);
        }

        debug!("NAV: @Override annotation found, navigating to super declaration");
        let workspace_revision = self.workspace_index.current_workspace_revision();
        let Some(file_revision) = self.workspace_index.current_file_revision(ctx.file_id) else {
            return Ok(None);
        };
        let Some(indexed) = self
            .workspace_index
            .get_indexed_revision_content(file_revision)
        else {
            return Ok(None);
        };
        if indexed.content.as_str() != ctx.source_text {
            debug!("NAV: request bytes do not match the indexed method revision");
            return Ok(None);
        }
        let Some(method_definition) = self
            .workspace_index
            .find_symbol_definition_at_revision_range(
                file_revision,
                workspace_revision,
                hyperion_workspace::TextRange::new(
                    token.text_range().start().into(),
                    token.text_range().end().into(),
                ),
                hyperion_workspace::DeclarationNamespace::Callable,
            )
        else {
            return Ok(None);
        };
        let method_symbol = method_definition.symbol;
        let Some(owner) = method_symbol.container.as_deref() else {
            return Ok(None);
        };
        let family = match self.workspace_index.find_current_callable_override_family(
            &SymbolTarget::for_symbol(&method_symbol),
            workspace_revision,
        ) {
            Ok(family) => family,
            Err(error) => {
                debug!("NAV: exact override family unavailable: {error}");
                return Ok(None);
            }
        };
        let inheritance = self.workspace_index.get_inheritance_chain(owner);
        let overridden = inheritance.iter().skip(1).find_map(|ancestor| {
            family.iter().find(|definition| {
                definition.symbol.container.as_deref() == Some(ancestor.as_str())
            })
        });
        let Some(overridden) = overridden else {
            debug!("NAV: FAIL exact method not found in any supertype");
            return Ok(None);
        };
        if !self
            .workspace_index
            .workspace_bindings_are_current(workspace_revision)
        {
            return Ok(None);
        }
        debug!(
            "NAV: OK Found exact overridden method {}",
            overridden.target.fqn
        );
        self.symbol_location_to_lsp_location(&overridden.location)
    }

    pub(crate) fn resolve_inheritance_clause(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        let Some(parent) = token.parent() else {
            return Ok(None);
        };

        let mut in_method_or_field = false;
        let mut check_node = Some(parent.clone());
        while let Some(node) = check_node {
            match node.kind() {
                SyntaxKind::METHOD_DECL | SyntaxKind::FIELD_DECL | SyntaxKind::BLOCK_STMT => {
                    in_method_or_field = true;
                    break;
                }
                kind if kind.is_type_declaration() => {
                    break;
                }
                _ => {}
            }
            check_node = node.parent();
        }

        if in_method_or_field {
            return Ok(None);
        }

        let mut current = Some(parent);
        let mut found_extends = false;
        let mut found_implements = false;
        let mut found_permits = false;

        while let Some(node) = current {
            for sibling in node
                .parent()
                .iter()
                .flat_map(rowan::SyntaxNode::children_with_tokens)
            {
                if let rowan::NodeOrToken::Token(t) = sibling {
                    if t.kind() == SyntaxKind::EXTENDS_KW {
                        found_extends = true;
                    } else if t.kind() == SyntaxKind::IMPLEMENTS_KW {
                        found_implements = true;
                        found_extends = false;
                    } else if t.kind() == SyntaxKind::PERMITS_KW {
                        found_permits = true;
                        found_implements = false;
                        found_extends = false;
                    }
                }
            }

            let is_type_decl = node.kind().is_type_declaration();
            if is_type_decl && (found_extends || found_implements || found_permits) {
                debug!(
                    "NAV: Token '{}' is in extends/implements/permits clause",
                    token.text()
                );

                let type_name = token.text().to_string();
                let current_package = extract_package_from_root(&ctx.parse_tree.syntax());

                // Check if the type name already contains dots (qualified name like "Renderer.DefaultImpl")
                let parent_fqn = if type_name.contains('.') {
                    // Already qualified - just prepend package if needed
                    if let Some(pkg) = current_package {
                        Some(format!("{pkg}.{type_name}"))
                    } else {
                        Some(type_name.clone())
                    }
                } else {
                    // Simple name - follow Java name resolution rules (JLS §6.4.1):
                    // 1. Check enclosing classes (innermost to outermost) - nested members shadow package types
                    // 2. Only then check package-level

                    let enclosing_classes = extract_enclosing_class_path(&node);
                    debug!("NAV: Enclosing classes: {:?}", enclosing_classes);

                    if enclosing_classes.is_empty() {
                        // Top-level class - just use package + name
                        if let Some(pkg) = current_package {
                            Some(format!("{pkg}.{type_name}"))
                        } else {
                            Some(type_name.clone())
                        }
                    } else {
                        // Try each enclosing scope from outermost to innermost
                        // We iterate from the root, trying progressively more qualified names
                        let mut found_fqn = None;

                        for depth in 0..=enclosing_classes.len() {
                            let class_path = if depth == 0 {
                                // Try just the package-level first (least qualified)
                                if let Some(ref pkg) = current_package {
                                    format!("{pkg}.{type_name}")
                                } else {
                                    type_name.clone()
                                }
                            } else {
                                // Try as member of enclosing class at this depth
                                let enclosing_path = enclosing_classes[0..depth].join(".");
                                if let Some(ref pkg) = current_package {
                                    format!("{pkg}.{enclosing_path}.{type_name}")
                                } else {
                                    format!("{enclosing_path}.{type_name}")
                                }
                            };

                            debug!("NAV: Trying FQN: {}", class_path);

                            // Check if this symbol exists
                            if self.workspace_index.find_symbol(&class_path).is_some() {
                                // Found! But keep checking more nested scopes as they shadow
                                found_fqn = Some(class_path);
                            }
                        }

                        // Return the most nested match (last one found, which shadows earlier ones)
                        found_fqn
                    }
                };

                let Some(parent_fqn) = parent_fqn else {
                    debug!("NAV: Could not resolve type name: {}", type_name);
                    break;
                };

                debug!("NAV: Resolved to FQN: {}", parent_fqn);

                if let Some(symbol_loc) = self.workspace_index.find_symbol(&parent_fqn) {
                    debug!("NAV: OK Found parent type");
                    return self.symbol_location_to_lsp_location(&symbol_loc);
                }

                break;
            }

            current = node.parent();
        }

        Ok(None)
    }

    pub(crate) fn resolve_inherited_member(
        &self,
        ctx: &NavigationContext,
        qualified_name: &str,
    ) -> Result<Option<Location>, String> {
        if qualified_name.contains('.') {
            return Ok(None);
        }

        debug!("NAV: Checking for inherited member '{}'", qualified_name);

        // Resolve the enclosing type at the CURSOR, not the file's first
        // top-level CLASS_DECL: multi-class files picked the wrong class, and
        // interfaces/enums/records never matched at all. This is
        // the one shared implementation; nested declarations are
        // joined outermost-first (`Outer.Inner`).
        let Some(current_class_fqn) = crate::inherited_members::enclosing_type_fqn(&ctx.token)
        else {
            return Ok(None);
        };
        debug!("NAV: Current class: {}", current_class_fqn);

        // Seed the walk with the class itself: find_member_with_inheritance
        // recurses through BOTH the parent chain and the implemented
        // interfaces at every level, so interface default methods are now
        // reachable (probing only `parent_class` skipped them).
        let is_callable = crate::navigation::ast_queries::is_followed_by_paren(&ctx.token)
            || find_method_call_for_token(&ctx.token).is_some()
            || ctx
                .token
                .parent_ancestors()
                .any(|node| node.kind() == SyntaxKind::METHOD_REF_EXPR);
        let symbol_loc = if is_callable {
            self.find_callable_with_inheritance(&current_class_fqn, qualified_name)
        } else {
            self.find_value_with_inheritance(&current_class_fqn, qualified_name)
        };
        if let Some(symbol_loc) = symbol_loc {
            debug!("NAV: OK Found inherited member via hierarchy walk");
            return self.symbol_location_to_lsp_location(&symbol_loc);
        }

        Ok(None)
    }
}
