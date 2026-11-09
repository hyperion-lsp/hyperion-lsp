// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Finalization of reference locations returned to LSP clients.

use tower_lsp::lsp_types::Location;

/// Merge reference sources into one deterministic set.
///
/// A location is identified by its complete URI and range. Comparing the full
/// URI matters for non-file schemes; comparing the complete range prevents two
/// references with the same start but different ends from being collapsed.
pub(crate) fn merge(
    mut primary: Vec<Location>,
    secondary: impl IntoIterator<Item = Location>,
) -> Vec<Location> {
    primary.extend(secondary);
    normalize(primary)
}

/// Sort and deduplicate one reference source.
pub(crate) fn normalize(mut locations: Vec<Location>) -> Vec<Location> {
    locations.sort_by(|left, right| {
        left.uri
            .as_str()
            .cmp(right.uri.as_str())
            .then(left.range.start.line.cmp(&right.range.start.line))
            .then(left.range.start.character.cmp(&right.range.start.character))
            .then(left.range.end.line.cmp(&right.range.end.line))
            .then(left.range.end.character.cmp(&right.range.end.character))
    });
    locations.dedup();
    locations
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Position, Range, Url};

    fn location(uri: &str, start: u32, end: u32) -> Location {
        Location::new(
            Url::parse(uri).expect("test URI"),
            Range::new(Position::new(2, start), Position::new(2, end)),
        )
    }

    #[test]
    fn merge_is_deterministic_and_only_removes_exact_locations() {
        let first = location("file:///b.java", 4, 8);
        let same_start_different_end = location("file:///b.java", 4, 9);
        let earlier_file = location("file:///a.java", 7, 8);

        let merged = merge(
            vec![first.clone(), same_start_different_end.clone()],
            vec![first, earlier_file.clone()],
        );

        assert_eq!(
            merged,
            vec![
                earlier_file,
                location("file:///b.java", 4, 8),
                same_start_different_end,
            ]
        );
    }
}
