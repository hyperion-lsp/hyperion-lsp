// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Position and Offset Conversion Utilities
//!
//! This module provides centralized utilities for converting between:
//! - LSP Position (line, character in UTF-16 code units)
//! - Byte offsets (UTF-8)
//! - rowan TextRange (used by the parser)
//!
//! # Coordinate Systems
//!
//! - **LSP Position**: (line: u32, character: u32) where character is UTF-16 code units
//! - **Byte Offset**: usize representing UTF-8 byte position in source text
//! - **rowan TextRange**: Byte range using rowan::TextSize
//!
//! # LSP Specification
//!
//! According to the LSP spec, positions use UTF-16 code units for the character offset.
//! This is because many editors (VSCode, etc.) use UTF-16 internally.
//! Rust strings are UTF-8, so we need careful conversion.
//!
//! # Examples
//!
//! ```rust
//! use hyperion_workspace::position_utils::*;
//! use lsp_types::Position;
//!
//! let source = "class Foo { }";
//!
//! // Convert LSP position to byte offset
//! let pos = Position::new(0, 6); // Points to 'F' in "Foo"
//! let offset = position_to_offset(source, pos).unwrap();
//! assert_eq!(offset, 6);
//!
//! // Convert byte offset to LSP position
//! let back = offset_to_position(source, offset);
//! assert_eq!(back, pos);
//! ```

use lsp_types::{Position, Range};
use thiserror::Error;

/// Line starts for one immutable document revision.
///
/// Construct this once when converting several positions from the same text.
/// Every conversion must receive the same immutable source used at construction.
#[derive(Clone, Debug)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        Self {
            line_starts: compute_line_starts(source),
        }
    }

    pub fn position_to_offset(
        &self,
        source: &str,
        position: Position,
    ) -> Result<usize, PositionError> {
        let line_idx = position.line as usize;
        if line_idx >= self.line_starts.len() {
            return Err(PositionError::PositionOutOfRange {
                position,
                line_count: self.line_starts.len(),
            });
        }
        let line_start = self.line_starts[line_idx];
        if position.character == 0 {
            return Ok(line_start);
        }
        let line_end = line_content_end(source, &self.line_starts, line_idx);
        let target_col = position.character;
        let mut col = 0u32;
        let mut offset = line_start;
        while offset < line_end {
            let Some(ch) = source[offset..].chars().next() else {
                break;
            };
            let next_col = col.saturating_add(ch.len_utf16() as u32);
            if next_col > target_col {
                return Err(PositionError::CharacterNotReachable {
                    line: position.line,
                    character: position.character,
                });
            }
            col = next_col;
            offset += ch.len_utf8();
            if col == target_col {
                return Ok(offset);
            }
        }
        if col == target_col {
            Ok(offset)
        } else {
            Err(PositionError::PositionOutOfRange {
                position,
                line_count: self.line_starts.len(),
            })
        }
    }

    pub fn offset_to_position(&self, source: &str, offset: usize) -> Position {
        let offset = normalize_offset(source, offset);
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let mut col = 0u32;
        let mut current_offset = self.line_starts[line_idx];
        while current_offset < offset {
            let Some(ch) = source[current_offset..].chars().next() else {
                break;
            };
            col = col.saturating_add(ch.len_utf16() as u32);
            current_offset += ch.len_utf8();
        }
        Position::new(line_idx as u32, col)
    }

    pub fn range_to_offsets(
        &self,
        source: &str,
        range: Range,
    ) -> Result<(usize, usize), PositionError> {
        let start = self.position_to_offset(source, range.start)?;
        let end = self.position_to_offset(source, range.end)?;
        if start > end {
            return Err(PositionError::ReversedRange { start, end });
        }
        Ok((start, end))
    }

    pub fn text_range_to_lsp_range(&self, source: &str, range: rowan::TextRange) -> Range {
        Range::new(
            self.offset_to_position(source, range.start().into()),
            self.offset_to_position(source, range.end().into()),
        )
    }
}

/// Failures while translating between UTF-16 LSP coordinates and byte offsets.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PositionError {
    #[error("Position out of range: {position:?} (source has {line_count} lines)")]
    PositionOutOfRange {
        position: Position,
        line_count: usize,
    },
    #[error(
        "Position character {character} not reachable in line {line} (line too long or position overflow)"
    )]
    CharacterNotReachable { line: u32, character: u32 },
    #[error("Invalid range: start ({start}) > end ({end})")]
    ReversedRange { start: usize, end: usize },
    #[error("Byte offset {offset} exceeds the LSP engine's u32 offset range")]
    OffsetTooLarge { offset: usize },
}

/// Allows typed position failures to cross older string-error provider boundaries
/// while those domains are migrated independently.
impl From<PositionError> for String {
    fn from(error: PositionError) -> Self {
        error.to_string()
    }
}
use tracing::debug;

fn compute_line_starts(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut starts = Vec::with_capacity(source.len() / 32 + 1);
    starts.push(0);

    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 2;
                } else {
                    i += 1;
                }
                starts.push(i);
            }
            b'\n' => {
                i += 1;
                starts.push(i);
            }
            _ => {
                if let Some(ch) = source[i..].chars().next() {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
        }
    }

    starts
}

fn line_content_end(source: &str, line_starts: &[usize], line_idx: usize) -> usize {
    let line_start = line_starts[line_idx];
    let mut end = line_starts
        .get(line_idx + 1)
        .copied()
        .unwrap_or(source.len());
    if end <= line_start {
        return end;
    }

    let bytes = source.as_bytes();
    if bytes[end - 1] == b'\n' {
        end -= 1;
        if end > line_start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
    } else if bytes[end - 1] == b'\r' {
        end -= 1;
    }

    end
}

/// Converts an LSP Position (line, UTF-16 character) to a byte offset.
///
/// # Arguments
///
/// * `source` - The source text
/// * `position` - The LSP position (line and UTF-16 character offset)
///
/// # Returns
///
/// * `Ok(offset)` - The byte offset corresponding to the position
/// * `Err(PositionError)` - If the position is not a valid UTF-16 boundary
///
/// # Examples
///
/// ```ignore
/// let source = "hello\nworld";
/// assert_eq!(position_to_offset(source, Position::new(0, 0)).unwrap(), 0);
/// assert_eq!(position_to_offset(source, Position::new(1, 0)).unwrap(), 6);
/// assert_eq!(position_to_offset(source, Position::new(1, 5)).unwrap(), 11);
/// ```
pub fn position_to_offset(source: &str, position: Position) -> Result<usize, PositionError> {
    LineIndex::new(source).position_to_offset(source, position)
}

/// Converts an LSP position to the `u32` byte-offset representation used by
/// rowan and Hyperion's semantic indices.
pub fn position_to_offset_u32(source: &str, position: Position) -> Result<u32, PositionError> {
    let offset = position_to_offset(source, position)?;
    u32::try_from(offset).map_err(|_| PositionError::OffsetTooLarge { offset })
}

/// Converts a byte offset to an LSP Position (line, UTF-16 character).
///
/// # Arguments
///
/// * `source` - The source text
/// * `offset` - The byte offset into the text
///
/// # Returns
///
/// The LSP Position (line, character in UTF-16 units)
///
/// # Behavior
///
/// If the offset is beyond the end of the source, returns a position at the end
/// of the last line.
///
/// # Examples
///
/// ```ignore
/// let source = "hello\nworld";
/// assert_eq!(offset_to_position(source, 0), Position::new(0, 0));
/// assert_eq!(offset_to_position(source, 6), Position::new(1, 0));
/// assert_eq!(offset_to_position(source, 11), Position::new(1, 5));
/// ```
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    LineIndex::new(source).offset_to_position(source, offset)
}

fn normalize_offset(source: &str, offset: usize) -> usize {
    // Clamp offset to valid UTF-8 character boundary
    // If offset is in the middle of a multi-byte character, round down to the previous boundary
    let offset = if offset >= source.len() {
        if offset > source.len() {
            debug!(
                "WARN: offset_to_position: offset {} is beyond file length {} (clamping to end)",
                offset,
                source.len()
            );
        }
        source.len()
    } else if !source.is_char_boundary(offset) {
        debug!("WARN: offset_to_position: offset {} is not at char boundary (rounding down to previous boundary)", offset);
        // Find the previous character boundary (round down)
        // This ensures we point to the start of the character containing the offset
        (0..=offset)
            .rev()
            .find(|&i| source.is_char_boundary(i))
            .unwrap_or(0)
    } else {
        offset
    };

    let bytes = source.as_bytes();

    // CRLF edge case: if offset points to \n in a \r\n pair, round down to \r
    // Per LSP spec, you cannot specify a position "inside" a \r\n line terminator.
    // This ensures roundtrip: offset_to_position(n) -> position_to_offset(pos) == n
    let offset = if offset > 0 && offset < source.len() {
        if bytes[offset] == b'\n' && bytes[offset - 1] == b'\r' {
            debug!(
                "WARN: offset_to_position: offset {} points to \\n in \\r\\n pair (rounding down to \\r)",
                offset
            );
            offset - 1
        } else {
            offset
        }
    } else {
        offset
    };

    offset
}

/// Converts a UTF-16 character offset to a UTF-8 byte offset within a line.
///
/// This is used when applying incremental text edits where the LSP client
/// provides character offsets in UTF-16 code units.
///
/// # Arguments
///
/// * `line` - The line of text (should not contain newline)
/// * `utf16_offset` - The character offset in UTF-16 code units
///
/// # Returns
///
/// The corresponding byte offset in UTF-8 encoding
///
/// # Note
///
/// If the UTF-16 offset falls in the middle of a multi-code-unit character
/// (like an emoji surrogate pair), this function will round to the end of
/// that character. LSP clients should always provide valid character boundaries.
///
/// # Examples
///
/// ```ignore
/// // ASCII: 1 UTF-8 byte = 1 UTF-16 code unit
/// assert_eq!(utf16_offset_to_utf8("hello", 3), 3);
///
/// // Multibyte: '世' is 3 UTF-8 bytes, 1 UTF-16 code unit
/// assert_eq!(utf16_offset_to_utf8("hello 世界", 7), 9);
/// ```
pub fn utf16_offset_to_utf8(line: &str, utf16_offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;

    for ch in line.chars() {
        if utf16_count >= utf16_offset {
            break;
        }

        utf8_offset += ch.len_utf8();
        utf16_count += ch.len_utf16();
    }

    // If we didn't reach the exact offset, we were beyond the string length
    // or in the middle of a multi-code-unit character
    if utf16_count != utf16_offset && utf16_count > 0 {
        debug!(
            "WARN: utf16_offset_to_utf8: requested UTF-16 offset {} doesn't align with character boundary (actual: {})",
            utf16_offset, utf16_count
        );
    }

    utf8_offset
}

/// Converts a UTF-8 byte offset to a UTF-16 code unit offset.
///
/// This is the inverse of `utf16_offset_to_utf8`.
///
/// # Arguments
///
/// * `text` - The text to measure (UTF-8)
///
/// # Returns
///
/// The length in UTF-16 code units
///
/// # Examples
///
/// ```ignore
/// // ASCII characters (1 UTF-8 byte, 1 UTF-16 code unit each)
/// assert_eq!(utf8_offset_to_utf16("hello"), 5);
///
/// // '世' is 3 UTF-8 bytes, 1 UTF-16 code unit
/// // '界' is 3 UTF-8 bytes, 1 UTF-16 code unit
/// assert_eq!(utf8_offset_to_utf16("hello 世界"), 8);
/// ```
pub fn utf8_offset_to_utf16(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Converts a rowan TextRange to an LSP Range.
///
/// This function converts byte-based positions (TextRange) to line/column-based
/// positions (LSP Range) by scanning through the source text.
///
/// # Arguments
///
/// * `text_range` - The rowan TextRange to convert (byte offsets)
/// * `source_text` - The source text of the document
///
/// # Returns
///
/// An LSP Range with line/column positions
pub fn text_range_to_lsp_range(text_range: rowan::TextRange, source_text: &str) -> Range {
    LineIndex::new(source_text).text_range_to_lsp_range(source_text, text_range)
}

#[cfg(test)]
mod line_index_tests {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn shared_index_handles_utf16_unicode_and_crlf() {
        let source = "ascii\r\nA😀界\r\ntail";
        let lines = LineIndex::new(source);

        assert_eq!(lines.position_to_offset(source, Position::new(1, 0)), Ok(7));
        assert_eq!(lines.position_to_offset(source, Position::new(1, 1)), Ok(8));
        assert_eq!(
            lines.position_to_offset(source, Position::new(1, 3)),
            Ok(12)
        );
        assert_eq!(lines.offset_to_position(source, 12), Position::new(1, 3));
        assert_eq!(lines.offset_to_position(source, 17), Position::new(2, 0));
        assert_eq!(
            lines.position_to_offset(source, Position::new(1, 2)),
            Err(PositionError::CharacterNotReachable {
                line: 1,
                character: 2,
            })
        );
        // The byte between CR and LF cannot be represented by LSP and rounds
        // down to the end of the line content.
        assert_eq!(lines.offset_to_position(source, 16), Position::new(1, 4));
    }

    #[test]
    fn one_line_index_build_serves_a_batch_of_ranges() {
        let mut source = String::new();
        for line in 0..10_000 {
            writeln!(source, "line {line} 😀\r").expect("writing to a String cannot fail");
        }
        let lines = LineIndex::new(&source);
        let starts_ptr = lines.line_starts.as_ptr();
        let starts_capacity = lines.line_starts.capacity();
        for offset in (0..source.len()).step_by(97) {
            let _ = lines.offset_to_position(&source, offset);
        }
        assert_eq!(lines.line_starts.as_ptr(), starts_ptr);
        assert_eq!(lines.line_starts.capacity(), starts_capacity);
        assert_eq!(lines.line_starts.len(), 10_001);
    }
}
