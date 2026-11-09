// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scope-aware Java references contained in the current semantic model.

use std::sync::Arc;

use hyperion_semantics::resolver::SymbolResolver;
use hyperion_semantics::symbol::SymbolTable;
use tower_lsp::lsp_types::{Location, Range, SymbolKind, Url};
use tower_lsp::CancellationToken;
use tracing::debug;

use crate::navigation::NavigationProvider;
use crate::navigation_util::{offset_to_position, position_to_offset};

pub(super) struct SameFileReferenceQuery<'a> {
    pub(super) root: &'a hyperion_parser::syntax::SyntaxNode,
    pub(super) source_text: &'a str,
    pub(super) uri: &'a Url,
    pub(super) target_name: &'a str,
    pub(super) cursor_offset: u32,
    pub(super) expected_kind: Option<SymbolKind>,
    pub(super) expected_signature: Option<&'a str>,
    pub(super) include_declaration: bool,
}

pub(super) enum SameFileReferenceResult {
    Found(Vec<Location>),
    Unresolved,
    Cancelled,
}

impl NavigationProvider<'_> {
    /// Resolve references whose identity is local to one Java semantic model.
    ///
    /// Workspace discovery deliberately happens before this phase. An
    /// unresolved local symbol therefore means "use the precise workspace
    /// results if any", while cancellation means "discard partial work".
    pub(super) fn find_same_file_references(
        &self,
        symbol_table: Arc<SymbolTable>,
        query: &SameFileReferenceQuery<'_>,
        cancellation: &CancellationToken,
    ) -> SameFileReferenceResult {
        debug!(
            "REFS: Same-file search for '{}' at offset {}",
            query.target_name, query.cursor_offset
        );

        let mut resolver = SymbolResolver::with_cached_table(symbol_table);
        let scope_at_cursor = resolver
            .symbol_table()
            .find_scope_at_offset(query.cursor_offset)
            .unwrap_or_else(|| resolver.symbol_table().root_scope());

        let Some(target_symbol) = resolver.resolve_in_scope(query.target_name, scope_at_cursor)
        else {
            debug!(
                "REFS: Could not resolve '{}' in scope {:?}",
                query.target_name, scope_at_cursor
            );
            return SameFileReferenceResult::Unresolved;
        };
        let target_symbol_id = target_symbol.symbol_id;
        debug!(
            "REFS: Resolved '{}' to symbol ID {:?} with type {:?}",
            query.target_name, target_symbol_id, target_symbol.ty
        );

        let mut references = Vec::new();
        if query.include_declaration {
            if let Some(symbol) = resolver.symbol_table().get_symbol(target_symbol_id) {
                let kind_matches = query.expected_kind.is_none_or(|expected_kind| {
                    Self::reference_kind_matches_expected(
                        expected_kind,
                        Some(SymbolKind::from(&symbol.kind)),
                        query.source_text,
                        symbol.location.range.start,
                        symbol.location.range.end,
                    )
                });
                if kind_matches {
                    references.push(Location {
                        uri: query.uri.clone(),
                        range: Range::new(
                            offset_to_position(
                                query.source_text,
                                symbol.location.range.start as usize,
                            ),
                            offset_to_position(
                                query.source_text,
                                symbol.location.range.end as usize,
                            ),
                        ),
                    });
                }
            }
        }

        for element in query.root.descendants_with_tokens() {
            if cancellation.is_cancelled() {
                return SameFileReferenceResult::Cancelled;
            }
            let Some(token) = element.as_token() else {
                continue;
            };
            if !token.kind().is_identifier() || token.text() != query.target_name {
                continue;
            }

            let range = token.text_range();
            if let Some(expected_kind) = query.expected_kind {
                let Some(parent) = token.parent() else {
                    continue;
                };
                let observed_kind = Self::expected_symbol_kind_from_context(&parent);
                if !Self::reference_kind_matches_expected(
                    expected_kind,
                    observed_kind,
                    query.source_text,
                    u32::from(range.start()),
                    u32::from(range.end()),
                ) {
                    continue;
                }
            }

            let token_offset = u32::from(range.start());
            let token_scope = resolver
                .symbol_table()
                .find_scope_at_offset(token_offset)
                .unwrap_or_else(|| resolver.symbol_table().root_scope());
            let resolves_to_target = resolver
                .resolve_in_scope(token.text(), token_scope)
                .is_some_and(|resolved| resolved.symbol_id == target_symbol_id);
            if !resolves_to_target {
                continue;
            }

            let is_declaration = resolver
                .symbol_table()
                .get_symbol(target_symbol_id)
                .is_some_and(|symbol| {
                    symbol.location.range.start == u32::from(range.start())
                        && symbol.location.range.end == u32::from(range.end())
                });
            if is_declaration {
                continue;
            }

            references.push(Location {
                uri: query.uri.clone(),
                range: Range::new(
                    offset_to_position(query.source_text, usize::from(range.start())),
                    offset_to_position(query.source_text, usize::from(range.end())),
                ),
            });
        }

        if let Some(expected_signature) = query.expected_signature {
            references.retain(|location| {
                position_to_offset(query.source_text, location.range.start).is_ok_and(|offset| {
                    self.same_file_method_ref_matches_signature(
                        query.root,
                        offset,
                        expected_signature,
                    )
                })
            });
        }

        debug!("Found {} same-file semantic references", references.len());
        SameFileReferenceResult::Found(references)
    }
}
