// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Find References Implementation
//!
//! This module provides LSP "Find References" functionality with scope-aware semantic analysis.
//!
//! # Known Limitations
//!
//! 1. **Workspace References**: Currently limited to same-file or indexed symbols.
//!    Cross-file references rely on workspace indexing which may be incomplete.
//!
//! 2. **Complex Language Features**: Some advanced Java features may not be fully
//!    supported by the semantic analyzer (e.g., complex generics, method references).
//!
//! # Safety Fixes Applied
//!
//! - Replaced unsafe `as usize` conversions with safe `usize::from()`
//! - Added scope-aware resolution to prevent missing references in nested scopes
//! - String-matching fallback removed; unresolved symbols return no results
//! - Validate stored ranges match expected symbol text to catch stale index data

use crate::navigation::NavigationProvider;
use crate::navigation_util::position_to_offset;
use crate::position_utils::LineIndex;

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{FileId, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_workspace::{
    CurrentReferences, DeclarationNamespace, FileRevisionToken, SymbolTarget,
};
use rowan::TextSize;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tower_lsp::lsp_types::{Location, Position, Range, SymbolKind, Url};
use tower_lsp::CancellationToken;
use tracing::debug;

mod same_file;

use self::same_file::{SameFileReferenceQuery, SameFileReferenceResult};

/// Converted workspace locations together with the exact query proof that owns
/// their ranges and negative scan result. The proof must survive until the
/// request's final publication fence.
struct CollectedWorkspaceReferences {
    locations: Vec<Location>,
    proof: CurrentReferences,
}

impl NavigationProvider<'_> {
    /// Find all references to the symbol at the given position
    ///
    /// Searches for references across the entire workspace using:
    /// 1. Workspace index for cross-file symbol lookup
    /// 2. Semantic analysis within each file for accurate matching
    ///
    /// Falls back to same-file semantic search if workspace lookup fails.
    ///
    /// # Arguments
    ///
    /// * `file_id` - The FileId of the document
    /// * `position` - The cursor position in the document
    /// * `source_text` - The source text of the document
    /// * `uri` - The URI of the document
    /// * `include_declaration` - Whether to include the declaration in results
    ///
    /// # Returns
    ///
    /// A vector of Locations pointing to all references of the symbol
    pub fn find_references(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
        include_declaration: bool,
    ) -> Result<Vec<Location>, String> {
        self.find_references_cancellable(
            file_id,
            position,
            source_text,
            uri,
            include_declaration,
            &CancellationToken::default(),
        )
    }

    /// Find references while cooperatively observing LSP request cancellation.
    pub fn find_references_cancellable(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
        include_declaration: bool,
        cancellation: &CancellationToken,
    ) -> Result<Vec<Location>, String> {
        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        // Single snapshot for the entire find-references request — one lock
        // acquisition instead of 4. All Salsa queries are memoized within the
        // same snapshot.
        let snapshot = self.txn.snapshot();

        let language = snapshot.file_language(file_id);

        // Unified language dispatch. Only Java may reach the Java CST below. A foreign
        // handler that declines fails the request: an empty list would tell the user
        // there are no references, which is not what happened.
        let ctx = crate::language_handler::LanguageHandlerContext {
            txn: self.txn,
            workspace_index: &self.workspace_index,
            source_providers: &self.source_providers,
            query_engine: Some(&self.query_engine),
        };
        let dispatched = ctx.dispatch_opt_in(
            language,
            |handler| {
                handler
                    .find_references(file_id, position, source_text, uri, include_declaration)
                    .map(|result| result.map(Some))
            },
            || Ok(None),
            || {
                Err(format!(
                    "find references is not available for {language:?} files yet"
                ))
            },
        )?;
        if let Some(references) = dispatched {
            return Ok(references);
        }

        let parse_tree = snapshot.parse_file(file_id);
        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        // Convert LSP position to offset
        let offset = position_to_offset(source_text, position)?;

        // Check if cursor is in comment or string - return empty vec early
        if crate::navigation::ast_queries::is_in_comment_or_string_java(
            &parse_tree.syntax(),
            offset,
        ) {
            debug!("REFS: Cursor in comment/string, skipping find-references");
            return Ok(vec![]);
        }

        // Find the token at position
        let token = parse_tree
            .syntax()
            .token_at_offset(TextSize::from(offset))
            .right_biased()
            .ok_or("No token at position")?;

        // Only handle identifiers (including contextual keywords like 'record', 'var', 'yield')
        if !token.kind().is_identifier() {
            return Ok(vec![]);
        }

        let target_name = token.text();
        debug!("Finding references to: {}", target_name);
        let expected_context_kind = token
            .parent()
            .as_ref()
            .and_then(Self::expected_symbol_kind_from_context);

        if token
            .parent()
            .is_some_and(|parent| parent.kind() == SyntaxKind::CONSTRUCTOR_DECL)
        {
            return self.find_constructor_references(
                file_id,
                &token,
                source_text,
                uri,
                include_declaration,
                &parse_tree.syntax(),
            );
        }

        // Collect workspace references (will be merged with same-file analysis)
        let mut workspace_refs: Vec<Location> = Vec::new();
        let mut workspace_reference_proofs: Vec<CurrentReferences> = Vec::new();
        let mut canonical_workspace_target = false;

        // Local declarations stay within the same-file lexical resolver.
        let is_local_declaration = token.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::PARAM
            )
        });

        // Resolve workspace declarations through the same exact pipeline as
        // go-to-definition before consulting any name-shaped semantic result.
        // FileSemantics intentionally represents methods as `Owner.name` and
        // therefore cannot distinguish overload declarations on its own.
        if !is_local_declaration {
            let expected_namespace = expected_context_kind.map(DeclarationNamespace::for_kind);
            if let Some(resolved) = self
                .resolve_workspace_target_at_position(
                    file_id,
                    position,
                    source_text,
                    uri,
                    expected_namespace,
                )
                .ok()
                .flatten()
            {
                // A Java call to `getNote()` is a use of the Kotlin property
                // `note` (and of its sibling accessors); the family is the
                // declaration plus every JVM-view alias.
                for member in self
                    .workspace_index
                    .jvm_reference_family(&resolved.definition.target)
                {
                    if let Some(collected) = self.collect_references_for_target(
                        &member,
                        include_declaration,
                        cancellation,
                    ) {
                        workspace_refs.extend(collected.locations);
                        workspace_reference_proofs.push(collected.proof);
                    }
                }
                // An exact position-to-declaration decision suppresses both
                // descriptorless FileSemantics lookup and same-file heuristics,
                // including when the current query produces no locations.
                canonical_workspace_target = true;
            }
        }

        // FileSemantics remains useful for external types and conservative
        // cases the workspace navigation pipeline cannot bind to an indexed
        // declaration. Never promote its descriptorless method shape to a
        // callable identity.
        if !is_local_declaration && !canonical_workspace_target {
            let file_sem = snapshot.file_semantics(file_id);
            if let Some(resolved) = file_sem.resolve_at(token.text_range()) {
                use hyperion_semantics::resolved_name::ResolvedName;
                let mut resolved_target = match resolved {
                    ResolvedName::Type { fqn, .. } => {
                        Some((fqn.clone(), Some(DeclarationNamespace::Type)))
                    }
                    ResolvedName::Method { .. } => {
                        debug!("REFS: descriptorless FileSemantics method is not an exact target");
                        canonical_workspace_target = true;
                        None
                    }
                    ResolvedName::Field {
                        owner_fqn, name, ..
                    } if !owner_fqn.is_empty() => Some((
                        format!("{owner_fqn}.{name}"),
                        Some(DeclarationNamespace::Value),
                    )),
                    ResolvedName::EnumConstant { enum_fqn, name, .. } if !enum_fqn.is_empty() => {
                        Some((
                            format!("{enum_fqn}.{name}"),
                            Some(DeclarationNamespace::Value),
                        ))
                    }
                    ResolvedName::LocalVariable { .. } | ResolvedName::TypeParameter { .. } => {
                        // Local variables and type parameters are same-file only.
                        // Skip workspace search, let same-file analysis handle them.
                        None
                    }
                    ResolvedName::External { fqn, .. } => Some((fqn.clone(), None)),
                    ResolvedName::Rejected(reason) => {
                        debug!("REFS: semantic binding was authoritatively rejected: {reason:?}");
                        canonical_workspace_target = true;
                        None
                    }
                    _ => None, // Package → fall through
                };

                // Type names are the ambiguous tier: on-demand imports let a
                // name-shaped binding pick a workspace type the canonical
                // position pipeline refused to elect. Value bindings carry
                // their owner in the FQN, so an indexed workspace declaration
                // for `Owner.member` is exactly the declaration under the
                // cursor. Suppressing those as well silently returned no
                // references for every field and enum-constant *use*, where
                // the canonical pipeline abstains rather than rejects.
                if let Some((fqn, Some(DeclarationNamespace::Type))) = resolved_target.as_ref() {
                    let target = SymbolTarget::new(fqn.clone(), DeclarationNamespace::Type);
                    if self
                        .workspace_index
                        .find_symbol_definition(&target)
                        .is_some_and(|definition| {
                            matches!(
                                definition.source,
                                hyperion_workspace::DefinitionSourceAuthority::Workspace(_)
                            )
                        })
                    {
                        debug!(
                            "REFS: canonical navigation rejected cached workspace type binding '{}'",
                            fqn
                        );
                        canonical_workspace_target = true;
                        resolved_target = None;
                    }
                }

                if let Some((fqn, namespace)) = resolved_target {
                    debug!(
                        "REFS: FileSemantics resolved '{}' to FQN '{}'",
                        target_name, fqn
                    );
                    let collected = namespace.map_or_else(
                        || self.collect_references_for_fqn(&fqn, include_declaration, cancellation),
                        |namespace| {
                            self.collect_references_for_target(
                                &SymbolTarget::new(fqn.clone(), namespace),
                                include_declaration,
                                cancellation,
                            )
                        },
                    );
                    if let Some(collected) = collected {
                        debug!(
                            "REFS: FileSemantics path found {} workspace references",
                            collected.locations.len()
                        );
                        workspace_refs = collected.locations;
                        workspace_reference_proofs.push(collected.proof);
                    }
                }
            }
        }

        let root = parse_tree.syntax();
        let query = SameFileReferenceQuery {
            root: &root,
            source_text,
            uri,
            target_name,
            cursor_offset: offset,
            expected_kind: expected_context_kind,
            expected_signature: None,
            include_declaration,
        };
        let references = if canonical_workspace_target {
            Vec::new()
        } else {
            match self.find_same_file_references(
                snapshot.file_symbol_table(file_id),
                &query,
                cancellation,
            ) {
                SameFileReferenceResult::Found(references) => references,
                SameFileReferenceResult::Unresolved => {
                    if workspace_reference_proofs
                        .iter()
                        .all(|proof| self.query_engine.references_are_current(proof))
                    {
                        return Ok(crate::reference_results::normalize(workspace_refs));
                    }
                    return Ok(Vec::new());
                }
                SameFileReferenceResult::Cancelled => return Ok(Vec::new()),
            }
        };

        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }
        let final_references = crate::reference_results::merge(references, workspace_refs);

        debug!(
            "Returning {} total references (after dedup and merge)",
            final_references.len()
        );
        if workspace_reference_proofs
            .iter()
            .all(|proof| self.query_engine.references_are_current(proof))
        {
            Ok(final_references)
        } else {
            Ok(Vec::new())
        }
    }

    fn expected_symbol_kind_from_context(
        parent: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<SymbolKind> {
        // A type spelling may sit inside FIELD_DECL, METHOD_DECL, or PARAM.
        // Those enclosing declarations describe a different identifier; the
        // nearest CLASS_TYPE is authoritative for the token under the cursor.
        if parent.kind() == SyntaxKind::CLASS_TYPE
            || parent
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::CLASS_TYPE)
        {
            return Some(SymbolKind::CLASS);
        }

        if parent
            .ancestors()
            .any(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
        {
            return Some(SymbolKind::METHOD);
        }

        if parent
            .ancestors()
            .any(|node| node.kind() == SyntaxKind::METHOD_DECL)
            && !parent
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            && !parent
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::PARAM)
        {
            return Some(SymbolKind::METHOD);
        }

        if parent.kind() == SyntaxKind::FIELD_DECL
            || parent
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::FIELD_DECL)
            || parent
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
        {
            return Some(SymbolKind::FIELD);
        }

        if parent
            .ancestors()
            .any(|node| node.kind() == SyntaxKind::ENUM_CONSTANT)
        {
            return Some(SymbolKind::ENUM_MEMBER);
        }

        if parent.kind() == SyntaxKind::CLASS_DECL {
            return Some(SymbolKind::CLASS);
        }

        if parent.kind() == SyntaxKind::INTERFACE_DECL {
            return Some(SymbolKind::INTERFACE);
        }

        if parent.kind() == SyntaxKind::ENUM_DECL {
            return Some(SymbolKind::ENUM);
        }

        None
    }

    fn symbol_kind_matches_context(actual: SymbolKind, expected: SymbolKind) -> bool {
        match expected {
            SymbolKind::METHOD => matches!(actual, SymbolKind::METHOD | SymbolKind::FUNCTION),
            SymbolKind::FIELD => matches!(actual, SymbolKind::FIELD | SymbolKind::PROPERTY),
            SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::ENUM | SymbolKind::STRUCT => {
                matches!(
                    actual,
                    SymbolKind::CLASS
                        | SymbolKind::INTERFACE
                        | SymbolKind::ENUM
                        | SymbolKind::STRUCT
                )
            }
            _ => actual == expected,
        }
    }

    fn reference_kind_matches_expected(
        expected: SymbolKind,
        observed: Option<SymbolKind>,
        content: &str,
        _range_start: u32,
        range_end: u32,
    ) -> bool {
        let method_like = Self::is_method_like_reference(content, range_end);
        match expected {
            SymbolKind::METHOD => {
                observed
                    .is_some_and(|kind| Self::symbol_kind_matches_context(kind, SymbolKind::METHOD))
                    || method_like
            }
            SymbolKind::FIELD => {
                !observed
                    .is_some_and(|kind| Self::symbol_kind_matches_context(kind, SymbolKind::METHOD))
                    && !method_like
            }
            SymbolKind::ENUM_MEMBER => observed.is_none_or(|kind| {
                Self::symbol_kind_matches_context(kind, SymbolKind::ENUM_MEMBER)
            }),
            _ => true,
        }
    }

    fn is_method_like_reference(content: &str, range_end: u32) -> bool {
        let bytes = content.as_bytes();
        let Ok(mut index) = usize::try_from(range_end) else {
            return false;
        };
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        index < bytes.len() && bytes[index] == b'('
    }

    /// Validate that the stored byte range still points to the expected symbol text.
    ///
    /// This catches stale index data where:
    /// 1. File was indexed with N lines
    /// 2. File was modified (now has N+M lines)
    /// 3. Stored byte ranges point to wrong positions in new content
    ///
    /// Returns true if the range is valid and points to the expected text.
    /// Collect workspace references for a given FQN using the query engine.
    ///
    /// Converts query engine results to LSP locations with validation and staleness checks.
    fn collect_references_for_fqn(
        &self,
        fqn: &str,
        include_declaration: bool,
        cancellation: &CancellationToken,
    ) -> Option<CollectedWorkspaceReferences> {
        let current = match self
            .query_engine
            .find_current_references_cancellable(fqn, || cancellation.is_cancelled())
        {
            Ok(current) if !current.is_empty() => current,
            _ => return None,
        };
        let locations = if include_declaration {
            current.locations().cloned().collect::<Vec<_>>()
        } else {
            current.usages.locations().cloned().collect::<Vec<_>>()
        };

        self.workspace_index.record_symbol_usage(fqn);

        let converted = self.convert_symbol_locations_to_lsp(&locations);
        self.query_engine
            .references_are_current(&current)
            .then_some(CollectedWorkspaceReferences {
                locations: converted,
                proof: current,
            })
    }

    /// Collect references for a declaration whose semantic namespace is known.
    ///
    /// Keeping the namespace through definition lookup and the usage scan is
    /// essential for legal Java collisions such as a field and nested type, or
    /// a constructor and ordinary method, sharing the same textual FQN.
    fn collect_references_for_target(
        &self,
        target: &SymbolTarget,
        include_declaration: bool,
        cancellation: &CancellationToken,
    ) -> Option<CollectedWorkspaceReferences> {
        let current = match self
            .query_engine
            .find_current_references_target_cancellable(target, || cancellation.is_cancelled())
        {
            Ok(current) if !current.is_empty() => current,
            _ => return None,
        };
        let locations = if include_declaration {
            current.locations().cloned().collect::<Vec<_>>()
        } else {
            current.usages.locations().cloned().collect::<Vec<_>>()
        };

        self.workspace_index.record_symbol_usage(&target.fqn);

        let converted = self.convert_symbol_locations_to_lsp(&locations);
        self.query_engine
            .references_are_current(&current)
            .then_some(CollectedWorkspaceReferences {
                locations: converted,
                proof: current,
            })
    }

    fn registered_semantic_texts(
        &self,
        locations: &[hyperion_workspace::SymbolLocation],
    ) -> HashMap<FileId, Arc<String>> {
        let file_ids = locations
            .iter()
            .map(|location| location.file_id)
            .collect::<HashSet<_>>();
        file_ids
            .into_iter()
            .filter_map(|file_id| self.txn.try_file_text(file_id).map(|text| (file_id, text)))
            .collect()
    }

    /// Converts a slice of `SymbolLocation`s to validated LSP `Location`s.
    ///
    /// For each location: resolves file path → URI, checks staleness, validates
    /// that the stored range still points to the expected symbol text, and converts
    /// byte offsets to LSP positions.
    ///
    /// Stale or invalid references are silently skipped.
    pub(crate) fn convert_symbol_locations_to_lsp(
        &self,
        locations: &[hyperion_workspace::SymbolLocation],
    ) -> Vec<Location> {
        struct FileConversion {
            content: Arc<String>,
            uri: Url,
            lines: LineIndex,
            semantic_text_matches: bool,
        }

        let mut results = Vec::with_capacity(locations.len());
        let semantic_texts = self.registered_semantic_texts(locations);
        let mut files = HashMap::<FileRevisionToken, Option<FileConversion>>::new();

        for symbol_loc in locations {
            let Some(revisioned) = self.reject_unrevisioned_workspace_location(symbol_loc) else {
                continue;
            };
            let file = files.entry(revisioned.revision).or_insert_with(|| {
                let indexed = self
                    .workspace_index
                    .get_indexed_revision_content(revisioned.revision)?;
                let semantic_text_matches = semantic_texts
                    .get(&symbol_loc.file_id)
                    .is_none_or(|text| text.as_str() == indexed.content.as_str());
                let uri = Url::from_file_path(&indexed.file_path).ok()?;
                let lines = LineIndex::new(&indexed.content);
                Some(FileConversion {
                    content: indexed.content,
                    uri,
                    lines,
                    semantic_text_matches,
                })
            });
            let Some(file) = file.as_ref() else {
                continue;
            };
            if !file.semantic_text_matches {
                continue;
            }
            if !Self::validate_reference_range(
                &file.content,
                revisioned.location.range.start,
                revisioned.location.range.end,
                &revisioned.location.name,
            ) {
                continue;
            }
            let Ok(start_offset) = usize::try_from(revisioned.location.range.start) else {
                continue;
            };
            let Ok(end_offset) = usize::try_from(revisioned.location.range.end) else {
                continue;
            };
            let start = file.lines.offset_to_position(&file.content, start_offset);
            let end = file.lines.offset_to_position(&file.content, end_offset);

            results.push(Location {
                uri: file.uri.clone(),
                range: Range::new(start, end),
            });
        }

        results
    }

    fn reject_unrevisioned_workspace_location(
        &self,
        location: &hyperion_workspace::SymbolLocation,
    ) -> Option<hyperion_workspace::RevisionedLocation> {
        if let Some(revisioned) = self.workspace_index.revisioned_location(location) {
            return Some(revisioned);
        }
        debug!(
            "REFS: omitting unrevisioned workspace range for '{}' until its semantic publication provenance is available",
            location.fqn
        );
        None
    }

    pub(crate) fn validate_reference_range(
        content: &str,
        range_start: u32,
        range_end: u32,
        expected_name: &str,
    ) -> bool {
        let start = range_start as usize;
        let end = range_end as usize;

        // Check bounds
        if start > end
            || end > content.len()
            || !content.is_char_boundary(start)
            || !content.is_char_boundary(end)
        {
            debug!(
                "Stale reference: invalid byte range {}..{} for content length {}",
                start,
                end,
                content.len()
            );
            return false;
        }

        // Extract text at the stored range
        let actual_text = &content[start..end];

        // The name field may be qualified ("ResourceType.UNKNOWN") or include a
        // method signature ("work()"). The range typically only covers the
        // identifier token (e.g., "UNKNOWN", "work").
        let expected_simple = expected_name.rsplit('.').next().unwrap_or(expected_name);
        let expected_identifier = expected_simple.split('(').next().unwrap_or(expected_simple);

        // Validate it matches the expected symbol name (full/simple/identifier)
        if actual_text != expected_name
            && actual_text != expected_simple
            && actual_text != expected_identifier
        {
            debug!(
                "Stale reference: expected '{}' or '{}' or '{}', found '{}' at {}..{}",
                expected_name, expected_simple, expected_identifier, actual_text, start, end
            );
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_workspace::{QueryEngine, WorkspaceIndex};
    use std::sync::Arc;

    #[test]
    fn collected_workspace_references_retain_their_final_revision_proof() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Target.java");
        let source = "package p; class Target {}";
        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index
            .index_file_with_source(&path, source)
            .expect("R1 should index");

        let workspace_index = Arc::new(workspace_index);
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&workspace_index),
            Arc::clone(&query_engine),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );

        let collected = provider
            .collect_references_for_fqn("p.Target", true, &CancellationToken::default())
            .expect("definition should produce a converted reference and proof");
        assert!(!collected.locations.is_empty());
        assert!(query_engine.references_are_current(&collected.proof));

        let raw_locations = collected.proof.locations().cloned().collect::<Vec<_>>();
        assert!(
            !provider
                .convert_symbol_locations_to_lsp(&raw_locations)
                .is_empty(),
            "the retained proof still converts while its revision is current"
        );

        workspace_index
            .index_file_with_source(&path, source)
            .expect("same-text R2 should publish");
        assert!(
            !query_engine.references_are_current(&collected.proof),
            "the retained R1 proof must reject a same-text R2 publication"
        );
    }

    #[test]
    fn definition_only_query_remains_collected_when_declaration_is_excluded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Target.java");
        let source = "package p; class Target {}";
        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let workspace_index = Arc::new(workspace_index);
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&workspace_index),
            Arc::clone(&query_engine),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let target = SymbolTarget::new("p.Target", DeclarationNamespace::Type);

        let collected = provider
            .collect_references_for_target(&target, false, &CancellationToken::default())
            .expect("a successful empty query must suppress the legacy second scan");

        assert!(collected.locations.is_empty());
        assert!(query_engine.references_are_current(&collected.proof));
    }

    #[test]
    fn nested_type_reference_pipeline_preserves_one_lexical_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("p/Containers.java");
        let source = r#"package p;
class Left {
    static class Node {}
    Node value;
}
class Right {
    static class Node {}
    Node value;
    Node echo(Node input) { return input; }
}
"#;
        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index
            .index_file_with_source(&path, source)
            .expect("nested type fixture should index");
        let file_id = workspace_index.get_file_id(&path).expect("fixture file id");
        let workspace_index = Arc::new(workspace_index);
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        host.with_db(|database| {
            database.register_file(
                file_id,
                Arc::new(source.to_owned()),
                hyperion_semantics::db::Language::Java,
            );
        });
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&workspace_index),
            Arc::clone(&query_engine),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let use_offset = source
            .find("    Node value;\n    Node echo")
            .expect("right nested type use")
            + 4;
        let position = LineIndex::new(source).offset_to_position(source, use_offset);
        let uri = Url::from_file_path(&path).expect("fixture URI");

        let resolved = provider
            .resolve_workspace_target_at_position(file_id, position, source, &uri, None)
            .expect("position resolution should succeed")
            .expect("right nested type should resolve");
        assert_eq!(resolved.definition.target.fqn.as_ref(), "p.Right.Node");
        assert_eq!(
            resolved.definition.target.namespace,
            DeclarationNamespace::Type
        );

        let current = query_engine
            .find_current_references_target(&resolved.definition.target)
            .expect("current references should resolve");
        assert_eq!(current.len(), 4, "current references: {current:#?}");
        let raw = current.locations().cloned().collect::<Vec<_>>();
        assert_eq!(
            provider.convert_symbol_locations_to_lsp(&raw).len(),
            4,
            "every revisioned reference should survive LSP conversion: {raw:#?}"
        );

        let references = provider
            .find_references(file_id, position, source, &uri, true)
            .expect("LSP references should resolve");
        assert_eq!(references.len(), 4, "LSP references: {references:#?}");
    }

    #[test]
    fn many_references_in_one_large_file_convert_with_one_line_index() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Many.java");
        let mut source = String::from("package p; class Many { void target() {} void use() {\n");
        for _ in 0..1_000 {
            source.push_str("// padding for a realistically large source file\n");
        }
        source.push_str("target();\n");
        source.push_str("} }");

        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index
            .index_file_with_source(&path, &source)
            .expect("large source should index");
        let file_id = workspace_index.get_file_id(&path).expect("indexed file id");
        let workspace_index = Arc::new(workspace_index);
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        host.with_db(|database| {
            database.set_file_text(file_id, Arc::new(source));
            database.set_file_language(file_id, hyperion_semantics::db::Language::Java);
        });
        let txn = host.read_now();
        let provider = NavigationProvider::new(
            &txn,
            Arc::clone(&workspace_index),
            Arc::clone(&query_engine),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        let target = SymbolTarget::new("p.Many.target()", DeclarationNamespace::Callable);
        let current = query_engine
            .find_current_references_target(&target)
            .expect("query target references");
        let seed = current
            .locations()
            .find(|location| location.name.as_ref() == "target")
            .cloned()
            .expect("one revisioned target location");
        let locations = std::iter::repeat_n(seed, 5_000).collect::<Vec<_>>();

        let started = std::time::Instant::now();
        let converted = provider.convert_symbol_locations_to_lsp(&locations);
        let elapsed = started.elapsed();

        assert_eq!(converted.len(), locations.len());
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "converting same-file references rebuilt per-location state: {elapsed:?}"
        );
    }
}
