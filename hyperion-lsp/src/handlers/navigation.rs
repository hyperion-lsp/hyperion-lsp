// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation request handlers: goto-definition, goto-implementation,
//! find-references, document-highlight, document/workspace symbols.
//!
//! Extracted from `server.rs` for maintainability.

use hyperion_semantics::db::FileId;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, GotoDefinitionResponse, Location, Url,
};
use tracing::debug;

use crate::navigation::NavigationProvider;
use crate::type_hierarchy::TypeHierarchyProvider;

/// Resolve goto-implementation by finding subtypes via the type hierarchy.
///
/// Falls back to the definition location if no implementations are found.
/// Shared implementation of the LSP goto-implementation request.
///
/// Kept public so integration tests and non-server embeddings exercise the
/// exact production dispatch instead of maintaining a second copy.
pub fn resolve_goto_implementation(
    navigation_provider: &NavigationProvider<'_>,
    type_hierarchy_provider: &TypeHierarchyProvider,
    file_id: FileId,
    position: tower_lsp::lsp_types::Position,
    source_text: &str,
    uri: &Url,
) -> Option<GotoDefinitionResponse> {
    // First, resolve what symbol we're on
    let definition = match navigation_provider.goto_definition(file_id, position, source_text, uri)
    {
        Ok(loc) => loc,
        Err(e) => {
            debug!("Goto definition for implementation failed: {}", e);
            None
        }
    };

    let def_location = definition?;

    match navigation_provider.method_implementation_locations(
        file_id,
        position,
        source_text,
        &def_location,
    ) {
        Ok(Some(locations)) if !locations.is_empty() => {
            debug!("Found {} method implementations", locations.len());
            return locations_to_goto_response(locations);
        }
        Ok(_) => {}
        Err(error) => debug!("Method implementation resolution failed: {error}"),
    }

    // Try to get the type hierarchy item for this symbol
    let type_item = type_hierarchy_provider.prepare_type_hierarchy(file_id, position, source_text);

    match type_item {
        Ok(Some(items)) if !items.is_empty() => {
            let item = &items[0];
            match type_hierarchy_provider.implementations(item) {
                Ok(Some(subtypes)) if !subtypes.is_empty() => {
                    let locations: Vec<Location> = subtypes
                        .into_iter()
                        .map(|t| Location {
                            uri: t.uri,
                            range: t.selection_range,
                        })
                        .collect();

                    debug!("Found {} implementations", locations.len());
                    locations_to_goto_response(locations)
                }
                _ => {
                    debug!("No implementations found, returning definition");
                    Some(GotoDefinitionResponse::Scalar(def_location))
                }
            }
        }
        _ => {
            debug!("Couldn't get type hierarchy, returning definition");
            Some(GotoDefinitionResponse::Scalar(def_location))
        }
    }
}

pub(crate) fn locations_to_goto_response(
    mut locations: Vec<Location>,
) -> Option<GotoDefinitionResponse> {
    match locations.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(locations.pop()?)),
        _ => Some(GotoDefinitionResponse::Array(locations)),
    }
}

/// Convert find-references locations into DocumentHighlight entries,
/// filtered to the current file only.
#[allow(clippy::single_option_map)]
pub(crate) fn document_highlights_from_references(
    locations: Option<Vec<Location>>,
    uri: &Url,
) -> Option<Vec<DocumentHighlight>> {
    locations.map(|locs| {
        locs.into_iter()
            .filter(|loc| loc.uri == *uri)
            .map(|loc| DocumentHighlight {
                range: loc.range,
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect::<Vec<_>>()
    })
}
