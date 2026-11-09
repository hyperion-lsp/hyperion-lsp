// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scala-specific navigation helpers
//!
//! This module contains goto-definition logic for Scala files.
//! Since Scala uses tree-sitter for symbol extraction and returns a minimal
//! rowan parse tree, we skip parse tree traversal and use direct symbol lookup.

use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use std::ops::Range;
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Position, Url};
use tracing::debug;

use crate::navigation_util::position_to_offset;
use crate::source_provider::SourceProviderChain;

/// Helper struct for Scala navigation operations
///
/// Holds references to shared resources needed for Scala symbol resolution.
pub(crate) struct ScalaNavigationHelper<'a> {
    pub workspace_index: &'a Arc<WorkspaceIndex>,
    source_providers: &'a Arc<SourceProviderChain>,
}

impl<'a> ScalaNavigationHelper<'a> {
    /// Create a new Scala navigation helper
    pub const fn new(
        workspace_index: &'a Arc<WorkspaceIndex>,
        source_providers: &'a Arc<SourceProviderChain>,
    ) -> Self {
        Self {
            workspace_index,
            source_providers,
        }
    }

    /// Scala-specific goto-definition implementation
    ///
    /// This implementation uses symbol-based lookup directly since Scala files
    /// have minimal rowan parse trees (they use tree-sitter for CST extraction).
    ///
    /// Resolution strategy:
    /// 1. Extract identifier at cursor position from source text
    /// 2. Look up symbol in workspace index
    /// 3. Return location if found
    pub fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        _uri: &Url,
    ) -> Result<Option<Location>, String> {
        debug!(
            "NAV: Scala goto-definition for {:?} at {:?}",
            file_id, position
        );

        // Convert position to offset
        let offset = position_to_offset(source_text, position)?;

        let identifier = scala_identifier_at_offset(source_text, offset as usize)?;
        debug!("NAV: Scala identifier at cursor: {}", identifier);

        if let Some(symbol) = resolve_scala_symbol_at_offset(
            self.workspace_index,
            file_id,
            source_text,
            offset as usize,
        )? {
            return self.symbol_location_to_lsp_location(&symbol);
        }

        debug!("NAV: No Scala symbol found for '{}'", identifier);
        Ok(None)
    }

    /// Convert workspace SymbolLocation to LSP Location
    fn symbol_location_to_lsp_location(
        &self,
        symbol_loc: &hyperion_workspace::SymbolLocation,
    ) -> Result<Option<Location>, String> {
        crate::navigation_util::symbol_location_to_lsp_location(
            self.workspace_index,
            symbol_loc,
            Some(self.source_providers),
        )
    }
}

/// Resolve a Scala declaration or reference under a UTF-8 byte offset using
/// the same usage resolver as workspace indexing.
pub(crate) fn resolve_scala_symbol_at_offset(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    source: &str,
    offset: usize,
) -> Result<Option<hyperion_workspace::SymbolLocation>, String> {
    let identifier_range = scala_identifier_range_at_offset(source, offset)?;
    let identifier = &source[identifier_range.clone()];

    let declaration = workspace_index
        .find_by_simple_name(identifier)
        .into_iter()
        .filter(|candidate| {
            candidate.file_id == file_id
                && candidate.range.start as usize <= identifier_range.start
                && candidate.range.end as usize >= identifier_range.end
        })
        .min_by_key(|candidate| {
            (
                candidate.range.end.saturating_sub(candidate.range.start),
                candidate.fqn.clone(),
            )
        });
    if declaration.is_some() {
        return Ok(declaration);
    }

    if let Some(imports) = workspace_index.get_imports(file_id) {
        let parsed = hyperion_scala::parser::parse(source)
            .map_err(|error| format!("Failed to parse Scala source: {error}"))?;
        let usages =
            hyperion_workspace::scala_usage_extractor::ScalaUsageExtractor::extract_from_tree(
                source,
                file_id,
                &imports,
                workspace_index,
                &parsed.tree,
            )
            .map_err(|error| format!("Failed to resolve Scala usages: {error}"))?;
        if let Some(usage) = usages
            .iter()
            .find(|usage| usage.location.range.start as usize == identifier_range.start)
        {
            if let Some(symbol) = workspace_index.find_symbol(&usage.fqn) {
                return Ok(Some(symbol));
            }
        }

        if let hyperion_workspace::TypeNameResolution::Exact(fqn) =
            workspace_index.resolve_type_name(file_id, &[], identifier)
        {
            if let Some(symbol) = workspace_index.find_symbol(&fqn) {
                return Ok(Some(symbol));
            }
        }
    }
    Ok(None)
}

/// Extract the Scala identifier adjacent to a UTF-8 byte offset.
pub(crate) fn scala_identifier_at_offset(source: &str, offset: usize) -> Result<&str, String> {
    let range = scala_identifier_range_at_offset(source, offset)?;
    Ok(&source[range])
}

pub(crate) fn scala_identifier_range_at_offset(
    source: &str,
    offset: usize,
) -> Result<Range<usize>, String> {
    if offset > source.len() {
        return Err(format!(
            "Offset {} exceeds source length {}",
            offset,
            source.len()
        ));
    }
    if !source.is_char_boundary(offset) {
        return Err(format!("Offset {offset} is not a UTF-8 character boundary"));
    }

    // Keep all coordinates in bytes. In particular, `offset == source.len()`
    // must inspect the character immediately before the cursor instead of
    // accidentally falling back to the first character in the file.
    let mut start = offset;
    while let Some(ch) = source[..start].chars().next_back() {
        if !is_identifier_char(ch) {
            break;
        }
        start -= ch.len_utf8();
    }

    let mut end = offset;
    while let Some(ch) = source[end..].chars().next() {
        if !is_identifier_char(ch) {
            break;
        }
        end += ch.len_utf8();
    }

    if start == end {
        return Err("No identifier at cursor position".to_string());
    }

    Ok(start..end)
}

/// Check if a character is valid in a Scala identifier
///
/// Scala identifiers can contain:
/// - Letters (Unicode)
/// - Digits (but not at the start)
/// - Underscores
/// - Dollar signs (but discouraged)
fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_identifier_char() {
        assert!(is_identifier_char('a'));
        assert!(is_identifier_char('Z'));
        assert!(is_identifier_char('0'));
        assert!(is_identifier_char('_'));
        assert!(is_identifier_char('$'));

        assert!(!is_identifier_char('.'));
        assert!(!is_identifier_char(' '));
        assert!(!is_identifier_char('('));
        assert!(!is_identifier_char(')'));
    }

    #[test]
    fn identifier_at_end_of_file_uses_trailing_identifier() {
        let source = "first\ntrailing";

        assert_eq!(
            scala_identifier_at_offset(source, source.len()),
            Ok("trailing")
        );
    }

    #[test]
    fn identifier_extraction_uses_byte_offsets_for_unicode() {
        let source = "val caféName = 1";
        let inside_name = source.find('é').expect("unicode identifier") + 'é'.len_utf8();

        assert_eq!(
            scala_identifier_at_offset(source, inside_name),
            Ok("caféName")
        );
    }

    #[test]
    fn identifier_extraction_rejects_invalid_offsets_and_delimiters() {
        assert!(scala_identifier_at_offset("", 0).is_err());
        assert!(scala_identifier_at_offset("é", 1).is_err());
        assert!(scala_identifier_at_offset("foo  bar", 4).is_err());
        assert_eq!(scala_identifier_at_offset("foo.bar", 3), Ok("foo"));
        assert_eq!(scala_identifier_at_offset("foo.bar", 4), Ok("bar"));
    }
}
