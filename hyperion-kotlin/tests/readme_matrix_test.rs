// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Proves that README.md's Kotlin coverage matrix matches measurement.
//!
//! The block in README.md between `<!-- KOTLIN_MATRIX_START -->` and
//! `<!-- KOTLIN_MATRIX_END -->` is not hand-written prose — it is
//! generated (see [`support::render_readme_matrix`]) from the same
//! corpus directory listing and known-failures ratchet that
//! `mass_parse_test.rs` asserts against. This test re-renders that block
//! and diffs it against what is actually checked into README.md, so the
//! README can never silently drift from what the corpus harness measures
//! (the exact failure mode the campaign's README claims had before this
//! harness existed).
//!
//! If this test fails, the fix is almost never to hand-edit README.md:
//! either the corpus/parser changed and the matrix needs regenerating
//! (copy the "RENDERED" block from the panic message into README.md
//! between the markers), or a corpus/ratchet change was made without
//! intending to affect the public README claims, worth a second look.

mod support;

use std::fs;
use std::path::PathBuf;

const START_MARKER: &str = "<!-- KOTLIN_MATRIX_START -->";
const END_MARKER: &str = "<!-- KOTLIN_MATRIX_END -->";

#[test]
fn readme_kotlin_matrix_matches_measured_corpus() {
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("hyperion-kotlin has a parent directory (the workspace root)")
        .join("README.md");

    let readme = fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", readme_path.display()));

    let start = readme.find(START_MARKER).unwrap_or_else(|| {
        panic!(
            "README.md is missing the `{START_MARKER}` marker (expected in {})",
            readme_path.display()
        )
    });
    let end = readme.find(END_MARKER).unwrap_or_else(|| {
        panic!(
            "README.md is missing the `{END_MARKER}` marker (expected in {})",
            readme_path.display()
        )
    });
    assert!(
        start < end,
        "`{START_MARKER}` must appear before `{END_MARKER}` in README.md"
    );

    let actual = readme[start + START_MARKER.len()..end].trim();
    let expected = support::render_readme_matrix();
    let expected = expected.trim();

    assert_eq!(
        actual, expected,
        "README.md's Kotlin matrix (between {START_MARKER} and {END_MARKER}) does not match \
         the measured corpus. Replace the block in README.md with the RENDERED text below:\n\n\
         ----- RENDERED (paste this between the markers) -----\n{expected}\n\
         ----- END RENDERED -----"
    );
}
