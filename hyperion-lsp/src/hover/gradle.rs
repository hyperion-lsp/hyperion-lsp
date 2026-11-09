// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle Hover Support
//!
//! This module provides hover functionality for Gradle build files.

use crate::groovy_support::{self, GradleSymbolExtractor};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use tracing::debug;

/// Helper for Gradle hover functionality
pub struct GradleHoverHelper;

impl GradleHoverHelper {
    /// Create a new GradleHoverHelper
    pub const fn new() -> Self {
        Self
    }

    /// Get hover information for a Gradle file position
    pub fn hover(&self, position: Position, source_text: &str) -> Result<Option<Hover>, String> {
        debug!("Computing Gradle hover at position {:?}", position);

        // Extract Gradle symbols from the file
        let extractor = GradleSymbolExtractor::new();
        let result = extractor.extract(source_text);

        // Find symbol at the cursor position
        if let Some(symbol) = extractor.symbol_at(&result.symbols, position) {
            debug!("Found Gradle symbol at position: {}", symbol.symbol.name);

            // Format the hover text using groovy_support
            let hover_text = groovy_support::format_hover_text(&symbol);

            let hover = self.create_hover(hover_text);

            return Ok(Some(hover));
        }

        // No Gradle symbol found at this position
        debug!("No Gradle symbol found at position {:?}", position);
        Ok(None)
    }

    /// Create a Hover response with markup content
    const fn create_hover(&self, text: String) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text,
            }),
            range: None,
        }
    }
}

impl Default for GradleHoverHelper {
    fn default() -> Self {
        Self::new()
    }
}
