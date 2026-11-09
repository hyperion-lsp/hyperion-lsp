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

//! Indentation amounts for [`Level`](super::level::Level)s and
//! [`Break`](super::break_::Break)s.
//!
//! Faithful port of `com.palantir.javaformat.Indent`. The indent is either a
//! constant, or a conditional whose value depends on whether a tagged break was
//! broken. Unlike the reference (which allows nested `Indent`s in the `If`
//! branches), the two branches here are plain `i32`s, which is sufficient for
//! the constructs this port needs to express.

use super::break_::BreakTag;
use super::state::State;

/// An indent for a `Level` or `Break`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Indent {
    /// A constant indent, independent of any break decisions.
    Const(i32),
    /// A conditional indent whose value depends on whether `tag` was broken.
    If {
        tag: BreakTag,
        then_indent: i32,
        else_indent: i32,
    },
}

impl Indent {
    /// The zero constant indent.
    pub const ZERO: Self = Self::Const(0);

    /// A constant indent of `n`.
    #[must_use]
    pub const fn constant(n: i32) -> Self {
        Self::Const(n)
    }

    /// A conditional indent.
    #[must_use]
    pub const fn if_broken(tag: BreakTag, then_indent: i32, else_indent: i32) -> Self {
        Self::If {
            tag,
            then_indent,
            else_indent,
        }
    }

    /// Evaluate this indent against the current `state`.
    #[must_use]
    pub fn eval(&self, state: &State) -> i32 {
        match self {
            Self::Const(n) => *n,
            Self::If {
                tag,
                then_indent,
                else_indent,
            } => {
                if state.was_break_taken(*tag) {
                    *then_indent
                } else {
                    *else_indent
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::break_::BreakTag;
    use super::super::state::State;
    use super::Indent;

    #[test]
    fn const_ignores_state() {
        let state = State::starting_state();
        assert_eq!(Indent::Const(7).eval(&state), 7);
        assert_eq!(Indent::ZERO.eval(&state), 0);
    }

    #[test]
    fn if_selects_branch_by_tag() {
        let tag = BreakTag::new();
        let indent = Indent::if_broken(tag, 8, 4);
        let base = State::starting_state();
        assert_eq!(indent.eval(&base), 4, "tag not taken -> else");
        let broken = base.break_taken(tag, true);
        assert_eq!(indent.eval(&broken), 8, "tag taken -> then");
    }
}
