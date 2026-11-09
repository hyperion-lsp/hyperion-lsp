// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin corpus conformance harness.
//!
//! Parses every `.kt` file in `tests/corpus/` (30+ original files written
//! for this harness, covering the Kotlin language surface — see each
//! file's header comment) and asserts:
//!
//! 1. Every file parses with zero reported errors, OR is on the
//!    known-failures ratchet ([`support::KNOWN_FAILURES`]).
//! 2. Every file's parse is lossless (the green tree round-trips the
//!    source exactly), OR is on the ratchet. A parse can report zero
//!    errors and still silently drop or duplicate tokens — see
//!    `tests/corpus/dsl_builders.kt` for a real example this harness
//!    caught — so error-count alone is not a trustworthy "parsed
//!    correctly" signal.
//! 3. No corpus file hangs the parser (wall-clock guard per file; see
//!    `tests/parser_termination.rs` for the bug class this defends
//!    against).
//!
//! The known-failures list is a ratchet: this test asserts the *exact*
//! set of currently-failing files matches it, in both directions, so it
//! can only shrink as the parser improves (a fixed bug whose file is not
//! removed from the ratchet fails the suite just as loudly as a new,
//! undocumented regression would).
//!
//! `HYPERION_KOTLIN_CORPUS` may be set to point this at a different (e.g.
//! much larger, real-world) corpus directory instead of the checked-in
//! one; the same clean/ratchet assertions apply to whatever directory is
//! in use.

mod support;

use std::collections::BTreeSet;
use std::fs;

#[test]
fn mass_parse_kotlin_corpus() {
    let dir = support::corpus_dir();
    assert!(
        dir.exists(),
        "Kotlin corpus directory not found: {}. Set HYPERION_KOTLIN_CORPUS to point at a \
         different corpus, or check tests/corpus/ is present.",
        dir.display()
    );

    let files = support::corpus_files(&dir);
    assert!(
        !files.is_empty(),
        "no .kt files found in corpus dir: {}",
        dir.display()
    );

    let mut actual_failures: BTreeSet<String> = BTreeSet::new();
    let mut total_lines = 0usize;

    for path in &files {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        total_lines += source.lines().count();

        let result = support::parse_with_guard(path, &source);
        if !result.is_clean() {
            actual_failures.insert(support::file_name(path));
        }
    }

    let expected_failures: BTreeSet<String> = support::KNOWN_FAILURES
        .iter()
        .map(|(name, _reason)| (*name).to_string())
        .collect();

    let unexpected_failures: Vec<&String> =
        actual_failures.difference(&expected_failures).collect();
    let stale_ratchet_entries: Vec<&String> =
        expected_failures.difference(&actual_failures).collect();

    println!(
        "Kotlin corpus conformance: {} files, {} lines, {} clean, {} known failures",
        files.len(),
        total_lines,
        files.len() - expected_failures.len(),
        expected_failures.len()
    );

    assert!(
        unexpected_failures.is_empty(),
        "corpus file(s) newly fail to parse cleanly (report reported errors, or a lossy \
         round-trip): {unexpected_failures:?}\n\
         If this is a genuine, newly-discovered parser gap, document it with a header comment \
         in the corpus file (see tests/corpus/local_classes.kt for the pattern) and add it to \
         KNOWN_FAILURES in tests/support/mod.rs. Otherwise this is a real regression — fix the \
         parser or the corpus file."
    );

    assert!(
        stale_ratchet_entries.is_empty(),
        "KNOWN_FAILURES in tests/support/mod.rs lists file(s) that now parse cleanly: \
         {stale_ratchet_entries:?}\n\
         The ratchet only shrinks: remove these entries (and update README.md's generated \
         matrix by re-running the README matrix test)."
    );
}
