// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Contracts for the shared LSP fixture marker parser.
//!
//! These live outside `lsp_test_framework.rs` so importing the framework from
//! another integration target does not re-run the same four tests in every
//! target.

use crate::lsp_test_framework::parse_markers;

#[test]
fn cursor_marker_is_removed_and_records_its_position() {
    let (clean, markers) = parse_markers("foo<|>bar");

    assert_eq!(clean, "foobar");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "<|>");
    assert_eq!(markers[0].character, 3);
}

#[test]
fn comment_marker_is_removed_and_named() {
    let (clean, markers) = parse_markers("foo /*target:bar*/ baz");

    assert_eq!(clean, "foo  baz");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "target:bar");
}

#[test]
fn caret_marker_points_to_the_previous_line() {
    let content = r"
            bar.baz();
            ^
        ";
    let (_clean, markers) = parse_markers(content);

    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "^");
    assert_eq!(markers[0].line, 1);
}

#[test]
fn multiple_marker_styles_are_collected() {
    let content = r"
            Foo f = new /*target:Foo*/Foo();
            f.bar<|>();
        ";
    let (_clean, markers) = parse_markers(content);

    assert_eq!(markers.len(), 2);
    assert!(markers.iter().any(|marker| marker.name == "target:Foo"));
    assert!(markers.iter().any(|marker| marker.name == "<|>"));
}
