// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Termination guarantees for the hand-written Kotlin parser.
//!
//! The parser is a recursive-descent/Pratt hybrid whose loops delegate to
//! sub-parsers. If a sub-parser returns without consuming a token — because the
//! current token is neither handled nor treated as a skippable recovery token —
//! the enclosing loop re-enters with an identical parser state and spins
//! forever. Two real instances of that shape existed:
//!
//! * `do { ... } while (c)` — `parse_statement` had no `DO_KW` arm, and the
//!   block-level error-recovery scan stops at statement-start keywords, of
//!   which `do` is one. Nothing was ever consumed.
//! * `when (val r = get())` — the subject was parsed with the expression
//!   parser, which refuses to consume `val`, so the when body never advanced.
//!
//! These tests pin down the behaviour for both, and run a battery of malformed
//! and truncated snippets under a wall-clock guard so that any future
//! reintroduction of the bug class fails fast instead of hanging CI.

use hyperion_kotlin::lexer::Lexer;
use hyperion_kotlin::parser::Parser;
use hyperion_kotlin::SyntaxNode;
use std::sync::mpsc;
use std::time::Duration;

/// Per-snippet wall-clock budget. Parsing any of these snippets takes
/// microseconds; a snippet that fails to terminate hits this instead.
const BUDGET: Duration = Duration::from_millis(500);

struct ParseOutcome {
    errors: usize,
    text: String,
}

/// Parse `src` on a worker thread and give up after `BUDGET`.
///
/// A non-terminating parse cannot be interrupted, so the worker is abandoned
/// and the test fails; the process exits shortly afterwards.
fn parse_guarded(src: &str) -> ParseOutcome {
    let owned = src.to_string();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let tokens = Lexer::new(&owned).collect();
        let (green, errors) = Parser::parse(tokens);
        let text = SyntaxNode::new_root(green).text().to_string();
        let _ = tx.send(ParseOutcome {
            errors: errors.len(),
            text,
        });
    });

    match rx.recv_timeout(BUDGET) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("parser failed to terminate within {BUDGET:?} on input: {src:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("parser panicked on input: {src:?}")
        }
    }
}

/// Parse must terminate and stay lossless (every token appears exactly once,
/// in order). The `stalled()` progress guard consumes tokens into `ERROR`
/// nodes, so losslessness is the check that it never drops or duplicates one.
fn assert_terminates(src: &str) -> usize {
    let outcome = parse_guarded(src);
    assert_eq!(
        outcome.text, src,
        "green tree must round-trip the source exactly for input: {src:?}"
    );
    outcome.errors
}

// ---------------------------------------------------------------------------
// The two known hangs — must terminate *and* parse cleanly (valid Kotlin).
// ---------------------------------------------------------------------------

#[test]
fn do_while_statement_parses_without_errors() {
    assert_eq!(
        assert_terminates("fun main() { do { x() } while (cond) }"),
        0
    );
}

#[test]
fn when_with_bound_subject_parses_without_errors() {
    assert_eq!(
        assert_terminates("fun f() = when (val r = get()) { is A -> 1 else -> 2 }"),
        0
    );
}

#[test]
fn do_while_variants_parse_without_errors() {
    for src in [
        "fun f() { do { } while (true) }",
        "fun f() { do { a(); b() } while (i < 10) }",
        "fun f() { do { do { x() } while (a) } while (b) }",
        "fun f() { while (a) { do { x() } while (b) } }",
        "fun f() { do { x() } while (a && b) }",
        "fun f() { for (i in 1..10) { do { x() } while (i > 0) } }",
    ] {
        assert_eq!(assert_terminates(src), 0, "expected clean parse of {src:?}");
    }
}

/// `reified` type-parameter modifiers and `typealias` declarations, added
/// alongside the do-while/when-subject fixes above: `parse_type_parameters`
/// only accepted `out`/`in` before the type-parameter name (no `reified`
/// arm), and `parse_top_level_decl` had no `TYPEALIAS_KW` arm at all, so
/// `typealias Name = String` silently fell through to the source-file loop's
/// one-token error recovery without ever recording an error - a misleading
/// "0 errors" for code that was never actually parsed as a typealias.
#[test]
fn reified_and_typealias_parse_without_errors() {
    for src in [
        "inline fun <reified T> f() {}",
        "inline fun <reified T, reified U> f() {}",
        "inline fun <T, reified U> f() {}",
        "inline fun <in T, reified U> f() {}",
        "typealias Name = String",
        "typealias Handler<T> = (T) -> Unit",
        "typealias Predicate<T> = (T) -> Boolean",
        // `reified` is a soft keyword; it must still work as a plain identifier.
        "val reified = 1",
        "fun f(reified: Int) = reified",
    ] {
        assert_eq!(assert_terminates(src), 0, "expected clean parse of {src:?}");
    }
}

#[test]
fn local_class_in_block_parses_without_errors() {
    assert_eq!(
        assert_terminates("fun make() { class Local(val x: Int) { fun value() = x } Local(1) }"),
        0
    );
}

#[test]
fn named_infix_calls_parse_without_errors() {
    for src in [
        "fun f() = 1 shl 2",
        "fun f() = 1 to 2 to 3",
        "fun f(n: Int) = 0 until n step 2",
        "fun f(x: Int, n: Int) = x in 1..n",
        "fun f() { val x = 1; shl(2) }",
    ] {
        assert_eq!(assert_terminates(src), 0, "expected clean parse of {src:?}");
    }
}

/// `typealias` is file-scoped only in Kotlin - it is never valid inside a
/// class body. Parsing it there must still terminate (and stay lossless) but
/// must record at least one error rather than silently accepting it or
/// falling back to the member-loop's un-reported single-token recovery.
#[test]
fn class_body_typealias_is_an_error_but_terminates() {
    for src in [
        "class A { typealias X = Int }",
        "class A { typealias Handler<T> = (T) -> Unit }",
    ] {
        let errors = assert_terminates(src);
        assert!(
            errors >= 1,
            "expected at least one error for class-body typealias {src:?}"
        );
    }
}

#[test]
fn when_subject_variants_parse_without_errors() {
    for src in [
        "fun f() = when (val r = get()) { is A -> 1 else -> 2 }",
        "fun f() = when (val r: Int = get()) { 1 -> 1 else -> 2 }",
        "fun f() = when (x) { is A -> 1 is B.C -> 2 else -> 3 }",
        "fun f() = when (x) { !is A -> 1 else -> 2 }",
        "fun f() = when (x) { in 1..10 -> 1 !in 20..30 -> 2 else -> 3 }",
        "fun f() = when (val r = get()) { is List<String> -> 1 else -> 2 }",
        "fun f(x: Any) = when (x) {\n is List<*> -> 1\n is Map<*, *> -> 2\n else -> 3\n}",
        "fun f() { val y = when (val r = get()) { is A -> 1 else -> 2 } }",
    ] {
        assert_eq!(assert_terminates(src), 0, "expected clean parse of {src:?}");
    }
}

// ---------------------------------------------------------------------------
// Fuzz-ish battery: malformed, truncated and adversarial snippets. These are
// allowed to report errors; they are not allowed to hang or lose tokens.
// ---------------------------------------------------------------------------

const MALFORMED: &[&str] = &[
    // Truncated / degenerate do-while
    "do",
    "do {",
    "do { }",
    "do { } while",
    "do { } while (",
    "fun f() { do }",
    "fun f() { do while (x) }",
    "fun f() { do { } while () }",
    "fun f() { do { } while (val) }",
    "fun f() { do { do { do } while } while } while",
    "fun f() { do { x() } while (cond)",
    "fun f() { { do { } while (a) } }",
    // Truncated / degenerate when
    "when",
    "when (",
    "when (val",
    "when (val r",
    "when (val r =",
    "when (val r = get()",
    "when (val r = get()) {",
    "when (val r = get()) { is",
    "when (val r = get()) { is A",
    "when (val r = get()) { is A ->",
    "when (x) { 1 -> 1;",
    "fun f() = when (val) { }",
    "fun f() = when (var x = ) { else -> 1 }",
    "fun f() = when (val x = y) { is -> 1 }",
    "fun f() = when { val -> 1 }",
    "fun f() = when (x) { , , -> 1 }",
    // Unbalanced braces / parens / brackets
    "fun f() {",
    "fun f() { { { {",
    "}}}}",
    "((((",
    "fun f() { val x = ( ( ( }",
    "class A { fun f() { do { } while (a) }",
    "class A {",
    "object O { do { } while (x) }",
    // Bare keywords in statement position
    "fun f() { val }",
    "fun f() { var }",
    "fun f() { fun }",
    "fun f() { class }",
    "fun f() { is }",
    "fun f() { -> }",
    "fun f() { else -> }",
    "fun f() { while }",
    "while (x)",
    "fun f() { for }",
    "for (i in xs)",
    "fun f() { try }",
    "fun f() { catch }",
    // Misc garbage
    "",
    " ",
    "@",
    "@:",
    "package",
    "import",
    "typealias",
    "typealias Name",
    "typealias Name =",
    "typealias Name<",
    "typealias Name<T",
    "typealias Name<T> =",
    "class A { typealias",
    "class A { typealias X",
    "class A { typealias X = Int",
    "fun f(vararg) {}",
    "fun <> f() {}",
    "fun <reified> f() {}",
    "fun <reified T f() {}",
    "fun f() where {}",
    "val x: () -> ",
    "val x by",
    // Truncated local property initializers. These used to be swallowed by
    // `parse_local_property_decl`'s flat token scan, which could not recurse
    // and so could not spin; routing them through the expression parser puts
    // them back on the recursive-descent paths that the guards protect.
    "fun f() { val x = }",
    "fun f() { val x = ",
    "fun f() { val x =",
    "fun f() { val x = when }",
    "fun f() { val x = when ( }",
    "fun f() { val x = when (",
    "fun f() { val x = when (y) }",
    "fun f() { val x = when (y) {",
    "fun f() { val x = when (y) { is }",
    "fun f() { val x = when (y) { -> }",
    "fun f() { val x = when (val }",
    "fun f() { val x = when (val z = ) { } }",
    "fun f() { val x = try }",
    "fun f() { val x = try {",
    "fun f() { val x = try { } catch }",
    "fun f() { val x = try { } catch ( }",
    "fun f() { val x = try { } catch (e: ) { } }",
    "fun f() { val x = try { } finally }",
    "fun f() { val x = if }",
    "fun f() { val x = if ( }",
    "fun f() { val x = if (c) }",
    "fun f() { val x = if (c) 1 else }",
    "fun f() { val x = do }",
    "fun f() { val x = do { } while }",
    "fun f() { val x = do { } while ( }",
    "fun f() { val x = { }",
    "fun f() { val x = {",
    "fun f() { val x = { -> }",
    "fun f() { val x = a. }",
    "fun f() { val x = a.( }",
    "fun f() { val x = a( }",
    "fun f() { val x = a[ }",
    "fun f() { val x = !! }",
    "fun f() { val x = a ?: }",
    "fun f() { val x = a as }",
    "fun f() { val x = a is }",
    "fun f() { val x = 1 to }",
    "fun f() { var x = when (",
    "fun f() { val x: Int = when (",
    "fun f() { val (a, b) = when (",
    "fun f() { val x by when (",
    "fun f() { val x by lazy {",
    "class A { fun f() { val x = when ( } }",
];

#[test]
fn malformed_snippets_terminate() {
    for src in MALFORMED {
        assert_terminates(src);
    }
}

/// Every prefix of the two repro inputs, which exercises truncation at every
/// token boundary rather than only at the hand-picked ones above.
#[test]
fn truncated_prefixes_terminate() {
    for full in [
        "fun main() { do { x() } while (cond) }",
        "fun f() = when (val r = get()) { is A -> 1 else -> 2 }",
        "class A { fun f() { do { when (val r = g()) { is B -> 1 else -> 2 } } while (c) } }",
        // Local property initializers, truncated at every token boundary.
        "fun f() { val x = when (y) { 1 -> 1 else -> 2 } }",
        "fun f() { val x = try { a() } catch (e: E) { b() } finally { c() } }",
        "fun f() { val x: Int = if (c) 1 else 2 }",
        "fun f() { val x = do { a() } while (c) }",
        "fun f() { val x by lazy { when (y) { else -> 1 } } }",
    ] {
        for end in 0..=full.len() {
            if !full.is_char_boundary(end) {
                continue;
            }
            assert_terminates(&full[..end]);
        }
    }
}

/// Deeply nested constructs must not blow the loop guards either.
#[test]
fn deeply_nested_snippets_terminate() {
    let mut src = String::from("fun f() {");
    for _ in 0..40 {
        src.push_str(" do { when (val v = g()) { is A -> 1 else -> 2 }");
    }
    for _ in 0..40 {
        src.push_str(" } while (c)");
    }
    src.push('}');
    assert_terminates(&src);
}
