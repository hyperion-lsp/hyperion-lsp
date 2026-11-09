// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scala hover information provider.
//!
//! This module provides hover information for Scala files, displaying:
//! - Method signatures with parameter and return types
//! - Field declarations with types
//! - Class/trait/object declarations

use hyperion_semantics::db::FileId;
use hyperion_workspace::{SymbolLocation, WorkspaceIndex};
use std::sync::Arc;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use tracing::debug;

use crate::navigation::scala::{resolve_scala_symbol_at_offset, scala_identifier_at_offset};
use crate::navigation_util::position_to_offset;

/// Helper struct for Scala hover operations
pub(crate) struct ScalaHoverHelper<'a> {
    workspace_index: &'a Arc<WorkspaceIndex>,
}

impl<'a> ScalaHoverHelper<'a> {
    /// Create a new Scala hover helper
    pub const fn new(workspace_index: &'a Arc<WorkspaceIndex>) -> Self {
        Self { workspace_index }
    }

    /// Provide hover information for Scala files
    ///
    /// This uses the workspace index to look up symbol information and
    /// formats it as hover content with type signatures.
    pub fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        debug!(
            "HOVER: Scala hover for file {:?} at {:?}",
            file_id, position
        );

        // Convert position to offset
        let offset = position_to_offset(source_text, position)?;

        let identifier = scala_identifier_at_offset(source_text, offset as usize).ok();

        let Some(identifier) = identifier else {
            debug!("HOVER: No identifier at cursor position");
            return Ok(None);
        };

        debug!("HOVER: Scala identifier at cursor: {}", identifier);

        let symbol = resolve_scala_symbol_at_offset(
            self.workspace_index,
            file_id,
            source_text,
            offset as usize,
        )?;

        let Some(symbol) = symbol else {
            debug!("HOVER: No symbol found for '{}'", identifier);
            return Ok(None);
        };

        // Format hover content based on symbol kind
        let content = self.format_symbol_hover(&symbol);

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }))
    }

    /// Format symbol information as hover content
    ///
    /// Displays type signatures with proper Scala syntax.
    fn format_symbol_hover(&self, symbol: &SymbolLocation) -> String {
        use tower_lsp::lsp_types::SymbolKind;

        // Get the full symbol information from workspace
        let full_symbol = self.workspace_index.get_symbol(symbol.fqn.as_ref());

        match symbol.kind {
            SymbolKind::METHOD => {
                if let Some(sym) = full_symbol {
                    // Format method signature with types
                    let params = if sym.parameters.is_empty() {
                        String::new()
                    } else {
                        // Parameters are stored as type strings
                        format!("({})", sym.parameters.join(", "))
                    };

                    let return_type = sym
                        .return_type
                        .as_deref()
                        .map(|t| format!(": {}", t))
                        .unwrap_or_default();

                    format!(
                        "```scala\ndef {}{}{}```",
                        sym.simple_name.as_ref(),
                        params,
                        return_type
                    )
                } else {
                    // Fallback if full symbol not found
                    format!("```scala\ndef {}```", symbol.name.as_ref())
                }
            }

            SymbolKind::FIELD | SymbolKind::PROPERTY => {
                if let Some(sym) = full_symbol {
                    // Format field with type
                    let field_type = sym
                        .return_type
                        .as_deref()
                        .map(|t| format!(": {}", t))
                        .unwrap_or_default();

                    let keyword = if sym.modifiers.is_mutable {
                        "var"
                    } else {
                        "val"
                    };

                    format!(
                        "```scala\n{} {}{}```",
                        keyword,
                        sym.simple_name.as_ref(),
                        field_type
                    )
                } else {
                    format!("```scala\nval {}```", symbol.name.as_ref())
                }
            }

            SymbolKind::CLASS => {
                format!("```scala\nclass {}```", symbol.name.as_ref())
            }

            SymbolKind::INTERFACE => {
                // Traits map to INTERFACE
                format!("```scala\ntrait {}```", symbol.name.as_ref())
            }

            SymbolKind::OBJECT => {
                format!("```scala\nobject {}```", symbol.name.as_ref())
            }

            _ => {
                // Default format for other symbol kinds
                format!("```scala\n{}```", symbol.name.as_ref())
            }
        }
    }
}
