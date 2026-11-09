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

//! Immutable formatting state.
//!
//! Faithful port of `com.palantir.javaformat.doc.State`. The reference uses
//! functional-java persistent maps keyed by object identity; here we key by the
//! unique `u32` ids assigned to each `Break`/`Level`, and use `im`'s persistent
//! `HashMap`/`HashSet` so that the many clones the solver performs share
//! structure (O(log n) instead of O(n)).

use im::{HashMap, HashSet};

use super::break_::{Break, BreakTag};
use super::indent::Indent;

/// Whether a particular break was broken, and at what indent column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakState {
    pub broken: bool,
    pub new_indent: i32,
}

/// Whether a particular level fit on exactly one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelState {
    pub one_line: bool,
}

/// State for writing. Immutable: every mutating method returns a new `State`.
#[derive(Clone, Debug)]
pub struct State {
    /// Last indent that was actually taken.
    pub last_indent: i32,
    /// Next indent, if the level is about to be broken.
    pub indent: i32,
    pub column: i32,
    pub must_break: bool,
    /// How many lines a particular formatting took.
    pub num_lines: i32,
    /// How many times we reached a branch where multiple formattings are
    /// considered. Expected runtime is exponential in this number.
    pub branching_coefficient: i32,

    break_tags_taken: HashSet<BreakTag>,
    /// Keyed by `Break` id.
    break_states: HashMap<u32, BreakState>,
    /// Keyed by `Level` id.
    level_states: HashMap<u32, LevelState>,
}

impl State {
    /// The initial state at column 0, indent 0.
    #[must_use]
    pub fn starting_state() -> Self {
        Self {
            last_indent: 0,
            indent: 0,
            column: 0,
            must_break: false,
            num_lines: 0,
            branching_coefficient: 0,
            break_tags_taken: HashSet::new(),
            break_states: HashMap::new(),
            level_states: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_break_state(&self, break_id: u32) -> BreakState {
        self.break_states
            .get(&break_id)
            .copied()
            .unwrap_or(BreakState {
                broken: false,
                new_indent: -1,
            })
    }

    #[must_use]
    pub fn was_break_taken(&self, tag: BreakTag) -> bool {
        self.break_tags_taken.contains(&tag)
    }

    #[must_use]
    pub(crate) fn is_one_line(&self, level_id: u32) -> bool {
        self.level_states
            .get(&level_id)
            .is_some_and(|ls| ls.one_line)
    }

    /// Record whether a break tag was taken.
    #[must_use]
    pub(crate) fn break_taken(&self, tag: BreakTag, broken: bool) -> Self {
        let currently = self.break_tags_taken.contains(&tag);
        if currently == broken {
            return self.clone();
        }
        let mut s = self.clone();
        if broken {
            s.break_tags_taken.insert(tag);
        } else {
            s.break_tags_taken.remove(&tag);
        }
        s
    }

    /// Increase the indent by `plus_indent`, and clear `must_break`. Does not
    /// commit to the indent yet, so `last_indent` is unchanged.
    #[must_use]
    pub(crate) fn with_indent_incremented_by(&self, plus_indent: &Indent) -> Self {
        let mut s = self.clone();
        s.indent = self.indent + plus_indent.eval(self);
        s.must_break = false;
        s
    }

    /// Reset any accumulated indent back to `last_indent`.
    #[must_use]
    pub(crate) fn with_no_indent(&self) -> Self {
        let mut s = self.clone();
        s.indent = self.last_indent;
        s.must_break = false;
        s
    }

    /// The current level is being broken and it has breaks in it; commit to the
    /// indent.
    #[must_use]
    pub(crate) fn with_broken_level(&self) -> Self {
        let mut s = self.clone();
        s.last_indent = self.indent;
        s
    }

    /// Update the state after processing a break, deciding if it was `broken`.
    #[must_use]
    pub(crate) fn with_break(&self, brk: &Break, broken: bool) -> Self {
        let mut s = self.clone();
        if broken {
            let new_column = (self.indent + brk.eval_plus_indent(self)).max(0);
            // lastIndent = indent -- we've proven we wrote at the new 'indent'.
            s.last_indent = self.indent;
            s.column = new_column;
            s.num_lines = self.num_lines + 1;
            s.break_states = self.break_states.update(
                brk.id(),
                BreakState {
                    broken: true,
                    new_indent: new_column,
                },
            );
        } else {
            s.column = self.column + brk.flat().chars().count() as i32;
        }
        s
    }

    /// Update the current state after having processed an *inner* level.
    ///
    /// Inherits the outer indent-related fields from `self`, but takes the
    /// column, line count, and global maps from the inner result.
    #[must_use]
    pub(crate) fn update_after_level(&self, after_inner_level: &Self) -> Self {
        Self {
            last_indent: self.last_indent,
            indent: self.indent,
            branching_coefficient: self.branching_coefficient,
            must_break: self.must_break,
            column: after_inner_level.column,
            num_lines: after_inner_level.num_lines,
            break_tags_taken: after_inner_level.break_tags_taken.clone(),
            break_states: after_inner_level.break_states.clone(),
            level_states: after_inner_level.level_states.clone(),
        }
    }

    #[must_use]
    pub(crate) fn with_column(&self, column: i32) -> Self {
        let mut s = self.clone();
        s.column = column;
        s
    }

    #[must_use]
    pub(crate) fn with_must_break(&self, must_break: bool) -> Self {
        let mut s = self.clone();
        s.must_break = must_break;
        s
    }

    #[must_use]
    pub(crate) fn with_new_branch(&self) -> Self {
        let mut s = self.clone();
        s.branching_coefficient = self.branching_coefficient + 1;
        s
    }

    #[must_use]
    pub(crate) fn with_level_state(&self, level_id: u32, level_state: LevelState) -> Self {
        let mut s = self.clone();
        s.level_states = self.level_states.update(level_id, level_state);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::super::indent::Indent;
    use super::{LevelState, State};

    #[test]
    fn starting_state_is_empty() {
        let s = State::starting_state();
        assert_eq!(s.column, 0);
        assert_eq!(s.indent, 0);
        assert_eq!(s.num_lines, 0);
        assert!(!s.get_break_state(1).broken);
        assert!(!s.is_one_line(1));
    }

    #[test]
    fn indent_and_no_indent_are_inverse_until_committed() {
        let s = State::starting_state();
        let bumped = s.with_indent_incremented_by(&Indent::Const(4));
        assert_eq!(bumped.indent, 4);
        assert_eq!(bumped.last_indent, 0, "not committed yet");
        let reset = bumped.with_no_indent();
        assert_eq!(reset.indent, 0, "reset to last_indent");
    }

    #[test]
    fn broken_level_commits_indent() {
        let s = State::starting_state().with_indent_incremented_by(&Indent::Const(4));
        let committed = s.with_broken_level();
        assert_eq!(committed.last_indent, 4);
    }

    #[test]
    fn update_after_level_keeps_outer_indent_but_inner_column() {
        let outer = State::starting_state().with_indent_incremented_by(&Indent::Const(4));
        let inner = outer
            .with_column(42)
            .with_level_state(9, LevelState { one_line: true });
        let merged = outer.update_after_level(&inner);
        assert_eq!(merged.indent, 4, "outer indent preserved");
        assert_eq!(merged.column, 42, "inner column adopted");
        assert!(merged.is_one_line(9), "inner level state adopted");
    }

    #[test]
    fn persistent_clone_does_not_mutate_original() {
        let base = State::starting_state();
        let derived = base.with_level_state(1, LevelState { one_line: true });
        assert!(!base.is_one_line(1), "original untouched by clone");
        assert!(derived.is_one_line(1));
    }
}
