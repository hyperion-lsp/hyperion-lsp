// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion generator context and trait definition
//!
//! Provides the shared context and trait interface for all completion generators.

use hyperion_semantics::db::{Database, FileId};
use hyperion_semantics::symbol::Type;
use lru::LruCache;
use parking_lot::Mutex;
use std::sync::Arc;
use tower_lsp::lsp_types::{CompletionItem, Position};

/// Shared context for all completion generators
///
/// Contains all dependencies needed by completion generators to produce
/// completion items.
pub struct CompletionGeneratorContext<'a> {
    /// Salsa snapshot - acquired once per completion to avoid lock contention
    /// IMPORTANT: Use ONLY this snapshot, never acquire database lock directly
    pub snapshot: &'a salsa::Snapshot<Database>,

    /// Workspace index for global symbol lookup
    pub workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,

    /// File being completed
    pub file_id: FileId,

    /// Cursor position in the file
    pub position: Position,

    /// Source text of the file
    pub source_text: &'a str,

    /// Cache for var type inference results (LRU with max 1000 entries)
    /// Shared across all generators in the same completion request
    pub var_type_cache: Arc<Mutex<LruCache<(FileId, u32), Type>>>,

    /// Whether the client renders LSP snippet syntax (`${1:name}`). When it
    /// does not, generators emit plain insert text.
    pub snippets: bool,
}

impl<'a> CompletionGeneratorContext<'a> {
    /// Create a new completion generator context
    pub const fn new(
        snapshot: &'a salsa::Snapshot<Database>,
        workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
        file_id: FileId,
        position: Position,
        source_text: &'a str,
        var_type_cache: Arc<Mutex<LruCache<(FileId, u32), Type>>>,
    ) -> Self {
        Self {
            snapshot,
            workspace_index,
            file_id,
            position,
            source_text,
            var_type_cache,
            snippets: true,
        }
    }

    /// Set whether the client supports snippet insert text.
    #[must_use]
    pub const fn with_snippets(mut self, snippets: bool) -> Self {
        self.snippets = snippets;
        self
    }
}

/// Trait for completion generators
///
/// Each generator is responsible for producing a specific type of completion
/// (symbols, local variables, member access, etc.)
pub trait CompletionGenerator {
    /// Generate completion items for the given context
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String>;
}
