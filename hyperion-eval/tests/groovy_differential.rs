// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0
//! Groovy differential harness: run a curated corpus of Groovy snippets through
//! Hyperion's build-free interpreter (`hyperion-eval -l groovy`) and through a
//! real `groovy` CLI, and assert the output matches.
//!
//! For each snippet `EXPR`, Hyperion evaluates `EXPR`
//! and prints the result's display form, while the oracle runs
//! `groovy -e 'println(EXPR)'`; the two stdout strings are normalised and
//! compared. A divergence is a real interpreter bug with a one-line repro.
//!
//! It is `#[ignore]`d (needs a `groovy` on PATH or under SDKMAN) and self-skips
//! if the CLI is absent, so it never breaks CI. Run it with:
//!   cargo test -p hyperion-eval --test groovy_differential -- --ignored --nocapture
//!
//! `groovy` is commonly installed via SDKMAN; if it is not on `PATH`, this
//! harness also probes `~/.sdkman/candidates/groovy/current/bin/groovy`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

const EVAL_BIN: &str = env!("CARGO_BIN_EXE_hyperion-eval");

/// Snippets whose output Hyperion should reproduce exactly. Each must be a
/// Groovy expression with a deterministic, side-effect-free value (so that
/// Hyperion's result display and `groovy`'s `println(EXPR)` agree). Grouped by
/// the interpreter feature they exercise; extend freely as coverage grows.
const SNIPPETS: &[&str] = &[
    "1 + \"a\"",
    // ── arithmetic ──────────────────────────────────────────────
    "1 + 2",
    "2 * 3 + 4",
    "10 % 3",
    "(1 + 2) * 3",
    "10 / 3",
    "10 / 2",
    "-10 / 3",
    "10.0 / 4",
    "(10 / 3) * 3",
    "1 + (10 / 3)",
    "-(10 / 3)",
    "(10 / 3) > 3",
    // ── long / double literals & mixed arithmetic ───────────────
    "10000000000L",
    "10000000000L + 5",
    "3000000000 * 2",
    "3.5",
    "4.0",
    "1.5 + 2.5",
    "3.5 * 2",
    "7.0",
    "0.1f",
    // ── strings & concatenation (incl. chains) ──────────────────
    "\"hello\".toUpperCase()",
    "\"hello\".length()",
    "\"hello\".startsWith(\"he\")",
    "\"hello\".endsWith(\"no\")",
    "\"a\" + \"b\" + \"c\"",
    "\"a\" + \"b\" + 1",
    "\"a\" + 1 + \"b\"",
    "\"n=\" + 42",
    "\"v=\" + true",
    "\"pi=\" + 3.14",
    "\"L=\" + 10000000000L",
    // ── comparisons (must display as true/false, not 1/0) ───────
    "2 > 1",
    "3 < 2",
    "5 >= 5",
    "1 == 1",
    "1 != 2",
    "\"r=\" + (2 > 1)",
    // ── power operator ** (integer base, non-negative integer exponent) ─
    "2 ** 10",
    "3 ** 4",
    "10 ** 3",
    "2 ** 0",
    "5 ** 2",
    "(2 ** 3) + 1",
    "2 ** -1",
    // ── logical operators (&& / || / !) on booleans & truthy values ─
    "true && false",
    "true && true",
    "false || true",
    "false || false",
    "true && false || true",
    "(2 > 1) && (3 > 2)",
    "(1 > 2) || (3 > 2)",
    "5 && 3",
    "!true",
    "!false",
    "!(1 > 2)",
    "!(true && false)",
    // ── collections ─────────────────────────────────────────────
    "[1, 2, 3]",
    "[1, 2, 3].size()",
    "[1, 2, 3].isEmpty()",
    "[].isEmpty()",
    "\"list=\" + [1, 2, 3]",
    // ── Default Groovy Methods (collection extensions) ──────────
    "[1, 2, 3].sum()",
    "[1, 2, 3].max()",
    "[1, 2, 3].min()",
    "[3, 1, 2].first()",
    "[3, 1, 2].last()",
    "[1, 2, 3].reverse()",
    "[\"a\", \"b\", \"c\"].join(\"-\")",
    "(1..5).sum()",
    // closure-taking DGM: each/collect/find/findAll/any/every
    "[1, 2, 3].collect { it * 2 }",
    "[1, 2, 3].findAll { it > 1 }",
    "[1, 2, 3].find { it > 1 }",
    "[1, 2, 3].any { it > 2 }",
    "[1, 2, 3].every { it > 0 }",
    "(1..5).collect { it * it }",
    "(1..10).findAll { it % 2 == 0 }",
    // a closure whose body is a comparison returns a boxed Boolean
    "[1, 2, 3].collect { it > 1 }",
    "[1, 2, 3].collect { it % 2 == 0 }",
    // multi-parameter closures: inject / fold
    "[1, 2, 3, 4].inject(0) { a, b -> a + b }",
    "[1, 2, 3].inject(1) { acc, x -> acc * x }",
    // ── more DGM: sort/unique/count/sum{}/contains/take/drop/indexOf/flatten/max{} ─
    "[3, 1, 2].sort()",
    "[1, 1, 2, 3, 3].unique()",
    "[1, 2, 3, 4].count { it % 2 == 0 }",
    "[1, 2, 3].count(2)",
    "[1, 2, 3].sum { it * 2 }",
    "[1, 2, 3].contains(2)",
    "[1, 2, 3].contains(5)",
    "[1, 2, 3, 4].take(2)",
    "[1, 2, 3, 4].drop(2)",
    "[1, 2, 3].indexOf(2)",
    "[[1, 2], [3, 4]].flatten()",
    "[1, 2, 3].max { -it }",
    "[3, 1, 2].sort().first()",
];

/// Snippets that are KNOWN to diverge from real Groovy today — confirmed
/// interpreter bugs with filed issues. They are reported by the harness (so the
/// work-list stays visible) but NOT asserted, so the gate stays green. Move a
/// snippet up into `SNIPPETS` once its bug is fixed.
const KNOWN_DIVERGENT: &[(&str, &str)] = &[
    // Residual: power overflowing `int` should widen to BigInteger,
    // and a floating base should yield BigDecimal. We saturate / stay Double.
    ("2 ** 40", "1099511627776"),
    ("2.0 ** 2", "4.00"),
];

fn groovy_bin() -> Option<PathBuf> {
    // 1. On PATH?
    if Command::new("groovy")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    {
        return Some(PathBuf::from("groovy"));
    }
    // 2. SDKMAN default location.
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".sdkman/candidates/groovy/current/bin/groovy");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn run_hyperion(snippet: &str) -> Result<String, String> {
    let out = Command::new(EVAL_BIN)
        .args(["-l", "groovy", snippet])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("spawn hyperion-eval: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "hyperion-eval failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn run_groovy(bin: &PathBuf, snippet: &str) -> Option<String> {
    // `println(EXPR)` renders the value the same way Hyperion's result display
    // does; wrapping in parens keeps operator snippets intact.
    let program = format!("println(({snippet}))");
    let out = Command::new(bin)
        .arg("-e")
        .arg(&program)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None; // oracle itself rejected the snippet — skip, don't fail
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn normalize(s: &str) -> String {
    s.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

#[test]
#[ignore = "needs a `groovy` CLI (PATH or SDKMAN); run with --ignored --nocapture"]
fn groovy_matches_real_groovy() {
    let Some(bin) = groovy_bin() else {
        eprintln!("SKIP: no `groovy` CLI on PATH or under ~/.sdkman");
        return;
    };
    eprintln!("oracle: {}", bin.display());

    let (mut matched, mut mismatched, mut gap) = (0, 0, 0);
    let mut divergences: Vec<(String, String, String)> = Vec::new();

    for &snippet in SNIPPETS {
        let Some(oracle) = run_groovy(&bin, snippet) else {
            continue; // oracle couldn't evaluate it (shouldn't happen for this corpus)
        };
        match run_hyperion(snippet) {
            Ok(hyp) => {
                if normalize(&hyp) == normalize(&oracle) {
                    matched += 1;
                } else {
                    mismatched += 1;
                    divergences.push((snippet.to_string(), normalize(&hyp), normalize(&oracle)));
                }
            }
            Err(reason) => {
                gap += 1;
                divergences.push((
                    snippet.to_string(),
                    format!("<error> {reason}"),
                    normalize(&oracle),
                ));
            }
        }
    }

    println!("\n════════ GROOVY DIFFERENTIAL (vs real groovy) ════════");
    println!("  snippets:    {}", SNIPPETS.len());
    println!("  ✅ match:     {matched}");
    println!("  ❌ MISMATCH:  {mismatched}   (interpreter bugs — investigate!)");
    println!("  ⚠️  gap:       {gap}   (groovy ran it; Hyperion errored)");
    for (snip, hyp, oracle) in &divergences {
        println!("\n  DIVERGENCE  {snip}");
        println!("    hyperion: {hyp:?}");
        println!("    groovy:   {oracle:?}");
    }

    // Report the documented known-divergent work-list. If one has silently
    // started matching, say so (it can be promoted into the asserted corpus).
    if !KNOWN_DIVERGENT.is_empty() {
        println!("\n  Known divergences (filed; not gated):");
        for &(snip, want) in KNOWN_DIVERGENT {
            let got = run_hyperion(snip).unwrap_or_else(|e| format!("<error> {e}"));
            let status = if normalize(&got) == normalize(want) {
                "NOW MATCHES → promote to SNIPPETS"
            } else {
                "still diverges"
            };
            println!(
                "    {snip:24} hyperion={:?} groovy={want:?}  [{status}]",
                normalize(&got)
            );
        }
    }

    // Regression gate: every snippet in this curated corpus is a feature
    // Hyperion claims to support and has been verified against real Groovy.
    // A mismatch or gap here is a confirmed interpreter regression.
    assert_eq!(
        mismatched + gap,
        0,
        "Hyperion's Groovy output diverged from real groovy — see divergences above"
    );
}
