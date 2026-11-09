// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Formatting
//!
//! Provides code formatting capabilities using hyperion-fmt (native Rust formatter).
//!
//! Supports:
//! - Document formatting (format entire file)
//! - Range formatting (format selected region)
//!
//! Uses palantir-java-format style by default:
//! - 120 character line width
//! - 4 space indentation

use hyperion_fmt::{format_source as fmt_source, FormatConfig};
use tower_lsp::lsp_types::{Position, Range, TextEdit};
use tracing::debug;

use crate::position_utils::{offset_to_position, LineIndex};

/// Formats a Java source file using hyperion-fmt (palantir-java-format style)
///
/// # Arguments
/// * `source` - The source code to format
/// * `range` - Optional range to format (None formats entire document)
///
/// # Returns
/// * `Ok(Vec<TextEdit>)` - Text edits to apply the formatting
/// * `Err(String)` - Error message if formatting failed
pub fn format_source(source: &str, range: Option<Range>) -> Result<Vec<TextEdit>, String> {
    let config = FormatConfig::default();

    // Format the source
    let formatted = if let Some(r) = range {
        // Convert LSP range to byte range
        let (start_byte, end_byte) = LineIndex::new(source)
            .range_to_offsets(source, r)
            .map_err(|error| format!("Invalid formatting range: {error}"))?;

        hyperion_fmt::format_range(source, start_byte..end_byte, &config)
            .map_err(|e| format!("Formatting failed: {e}"))?
    } else {
        fmt_source(source, &config).map_err(|e| format!("Formatting failed: {e}"))?
    };

    // If source is already formatted, return empty edit list
    if formatted == source {
        debug!("Source already formatted");
        return Ok(Vec::new());
    }

    // For range formatting, return a minimal edit instead of the entire file.
    if range.is_some() {
        if let Some((start, end, start_f, end_f)) = diff_range(source, &formatted) {
            let start = clamp_to_char_boundary(source, start);
            let end = clamp_to_char_boundary(source, end);
            let start_f = clamp_to_char_boundary(&formatted, start_f);
            let end_f = clamp_to_char_boundary(&formatted, end_f);

            if start == end && start_f == end_f {
                return Ok(Vec::new());
            }

            let edit_range = Range {
                start: offset_to_position(source, start),
                end: offset_to_position(source, end),
            };
            if let Some(requested) = range {
                let first_allowed_line = requested.start.line.saturating_sub(1);
                let last_allowed_line = requested.end.line.saturating_add(1);
                if edit_range.start.line < first_allowed_line
                    || edit_range.start.line > last_allowed_line
                    || edit_range.end.line > last_allowed_line
                {
                    debug!(
                        "Range formatter expanded {:?} to unrelated edit {:?}; returning no edit",
                        requested, edit_range
                    );
                    return Ok(Vec::new());
                }
            }
            let new_text = formatted[start_f..end_f].to_string();
            return Ok(vec![TextEdit {
                range: edit_range,
                new_text,
            }]);
        }
    }

    // Calculate the full range of the document.
    // Using offset_to_position(source.len()) keeps CRLF/UTF-16 handling consistent
    // with the rest of the LSP position conversion utilities.
    let full_range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: offset_to_position(source, source.len()),
    };

    Ok(vec![TextEdit {
        range: full_range,
        new_text: formatted,
    }])
}

fn clamp_to_char_boundary(source: &str, offset: usize) -> usize {
    if offset >= source.len() {
        return source.len();
    }
    if source.is_char_boundary(offset) {
        return offset;
    }
    (0..=offset)
        .rev()
        .find(|&i| source.is_char_boundary(i))
        .unwrap_or(0)
}

fn diff_range(original: &str, formatted: &str) -> Option<(usize, usize, usize, usize)> {
    if original == formatted {
        return None;
    }

    let orig_bytes = original.as_bytes();
    let fmt_bytes = formatted.as_bytes();
    let min_len = orig_bytes.len().min(fmt_bytes.len());

    let mut prefix = 0;
    while prefix < min_len && orig_bytes[prefix] == fmt_bytes[prefix] {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < (min_len - prefix)
        && orig_bytes[orig_bytes.len() - 1 - suffix] == fmt_bytes[fmt_bytes.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let start = prefix;
    let end = orig_bytes.len().saturating_sub(suffix);
    let start_f = prefix;
    let end_f = fmt_bytes.len().saturating_sub(suffix);

    Some((start, end, start_f, end_f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position_utils::position_to_offset;

    #[test]
    fn range_formatting_rejects_invalid_utf16_ranges() {
        let source = "class Foo { String s = \"😀\";int  value;}";
        let emoji = source.find('😀').expect("emoji fixture");
        let inside_surrogate = source[..emoji].encode_utf16().count() as u32 + 1;
        for range in [
            Range::new(Position::new(0, 0), Position::new(1, 0)),
            Range::new(Position::new(0, 0), Position::new(0, 999)),
            Range::new(Position::new(0, 0), Position::new(0, inside_surrogate)),
            Range::new(Position::new(0, 10), Position::new(0, 5)),
        ] {
            assert!(
                format_source(source, Some(range)).is_err(),
                "invalid selection {range:?} must not produce formatting edits"
            );
        }
    }

    #[test]
    fn range_formatting_edits_apply_to_utf16_crlf_source() {
        let source = "class Foo {\r\n    String s = \"😀\"; int  value;\r\n}\r\n";
        let start = source.find("int  value;").expect("field fixture");
        let range = Range::new(
            offset_to_position(source, start),
            offset_to_position(source, start + "int  value;".len()),
        );
        let edits = format_source(source, Some(range)).expect("valid UTF-16 range");
        assert_eq!(edits.len(), 1);
        let mut applied = source.to_string();
        for edit in edits.into_iter().rev() {
            crate::position_utils::apply_incremental_change(
                &mut applied,
                edit.range,
                &edit.new_text,
            )
            .expect("formatting edit applies in the editor coordinate system");
        }
        assert!(applied.contains("int value;"), "{applied}");
        assert!(applied.contains("String s = \"😀\";"), "{applied}");
        assert!(applied.starts_with("class Foo {\r\n"), "{applied}");
        assert!(applied.ends_with("\r\n}\r\n"), "{applied}");
    }

    #[test]
    fn test_format_source_simple() {
        let source = "class  Foo{int x;}";
        let result = format_source(source, None);

        assert!(result.is_ok(), "format failed: {:?}", result.err());
        let edits = result.expect("should have edits");

        // Should produce some formatting changes
        if !edits.is_empty() {
            let formatted = &edits[0].new_text;
            assert!(formatted.contains("class"));
            assert!(formatted.contains("Foo"));
        }
    }

    #[test]
    fn test_format_already_formatted() {
        // Note: palantir style uses 4-space indent
        let source = "class Foo {\n    int x;\n}\n";
        let result = format_source(source, None);

        assert!(result.is_ok());
        // Might return edits due to style differences, that's OK
    }

    #[test]
    fn test_format_preserves_content() {
        let source = "class Foo {\n    void bar() {\n        return;\n    }\n}\n";
        let result = format_source(source, None);

        assert!(result.is_ok());
        let edits = result.expect("should format");

        if !edits.is_empty() {
            let formatted = &edits[0].new_text;
            // Check that essential content is preserved
            assert!(
                formatted.contains("void"),
                "should contain 'void': {}",
                formatted
            );
            assert!(
                formatted.contains("bar"),
                "should contain 'bar': {}",
                formatted
            );
            assert!(
                formatted.contains("return"),
                "should contain 'return': {}",
                formatted
            );
        }
    }

    #[test]
    fn test_position_to_offset() {
        let source = "line1\nline2\nline3";

        // First character of first line
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .expect("valid position"),
            0
        );

        // First character of second line
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 1,
                    character: 0
                }
            )
            .expect("valid position"),
            6 // "line1\n" = 6 bytes
        );

        // Middle of second line
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 1,
                    character: 2
                }
            )
            .expect("valid position"),
            8 // "line1\nli" = 8 bytes
        );
    }

    #[test]
    fn test_range_formatting_returns_local_edit() {
        let source = "class Foo{\n    int  x;\n    int  y;\n}\n";
        let range = Range {
            start: Position {
                line: 1,
                character: 4,
            },
            end: Position {
                line: 1,
                character: 11,
            },
        };

        let edits = format_source(source, Some(range)).expect("range format failed");
        assert_eq!(edits.len(), 1, "expected a single edit");

        let edit = &edits[0];
        assert!(
            edit.range.start.line >= 1,
            "range formatting should not touch file header"
        );
        assert!(
            edit.range.end.line <= 2,
            "range formatting should be localized to the selection area"
        );
    }

    #[test]
    fn test_lsp_position_to_byte_offset_utf16() {
        // Test with emoji (U+1F600 GRINNING FACE) which is:
        // - 2 UTF-16 code units (surrogate pair)
        // - 4 UTF-8 bytes
        let source = "a\u{1F600}b"; // "a😀b"

        // Position after 'a' (UTF-16 offset 1)
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 0,
                    character: 1
                }
            )
            .expect("valid position"),
            1 // byte offset of emoji start
        );

        // Position after emoji (UTF-16 offset 3, since emoji is 2 UTF-16 units)
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 0,
                    character: 3
                }
            )
            .expect("valid position"),
            5 // 1 (a) + 4 (emoji bytes) = 5
        );

        // Position at 'b' is the same as after emoji
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 0,
                    character: 3
                }
            )
            .expect("valid position"),
            5
        );
    }

    #[test]
    fn test_lsp_position_to_byte_offset_bmp_chars() {
        // Test with BMP characters (single UTF-16 code unit each)
        // Chinese characters are 3 UTF-8 bytes but 1 UTF-16 code unit
        let source = "a\u{4E2D}b"; // "a中b"

        // Position after 'a' (UTF-16 offset 1)
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 0,
                    character: 1
                }
            )
            .expect("valid position"),
            1
        );

        // Position after '中' (UTF-16 offset 2)
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 0,
                    character: 2
                }
            )
            .expect("valid position"),
            4 // 1 (a) + 3 (中 in UTF-8) = 4
        );
    }

    #[test]
    fn test_lsp_position_to_byte_offset_crlf() {
        let source = "line1\r\nline2\r\n";
        assert_eq!(
            position_to_offset(
                source,
                Position {
                    line: 1,
                    character: 0
                }
            )
            .expect("valid position"),
            7
        );
    }

    #[test]
    fn test_full_document_range_without_trailing_newline() {
        let source = "class Foo {\n    int x;\n}";
        let edits = format_source(source, None).expect("format should succeed");
        if let Some(edit) = edits.first() {
            let expected_end = offset_to_position(source, source.len());
            assert_eq!(edit.range.end, expected_end);
        }
    }
}
