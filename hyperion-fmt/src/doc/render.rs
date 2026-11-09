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

//! Rendering the final output after breaking decisions have been made.
//!
//! Port of `Doc.write` / `Level.write` / `Break.write` plus the pending-indent
//! discipline of `com.palantir.javaformat.java.JavaOutput`:
//!
//! * Indentation after a newline is written lazily, only just before the next
//!   visible character (`indent()` sets `spaces_pending`; those spaces are
//!   flushed only when a real character is appended).
//! * Trailing spaces before a newline are dropped (a newline resets
//!   `spaces_pending` to 0).
//! * Blank lines never carry trailing whitespace, and a trailing all-whitespace
//!   line is dropped on `finish`.

use super::break_::Break;
use super::compute::split_by_breaks;
use super::level::Level;
use super::state::State;
use super::Doc;

/// Accumulates output using palantir's `spacesPending` / `newlinesPending`
/// model, guaranteeing no trailing whitespace on any line.
#[derive(Debug, Default)]
pub struct Output {
    lines: Vec<String>,
    line_builder: String,
    spaces_pending: usize,
    newlines_pending: usize,
}

impl Output {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append text (`JavaOutput.append`, without input-range tracking).
    pub fn append(&mut self, text: &str) {
        if is_newline(text) {
            // Swallow extra newlines; drop any pending spaces before the newline.
            if self.newlines_pending == 0 {
                self.newlines_pending += 1;
            }
            self.spaces_pending = 0;
            return;
        }
        for c in text.chars() {
            match c {
                ' ' => self.spaces_pending += 1,
                '\n' => {
                    self.spaces_pending = 0;
                    self.newlines_pending += 1;
                }
                // A lone '\r' or the '\r' of a "\r\n" is ignored; the following
                // '\n' (if any) produces the single newline.
                '\r' => {}
                _ => {
                    while self.newlines_pending > 0 {
                        // Drop leading blank lines.
                        if !self.lines.is_empty() || !self.line_builder.is_empty() {
                            self.lines.push(std::mem::take(&mut self.line_builder));
                        }
                        self.line_builder.clear();
                        self.newlines_pending -= 1;
                    }
                    while self.spaces_pending > 0 {
                        self.line_builder.push(' ');
                        self.spaces_pending -= 1;
                    }
                    self.line_builder.push(c);
                }
            }
        }
    }

    /// Set the pending indentation for the next visible character.
    pub fn indent(&mut self, indent: i32) {
        self.spaces_pending = indent.max(0) as usize;
    }

    /// Flush any incomplete last line and produce the final string.
    #[must_use]
    pub fn finish(mut self) -> String {
        // Only keep the last line if it has non-whitespace content.
        if !self.line_builder.trim().is_empty() {
            self.lines.push(std::mem::take(&mut self.line_builder));
        }
        self.lines.join("\n")
    }
}

fn is_newline(s: &str) -> bool {
    s == "\n" || s == "\r\n" || s == "\r"
}

impl Doc {
    /// Write this doc to `output` (`Doc.write`).
    pub(crate) fn write(&self, state: &State, output: &mut Output) {
        match self {
            Self::Level(l) => l.write(state, output),
            Self::Break(b) => b.write(state, output),
            Self::Text(s) | Self::Literal(s) => output.append(s),
            Self::Space => output.append(" "),
        }
    }
}

impl Level {
    /// Write this level (`Level.write` / `Level.writeFilled`).
    pub(crate) fn write(&self, state: &State, output: &mut Output) {
        if state.is_one_line(self.id) {
            // Defined because the level's width is finite when it is one line.
            output.append(&self.flat());
        } else {
            let splits_breaks = split_by_breaks(&self.docs);
            for doc in &splits_breaks.splits[0] {
                doc.write(state, output);
            }
            for i in 0..splits_breaks.breaks.len() {
                splits_breaks.breaks[i].write(state, output);
                for doc in &splits_breaks.splits[i + 1] {
                    doc.write(state, output);
                }
            }
        }
    }
}

impl Break {
    /// Write this break (`Break.write`).
    pub(crate) fn write(&self, state: &State, output: &mut Output) {
        let break_state = state.get_break_state(self.id());
        if break_state.broken {
            output.append("\n");
            output.indent(break_state.new_indent);
        } else {
            output.append(self.flat());
        }
    }
}

/// Solve breaking decisions for `level` at `max_width`, then render it.
#[must_use]
pub fn format(level: &Level, max_width: i32) -> String {
    let state = State::starting_state();
    let final_state = level.compute_breaks(max_width, state);
    write_doc(level, &final_state)
}

/// Render `level` using an already-computed `state`.
#[must_use]
pub fn write_doc(level: &Level, state: &State) -> String {
    let mut output = Output::new();
    level.write(state, &mut output);
    output.finish()
}

#[cfg(test)]
mod tests {
    use super::Output;

    #[test]
    fn indent_is_lazy_and_flushed_before_visible_char() {
        let mut out = Output::new();
        out.append("a");
        out.append("\n");
        out.indent(4);
        out.append("b");
        assert_eq!(out.finish(), "a\n    b");
    }

    #[test]
    fn trailing_spaces_before_newline_are_dropped() {
        let mut out = Output::new();
        out.append("a");
        out.append("   "); // pending spaces
        out.append("\n"); // newline drops them
        out.append("b");
        assert_eq!(out.finish(), "a\nb");
    }

    #[test]
    fn trailing_indent_at_eof_is_dropped() {
        let mut out = Output::new();
        out.append("a");
        out.append("\n");
        out.indent(4); // never followed by a visible char
        assert_eq!(out.finish(), "a");
    }

    #[test]
    fn blank_line_carries_no_whitespace() {
        // Two newlines with an indent between produce a blank middle line with no
        // trailing whitespace.
        let mut out = Output::new();
        out.append("a");
        out.append("\n");
        out.append("\n");
        out.indent(4);
        out.append("b");
        let result = out.finish();
        for line in result.split('\n') {
            assert_eq!(line, line.trim_end());
        }
    }

    #[test]
    fn leading_blank_lines_are_dropped() {
        let mut out = Output::new();
        out.append("\n");
        out.append("a");
        assert_eq!(out.finish(), "a");
    }
}
