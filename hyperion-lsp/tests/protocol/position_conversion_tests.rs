// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Public contracts for translating between LSP UTF-16 coordinates and the
//! byte offsets used by the parser. Keep boundary semantics in this one suite:
//! every editor-facing feature relies on the same conversion API.

use hyperion_lsp::position_utils::{
    apply_incremental_change, lsp_range_to_text_edit, offset_to_position, position_to_offset,
    text_range_to_lsp_range, utf16_offset_to_utf8, utf8_offset_to_utf16, PositionError,
};
use rowan::{TextRange, TextSize};
use tower_lsp::lsp_types::{Position, Range};

#[test]
fn valid_lsp_positions_map_to_exact_byte_offsets() {
    let cases = [
        ("", Position::new(0, 0), 0),
        ("hello", Position::new(0, 5), 5),
        ("one\ntwo", Position::new(1, 0), 4),
        ("one\r\ntwo", Position::new(1, 0), 5),
        ("one\rtwo", Position::new(1, 0), 4),
        ("世界", Position::new(0, 1), 3),
        ("a👋b", Position::new(0, 3), 5),
        ("a👋b\r\n世界", Position::new(1, 2), 14),
    ];

    for (source, position, expected_offset) in cases {
        assert_eq!(
            position_to_offset(source, position),
            Ok(expected_offset),
            "wrong byte offset for {position:?} in {source:?}"
        );
        assert_eq!(
            offset_to_position(source, expected_offset),
            position,
            "position did not round-trip in {source:?}"
        );
    }
}

#[test]
fn invalid_lsp_positions_are_rejected() {
    let source = "a👋b\r\nend";

    assert_eq!(
        position_to_offset(source, Position::new(0, 2)),
        Err(PositionError::CharacterNotReachable {
            line: 0,
            character: 2,
        })
    );
    assert_eq!(
        position_to_offset(source, Position::new(2, 0)),
        Err(PositionError::PositionOutOfRange {
            position: Position::new(2, 0),
            line_count: 2,
        })
    );

    for position in [Position::new(0, 5), Position::new(1, 4)] {
        assert!(matches!(
            position_to_offset(source, position),
            Err(PositionError::PositionOutOfRange { .. })
        ));
    }
}

#[test]
fn byte_offsets_are_clamped_to_representable_lsp_boundaries() {
    let source = "你👋\r\n好";

    let cases = [
        (1, Position::new(0, 0)), // middle of 你 rounds down
        (4, Position::new(0, 1)), // middle of 👋 rounds down
        (7, Position::new(0, 3)), // CR in CRLF
        (8, Position::new(0, 3)), // LF in CRLF also resolves before CRLF
        (usize::MAX, Position::new(1, 1)),
    ];

    for (offset, expected) in cases {
        assert_eq!(offset_to_position(source, offset), expected);
    }
}

#[test]
fn every_character_boundary_round_trips_in_a_mixed_document() {
    let source = (0..300)
        .map(|line| format!("line {line}: 世界 👋"))
        .collect::<Vec<_>>()
        .join("\n");

    for offset in source
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(source.len()))
    {
        let position = offset_to_position(&source, offset);
        assert_eq!(
            position_to_offset(&source, position),
            Ok(offset),
            "round-trip failed at byte {offset} ({position:?})"
        );
    }
}

#[test]
fn scalar_utf_conversions_have_explicit_boundary_semantics() {
    let utf16_to_utf8 = [
        ("hello", 3, 3),
        ("世界", 1, 3),
        ("a👋b", 1, 1),
        ("a👋b", 2, 5), // middle of a surrogate pair rounds forward
        ("a👋b", 3, 5),
        ("a👋b", 99, 6),
    ];
    for (source, utf16, expected_utf8) in utf16_to_utf8 {
        assert_eq!(utf16_offset_to_utf8(source, utf16), expected_utf8);
    }

    for (source, expected) in [
        ("", 0),
        ("hello", 5),
        ("世界", 2),
        ("a👋b", 4),
        ("e\u{301}", 2),
    ] {
        assert_eq!(utf8_offset_to_utf16(source), expected);
    }
}

#[test]
fn parser_ranges_translate_to_exact_lsp_ranges() {
    let source = "a\n世👋z";
    let range = TextRange::new(TextSize::from(2), TextSize::from(9));

    assert_eq!(
        text_range_to_lsp_range(range, source),
        Range::new(Position::new(1, 0), Position::new(1, 3))
    );

    let beyond_eof = TextRange::new(TextSize::from(0), TextSize::from(100));
    assert_eq!(
        text_range_to_lsp_range(beyond_eof, source).end,
        Position::new(1, 4)
    );
}

#[test]
fn incremental_changes_apply_editor_ranges_in_utf16_coordinates() {
    let cases = [
        (
            "hello world",
            Range::new(Position::new(0, 6), Position::new(0, 11)),
            "Rust",
            "hello Rust",
        ),
        (
            "hello world",
            Range::new(Position::new(0, 6), Position::new(0, 6)),
            "wide ",
            "hello wide world",
        ),
        (
            "one\ntwo\nthree",
            Range::new(Position::new(1, 0), Position::new(2, 0)),
            "",
            "one\nthree",
        ),
        (
            "a 👋 b",
            Range::new(Position::new(0, 2), Position::new(0, 4)),
            "世界",
            "a 世界 b",
        ),
    ];

    for (initial, range, replacement, expected) in cases {
        let mut actual = initial.to_owned();
        apply_incremental_change(&mut actual, range, replacement)
            .expect("fixture range must be valid");
        assert_eq!(actual, expected);
    }
}

#[test]
fn invalid_incremental_changes_are_atomic() {
    let cases = [
        Range::new(Position::new(0, 4), Position::new(0, 2)),
        Range::new(Position::new(0, 0), Position::new(1, 0)),
        Range::new(Position::new(0, 1), Position::new(0, 99)),
    ];

    for range in cases {
        let mut source = "hello".to_owned();
        assert!(apply_incremental_change(&mut source, range, "x").is_err());
        assert_eq!(source, "hello", "a rejected edit mutated the document");
    }
}

#[test]
fn lsp_ranges_convert_to_parser_edits_without_losing_utf8_offsets() {
    let source = "a 👋 value";
    let range = Range::new(Position::new(0, 2), Position::new(0, 4));
    let edit = lsp_range_to_text_edit(source, range, "世界").expect("valid edit");

    assert_eq!(edit.range, TextRange::new(2.into(), 6.into()));
    assert_eq!(edit.new_text, "世界");

    let reversed = Range::new(Position::new(0, 4), Position::new(0, 2));
    assert_eq!(
        lsp_range_to_text_edit(source, reversed, "x"),
        Err(PositionError::ReversedRange { start: 6, end: 2 })
    );
    let beyond_eof = Range::new(Position::new(0, 0), Position::new(1, 0));
    assert!(lsp_range_to_text_edit(source, beyond_eof, "x").is_err());
}
