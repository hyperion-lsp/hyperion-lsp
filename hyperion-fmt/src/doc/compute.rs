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

//! The breaking algorithm.
//!
//! Faithful port of the decision logic in `com.palantir.javaformat.doc.Level`:
//! `computeBreaks`, `tryToFitOnOneLine`, the four `BreakBehaviour` cases
//! (`BreakImpl`), `tryBreakLastLevel` / `tryInlineSuffix` / `tryBreakInnerLevel`
//! and its `acceptInlineChain` / `checkInner` helpers,
//! `tryInlinePrefixOntoCurrentLine`, `handle_breakOnlyIfInnerLevelsThenFitOnOneLine`,
//! `computeBroken` / `computeBreakAndSplit`, plus the `CountWidthUntilBreakVisitor`
//! and `StartsWithBreakVisitor` used along the way.
//!
//! The reference threads all of this through the `Obs` exploration harness. That
//! harness only observes (for debugging); here it is collapsed to plain
//! functions returning `State` / `Option<State>`. `Exploration.markAccepted()`
//! becomes "return the state", `LevelNode.explore`/`maybeExplore` become "call
//! the closure", and `newChildNode` / `finishLevel` are identities. What is
//! preserved exactly is the decision logic, its ordering, and the acceptance
//! criteria.

use super::break_::{Break, FillMode};
use super::level::{
    BreakBehaviour, Complexity, LastLevelBreakability, Level, PartialInlineability,
};
use super::state::{LevelState, State};
use super::{add_width, sum_width, Doc, INFINITY};

/// The depth of nested levels from which we explore both breaking and not
/// breaking the current level (`Level.LAST_LEVELS_TO_EXPLORE`).
const LAST_LEVELS_TO_EXPLORE: i32 = 8;

impl Doc {
    /// Make breaking decisions for this doc, returning the new state.
    ///
    /// Port of `Doc.computeBreaks`. `Break`s are handled specially by their
    /// containing `Level` and never reach here.
    #[must_use]
    pub(crate) fn compute_breaks(&self, max_width: i32, state: State) -> State {
        match self {
            Self::Level(l) => l.compute_breaks(max_width, state),
            Self::Text(s) | Self::Literal(s) => {
                let col = state.column + s.chars().count() as i32;
                state.with_column(col)
            }
            Self::Space => {
                let col = state.column + 1;
                state.with_column(col)
            }
            Self::Break(_) => {
                unreachable!(
                    "Break docs are handled by their containing Level, not via compute_breaks"
                )
            }
        }
    }
}

impl Level {
    /// Make breaking decisions for this level (`Level.computeBreaks`).
    #[must_use]
    pub fn compute_breaks(&self, max_width: i32, state: State) -> State {
        match self.try_to_fit_on_one_line(max_width, state.column, self.docs.iter()) {
            Some(new_width) => state
                .with_column(new_width)
                .with_level_state(self.id, LevelState { one_line: true }),
            None => {
                let new_state = self.break_impl(max_width, &state);
                state.update_after_level(&new_state)
            }
        }
    }

    /// Try to fit `docs` (which belong to this level) onto one line starting at
    /// `start_column`, returning the resulting column if possible. Takes this
    /// level's `column_limit_before_last_break` into account.
    ///
    /// Port of `Level.tryToFitOnOneLine`.
    fn try_to_fit_on_one_line<'a>(
        &self,
        max_width: i32,
        start_column: i32,
        docs: impl IntoIterator<Item = &'a Doc>,
    ) -> Option<i32> {
        let mut column = start_column;
        let mut column_before_last_break = 0; // Not activated by default.
        for doc in docs {
            match doc {
                Doc::Break(b) => {
                    if b.has_column_limit {
                        column_before_last_break = column;
                    }
                    column = add_width(column, doc.width());
                }
                Doc::Level(inner) => {
                    // Inner levels might have nested column-limited breaks, so recurse.
                    column = inner.try_to_fit_on_one_line(max_width, column, inner.docs.iter())?;
                }
                _ => column = add_width(column, doc.width()),
            }
        }
        // Check that widthBeforeLastBreak fits in the (more restrictive) limit.
        if let Some(limit) = self.column_limit_before_last_break() {
            if column_before_last_break > limit as i32 {
                return None;
            }
        }
        // Check that the entirety of this level fits on the current line.
        if column <= max_width {
            Some(column)
        } else {
            None
        }
    }

    /// Dispatch on this level's `BreakBehaviour` (`Level.BreakImpl`).
    fn break_impl(&self, max_width: i32, state: &State) -> State {
        match self.break_behaviour() {
            BreakBehaviour::BreakThisLevel => self.break_normally(max_width, state),
            BreakBehaviour::PreferBreakingLastInnerLevel { .. } => {
                self.prefer_breaking_last_inner_level(max_width, state)
            }
            BreakBehaviour::BreakOnlyIfInnerLevelsThenFitOnOneLine {
                keep_indent_when_inlined,
            } => {
                self.break_only_if_inner_levels_then_fit(max_width, state, keep_indent_when_inlined)
            }
            BreakBehaviour::InlineSuffix => self.inline_suffix(max_width, state),
        }
    }

    /// Break this level normally: increment the indent by `plus_indent`, then
    /// lay out the breaks. Port of `Level.breakNormally`.
    fn break_normally(&self, max_width: i32, state: &State) -> State {
        let state_for_broken = state.with_indent_incremented_by(self.get_plus_indent());
        self.compute_broken(max_width, state_for_broken)
    }

    /// `BreakImpl.preferBreakingLastInnerLevel`.
    fn prefer_breaking_last_inner_level(&self, max_width: i32, state: &State) -> State {
        let max_depth = self.compute_max_depth();
        // Explore both breaking and not breaking if we're in the last
        // LAST_LEVELS_TO_EXPLORE levels. Choose the better one based on lines,
        // preferring breaks if the outcome is the same (reference
        // Level.java:207-213).
        if max_depth <= LAST_LEVELS_TO_EXPLORE {
            let branch_state = state.with_new_branch();
            let broken = self.break_normally(max_width, &branch_state);
            let state1 = branch_state.with_no_indent();
            if let Some(last_level_broken) = self.try_break_last_level(max_width, &state1, true) {
                if last_level_broken.num_lines < broken.num_lines {
                    return last_level_broken;
                }
            }
            broken
        } else {
            let state1 = state.with_no_indent();
            if let Some(last_level_broken) = self.try_break_last_level(max_width, &state1, true) {
                return last_level_broken;
            }
            // Fall back to breaking normally.
            self.break_normally(max_width, state)
        }
    }

    /// `BreakImpl.breakOnlyIfInnerLevelsThenFitOnOneLine`.
    fn break_only_if_inner_levels_then_fit(
        &self,
        max_width: i32,
        state: &State,
        keep_indent: bool,
    ) -> State {
        let broken = self.break_normally(max_width, state);
        self.handle_break_only_if_inner_levels_then_fit(max_width, state, &broken, keep_indent)
            .unwrap_or(broken)
    }

    /// `BreakImpl.inlineSuffix`.
    fn inline_suffix(&self, max_width: i32, state: &State) -> State {
        self.try_inline_suffix(max_width, state, true)
            .unwrap_or_else(|| self.break_normally(max_width, state))
    }

    /// Port of `Level.handle_breakOnlyIfInnerLevelsThenFitOnOneLine`.
    ///
    /// `broken_state` is the result of [`Self::break_normally`], passed in for
    /// efficiency (as in the reference).
    fn handle_break_only_if_inner_levels_then_fit(
        &self,
        max_width: i32,
        state: &State,
        broken_state: &State,
        keep_indent: bool,
    ) -> Option<State> {
        let inner_levels = self.non_empty_inner_level_indices();

        // Note (per reference): we don't compare line counts here, since a single
        // reflowed comment could be misleading; instead check if any inner level
        // was actually broken.
        let any_level_was_broken = inner_levels.iter().any(|&i| {
            if let Doc::Level(l) = &self.docs[i] {
                !broken_state.is_one_line(l.id)
            } else {
                false
            }
        });

        if !any_level_was_broken {
            return Some(broken_state.clone());
        }

        let partially_inlined =
            self.try_inline_prefix_onto_current_line(max_width, state, keep_indent)?;

        let body_is_complex = inner_levels.iter().any(|&i| {
            if let Doc::Level(l) = &self.docs[i] {
                l.open_op.complexity == Complexity::Complex
            } else {
                false
            }
        });

        if body_is_complex || partially_inlined.num_lines < broken_state.num_lines {
            Some(partially_inlined)
        } else {
            None
        }
    }

    /// Indices of inner-level docs that are non-empty per `StartsWithBreakVisitor`.
    /// Port of `Level.getNonEmptyInnerLevels` (returning indices to preserve the
    /// reference's identity-based `indexOf`).
    fn non_empty_inner_level_indices(&self) -> Vec<usize> {
        self.docs
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                matches!(d, Doc::Level(_)) && starts_with_break(d) != StartsWith::Empty
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Port of `Level.tryInlinePrefixOntoCurrentLine`.
    fn try_inline_prefix_onto_current_line(
        &self,
        max_width: i32,
        state: &State,
        keep_indent: bool,
    ) -> Option<State> {
        // The last non-empty inner level (skipping empty in-between levels).
        let indices = self.non_empty_inner_level_indices();
        let last_index = *indices.last()?;
        let last_level = match &self.docs[last_index] {
            Doc::Level(l) => l,
            _ => return None,
        };

        // Width of tokens/breaks before the last level: these must always fit.
        let leading_docs = &self.docs[..last_index];
        let mut leading_width = sum_width(leading_docs.iter());

        // Plus the prefix of the last level we insist stays on the same line.
        leading_width = add_width(
            leading_width,
            count_width_until_break_level(last_level, max_width - state.indent),
        );

        let fits = leading_width < INFINITY && state.column + leading_width <= max_width;
        if !fits {
            return None;
        }

        let mut new_state = state.with_no_indent();
        if keep_indent {
            new_state = new_state.with_indent_incremented_by(self.get_plus_indent());
        }
        let splits_breaks = split_by_breaks(&self.docs);
        Some(lay_out_level_on_one_line(
            max_width,
            new_state,
            &splits_breaks,
        ))
    }

    /// Port of `Level.tryBreakLastLevel`.
    fn try_break_last_level(
        &self,
        max_width: i32,
        state: &State,
        is_simple_inlining_so_far: bool,
    ) -> Option<State> {
        let last = self.docs.last()?;
        let Doc::Level(inner_level) = last else {
            return None;
        };
        self.try_break_inner_level(max_width, state, inner_level, is_simple_inlining_so_far)
    }

    /// Port of `Level.tryInlineSuffix`.
    fn try_inline_suffix(
        &self,
        max_width: i32,
        state: &State,
        is_simple_inlining_so_far: bool,
    ) -> Option<State> {
        // Reference asserts the last doc is a level starting with a break; we
        // treat a violated precondition as "cannot inline" rather than panicking.
        let last = self.docs.last()?;
        if !matches!(last, Doc::Level(_)) || starts_with_break(last) != StartsWith::Yes {
            return None;
        }
        if self.docs.len() < 2 {
            return None;
        }
        let Doc::Level(last_level_before_suffix) = &self.docs[self.docs.len() - 2] else {
            return None;
        };
        self.try_break_inner_level(
            max_width,
            state,
            last_level_before_suffix,
            is_simple_inlining_so_far,
        )
    }

    /// Port of `Level.tryBreakInnerLevel`.
    fn try_break_inner_level(
        &self,
        max_width: i32,
        state: &State,
        inner_level: &Self,
        is_simple_inlining_so_far: bool,
    ) -> Option<State> {
        // Only split levels that have declared they want to be split this way.
        if inner_level.breakability_if_last_level() == LastLevelBreakability::Abort {
            return None;
        }

        // See if we can fill in everything but the inner level.
        let inner_index = self
            .docs
            .iter()
            .position(|d| matches!(d, Doc::Level(l) if l.id == inner_level.id))?;
        let leading_docs = &self.docs[..inner_index];
        self.try_to_fit_on_one_line(max_width, state.column, leading_docs.iter())?;

        let trailing_docs = &self.docs[inner_index + 1..];
        let trailing_width = sum_width(trailing_docs.iter());
        if trailing_width >= INFINITY {
            return None;
        }

        let prefix_splits_breaks = split_by_breaks(leading_docs);
        let is_simple_inlining =
            is_simple_inlining_so_far && self.open_op.complexity == Complexity::Simple;

        let state1 = lay_out_level_on_one_line(max_width, state.clone(), &prefix_splits_breaks);
        // If a break was still forced even though the prefix fit, abort. (Could
        // happen if inner levels set a columnLimitBeforeLastBreak.)
        if state1.num_lines != state.num_lines {
            return None;
        }

        let state2 = match inner_level.breakability_if_last_level() {
            LastLevelBreakability::AcceptInlineChainIfSimpleOtherwiseCheckInner => {
                if is_simple_inlining {
                    Self::try_break_inner_level_accept_inline_chain(max_width, inner_level, &state1)
                } else {
                    Self::try_break_inner_level_check_inner(
                        max_width,
                        inner_level,
                        is_simple_inlining,
                        &state1,
                    )
                }
            }
            LastLevelBreakability::CheckInner => Self::try_break_inner_level_check_inner(
                max_width,
                inner_level,
                is_simple_inlining,
                &state1,
            ),
            LastLevelBreakability::AcceptInlineChain => {
                Self::try_break_inner_level_accept_inline_chain(max_width, inner_level, &state1)
            }
            LastLevelBreakability::Abort => None,
        };

        // Do we have a suffix to inline too?
        let state_after_inner = state2?;
        let column =
            self.try_to_fit_on_one_line(max_width, state_after_inner.column, trailing_docs.iter())?;
        Some(state_after_inner.with_column(column))
    }

    /// Port of `Level.tryBreakInnerLevel_acceptInlineChain`.
    fn try_break_inner_level_accept_inline_chain(
        max_width: i32,
        last_level: &Self,
        state: &State,
    ) -> Option<State> {
        // We're allowed to break here, but first verify there's room to inline
        // this last level's prefix.
        let extra_width = count_width_until_break_level(last_level, max_width - state.indent);
        let still_fits = extra_width < INFINITY && state.column + extra_width <= max_width;
        if !still_fits {
            return None;
        }
        // computeBreaks (not computeBroken) so it can recurse into last_level.
        Some(last_level.compute_breaks(max_width, state.clone()))
    }

    /// Port of `Level.tryBreakInnerLevel_checkInner`. Dispatches on the inner
    /// level's own `BreakBehaviour`.
    fn try_break_inner_level_check_inner(
        max_width: i32,
        inner_level: &Self,
        is_simple_inlining: bool,
        state: &State,
    ) -> Option<State> {
        match inner_level.break_behaviour() {
            BreakBehaviour::PreferBreakingLastInnerLevel {
                keep_indent_when_inlined,
            } => {
                let state1 = if keep_indent_when_inlined {
                    state.with_indent_incremented_by(inner_level.get_plus_indent())
                } else {
                    state.clone()
                };
                inner_level.try_break_last_level(max_width, &state1, is_simple_inlining)
            }
            BreakBehaviour::InlineSuffix => {
                inner_level.try_inline_suffix(max_width, state, is_simple_inlining)
            }
            BreakBehaviour::BreakOnlyIfInnerLevelsThenFitOnOneLine {
                keep_indent_when_inlined,
            } => {
                // This case currently only matches lambda expressions (no curlies).
                let state1 = if keep_indent_when_inlined {
                    state.with_indent_incremented_by(inner_level.get_plus_indent())
                } else {
                    state.clone()
                };

                // Only accept levels meant to look good when partially inlined
                // (e.g. method chains). Inspect the inner level's own last inner
                // level, two levels down.
                let last = inner_level.docs.last()?;
                let Doc::Level(last_level2) = last else {
                    return None;
                };
                match last_level2.breakability_if_last_level() {
                    LastLevelBreakability::Abort | LastLevelBreakability::CheckInner => None,
                    LastLevelBreakability::AcceptInlineChain => {
                        let broken = inner_level.break_normally(max_width, state);
                        inner_level.handle_break_only_if_inner_levels_then_fit(
                            max_width,
                            &state1,
                            &broken,
                            keep_indent_when_inlined,
                        )
                    }
                    // Specific to lambda body expressions - falls back to
                    // breakNormally in preferBreakingLastInnerLevel.
                    LastLevelBreakability::AcceptInlineChainIfSimpleOtherwiseCheckInner => None,
                }
            }
            // We don't know how to fit the inner level on the same line, so bail.
            BreakBehaviour::BreakThisLevel => None,
        }
    }

    /// Mark breaks in this level as not broken, but lay out inner levels
    /// normally. The resulting `must_break` is true if this level did not fit on
    /// exactly one line. Port of `Level.tryToLayOutLevelOnOneLine` (see the free
    /// function [`lay_out_level_on_one_line`]).
    fn compute_broken(&self, max_width: i32, state: State) -> State {
        let splits_breaks = split_by_breaks(&self.docs);

        let mut state = state;
        if !splits_breaks.breaks.is_empty() {
            state = state.with_broken_level();
        }

        state = self.compute_break_and_split(max_width, state, None, &splits_breaks.splits[0]);

        // Handle following breaks and splits.
        for i in 0..splits_breaks.breaks.len() {
            state = self.compute_break_and_split(
                max_width,
                state,
                Some(splits_breaks.breaks[i]),
                &splits_breaks.splits[i + 1],
            );
        }
        state
    }

    /// Lay out a break-separated group of docs. Port of
    /// `Level.computeBreakAndSplit`.
    fn compute_break_and_split(
        &self,
        max_width: i32,
        state: State,
        opt_break: Option<&Break>,
        split: &[&Doc],
    ) -> State {
        let break_width = opt_break.map_or(0, Break::width);
        let split_width = sum_width(split.iter().copied());

        let should_break = opt_break.is_some_and(|b| b.fill_mode == FillMode::Unified)
            || state.must_break
            || break_width >= INFINITY
            || self
                .try_to_fit_on_one_line(
                    max_width,
                    state.column + break_width,
                    split.iter().copied(),
                )
                .is_none();

        let mut state = state;
        if let Some(b) = opt_break {
            state = b.apply_break(state, should_break);
        }
        let enough_room = state.column + split_width <= max_width;
        state = compute_split(
            max_width,
            split.iter().copied(),
            state.with_must_break(false),
        );
        if !enough_room {
            state = state.with_must_break(true); // Break after, too.
        }
        state
    }

    /// The approximate maximum depth of nested (break-containing) levels. Port of
    /// `Level.computeMaxDepth`.
    fn compute_max_depth(&self) -> i32 {
        let mut max_child_depth = 0;
        for doc in &self.docs {
            if let Doc::Level(child) = doc {
                max_child_depth = max_child_depth.max(child.compute_max_depth());
            }
        }
        // Presence of breaks is a proxy for meaningful structural nesting.
        let has_breaks = self.docs.iter().any(|d| matches!(d, Doc::Break(_)));
        if has_breaks {
            1 + max_child_depth
        } else {
            max_child_depth
        }
    }
}

/// A level's docs, grouped into break-separated splits.
pub(crate) struct SplitsBreaks<'a> {
    /// Groups of docs separated by breaks. `splits.len() == breaks.len() + 1`.
    pub(crate) splits: Vec<Vec<&'a Doc>>,
    /// Breaks between the splits.
    pub(crate) breaks: Vec<&'a Break>,
}

/// Port of `Level.splitByBreaks`.
pub(crate) fn split_by_breaks(docs: &[Doc]) -> SplitsBreaks<'_> {
    let mut splits: Vec<Vec<&Doc>> = Vec::new();
    let mut breaks: Vec<&Break> = Vec::new();
    let mut current: Vec<&Doc> = Vec::new();
    for doc in docs {
        if let Doc::Break(b) = doc {
            splits.push(std::mem::take(&mut current));
            breaks.push(b);
        } else {
            current.push(doc);
        }
    }
    splits.push(current);
    SplitsBreaks { splits, breaks }
}

/// Port of `Level.tryToLayOutLevelOnOneLine` (does not reference the owning
/// level's docs, so a free function).
fn lay_out_level_on_one_line(max_width: i32, state: State, splits_breaks: &SplitsBreaks) -> State {
    let mut state = state;
    for i in 0..splits_breaks.splits.len() {
        if i > 0 {
            state = splits_breaks.breaks[i - 1].apply_break(state, false);
        }
        let split = &splits_breaks.splits[i];
        let split_width = sum_width(split.iter().copied());
        let enough_room = state.column + split_width <= max_width;
        state = compute_split(
            max_width,
            split.iter().copied(),
            state.with_must_break(false),
        );
        if !enough_room {
            state = state.with_must_break(true);
        }
    }
    state
}

/// Port of `Level.computeSplit`.
fn compute_split<'a>(
    max_width: i32,
    docs: impl IntoIterator<Item = &'a Doc>,
    state: State,
) -> State {
    let mut state = state;
    for doc in docs {
        state = doc.compute_breaks(max_width, state);
    }
    state
}

// ---------------------------------------------------------------------------
// StartsWithBreakVisitor
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StartsWith {
    Empty,
    No,
    Yes,
}

/// Port of `StartsWithBreakVisitor`.
fn starts_with_break(doc: &Doc) -> StartsWith {
    match doc {
        Doc::Break(_) => StartsWith::Yes,
        Doc::Space | Doc::Text(_) | Doc::Literal(_) => StartsWith::No,
        Doc::Level(l) => {
            for child in &l.docs {
                let r = starts_with_break(child);
                if r != StartsWith::Empty {
                    return r;
                }
            }
            StartsWith::Empty
        }
    }
}

// ---------------------------------------------------------------------------
// CountWidthUntilBreakVisitor
// ---------------------------------------------------------------------------

/// Port of `CountWidthUntilBreakVisitor.visit` for an arbitrary doc.
fn count_width_until_break_doc(doc: &Doc, available_width: i32) -> i32 {
    match doc {
        Doc::Space | Doc::Text(_) | Doc::Literal(_) | Doc::Break(_) => doc.width(),
        Doc::Level(l) => count_width_until_break_level(l, available_width),
    }
}

/// Port of `CountWidthUntilBreakVisitor.visitLevel`.
fn count_width_until_break_level(level: &Level, available_width: i32) -> i32 {
    if level.partial_inlineability() == PartialInlineability::IfFirstLevelFits
        && !level.docs.is_empty()
        // If this prefix wouldn't fit on a new line, don't consider it at all.
        && level.docs[0].width() <= available_width
    {
        // Incorporate only the length of this prefix (the first level).
        return level.docs[0].width();
    }
    // Otherwise, drill down into the first non-empty level.
    if let Some(idx) = first_non_empty_level(&level.docs) {
        return count_width_until_break_doc(&level.docs[idx], available_width);
    }
    // Level starts with tokens directly: sum widths up to the first break.
    let mut sum = 0;
    for doc in &level.docs {
        if matches!(doc, Doc::Break(_)) {
            break;
        }
        sum = add_width(sum, doc.width());
    }
    sum
}

/// Port of `CountWidthUntilBreakVisitor.getFirstNonEmptyLevel`.
fn first_non_empty_level(docs: &[Doc]) -> Option<usize> {
    for (idx, doc) in docs.iter().enumerate() {
        if !matches!(doc, Doc::Level(_)) {
            return None;
        }
        if starts_with_break(doc) != StartsWith::Empty {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::break_::{Break, FillMode};
    use super::super::indent::Indent;
    use super::super::level::{Level, OpenOp, PartialInlineability};
    use super::super::Doc;
    use super::{
        count_width_until_break_level, first_non_empty_level, split_by_breaks, starts_with_break,
        StartsWith,
    };

    fn lvl(docs: Vec<Doc>) -> Level {
        Level::new(OpenOp::default(), docs)
    }

    #[test]
    fn split_by_breaks_groups_splits() {
        let docs = vec![
            Doc::text("a"),
            Doc::brk(Break::make(FillMode::Unified, " ", Indent::ZERO)),
            Doc::text("b"),
            Doc::brk(Break::make(FillMode::Unified, " ", Indent::ZERO)),
            Doc::text("c"),
        ];
        let sb = split_by_breaks(&docs);
        assert_eq!(sb.breaks.len(), 2);
        assert_eq!(sb.splits.len(), 3);
        assert_eq!(sb.splits[0].len(), 1);
    }

    #[test]
    fn starts_with_break_classifies() {
        assert_eq!(starts_with_break(&Doc::text("x")), StartsWith::No);
        assert_eq!(
            starts_with_break(&Doc::brk(Break::make(FillMode::Unified, "", Indent::ZERO))),
            StartsWith::Yes
        );
        // Empty level (only empty nested levels) -> Empty.
        let empty = Doc::level(lvl(vec![Doc::level(lvl(vec![]))]));
        assert_eq!(starts_with_break(&empty), StartsWith::Empty);
        // Level starting with a break -> Yes.
        let starts = Doc::level(lvl(vec![
            Doc::brk(Break::make(FillMode::Unified, "", Indent::ZERO)),
            Doc::text("x"),
        ]));
        assert_eq!(starts_with_break(&starts), StartsWith::Yes);
    }

    #[test]
    fn count_width_sums_tokens_until_first_break() {
        let level = lvl(vec![
            Doc::text("abcd"),
            Doc::brk(Break::make(FillMode::Unified, "", Indent::ZERO)),
            Doc::text("efgh"),
        ]);
        assert_eq!(count_width_until_break_level(&level, 100), 4);
    }

    #[test]
    fn count_width_if_first_level_fits_uses_first_child() {
        let level = Level::new(
            OpenOp::default().with_partial_inlineability(PartialInlineability::IfFirstLevelFits),
            vec![Doc::text("abcdef"), Doc::text("ignored")],
        );
        // First child width 6 fits in avail 10 -> returned; larger avail bound
        // would still return 6.
        assert_eq!(count_width_until_break_level(&level, 10), 6);
        // If the first child doesn't fit in avail, fall through to summing.
        assert_eq!(count_width_until_break_level(&level, 3), 13);
    }

    #[test]
    fn first_non_empty_level_finds_index() {
        let docs = vec![
            Doc::level(lvl(vec![])),
            Doc::level(lvl(vec![Doc::brk(Break::make(
                FillMode::Unified,
                "",
                Indent::ZERO,
            ))])),
        ];
        assert_eq!(first_non_empty_level(&docs), Some(1));
        // A non-level doc breaks the search.
        let docs2 = vec![Doc::text("x")];
        assert_eq!(first_non_empty_level(&docs2), None);
    }
}
