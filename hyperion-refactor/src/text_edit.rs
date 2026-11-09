// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Text Edit Utilities
//!
//! Provides utilities for representing and applying text edits to source code.
//! These are the low-level primitives used by all refactoring operations.
//!
//! # Design
//!
//! - `TextEdit` represents a single atomic edit (replace a range with new text)
//! - Edits can be applied in bulk with automatic conflict detection
//! - Supports both simple replacements and complex multi-edit operations
//!
//! # Example
//!
//! ```ignore
//! use hyperion_refactor::text_edit::TextEdit;
//! use rowan::TextRange;
//!
//! let source = "Hello, world!";
//! let edits = vec![
//!     TextEdit::new(TextRange::new(0.into(), 5.into()), "Hi".to_string()),
//!     TextEdit::new(TextRange::new(7.into(), 12.into()), "universe".to_string()),
//! ];
//!
//! let result = TextEdit::apply_edits(source, &edits);
//! assert_eq!(result, "Hi, universe!");
//! ```

use rowan::{TextRange, TextSize};
use std::cmp::Ordering;

/// Represents a single text edit operation
///
/// A text edit replaces a range of text with new content. The range can be:
/// - Empty (insertion at a point)
/// - Non-empty (replacement or deletion)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// The range to replace (can be empty for insertion)
    pub range: TextRange,
    /// The new text to insert (can be empty for deletion)
    pub new_text: String,
}

impl TextEdit {
    /// Create a new text edit
    ///
    /// # Arguments
    /// * `range` - The range of text to replace
    /// * `new_text` - The new text to insert
    pub fn new(range: TextRange, new_text: String) -> Self {
        Self { range, new_text }
    }

    /// Create an insertion edit at a specific position
    pub fn insert(position: TextSize, text: String) -> Self {
        Self {
            range: TextRange::empty(position),
            new_text: text,
        }
    }

    /// Create a deletion edit for a specific range
    pub fn delete(range: TextRange) -> Self {
        Self {
            range,
            new_text: String::new(),
        }
    }

    /// Create a replacement edit
    pub fn replace(range: TextRange, new_text: String) -> Self {
        Self { range, new_text }
    }

    /// Check if this is an insertion (empty range)
    pub fn is_insertion(&self) -> bool {
        self.range.is_empty()
    }

    /// Check if this is a deletion (empty new text)
    pub fn is_deletion(&self) -> bool {
        self.new_text.is_empty()
    }

    /// Check if this is a replacement (both range and new text non-empty)
    pub fn is_replacement(&self) -> bool {
        !self.range.is_empty() && !self.new_text.is_empty()
    }

    /// Get the start position of this edit
    pub fn start(&self) -> TextSize {
        self.range.start()
    }

    /// Get the end position of this edit
    pub fn end(&self) -> TextSize {
        self.range.end()
    }

    /// Apply multiple edits to source text
    ///
    /// This function applies a set of edits to source text in a single pass.
    /// The edits are automatically sorted by position, and overlapping edits
    /// will cause a panic.
    ///
    /// # Arguments
    /// * `source` - The original source text
    /// * `edits` - The edits to apply (order doesn't matter)
    ///
    /// # Returns
    /// The modified source text after all edits are applied
    ///
    /// # Panics
    /// Panics if edits overlap, are out of bounds, or split a UTF-8 code point.
    pub fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
        Self::try_apply_edits(source, edits).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Try to apply edits, returning an error if they're invalid
    ///
    /// This is a fallible version of `apply_edits` that returns Result
    /// instead of panicking.
    ///
    /// # Arguments
    /// * `source` - The original source text
    /// * `edits` - The edits to apply
    ///
    /// # Returns
    /// * `Ok(String)` - The modified text
    /// * `Err(String)` - Error description
    pub fn try_apply_edits(source: &str, edits: &[TextEdit]) -> Result<String, String> {
        if edits.is_empty() {
            return Ok(source.to_string());
        }

        // Sort edits by start position
        let mut sorted_edits = edits.to_vec();
        sorted_edits.sort_by(|a, b| {
            a.range
                .start()
                .cmp(&b.range.start())
                .then_with(|| a.range.end().cmp(&b.range.end()))
        });

        // Check for overlaps
        for window in sorted_edits.windows(2) {
            let curr = &window[0];
            let next = &window[1];
            if curr.range.end() > next.range.start() {
                return Err(format!(
                    "Overlapping edits: {:?} and {:?}",
                    curr.range, next.range
                ));
            }
        }

        let mut result = String::with_capacity(source.len());
        let mut last_pos = 0usize;

        for edit in sorted_edits {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();

            // Validate bounds
            if start > source.len() || end > source.len() {
                return Err(format!(
                    "Edit out of bounds: range {:?}, source length {}",
                    edit.range,
                    source.len()
                ));
            }
            if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
                return Err(format!(
                    "Edit range {:?} is not on UTF-8 character boundaries",
                    edit.range
                ));
            }

            // Add unchanged text before this edit
            result.push_str(&source[last_pos..start]);

            // Add the new text
            result.push_str(&edit.new_text);

            last_pos = end;
        }

        // Add remaining text after last edit
        result.push_str(&source[last_pos..]);

        Ok(result)
    }
}

impl PartialOrd for TextEdit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TextEdit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.range
            .start()
            .cmp(&other.range.start())
            .then_with(|| self.range.end().cmp(&other.range.end()))
            .then_with(|| self.new_text.cmp(&other.new_text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_replacement() {
        let source = "Hello, world!";
        let edit = TextEdit::new(TextRange::new(7.into(), 12.into()), "universe".to_string());

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, universe!");
    }

    #[test]
    fn test_insertion() {
        let source = "Hello!";
        let edit = TextEdit::insert(5.into(), ", world".to_string());

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_deletion() {
        let source = "Hello, cruel world!";
        let edit = TextEdit::delete(TextRange::new(6.into(), 12.into()));

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_multiple_edits() {
        let source = "one two three";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 3.into()), "1".to_string()),
            TextEdit::replace(TextRange::new(4.into(), 7.into()), "2".to_string()),
            TextEdit::replace(TextRange::new(8.into(), 13.into()), "3".to_string()),
        ];

        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "1 2 3");
    }

    #[test]
    fn test_multiple_edits_unsorted() {
        let source = "abc def ghi";
        let edits = vec![
            TextEdit::replace(TextRange::new(8.into(), 11.into()), "3".to_string()),
            TextEdit::replace(TextRange::new(0.into(), 3.into()), "1".to_string()),
            TextEdit::replace(TextRange::new(4.into(), 7.into()), "2".to_string()),
        ];

        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "1 2 3");
    }

    #[test]
    fn test_empty_edits() {
        let source = "unchanged";
        let result = TextEdit::apply_edits(source, &[]);
        assert_eq!(result, "unchanged");
    }

    #[test]
    fn test_adjacent_edits() {
        let source = "abcdef";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 3.into()), "123".to_string()),
            TextEdit::replace(TextRange::new(3.into(), 6.into()), "456".to_string()),
        ];

        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "123456");
    }

    #[test]
    #[should_panic(expected = "Overlapping edits")]
    fn test_overlapping_edits_panic() {
        let source = "abcdef";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 4.into()), "1234".to_string()),
            TextEdit::replace(TextRange::new(2.into(), 6.into()), "5678".to_string()),
        ];

        TextEdit::apply_edits(source, &edits);
    }

    #[test]
    fn test_try_apply_overlapping_edits() {
        let source = "abcdef";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 4.into()), "1234".to_string()),
            TextEdit::replace(TextRange::new(2.into(), 6.into()), "5678".to_string()),
        ];

        let result = TextEdit::try_apply_edits(source, &edits);
        assert!(result.is_err());
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_out_of_bounds_edit_panic() {
        let source = "short";
        let edit = TextEdit::replace(TextRange::new(0.into(), 100.into()), "x".to_string());

        TextEdit::apply_edits(source, &[edit]);
    }

    #[test]
    fn test_try_apply_out_of_bounds() {
        let source = "short";
        let edit = TextEdit::replace(TextRange::new(0.into(), 100.into()), "x".to_string());

        let result = TextEdit::try_apply_edits(source, &[edit]);
        assert!(result.is_err());
    }

    #[test]
    fn test_try_apply_rejects_non_utf8_boundary() {
        let source = "aéz";
        let edit = TextEdit::replace(TextRange::new(1.into(), 2.into()), "e".to_string());

        let result = TextEdit::try_apply_edits(source, &[edit]);

        assert!(result.is_err_and(|error| error.contains("UTF-8 character boundaries")));
    }

    #[test]
    fn test_insertion_at_start() {
        let source = "world";
        let edit = TextEdit::insert(0.into(), "Hello, ".to_string());

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world");
    }

    #[test]
    fn test_insertion_at_end() {
        let source = "Hello";
        let edit = TextEdit::insert(5.into(), ", world!".to_string());

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_replace_entire_text() {
        let source = "old text";
        let edit = TextEdit::replace(
            TextRange::new(0.into(), 8.into()),
            "completely new".to_string(),
        );

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "completely new");
    }

    #[test]
    fn test_delete_entire_text() {
        let source = "delete me";
        let edit = TextEdit::delete(TextRange::new(0.into(), 9.into()));

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_is_methods() {
        let insert = TextEdit::insert(0.into(), "text".to_string());
        assert!(insert.is_insertion());
        assert!(!insert.is_deletion());
        assert!(!insert.is_replacement());

        let delete = TextEdit::delete(TextRange::new(0.into(), 5.into()));
        assert!(!delete.is_insertion());
        assert!(delete.is_deletion());
        assert!(!delete.is_replacement());

        let replace = TextEdit::replace(TextRange::new(0.into(), 5.into()), "new".to_string());
        assert!(!replace.is_insertion());
        assert!(!replace.is_deletion());
        assert!(replace.is_replacement());
    }

    #[test]
    fn test_complex_multi_edit() {
        let source = "The quick brown fox jumps over the lazy dog";
        let edits = vec![
            TextEdit::replace(TextRange::new(4.into(), 9.into()), "slow".to_string()),
            TextEdit::replace(TextRange::new(10.into(), 15.into()), "red".to_string()),
            TextEdit::replace(TextRange::new(20.into(), 25.into()), "walks".to_string()),
            TextEdit::replace(
                TextRange::new(35.into(), 39.into()),
                "energetic".to_string(),
            ),
        ];

        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "The slow red fox walks over the energetic dog");
    }

    #[test]
    fn test_unicode_text() {
        let source = "Hello, 世界!";
        let edit = TextEdit::replace(
            TextRange::new(7.into(), 13.into()), // Note: 世界 is 6 bytes
            "world".to_string(),
        );

        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world!");
    }
}
