// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Apply a validated text edit and parse the resulting source.
//!
//! The public API retains the old tree argument for callers, but tree surgery
//! is not implemented. Parsing the edited source once keeps the syntax tree and
//! diagnostics consistent without spending time locating an unused region.

use crate::lexer::Lexer;
use crate::parser::{ParseError, Parser};
use rowan::{GreenNode, TextRange, TextSize};
use tracing::warn;

/// Represents a text edit operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// The range of text being replaced (in bytes)
    pub range: TextRange,
    /// The new text to insert
    pub new_text: String,
}

impl TextEdit {
    /// Create a new text edit
    pub const fn new(range: TextRange, new_text: String) -> Self {
        Self { range, new_text }
    }

    /// Get the number of bytes deleted
    pub const fn deleted_len(&self) -> TextSize {
        self.range.len()
    }

    /// Get the number of bytes inserted
    pub fn inserted_len(&self) -> TextSize {
        TextSize::from(self.new_text.len() as u32)
    }
}

/// Parse source text after applying a validated edit
///
/// The old tree is currently unused; it remains in the signature for callers
/// until a true incremental parser is available.
///
/// # Arguments
///
/// * `old_tree` - The previous GreenNode (reserved for future reuse)
/// * `old_source` - The source text that was parsed to create `old_tree`
/// * `edit` - The text edit to apply
///
/// # Returns
///
/// A tuple of `(GreenNode, Vec<ParseError>)`:
/// - The new `GreenNode` with the edit applied
/// - Any parse errors encountered (may be empty)
///
/// # Correctness
///
/// The function maintains the invariant that:
/// ```text
/// incremental_parse(old_tree, old_source, edit) == full_parse(new_source)
/// ```
///
/// Tests compare the result with parsing the edited source directly.
pub fn incremental_parse(
    _old_tree: &GreenNode,
    old_source: &str,
    edit: &TextEdit,
) -> (GreenNode, Vec<ParseError>) {
    // M3: Validate edit range before applying
    let start: usize = edit.range.start().into();
    let end: usize = edit.range.end().into();

    // Check for invalid range (start > end)
    if start > end {
        warn!(
            "Invalid edit range: start ({}) > end ({}), falling back to full parse",
            start, end
        );
        return full_parse(old_source);
    }

    // Check for range beyond source bounds
    if end > old_source.len() {
        warn!(
            "Invalid edit range: end ({}) > source length ({}), falling back to full parse",
            end,
            old_source.len()
        );
        return full_parse(old_source);
    }

    // Apply the edit to get the new source
    let new_source = match apply_edit(old_source, edit) {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "Failed to apply edit ({}). This likely indicates a client error. \
                 Falling back to full parse with old source.",
                e
            );
            // Fall back to parsing the old source unchanged
            // The client should send a full didChange with the correct source next
            return full_parse(old_source);
        }
    };

    full_parse(&new_source)
}

/// Apply a text edit to source text
///
/// Returns an error if the edit range doesn't fall on UTF-8 character boundaries.
/// This can happen if the LSP client sends byte offsets that are misaligned.
fn apply_edit(source: &str, edit: &TextEdit) -> Result<String, String> {
    let start: usize = edit.range.start().into();
    let end: usize = edit.range.end().into();

    // Validate UTF-8 boundaries to prevent panics
    if !source.is_char_boundary(start) {
        return Err(format!(
            "Start offset {start} is not on UTF-8 character boundary"
        ));
    }
    if !source.is_char_boundary(end) {
        return Err(format!(
            "End offset {end} is not on UTF-8 character boundary"
        ));
    }
    if end > source.len() {
        return Err(format!(
            "End offset {} exceeds source length {}",
            end,
            source.len()
        ));
    }
    if start > end {
        return Err(format!(
            "Start offset {start} is greater than end offset {end}"
        ));
    }

    let mut result =
        String::with_capacity(source.len().saturating_sub(end - start) + edit.new_text.len());

    result.push_str(&source[..start]);
    result.push_str(&edit.new_text);
    result.push_str(&source[end..]);

    Ok(result)
}

/// Full parse of source text
fn full_parse(source: &str) -> (GreenNode, Vec<ParseError>) {
    let tokens = Lexer::new(source).collect();
    Parser::parse(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_edit_insert() {
        let source = "class Foo { }";
        let edit = TextEdit::new(
            TextRange::new(TextSize::from(12), TextSize::from(12)),
            "void bar() {} ".to_string(),
        );

        let result = apply_edit(source, &edit).expect("valid edit");
        assert_eq!(result, "class Foo { void bar() {} }");
    }

    #[test]
    fn test_apply_edit_replace() {
        let source = "class Foo { }";
        let edit = TextEdit::new(
            TextRange::new(TextSize::from(6), TextSize::from(9)),
            "Bar".to_string(),
        );

        let result = apply_edit(source, &edit).expect("valid edit");
        assert_eq!(result, "class Bar { }");
    }

    #[test]
    fn test_apply_edit_delete() {
        let source = "class Foo { void bar() {} }";
        let edit = TextEdit::new(
            TextRange::new(TextSize::from(12), TextSize::from(26)),
            String::new(),
        );

        let result = apply_edit(source, &edit).expect("valid edit");
        assert_eq!(result, "class Foo { }");
    }

    #[test]
    fn test_incremental_parse_simple_edit() {
        let old_source = r#"class Foo {
    void bar() {
        System.out.println("old");
    }
}"#;

        let (old_tree, errors) = full_parse(old_source);
        assert!(errors.is_empty());

        // Edit: change "old" to "new"
        let old_pos = old_source.find("old").expect("should find 'old'");
        let edit = TextEdit::new(
            TextRange::new(
                TextSize::from(old_pos as u32),
                TextSize::from((old_pos + 3) as u32),
            ),
            "new".to_string(),
        );

        let (new_tree, errors) = incremental_parse(&old_tree, old_source, &edit);
        assert!(errors.is_empty());

        // Verify the result matches full parse
        let new_source = apply_edit(old_source, &edit).expect("valid edit");
        let (expected_tree, expected_errors) = full_parse(&new_source);

        assert_eq!(new_tree, expected_tree);
        assert_eq!(errors, expected_errors);
    }

    #[test]
    fn test_apply_edit_utf8_chinese_characters() {
        // Chinese characters "世界" (world) are 3 bytes each
        let source = "class 世界 { }";

        // Valid edit - replace "世界" with "Hello"
        let chinese_start = source.find('世').expect("should find Chinese char");
        let chinese_end = chinese_start + "世界".len();

        let edit = TextEdit::new(
            TextRange::new(
                TextSize::from(chinese_start as u32),
                TextSize::from(chinese_end as u32),
            ),
            "Hello".to_string(),
        );

        let result = apply_edit(source, &edit).expect("valid UTF-8 edit");
        assert_eq!(result, "class Hello { }");
    }

    #[test]
    fn test_apply_edit_utf8_boundary_error() {
        // Chinese characters "世界" are 3 bytes each
        let source = "class 世界 { }";

        let chinese_start = source.find('世').expect("should find Chinese char");

        // Invalid edit - try to slice in the middle of a multi-byte character
        // The character starts at chinese_start and is 3 bytes, so chinese_start+1 is mid-character
        let bad_edit = TextEdit::new(
            TextRange::new(
                TextSize::from((chinese_start + 1) as u32),
                TextSize::from((chinese_start + 2) as u32),
            ),
            "X".to_string(),
        );

        let error = apply_edit(source, &bad_edit)
            .expect_err("an edit inside a UTF-8 code point must be rejected");
        assert!(error.contains("UTF-8 character boundary"));
    }

    #[test]
    fn test_apply_edit_emoji() {
        // Emoji 🚀 is a 4-byte UTF-8 character
        let source = "class Test { // 🚀 rocket }";
        let emoji_pos = source.find('🚀').expect("should find emoji");

        // Valid edit - replace emoji with text
        let edit = TextEdit::new(
            TextRange::new(
                TextSize::from(emoji_pos as u32),
                TextSize::from((emoji_pos + 4) as u32),
            ),
            "Rocket".to_string(),
        );

        let result = apply_edit(source, &edit).expect("valid emoji edit");
        assert_eq!(result, "class Test { // Rocket rocket }");
    }

    #[test]
    fn test_apply_edit_emoji_boundary_error() {
        // Emoji 🚀 is a 4-byte UTF-8 character
        let source = "class Test { // 🚀 }";
        let emoji_pos = source.find('🚀').expect("should find emoji");

        // Invalid edit - try to start in middle of emoji
        let bad_edit = TextEdit::new(
            TextRange::new(
                TextSize::from((emoji_pos + 1) as u32), // Mid-emoji!
                TextSize::from((emoji_pos + 4) as u32),
            ),
            "X".to_string(),
        );

        let error = apply_edit(source, &bad_edit)
            .expect_err("an edit inside an emoji code point must be rejected");
        assert!(error.contains("UTF-8 character boundary"));
    }

    #[test]
    fn test_apply_edit_out_of_bounds() {
        let source = "class Foo { }";
        let source_len = source.len();

        // Edit extends beyond source length
        // Note: We manually construct a TextEdit with an invalid range
        // (TextRange::new would panic if end > u32::MAX, but not if end is just beyond source)
        let bad_edit = TextEdit {
            range: TextRange::new(TextSize::from(0), TextSize::from(source_len as u32 + 100)),
            new_text: "X".to_string(),
        };

        let err_msg =
            apply_edit(source, &bad_edit).expect_err("an edit beyond the source must be rejected");
        // The error could be either "exceeds source length" or "not on UTF-8 character boundary"
        // depending on which check triggers first
        assert!(
            err_msg.contains("exceeds source length")
                || err_msg.contains("UTF-8 character boundary"),
            "Unexpected error message: {err_msg}",
        );
    }

    #[test]
    fn test_apply_edit_start_equals_end_is_valid() {
        let source = "class Foo { }";

        // Start == end is valid (it's an insertion point)
        let edit = TextEdit::new(
            TextRange::new(TextSize::from(10), TextSize::from(10)),
            "X".to_string(),
        );

        let result = apply_edit(source, &edit).expect("an empty range is a valid insertion point");
        assert_eq!(result, "class Foo X{ }");
    }

    // M3: Tests for edit range validation in incremental_parse
    #[test]
    fn test_incremental_parse_invalid_range_beyond_source() {
        let old_source = "class Foo { }";
        let (old_tree, _) = full_parse(old_source);

        // Range beyond source (end > source.len())
        let bad_edit = TextEdit::new(
            TextRange::new(TextSize::from(0), TextSize::from(1000)),
            "x".to_string(),
        );

        // Should not panic, should fall back to full parse
        let (new_tree, _errors) = incremental_parse(&old_tree, old_source, &bad_edit);

        // Invalid edit range should fall back to reparsing the unchanged source.
        assert_eq!(new_tree, old_tree);
    }

    #[test]
    fn test_incremental_parse_empty_range_insertion() {
        // M3: Test that empty ranges (start == end) work correctly as insertions
        let old_source = "class Foo { }";
        let (old_tree, _) = full_parse(old_source);

        // Zero-length range (insertion) at position 6
        let insertion_edit = TextEdit::new(
            TextRange::new(TextSize::from(6), TextSize::from(6)),
            "New".to_string(),
        );

        // Should work fine
        let (new_tree, _errors) = incremental_parse(&old_tree, old_source, &insertion_edit);

        // Verify result matches full parse
        let new_source = apply_edit(old_source, &insertion_edit)
            .expect("insertion at a UTF-8 boundary must succeed");
        let (expected_tree, _) = full_parse(&new_source);
        assert_eq!(new_tree, expected_tree);
        assert_eq!(new_source, "class NewFoo { }");
    }

    #[test]
    fn test_incremental_parse_zero_length_range_at_end() {
        let old_source = "class Foo { }";
        let (old_tree, _) = full_parse(old_source);

        // Zero-length range at end of source (insertion point)
        let source_len = old_source.len() as u32;
        let edit = TextEdit::new(
            TextRange::new(TextSize::from(source_len), TextSize::from(source_len)),
            " // comment".to_string(),
        );

        // Should work correctly
        let (new_tree, _errors) = incremental_parse(&old_tree, old_source, &edit);

        // Verify result matches full parse
        let new_source =
            apply_edit(old_source, &edit).expect("insertion at end of source must succeed");
        let (expected_tree, _) = full_parse(&new_source);
        assert_eq!(new_tree, expected_tree);
    }

    #[test]
    fn test_incremental_parse_range_exactly_at_boundary() {
        let old_source = "class Foo { }";
        let (old_tree, _) = full_parse(old_source);
        let source_len = old_source.len();

        // Range exactly at source boundary (end == source.len())
        let edit = TextEdit::new(
            TextRange::new(TextSize::from(0), TextSize::from(source_len as u32)),
            "class Bar { }".to_string(),
        );

        // Should work correctly
        let (new_tree, _errors) = incremental_parse(&old_tree, old_source, &edit);

        // Verify result matches full parse
        let (expected_tree, _) = full_parse("class Bar { }");
        assert_eq!(new_tree, expected_tree);
    }
}
