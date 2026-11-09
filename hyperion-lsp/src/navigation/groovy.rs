// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy/Gradle navigation support
//!
//! This module provides goto-definition functionality for Groovy files,
//! including Gradle build scripts.

use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Position, Url};
use tracing::debug;

use crate::groovy_support::GradleSymbolExtractor;
use crate::navigation_util::position_to_offset;
use crate::source_provider::SourceProviderChain;

/// Helper for Groovy/Gradle navigation operations
pub struct GroovyNavigationHelper<'a> {
    workspace_index: &'a Arc<WorkspaceIndex>,
    source_providers: &'a Arc<SourceProviderChain>,
}

impl<'a> GroovyNavigationHelper<'a> {
    /// Create a new GroovyNavigationHelper
    pub const fn new(
        workspace_index: &'a Arc<WorkspaceIndex>,
        source_providers: &'a Arc<SourceProviderChain>,
    ) -> Self {
        Self {
            workspace_index,
            source_providers,
        }
    }

    /// Get goto-definition for a Gradle file position
    pub fn goto_gradle_definition(
        &self,
        _file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        debug!(
            "NAV: Computing Gradle goto-definition at position {:?}",
            position
        );

        // Extract Gradle symbols from the file
        let extractor = GradleSymbolExtractor::new();
        let result = extractor.extract(source_text);

        // Find symbol at the cursor position
        if let Some(symbol) = extractor.symbol_at(&result.symbols, position) {
            debug!(
                "NAV: Found Gradle symbol at position: {}",
                symbol.symbol.name
            );

            // For Gradle symbols, return the location of the symbol itself in the file
            let location = Location {
                uri: uri.clone(),
                range: symbol.range,
            };

            return Ok(Some(location));
        }

        // No Gradle symbol found at this position
        debug!("NAV: No Gradle symbol found at position {:?}", position);
        Ok(None)
    }

    /// Groovy-specific goto-definition implementation
    ///
    /// Uses tree-sitter to parse Groovy and find the identifier at cursor position,
    /// then resolves it through the WorkspaceIndex.
    pub fn goto_definition_groovy(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        _uri: &Url,
    ) -> Result<Option<Location>, String> {
        use hyperion_groovy::tree_sitter_wrapper::GroovyParser;

        debug!(
            "NAV: Groovy goto-definition for {:?} at {:?}",
            file_id, position
        );

        // Convert position to byte offset
        let offset = position_to_offset(source_text, position)?;

        // Parse with tree-sitter
        let mut parser = GroovyParser::new().map_err(|e| format!("{}", e))?;
        let tree = parser
            .parse_tree(source_text)
            .map_err(|e| format!("{}", e))?;
        let root = tree.root_node();

        // Find the node at the cursor position
        let node = root.descendant_for_byte_range(offset as usize, offset as usize);
        let node = match node {
            Some(n) => n,
            None => {
                debug!("NAV: No node found at offset {}", offset);
                return Ok(None);
            }
        };

        debug!(
            "NAV: Found Groovy node: kind={}, text={:?}",
            node.kind(),
            &source_text[node.start_byte()..node.end_byte()]
        );

        // Get the identifier text - walk up to find identifier if on punctuation
        let (identifier, identifier_start) = if node.kind() == "identifier"
            || node.kind() == "type_identifier"
        {
            (
                source_text[node.start_byte()..node.end_byte()].to_string(),
                node.start_byte(),
            )
        } else {
            // Try to find identifier in parent nodes
            let mut current = Some(node);
            let mut found_ident: Option<(String, usize)> = None;
            while let Some(n) = current {
                for i in 0..n.child_count() {
                    if let Some(child) = n.child(i) {
                        if child.kind() == "identifier" || child.kind() == "type_identifier" {
                            // Check if cursor is in or near this identifier
                            if child.start_byte() <= offset as usize
                                && offset as usize <= child.end_byte()
                            {
                                found_ident = Some((
                                    source_text[child.start_byte()..child.end_byte()].to_string(),
                                    child.start_byte(),
                                ));
                                break;
                            }
                        }
                    }
                }
                if found_ident.is_some() {
                    break;
                }
                current = n.parent();
            }
            match found_ident {
                Some((id, start)) => (id, start),
                None => {
                    debug!("NAV: No identifier found at position");
                    return Ok(None);
                }
            }
        };

        debug!("NAV: Looking up Groovy identifier: {}", identifier);

        // Use the same scope-aware resolver as indexing. This avoids a second,
        // regex-based model of local bindings and keeps navigation correct when
        // two methods reuse a local name, when safe navigation is used, and for
        // inherited members.
        if let Some(imports) = self.workspace_index.get_imports(file_id) {
            let usages =
                hyperion_workspace::groovy_usage_extractor::GroovyUsageExtractor::extract_from_tree(
                    source_text,
                    file_id,
                    &imports,
                    self.workspace_index,
                    &tree,
                )
                .map_err(|error| format!("Failed to resolve Groovy usages: {error}"))?;
            if let Some(usage) = usages
                .iter()
                .find(|usage| usage.location.range.start as usize == identifier_start)
            {
                if let Some(location) = self.resolve_symbol_by_fqn(&usage.fqn)? {
                    debug!("NAV: Resolved Groovy usage to {}", usage.fqn);
                    return Ok(Some(location));
                }
            }

            // Some syntactic positions are declarations rather than usages.
            // Resolve them through the canonical import environment before
            // considering a workspace-wide heuristic.
            if let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
                .workspace_index
                .resolve_type_name(file_id, &[], &identifier)
            {
                if let Some(location) = self.resolve_symbol_by_fqn(&fqn)? {
                    debug!("NAV: Resolved Groovy identifier to {}", fqn);
                    return Ok(Some(location));
                }
            }
        }

        debug!("NAV: No Groovy definition found for: {}", identifier);
        Ok(None)
    }

    /// Resolve a symbol by its fully qualified name
    fn resolve_symbol_by_fqn(&self, fqn: &str) -> Result<Option<Location>, String> {
        let Some(symbol) = self.workspace_index.find_symbol(fqn) else {
            return Ok(None);
        };
        self.symbol_location_to_lsp_location(&symbol)
    }

    fn symbol_location_to_lsp_location(
        &self,
        symbol: &hyperion_workspace::SymbolLocation,
    ) -> Result<Option<Location>, String> {
        crate::navigation_util::symbol_location_to_lsp_location(
            self.workspace_index,
            symbol,
            Some(self.source_providers),
        )
    }
}
