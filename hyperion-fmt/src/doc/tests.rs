// Copyright 2025 Adrian Helvik
//
// THIRD-PARTY ATTRIBUTION
//
// This file is a Rust port of parts of palantir-java-format
// (https://github.com/palantir/palantir-java-format), Copyright Palantir
// Technologies Inc., which is itself derived from google-java-format
// (https://github.com/google/google-java-format), Copyright Google LLC. Both
// upstream works are licensed under the Apache License, Version 2.0.
//
// You may obtain a copy of the Apache License, Version 2.0 at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// or in the file LICENSE-APACHE-2.0 at the root of this repository. Required
// attribution notices are reproduced in the NOTICE file at the root of this
// repository.
//
// These tests are original work written for the Rust port, but they encode the
// expected behaviour of the upstream palantir-java-format layout solver and are
// therefore covered by this attribution as a precaution.
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the Apache License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//
// HYPERION LICENSE
//
// Hyperion's own contributions in this file are Copyright 2025 Adrian Helvik
// and are additionally licensed under the PolyForm Shield License 1.0.0; see
// the LICENSE file at the root of this repository.
// SPDX-License-Identifier: Apache-2.0 AND PolyForm-Shield-1.0.0

//! Integration-style tests validating the ported layout solver against the
//! behaviour of palantir-java-format.

use super::break_::FillMode;
use super::*;

// --- small construction helpers -------------------------------------------

fn t(s: &str) -> Doc {
    Doc::text(s)
}

/// A unified break with the given flat text and zero extra indent.
fn unified(flat: &str) -> Doc {
    Doc::brk(Break::make(FillMode::Unified, flat, Indent::ZERO))
}

/// An independent break with the given flat text and zero extra indent.
fn independent(flat: &str) -> Doc {
    Doc::brk(Break::make(FillMode::Independent, flat, Indent::ZERO))
}

// ---------------------------------------------------------------------------
// 1. Flat fit
// ---------------------------------------------------------------------------

#[test]
fn flat_fit_renders_one_line() {
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(4)),
        vec![t("a"), unified(" "), t("b")],
    );
    assert_eq!(format(&lvl, 80), "a b");
}

// ---------------------------------------------------------------------------
// 2. BreakThisLevel: all unified breaks break, indent = base + plus_indent
// ---------------------------------------------------------------------------

#[test]
fn break_this_level_breaks_all_unified() {
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(4)),
        vec![t("aa"), unified(" "), t("bb"), unified(" "), t("cc")],
    );
    // Doesn't fit in 5 columns -> break; subsequent lines indented by 4.
    assert_eq!(format(&lvl, 5), "aa\n    bb\n    cc");
}

// ---------------------------------------------------------------------------
// 3. Independent (fill) breaks decide individually by remaining width
// ---------------------------------------------------------------------------

#[test]
fn independent_breaks_fill() {
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(0)),
        vec![
            t("aaaa"),
            independent(" "),
            t("bbbb"),
            independent(" "),
            t("cccc"),
        ],
    );
    // "aaaa bbbb" fits in 10, so the first break stays; the second must break.
    assert_eq!(format(&lvl, 10), "aaaa bbbb\ncccc");
}

// ---------------------------------------------------------------------------
// 4. Forced break -> containing level width is INFINITY -> parent must handle
// ---------------------------------------------------------------------------

#[test]
fn forced_break_forces_even_with_huge_width() {
    let inner = Level::new(
        OpenOp::simple(Indent::Const(4)),
        vec![t("x"), Doc::brk(Break::make_forced()), t("y")],
    );
    let outer = Level::new(
        OpenOp::simple(Indent::Const(0)),
        vec![t("pre"), Doc::level(inner)],
    );
    // Even with an enormous max width, the forced break splits the inner level.
    assert_eq!(format(&outer, 200), "prex\n    y");
}

#[test]
fn forced_break_has_infinite_width() {
    let lvl = Level::new(
        OpenOp::simple(Indent::ZERO),
        vec![t("x"), Doc::brk(Break::make_forced())],
    );
    assert_eq!(lvl.width(), INFINITY);
}

// ---------------------------------------------------------------------------
// 5. PreferBreakingLastInnerLevel: partial inline vs full break
// ---------------------------------------------------------------------------

/// `receiver.method(aaaa, bbbb, cccc)` shaped as a chain whose last inner level
/// (the argument list) is the preferred break target.
fn chain_doc(args_breakability: LastLevelBreakability) -> Level {
    let args = Level::new(
        OpenOp::simple(Indent::Const(4)).with_breakability(args_breakability),
        vec![
            t("aaaa"),
            unified(", "),
            t("bbbb"),
            unified(", "),
            t("cccc"),
            t(")"),
        ],
    );
    let dot = Doc::brk(Break::make(FillMode::Unified, "", Indent::Const(0)));
    Level::new(
        OpenOp::simple(Indent::Const(4)).with_break_behaviour(
            BreakBehaviour::PreferBreakingLastInnerLevel {
                keep_indent_when_inlined: false,
            },
        ),
        vec![t("receiver"), dot, t(".method("), Doc::level(args)],
    )
}

#[test]
fn prefer_breaking_last_inner_level_partial_inline_wins() {
    let lvl = chain_doc(LastLevelBreakability::AcceptInlineChain);
    // At width 24, breaking the chain (dot) makes the tail overflow so the args
    // break too (3 lines / numLines 3). Breaking only the args, keeping the
    // prefix inline, is 3 lines but only numLines 2 -> fewer lines wins.
    assert_eq!(
        format(&lvl, 24),
        "receiver.method(aaaa\n    bbbb\n    cccc)"
    );
}

#[test]
fn prefer_breaking_last_inner_level_full_break_wins() {
    let lvl = chain_doc(LastLevelBreakability::AcceptInlineChain);
    // At width 30, breaking the chain lets the whole call fit on the next line
    // (numLines 1), which beats partial inline (numLines 2).
    assert_eq!(format(&lvl, 30), "receiver\n    .method(aaaa, bbbb, cccc)");
}

// ---------------------------------------------------------------------------
// 6. BreakOnlyIfInnerLevelsThenFitOnOneLine
// ---------------------------------------------------------------------------

fn assignment_doc(rhs: Level) -> Level {
    Level::new(
        OpenOp::simple(Indent::Const(4)).with_break_behaviour(
            BreakBehaviour::BreakOnlyIfInnerLevelsThenFitOnOneLine {
                keep_indent_when_inlined: false,
            },
        ),
        vec![t("lhs ="), unified(" "), Doc::level(rhs)],
    )
}

#[test]
fn break_only_if_breaks_when_rhs_then_fits() {
    let rhs = Level::new(OpenOp::simple(Indent::Const(4)), vec![t("reallyLongValue")]);
    let lvl = assignment_doc(rhs);
    // Breaking after `=` makes the RHS fit on its own line -> take that break.
    assert_eq!(format(&lvl, 20), "lhs =\n    reallyLongValue");
}

#[test]
fn break_only_if_prefix_inlines_when_rhs_still_too_wide() {
    let rhs = Level::new(
        OpenOp::simple(Indent::Const(4)),
        vec![
            t("aaaaaaaa"),
            unified(""),
            t("bbbbbbbb"),
            unified(""),
            t("cccccccc"),
        ],
    );
    let lvl = assignment_doc(rhs);
    // The RHS wouldn't fit on one line even when broken after `=`, so the prefix
    // `lhs = ` stays on the current line and the RHS is inlined onto it.
    assert_eq!(
        format(&lvl, 20),
        "lhs = aaaaaaaa\n    bbbbbbbb\n    cccccccc"
    );
}

// ---------------------------------------------------------------------------
// 7. Indent::If resolves against whether the tagged break was taken
// ---------------------------------------------------------------------------

#[test]
fn indent_if_then_branch_when_tag_broken() {
    let tag = BreakTag::new();
    let b1 = Break::make_tagged(FillMode::Unified, "", Indent::Const(0), tag);
    let b2 = Break::make(FillMode::Unified, "", Indent::if_broken(tag, 8, 4));
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(0)),
        vec![
            t("aaaa"),
            Doc::brk(b1),
            t("bb"),
            Doc::brk(b2),
            t("cccccccc"),
        ],
    );
    // Both unified breaks break; the first (tagged) is taken, so the conditional
    // indent selects the `then` value of 8.
    assert_eq!(format(&lvl, 12), "aaaa\nbb\n        cccccccc");
}

#[test]
fn indent_if_else_branch_when_tag_not_broken() {
    let tag = BreakTag::new();
    let b1 = Break::make_tagged(FillMode::Independent, " ", Indent::Const(0), tag);
    let b2 = Break::make(FillMode::Independent, " ", Indent::if_broken(tag, 8, 4));
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(0)),
        vec![
            t("aaaa"),
            Doc::brk(b1),
            t("bb"),
            Doc::brk(b2),
            t("cccccccc"),
        ],
    );
    // The first (tagged) break fits and is NOT taken, so the conditional indent
    // selects the `else` value of 4.
    assert_eq!(format(&lvl, 12), "aaaa bb\n    cccccccc");
}

// ---------------------------------------------------------------------------
// 8. column_limit_before_last_break
// ---------------------------------------------------------------------------

#[test]
fn column_limit_before_last_break_forces_break() {
    let brk = Break::make(FillMode::Unified, " ", Indent::Const(0)).with_column_limit();
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(4)).with_column_limit_before_last_break(10),
        vec![t("aaaaaaaaaaaa"), Doc::brk(brk), t("b")],
    );
    // Total width (14) fits in 30, but the column-limited break would start at
    // column 12 > 10, so the level is forced to break.
    assert_eq!(format(&lvl, 30), "aaaaaaaaaaaa\n    b");
}

#[test]
fn column_limit_before_last_break_stays_when_under_limit() {
    let brk = Break::make(FillMode::Unified, " ", Indent::Const(0)).with_column_limit();
    let lvl = Level::new(
        OpenOp::simple(Indent::Const(4)).with_column_limit_before_last_break(10),
        vec![t("aaaa"), Doc::brk(brk), t("b")],
    );
    // The break starts at column 4 <= 10, so the level stays on one line.
    assert_eq!(format(&lvl, 30), "aaaa b");
}

// ---------------------------------------------------------------------------
// 9. LastLevelBreakability chain
// ---------------------------------------------------------------------------

#[test]
fn breakability_abort_rejects_inlining() {
    let lvl = chain_doc(LastLevelBreakability::Abort);
    // ABORT rejects the inline-chain path, so we fall back to the full break.
    assert_eq!(
        format(&lvl, 24),
        "receiver\n    .method(aaaa\n        bbbb\n        cccc)"
    );
}

#[test]
fn breakability_accept_inline_chain_accepts() {
    let lvl = chain_doc(LastLevelBreakability::AcceptInlineChain);
    // ACCEPT_INLINE_CHAIN keeps the prefix inline and breaks only the args.
    assert_eq!(
        format(&lvl, 24),
        "receiver.method(aaaa\n    bbbb\n    cccc)"
    );
}

#[test]
fn breakability_check_inner_without_recursible_inner_aborts() {
    // CHECK_INNER delegates to the inner level's break behaviour. Here the args'
    // behaviour is BreakThisLevel, so there is nothing to recurse into and the
    // inline chain aborts, falling back to the full break.
    let lvl = chain_doc(LastLevelBreakability::CheckInner);
    assert_eq!(
        format(&lvl, 24),
        "receiver\n    .method(aaaa\n        bbbb\n        cccc)"
    );
}

#[test]
fn breakability_check_inner_recurses() {
    // outer (preferBreaking) -> mid (CHECK_INNER, preferBreaking) -> args
    // (ACCEPT_INLINE_CHAIN). CHECK_INNER on `mid` recurses via mid's
    // preferBreakingLastInnerLevel into `args`, which accepts the inline chain.
    let args = Level::new(
        OpenOp::simple(Indent::Const(4))
            .with_breakability(LastLevelBreakability::AcceptInlineChain),
        vec![
            t("pppp"),
            unified(", "),
            t("qqqq"),
            unified(", "),
            t("rrrr"),
            t("))"),
        ],
    );
    let mid = Level::new(
        OpenOp::simple(Indent::Const(4))
            .with_breakability(LastLevelBreakability::CheckInner)
            .with_break_behaviour(BreakBehaviour::PreferBreakingLastInnerLevel {
                keep_indent_when_inlined: false,
            }),
        vec![t("bbb.ccc("), Doc::level(args)],
    );
    let dot = Doc::brk(Break::make(FillMode::Unified, "", Indent::Const(0)));
    let outer = Level::new(
        OpenOp::simple(Indent::Const(4)).with_break_behaviour(
            BreakBehaviour::PreferBreakingLastInnerLevel {
                keep_indent_when_inlined: false,
            },
        ),
        vec![t("recv"), dot, t(".aaa("), Doc::level(mid)],
    );
    // At width 22 the inlined prefix `recv.aaa(bbb.ccc(pppp` fits (21 cols) and
    // the resulting partial-inline layout (numLines 2) beats the full break
    // (numLines 3), so CHECK_INNER's recursion is what selects it.
    assert_eq!(
        format(&outer, 22),
        "recv.aaa(bbb.ccc(pppp\n    qqqq\n    rrrr))"
    );
}

// ---------------------------------------------------------------------------
// 10. Renderer: no trailing whitespace on any line, ever
// ---------------------------------------------------------------------------

#[test]
fn renderer_never_emits_trailing_whitespace() {
    let layouts = vec![
        format(&chain_doc(LastLevelBreakability::AcceptInlineChain), 24),
        format(&chain_doc(LastLevelBreakability::AcceptInlineChain), 30),
        format(&chain_doc(LastLevelBreakability::Abort), 24),
        format(
            &assignment_doc(Level::new(
                OpenOp::simple(Indent::Const(4)),
                vec![
                    t("aaaaaaaa"),
                    unified(""),
                    t("bbbbbbbb"),
                    unified(""),
                    t("cccccccc"),
                ],
            )),
            20,
        ),
        format(
            &Level::new(
                OpenOp::simple(Indent::Const(4)),
                vec![t("aa"), unified(" "), t("bb"), unified(" "), t("cc")],
            ),
            5,
        ),
    ];
    for layout in layouts {
        for line in layout.split('\n') {
            assert_eq!(
                line,
                line.trim_end(),
                "line had trailing whitespace: {line:?}"
            );
        }
    }
}
