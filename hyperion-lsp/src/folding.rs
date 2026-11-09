// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Folding Ranges Provider
//!
//! Provides folding ranges for code regions that can be collapsed in the editor.
//! Supports folding for:
//! - Class declarations
//! - Method declarations
//! - Constructor declarations
//! - Blocks
//! - Import lists
//! - Comments

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{FileId, ParseDatabase, SourceDatabase};
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};
use tracing::debug;

use crate::position_utils::LineIndex;
use crate::semantic_host::ReadTxn;

/// Provides folding range information
#[derive(Default)]
pub struct FoldingProvider;

impl FoldingProvider {
    /// Creates a new FoldingProvider
    pub const fn new() -> Self {
        Self
    }

    /// Get folding ranges for a file
    pub fn folding_ranges(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        source_text: &str,
    ) -> Result<Vec<FoldingRange>, String> {
        debug!("Computing folding ranges for file {:?}", file_id);

        let root = {
            if txn.file_text(file_id).as_str() != source_text {
                return Ok(Vec::new());
            }
            let parse_tree = txn.parse_file(file_id);
            parse_tree.syntax()
        };

        let lines = LineIndex::new(source_text);
        let mut ranges = Vec::new();
        // Visit each syntax element once. Declaration/control-flow ancestors
        // must not invent a second region spanning their descendant blocks.
        for element in root.descendants_with_tokens() {
            let (range, kind) = match element {
                rowan::NodeOrToken::Node(node)
                    if matches!(
                        node.kind(),
                        SyntaxKind::CLASS_DECL
                            | SyntaxKind::INTERFACE_DECL
                            | SyntaxKind::ENUM_DECL
                            | SyntaxKind::ANNOTATION_DECL
                            | SyntaxKind::RECORD_DECL
                            | SyntaxKind::BLOCK_STMT
                            | SyntaxKind::SWITCH_STMT
                            | SyntaxKind::SWITCH_EXPR
                    ) =>
                {
                    let mut braces = node.children_with_tokens().filter_map(|element| {
                        element.into_token().filter(|token| {
                            matches!(token.kind(), SyntaxKind::L_BRACE | SyntaxKind::R_BRACE)
                        })
                    });
                    let Some(open) = braces
                        .next()
                        .filter(|token| token.kind() == SyntaxKind::L_BRACE)
                    else {
                        continue;
                    };
                    let Some(close) = braces
                        .last()
                        .filter(|token| token.kind() == SyntaxKind::R_BRACE)
                    else {
                        continue;
                    };
                    (
                        rowan::TextRange::new(open.text_range().start(), close.text_range().end()),
                        FoldingRangeKind::Region,
                    )
                }
                rowan::NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::DOC_COMMENT | SyntaxKind::BLOCK_COMMENT
                    ) =>
                {
                    (token.text_range(), FoldingRangeKind::Comment)
                }
                _ => continue,
            };
            let range = lines.text_range_to_lsp_range(source_text, range);
            if range.start.line < range.end.line {
                ranges.push(FoldingRange {
                    start_line: range.start.line,
                    start_character: Some(range.start.character),
                    end_line: range.end.line,
                    end_character: Some(range.end.character),
                    kind: Some(kind),
                    collapsed_text: None,
                });
            }
        }
        debug!("Generated {} folding ranges", ranges.len());
        Ok(ranges)
    }
}
