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
// This file has been modified from the upstream work: it is a translation of
// the referenced Java classes into Rust, with the structural differences
// described in the module documentation below. The specific upstream classes
// this file derives from are named there.
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

//! Levels and their open-op characteristics.
//!
//! Faithful port of the data-carrying parts of
//! `com.palantir.javaformat.doc.Level`, `com.palantir.javaformat.OpenOp`,
//! `com.palantir.javaformat.BreakBehaviour`,
//! `com.palantir.javaformat.LastLevelBreakability` and
//! `com.palantir.javaformat.PartialInlineability`. The breaking algorithm itself
//! lives in [`super::compute`].

use super::indent::Indent;
use super::{next_id, sum_width, Doc};

/// When a level doesn't fit on one line, controls whether it is broken (its
/// breaks taken) or partially inlined onto the current line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakBehaviour {
    /// Break this level's breaks as usual.
    BreakThisLevel,
    /// Prefer breaking the last inner level if that keeps the rest on one line.
    PreferBreakingLastInnerLevel { keep_indent_when_inlined: bool },
    /// Break only if by doing so all inner levels then fit on a single line.
    BreakOnlyIfInnerLevelsThenFitOnOneLine { keep_indent_when_inlined: bool },
    /// Attempt to inline the suffix (last inner level) of this level.
    InlineSuffix,
}

/// How to decide whether to break the last inner level of a parent using
/// [`BreakBehaviour::PreferBreakingLastInnerLevel`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LastLevelBreakability {
    /// Default: abort the inline chain if we arrive here.
    Abort,
    /// Unconditionally allow ending an inline chain at this level.
    AcceptInlineChain,
    /// Delegate to the breakability of this level's own last inner level.
    CheckInner,
    /// Behaves like [`Self::AcceptInlineChain`] if the inlined levels so far are
    /// all simple, otherwise like [`Self::CheckInner`].
    AcceptInlineChainIfSimpleOtherwiseCheckInner,
}

/// A level is "simple" if it doesn't have multiple parameters / chained calls.
/// Used to poison partial inlining once a parent level is too complicated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Complexity {
    Simple,
    Complex,
}

/// The additional conditions under which a level may be partially inlined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartialInlineability {
    /// May always be partially inlined regardless of remaining space.
    AlwaysPartiallyInlineable,
    /// Partially inlineable only if the *first* inner level fits on the line.
    IfFirstLevelFits,
}

/// The immutable characteristics of a level, determined before its contents are
/// available. Port of `OpenOp`.
#[derive(Clone, Debug)]
pub struct OpenOp {
    pub plus_indent: Indent,
    pub break_behaviour: BreakBehaviour,
    pub breakability_if_last_level: LastLevelBreakability,
    pub column_limit_before_last_break: Option<u32>,
    pub complexity: Complexity,
    pub partial_inlineability: PartialInlineability,
    pub debug_name: Option<&'static str>,
}

impl OpenOp {
    /// An ordinary `OpenOp` with all reference defaults and the given indent.
    #[must_use]
    pub const fn simple(plus_indent: Indent) -> Self {
        Self {
            plus_indent,
            break_behaviour: BreakBehaviour::BreakThisLevel,
            breakability_if_last_level: LastLevelBreakability::Abort,
            column_limit_before_last_break: None,
            complexity: Complexity::Simple,
            partial_inlineability: PartialInlineability::AlwaysPartiallyInlineable,
            debug_name: None,
        }
    }

    #[must_use]
    pub const fn with_break_behaviour(mut self, b: BreakBehaviour) -> Self {
        self.break_behaviour = b;
        self
    }

    #[must_use]
    pub const fn with_breakability(mut self, b: LastLevelBreakability) -> Self {
        self.breakability_if_last_level = b;
        self
    }

    #[must_use]
    pub const fn with_column_limit_before_last_break(mut self, n: u32) -> Self {
        self.column_limit_before_last_break = Some(n);
        self
    }

    #[must_use]
    pub const fn with_complexity(mut self, c: Complexity) -> Self {
        self.complexity = c;
        self
    }

    #[must_use]
    pub const fn with_partial_inlineability(mut self, p: PartialInlineability) -> Self {
        self.partial_inlineability = p;
        self
    }

    #[must_use]
    pub const fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }
}

impl Default for OpenOp {
    fn default() -> Self {
        Self::simple(Indent::ZERO)
    }
}

/// A `Level` inside a `Doc`.
#[derive(Clone, Debug)]
pub struct Level {
    /// Globally-unique id, used as a key into [`super::state::State`]'s
    /// level-state map.
    pub(crate) id: u32,
    pub(crate) open_op: OpenOp,
    pub(crate) docs: Vec<Doc>,
}

impl Level {
    /// Construct a level from its open-op and child docs.
    #[must_use]
    pub fn new(open_op: OpenOp, docs: Vec<Doc>) -> Self {
        Self {
            id: next_id(),
            open_op,
            docs,
        }
    }

    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub fn docs(&self) -> &[Doc] {
        &self.docs
    }

    #[must_use]
    pub(crate) const fn get_plus_indent(&self) -> &Indent {
        &self.open_op.plus_indent
    }

    #[must_use]
    pub(crate) const fn break_behaviour(&self) -> BreakBehaviour {
        self.open_op.break_behaviour
    }

    #[must_use]
    pub(crate) const fn breakability_if_last_level(&self) -> LastLevelBreakability {
        self.open_op.breakability_if_last_level
    }

    #[must_use]
    pub(crate) const fn partial_inlineability(&self) -> PartialInlineability {
        self.open_op.partial_inlineability
    }

    #[must_use]
    pub(crate) const fn column_limit_before_last_break(&self) -> Option<u32> {
        self.open_op.column_limit_before_last_break
    }

    /// The flat width of this level, or [`super::INFINITY`] if it must be broken.
    #[must_use]
    pub(crate) fn width(&self) -> i32 {
        sum_width(self.docs.iter())
    }

    /// This level's flat (single-line) rendering. Not defined if it contains
    /// forced breaks.
    #[must_use]
    pub(crate) fn flat(&self) -> String {
        self.docs.iter().map(Doc::flat).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::break_::{Break, FillMode};
    use super::super::indent::Indent;
    use super::super::{Doc, INFINITY};
    use super::{Level, OpenOp};

    #[test]
    fn width_and_flat_sum_children() {
        let lvl = Level::new(
            OpenOp::default(),
            vec![Doc::text("ab"), Doc::space(), Doc::text("cd")],
        );
        assert_eq!(lvl.width(), 5);
        assert_eq!(lvl.flat(), "ab cd");
    }

    #[test]
    fn width_is_infinite_with_forced_break() {
        let lvl = Level::new(
            OpenOp::default(),
            vec![Doc::text("a"), Doc::brk(Break::make_forced())],
        );
        assert_eq!(lvl.width(), INFINITY);
    }

    #[test]
    fn openop_builder_sets_fields() {
        let op = OpenOp::simple(Indent::Const(4))
            .with_column_limit_before_last_break(80)
            .with_debug_name("call");
        assert_eq!(op.column_limit_before_last_break, Some(80));
        assert_eq!(op.debug_name, Some("call"));
    }

    #[test]
    fn nested_flat_concatenates() {
        let inner = Level::new(
            OpenOp::default(),
            vec![
                Doc::text("x"),
                Doc::brk(Break::make(FillMode::Unified, "+", Indent::ZERO)),
                Doc::text("y"),
            ],
        );
        let outer = Level::new(
            OpenOp::default(),
            vec![Doc::text("("), Doc::level(inner), Doc::text(")")],
        );
        assert_eq!(outer.flat(), "(x+y)");
    }
}
