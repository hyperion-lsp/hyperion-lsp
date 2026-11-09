// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP coordinate conversions and incremental editing adapters.
//!
//! Coordinate conversion is shared with workspace queries so every editor-facing
//! range uses the same UTF-16 and line-ending rules.

pub(crate) use hyperion_workspace::position_utils::LineIndex;
pub use hyperion_workspace::position_utils::{
    offset_to_position, position_to_offset, position_to_offset_u32, text_range_to_lsp_range,
    utf16_offset_to_utf8, utf8_offset_to_utf16, PositionError,
};
use tower_lsp::lsp_types::Range;

/// Applies an incremental text change to a document.
///
/// This function converts LSP Range coordinates (line/character) into byte offsets
/// and applies the text change in-place.
///
/// # Arguments
///
/// * `text` - The current document text (will be modified in-place)
/// * `range` - The range of text to replace (in LSP coordinates)
/// * `new_text` - The new text to insert
///
/// # Returns
///
/// * `Ok(())` - If the change was applied successfully
/// * `Err(PositionError)` - If the range is invalid
///
/// # LSP Specification
///
/// LSP positions are (line, character) tuples where:
/// - Lines are 0-indexed
/// - Characters are UTF-16 code units
///
/// Rust strings use UTF-8 encoding, so we need to convert between UTF-16 positions
/// and UTF-8 byte offsets carefully.
pub fn apply_incremental_change(
    text: &mut String,
    range: Range,
    new_text: &str,
) -> Result<(), PositionError> {
    let (start_offset, end_offset) = LineIndex::new(text).range_to_offsets(text, range)?;

    // Replace the text in the range
    text.replace_range(start_offset..end_offset, new_text);
    Ok(())
}

/// Convert an LSP Range and new text to a hyperion_parser::TextEdit
///
/// This enables incremental parsing by converting LSP text changes to the format
/// expected by the incremental parser.
///
/// # Arguments
///
/// * `source` - The current source text (before the edit)
/// * `range` - The LSP Range being replaced
/// * `new_text` - The new text to insert
///
/// # Returns
///
/// A `hyperion_parser::TextEdit` suitable for incremental parsing, or an error
/// if the range is invalid.
///
/// # Example
///
/// ```rust
/// use hyperion_lsp::position_utils::lsp_range_to_text_edit;
/// use tower_lsp::lsp_types::{Position, Range};
///
/// let source = "class Foo { }";
/// let range = Range::new(Position::new(0, 6), Position::new(0, 9));
/// let edit = lsp_range_to_text_edit(source, range, "Bar").unwrap();
/// // edit can now be passed to incremental_parse()
/// ```
pub fn lsp_range_to_text_edit(
    source: &str,
    range: Range,
    new_text: &str,
) -> Result<hyperion_parser::TextEdit, PositionError> {
    let (start_offset, end_offset) = LineIndex::new(source).range_to_offsets(source, range)?;

    use rowan::TextRange;
    Ok(hyperion_parser::TextEdit::new(
        TextRange::new(
            offset_to_text_size(start_offset)?,
            offset_to_text_size(end_offset)?,
        ),
        new_text.to_string(),
    ))
}

fn offset_to_text_size(offset: usize) -> Result<rowan::TextSize, PositionError> {
    let offset = u32::try_from(offset).map_err(|_| PositionError::OffsetTooLarge { offset })?;
    Ok(rowan::TextSize::from(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rowan_offsets_reject_values_that_do_not_fit() {
        let oversized = usize::try_from(u64::from(u32::MAX) + 1);
        if let Ok(oversized) = oversized {
            assert_eq!(
                offset_to_text_size(oversized),
                Err(PositionError::OffsetTooLarge { offset: oversized })
            );
        }
    }
}
