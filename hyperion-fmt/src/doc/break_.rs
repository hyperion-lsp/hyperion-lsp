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

//! Optional line breaks.
//!
//! Faithful port of `com.palantir.javaformat.doc.Break`,
//! `com.palantir.javaformat.doc.FillMode` and
//! `com.palantir.javaformat.doc.BreakTag`.

use super::indent::Indent;
use super::next_id;
use super::state::State;
use super::INFINITY;

/// Each `Break` in a `Level` is either [`FillMode::Unified`],
/// [`FillMode::Independent`] or [`FillMode::Forced`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FillMode {
    /// If a `Level` will not fit on one line, all of its `Unified` breaks break.
    Unified,
    /// If a `Level` will not fit on one line, its `Independent` breaks are broken
    /// independently of each other, to fill in the `Level`.
    Independent,
    /// A `Forced` break always breaks, and any `Level` it appears in will not fit
    /// on one line.
    Forced,
}

/// Unique identifier for a break. Conditional structures such as [`Indent::If`]
/// behave differently based on whether this tag was broken.
///
/// Port of `com.palantir.javaformat.doc.BreakTag` — a `HasUniqueId`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BreakTag(pub(crate) u32);

impl BreakTag {
    /// Allocate a fresh, globally-unique break tag.
    #[must_use]
    pub fn new() -> Self {
        Self(next_id())
    }
}

impl Default for BreakTag {
    fn default() -> Self {
        Self::new()
    }
}

/// A leaf node in a `Doc` for an optional break.
#[derive(Clone, Debug)]
pub struct Break {
    /// Globally-unique id, used as a key into [`State`]'s break-state map.
    pub(crate) id: u32,
    pub(crate) fill_mode: FillMode,
    pub(crate) flat: String,
    pub(crate) plus_indent: Indent,
    pub(crate) tag: Option<BreakTag>,
    /// If this break is restricted by its parent `Level`'s
    /// `column_limit_before_last_break`.
    pub(crate) has_column_limit: bool,
}

impl Break {
    /// Make a `Break`.
    #[must_use]
    pub fn make(fill_mode: FillMode, flat: impl Into<String>, plus_indent: Indent) -> Self {
        Self {
            id: next_id(),
            fill_mode,
            flat: flat.into(),
            plus_indent,
            tag: None,
            has_column_limit: false,
        }
    }

    /// Make a `Break` with an associated [`BreakTag`].
    #[must_use]
    pub fn make_tagged(
        fill_mode: FillMode,
        flat: impl Into<String>,
        plus_indent: Indent,
        tag: BreakTag,
    ) -> Self {
        Self {
            id: next_id(),
            fill_mode,
            flat: flat.into(),
            plus_indent,
            tag: Some(tag),
            has_column_limit: false,
        }
    }

    /// Make a forced `Break`.
    #[must_use]
    pub fn make_forced() -> Self {
        Self {
            id: next_id(),
            fill_mode: FillMode::Forced,
            flat: String::new(),
            plus_indent: Indent::ZERO,
            tag: None,
            has_column_limit: false,
        }
    }

    /// Mark this break as being restricted by its parent level's
    /// `column_limit_before_last_break`.
    #[must_use]
    pub const fn with_column_limit(mut self) -> Self {
        self.has_column_limit = true;
        self
    }

    #[must_use]
    pub(crate) const fn id(&self) -> u32 {
        self.id
    }

    /// The break's extra indent, evaluated against `state`.
    #[must_use]
    pub(crate) fn eval_plus_indent(&self, state: &State) -> i32 {
        self.plus_indent.eval(state)
    }

    /// Is this break forced?
    #[must_use]
    pub fn is_forced(&self) -> bool {
        self.fill_mode == FillMode::Forced
    }

    /// The width of this break, or [`INFINITY`] if it is forced.
    #[must_use]
    pub(crate) fn width(&self) -> i32 {
        if self.is_forced() {
            INFINITY
        } else {
            self.flat.chars().count() as i32
        }
    }

    /// The break's flat (unbroken) rendering.
    #[must_use]
    pub(crate) fn flat(&self) -> &str {
        &self.flat
    }

    /// Update the state for this break, deciding whether it is `broken`.
    ///
    /// Port of `Break.computeBreaks(State, boolean)`. Note that the break's tag
    /// is updated *before* the break's `plusIndent` is evaluated, so that an
    /// [`Indent::If`] referring to this break's own tag sees the fresh value.
    #[must_use]
    pub(crate) fn apply_break(&self, state_in: State, broken: bool) -> State {
        let state = match self.tag {
            Some(tag) => state_in.break_taken(tag, broken),
            None => state_in,
        };
        state.with_break(self, broken)
    }
}

#[cfg(test)]
mod tests {
    use super::super::indent::Indent;
    use super::super::state::State;
    use super::INFINITY;
    use super::{Break, BreakTag, FillMode};

    #[test]
    fn forced_break_is_infinite_and_forced() {
        let b = Break::make_forced();
        assert!(b.is_forced());
        assert_eq!(b.width(), INFINITY);
    }

    #[test]
    fn plain_break_width_is_flat_len() {
        let b = Break::make(FillMode::Unified, ", ", Indent::ZERO);
        assert!(!b.is_forced());
        assert_eq!(b.width(), 2);
        assert_eq!(b.flat(), ", ");
    }

    #[test]
    fn unique_ids_are_distinct() {
        let a = Break::make(FillMode::Unified, "", Indent::ZERO);
        let b = Break::make(FillMode::Unified, "", Indent::ZERO);
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn apply_break_updates_tag_before_evaluating_indent() {
        // A break whose own plusIndent depends on its own tag must see the tag as
        // taken when broken (reference Break.computeBreaks(State, boolean)).
        let tag = BreakTag::new();
        let b = Break::make_tagged(FillMode::Unified, "", Indent::if_broken(tag, 8, 4), tag);
        let state = State::starting_state();
        let after = b.apply_break(state, true);
        assert!(after.was_break_taken(tag));
        // newColumn = indent(0) + then-indent(8).
        assert_eq!(after.column, 8);
    }
}
