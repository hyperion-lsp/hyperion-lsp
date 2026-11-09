// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::JavaLanguage;
use hyperion_semantics::db::{FileId, ParseTree};
use hyperion_workspace::WorkspaceIndex;
use rowan::SyntaxToken;
use std::sync::Arc;
use tower_lsp::lsp_types::{Position, Url};

/// Shared, read-only data used by the individual navigation resolvers.
///
/// The context owns the parse tree and exposes the frequently accessed
/// inputs (like the cursor offset and the primary syntax token) so that the
/// resolvers can focus on their specific matching logic.
pub struct NavigationContext<'a> {
    pub file_id: FileId,
    pub position: Position,
    pub offset: usize,
    pub uri: &'a Url,
    pub source_text: &'a str,
    pub parse_tree: &'a ParseTree,
    pub token: SyntaxToken<JavaLanguage>,
    pub workspace_index: Arc<WorkspaceIndex>,
}

impl<'a> NavigationContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        file_id: FileId,
        position: Position,
        uri: &'a Url,
        source_text: &'a str,
        parse_tree: &'a ParseTree,
        offset: usize,
        token: SyntaxToken<JavaLanguage>,
        workspace_index: Arc<WorkspaceIndex>,
    ) -> Self {
        Self {
            file_id,
            position,
            offset,
            uri,
            source_text,
            parse_tree,
            token,
            workspace_index,
        }
    }
}
