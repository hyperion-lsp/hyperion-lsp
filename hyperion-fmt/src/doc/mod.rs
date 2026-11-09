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

//! A faithful Rust port of the layout-solver core of
//! [palantir-java-format](https://github.com/palantir/palantir-java-format).
//!
//! The document model is a tree of [`Doc`] nodes. A top-level [`Level`] contains
//! a sequence of `Doc`s, including nested `Level`s. Leaf docs are tokens
//! ([`Doc::Text`]), non-breaking spaces ([`Doc::Space`]), verbatim literals /
//! comments ([`Doc::Literal`]) and optional [`Break`]s.
//!
//! The solver ([`compute`]) makes breaking decisions producing a [`State`], and
//! the renderer ([`render`]) writes the final output using the pending-indent
//! discipline of palantir's `JavaOutput` (no trailing whitespace on any line).
//!
//! This module is intentionally *not* wired into the formatting pipeline yet; it
//! exists to be validated against the reference before being adopted.

use std::sync::atomic::{AtomicU32, Ordering};

pub mod break_;
pub mod compute;
pub mod indent;
pub mod level;
pub mod render;
pub mod state;

#[cfg(test)]
mod tests;

pub use break_::{Break, BreakTag, FillMode};
pub use indent::Indent;
pub use level::{
    BreakBehaviour, Complexity, LastLevelBreakability, Level, OpenOp, PartialInlineability,
};
pub use render::{format, write_doc, Output};
pub use state::{BreakState, LevelState, State};

/// Sentinel width for a doc that "must be broken".
///
/// Stands in for `Float.POSITIVE_INFINITY` in the reference. Chosen well below
/// `i32::MAX` so that sums involving it cannot overflow (see [`add_width`]).
pub const INFINITY: i32 = i32::MAX / 4;

/// Global id generator, mirroring the reference's `HasUniqueId`. Each `Level`,
/// `Break` and `BreakTag` gets a unique id used as a map key in [`State`].
static UNIQUE_ID: AtomicU32 = AtomicU32::new(1);

pub(crate) fn next_id() -> u32 {
    UNIQUE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Saturating width addition: if either operand is [`INFINITY`], the result is
/// [`INFINITY`]. This mirrors float infinity arithmetic while staying in `i32`.
#[must_use]
pub(crate) const fn add_width(a: i32, b: i32) -> i32 {
    if a >= INFINITY || b >= INFINITY {
        INFINITY
    } else {
        a + b
    }
}

/// The width of a sequence of docs, or [`INFINITY`] if any must be broken.
#[must_use]
pub(crate) fn sum_width<'a>(docs: impl IntoIterator<Item = &'a Doc>) -> i32 {
    let mut width = 0;
    for doc in docs {
        width = add_width(width, doc.width());
    }
    width
}

/// A node in a document tree.
///
/// Combines the reference's `Token`, `NonBreakingSpace`, `Comment`, `Break` and
/// `Level` leaf/branch kinds. `Text` covers language tokens, `Space` is a
/// non-breaking space, and `Literal` covers comments / verbatim text.
#[derive(Clone, Debug)]
pub enum Doc {
    Level(Level),
    Break(Break),
    /// A language token.
    Text(String),
    /// A single non-breaking space.
    Space,
    /// Verbatim text (comments and the like); rendered as-is.
    Literal(String),
}

impl Doc {
    /// Construct a text (token) doc.
    #[must_use]
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Construct a verbatim literal doc.
    #[must_use]
    pub fn literal(s: impl Into<String>) -> Self {
        Self::Literal(s.into())
    }

    /// Construct a non-breaking space doc.
    #[must_use]
    pub const fn space() -> Self {
        Self::Space
    }

    /// Wrap a break as a doc.
    #[must_use]
    pub const fn brk(b: Break) -> Self {
        Self::Break(b)
    }

    /// Wrap a level as a doc.
    #[must_use]
    pub const fn level(l: Level) -> Self {
        Self::Level(l)
    }

    /// The width of this doc, or [`INFINITY`] if it must be broken.
    #[must_use]
    pub(crate) fn width(&self) -> i32 {
        match self {
            Self::Level(l) => l.width(),
            Self::Break(b) => b.width(),
            Self::Text(s) | Self::Literal(s) => s.chars().count() as i32,
            Self::Space => 1,
        }
    }

    /// This doc's flat (single-line) rendering.
    #[must_use]
    pub(crate) fn flat(&self) -> String {
        match self {
            Self::Level(l) => l.flat(),
            Self::Break(b) => b.flat().to_owned(),
            Self::Text(s) | Self::Literal(s) => s.clone(),
            Self::Space => " ".to_owned(),
        }
    }
}
