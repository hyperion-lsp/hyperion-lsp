// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Compile-time contract that language extractors and the workspace exchange
//! one byte-range type without field-by-field adapters.

use hyperion_types::TextRange;

fn through_kotlin(range: hyperion_kotlin::TextRange) -> TextRange {
    range
}

fn through_groovy(range: hyperion_groovy::lsp_symbol_extractor::TextRange) -> TextRange {
    range
}

fn through_scala(range: hyperion_scala::TextRange) -> TextRange {
    range
}

fn through_workspace(range: hyperion_workspace::TextRange) -> TextRange {
    range
}

#[test]
fn language_symbol_ranges_share_the_canonical_half_open_contract() {
    let canonical = TextRange::new(10, 20);

    assert_eq!(through_kotlin(canonical), canonical);
    assert_eq!(through_groovy(canonical), canonical);
    assert_eq!(through_scala(canonical), canonical);
    assert_eq!(through_workspace(canonical), canonical);
    assert_eq!(canonical.len(), 10);
    assert!(canonical.contains(10));
    assert!(canonical.contains(19));
    assert!(!canonical.contains(20));
}
