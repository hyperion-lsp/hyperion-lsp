// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared support for the Kotlin corpus conformance harness.
//!
//! Used by `mass_parse_test.rs` (asserts the corpus parses cleanly) and by
//! the README-matrix generator test (renders the same measured results
//! into the table checked into README.md), so both stay derived from a
//! single source of truth instead of drifting apart.
//!
//! Each integration test binary compiles this module as its own separate
//! copy (the standard `tests/support/mod.rs` sharing pattern), so any
//! given binary only uses a subset of these items — hence `dead_code` is
//! allowed here rather than at each unused-item site.
#![allow(dead_code)]

use hyperion_kotlin::lexer::Lexer;
use hyperion_kotlin::parser::Parser;
use hyperion_kotlin::SyntaxNode;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// The corpus checked into the repository: 30-60 hand-written, original
/// Kotlin files covering the language surface (see each file's own header
/// comment for what it targets and, for the known-failure files, exactly
/// which parser gap it demonstrates).
pub const DEFAULT_CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus");

/// Known-failures ratchet: `(corpus file name, one-line reason)`.
///
/// Every file name here is a corpus file (under `tests/corpus/`) that is
/// currently known NOT to parse cleanly (either reported parse errors, or
/// a lossy — non-round-tripping — parse despite zero reported errors).
/// Each one is a genuine, deliberately-isolated parser gap; the reason
/// string is a one-line summary for humans and for the generated README
/// matrix — read the file's own header comment for the full detail.
///
/// This list is a ratchet: it may only shrink as the parser improves.
/// `mass_parse_test.rs` asserts that the *exact* set of currently-failing
/// corpus files matches this list — not a subset check — so that fixing a
/// bug and forgetting to shrink this list also fails the suite, keeping
/// the ratchet honest instead of accumulating stale entries.
pub const KNOWN_FAILURES: &[(&str, &str)] = &[];

/// Resolve the corpus directory to scan: `HYPERION_KOTLIN_CORPUS` if set
/// (so a developer can point this at a much larger real-world corpus),
/// otherwise the small corpus checked into the repo.
pub fn corpus_dir() -> PathBuf {
    match std::env::var_os("HYPERION_KOTLIN_CORPUS") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(DEFAULT_CORPUS_DIR),
    }
}

/// All `.kt` files directly under `dir` (non-recursive is sufficient for
/// the checked-in corpus, but `WalkDir` handles a nested external corpus
/// too), in a stable sorted order.
pub fn corpus_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "kt"))
        .map(|e| e.path().to_path_buf())
        .collect();
    files.sort();
    files
}

/// Outcome of parsing a single corpus file.
pub struct FileParseResult {
    /// Number of parse errors reported by `Parser::parse`.
    pub error_count: usize,
    /// Whether the green tree round-trips the source exactly. A parse can
    /// report zero errors and still be lossy — see
    /// `tests/corpus/dsl_builders.kt` for a real example — which is why
    /// this harness checks both instead of trusting `error_count` alone.
    pub lossless: bool,
}

impl FileParseResult {
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.error_count == 0 && self.lossless
    }
}

/// Per-file wall-clock budget for the non-termination guard. Corpus files
/// are at most ~150 lines and parse in microseconds; a file that blows
/// this budget has hit the parser-hang bug class documented in
/// `tests/parser_termination.rs`, not a slow-but-correct parse.
const BUDGET: Duration = Duration::from_secs(2);

/// Parse `source` on a worker thread with a wall-clock budget, so a
/// non-terminating parse fails the test loudly instead of hanging CI.
///
/// # Panics
///
/// Panics if the parse does not finish within [`BUDGET`], or if the
/// parser thread panics (e.g. on a stack overflow).
pub fn parse_with_guard(path: &Path, source: &str) -> FileParseResult {
    let owned = source.to_string();
    let (tx, rx) = mpsc::channel();
    let display = path.display().to_string();

    std::thread::spawn(move || {
        let tokens = Lexer::new(&owned).collect();
        let (green, errors) = Parser::parse(tokens);
        let text = SyntaxNode::new_root(green).text().to_string();
        let _ = tx.send(FileParseResult {
            error_count: errors.len(),
            lossless: text == owned,
        });
    });

    match rx.recv_timeout(BUDGET) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("parser failed to terminate within {BUDGET:?} on file: {display}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("parser panicked while parsing file: {display}")
        }
    }
}

/// File name (not full path) of a corpus file, for matching against
/// [`KNOWN_FAILURES`].
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

/// `"some_corpus_file"` -> `"Some Corpus File"`. Deterministic and
/// mechanical on purpose (no hand-tuned per-file overrides) so the README
/// matrix is genuinely regenerated from the corpus directory listing
/// rather than a hand-maintained description that can drift from it.
#[must_use]
pub fn prettify_stem(stem: &str) -> String {
    stem.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render the Kotlin corpus conformance matrix as a Markdown fragment,
/// from the same measured source of truth (`KNOWN_FAILURES` +
/// `tests/corpus/` directory listing) that `mass_parse_test.rs` asserts
/// against. Does not include the `KOTLIN_MATRIX_START`/`END` marker
/// lines themselves — those live in README.md and bracket this content.
///
/// # Panics
///
/// Panics if the default corpus directory cannot be read.
#[must_use]
pub fn render_readme_matrix() -> String {
    use std::fmt::Write as _;

    let dir = PathBuf::from(DEFAULT_CORPUS_DIR);
    let files = corpus_files(&dir);
    let total_lines: usize = files
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
                .lines()
                .count()
        })
        .sum();

    let known_failures: std::collections::HashMap<&str, &str> =
        KNOWN_FAILURES.iter().copied().collect();
    let clean_count = files.len() - known_failures.len();
    let total_files = files.len();
    let failure_count = known_failures.len();

    let mut out = String::new();
    out.push_str(
        "<!-- Generated by hyperion-kotlin/tests/readme_matrix_test.rs from \
         hyperion-kotlin/tests/corpus/ and tests/support/mod.rs::KNOWN_FAILURES. \
         Do not hand-edit this block: fix the parser or the corpus, update KNOWN_FAILURES, \
         then run `cargo nextest run -p hyperion-kotlin --test readme_matrix_test` and paste \
         its failure diff here. -->\n\n",
    );
    let _ = write!(
        out,
        "Measured from `hyperion-kotlin/tests/corpus/` ({total_files} original Kotlin files, \
         {total_lines} lines) via `cargo nextest run -p hyperion-kotlin --test \
         mass_parse_test`: **{clean_count}/{total_files} files parse cleanly** (zero errors, \
         lossless round-trip); {failure_count} known parser gaps tracked below as a ratchet \
         (see `tests/support/mod.rs::KNOWN_FAILURES`).\n\n"
    );
    out.push_str("| Corpus file | Coverage area | Status |\n");
    out.push_str("|---|---|---|\n");

    for path in &files {
        let name = file_name(path);
        let stem = name.trim_end_matches(".kt");
        let area = prettify_stem(stem);
        let status = known_failures.get(name.as_str()).map_or_else(
            || "✅ Clean".to_string(),
            |reason| format!("⚠️ Known gap — {reason}"),
        );
        let _ = writeln!(out, "| `{name}` | {area} | {status} |");
    }

    out
}
