// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Oppen print phase - emits formatted output
//!
//! The print phase uses sizing information to decide whether groups should
//! be broken, then emits the final formatted text.

use crate::config::FormatConfig;
use crate::ir::{BreakType, DocToken, Document};

use super::scan::{Scanner, Size};

/// Printer state during output generation
pub struct Printer {
    config: FormatConfig,
}

impl Printer {
    /// Create a new printer with the given configuration
    pub const fn new(config: FormatConfig) -> Self {
        Self { config }
    }

    /// Print a document to a string
    pub fn print(&self, document: &Document) -> String {
        // Scan phase: compute sizes
        let sizes = Scanner::new(document).scan();

        // Print phase: emit output
        let mut state = PrintState::new(&self.config);
        state.print_tokens(&document.tokens, &sizes);

        let mut output = state.finish();

        // Ensure final newline if configured
        if self.config.insert_final_newline && !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }

        output
    }
}

/// Internal state for the print phase
struct PrintState<'a> {
    config: &'a FormatConfig,
    /// Output buffer
    output: String,
    /// Current column position (0-indexed)
    column: usize,
    /// Current indentation level (in spaces)
    indent: usize,
    /// Indentation scheduled after a newline but not yet written.
    ///
    /// Written lazily before the next visible character so that blank lines
    /// never carry trailing whitespace.
    pending_indent: Option<usize>,
    /// Stack of (indent, broken) for active groups
    group_stack: Vec<GroupState>,
}

#[derive(Debug, Clone, Copy)]
struct GroupState {
    /// Indentation when this group was entered
    indent: usize,
    /// Whether this group is broken
    broken: bool,
    /// Ambient offset from this group (applied to all nested breaks)
    ambient_offset: i32,
}

impl<'a> PrintState<'a> {
    const fn new(config: &'a FormatConfig) -> Self {
        Self {
            config,
            output: String::new(),
            column: 0,
            indent: 0,
            pending_indent: None,
            group_stack: Vec::new(),
        }
    }

    /// Write any scheduled indentation. Must be called before emitting visible text.
    fn flush_indent(&mut self) {
        if let Some(n) = self.pending_indent.take() {
            if self.config.use_tabs && self.config.tab_width > 0 {
                // Whole indent levels become tabs. A remainder short of a level stays spaces.
                let width = self.config.tab_width;
                self.output.push_str(&"\t".repeat(n / width));
                self.output.push_str(&" ".repeat(n % width));
            } else {
                self.output.push_str(&" ".repeat(n));
            }
        }
    }

    fn print_tokens(&mut self, tokens: &[DocToken], sizes: &[Size]) {
        for (i, token) in tokens.iter().enumerate() {
            self.print_token(token, sizes.get(i).copied().unwrap_or_else(Size::zero));
        }
    }

    fn print_token(&mut self, token: &DocToken, size: Size) {
        match token {
            DocToken::Text(s) => {
                self.flush_indent();
                self.output.push_str(s);
                self.column += s.chars().count();
            }

            DocToken::Break(b) => {
                let broken = self.is_broken();
                if broken {
                    // Breaking: emit newline, schedule indent lazily
                    self.output.push('\n');

                    // Calculate cumulative ambient offset from BROKEN parent groups only
                    // Only broken groups contribute their ambient offset (break-aware)
                    let ambient = self
                        .group_stack
                        .iter()
                        .filter(|g| g.broken)
                        .map(|g| g.ambient_offset)
                        .sum::<i32>();

                    #[allow(clippy::cast_sign_loss)]
                    let new_indent = (i32::try_from(self.indent).unwrap_or(0) + b.offset + ambient)
                        .max(0) as usize;
                    self.pending_indent = Some(new_indent);
                    self.column = new_indent;
                    if !b.break_text.is_empty() {
                        self.flush_indent();
                        self.output.push_str(&b.break_text);
                        self.column += b.break_text.chars().count();
                    }
                } else if !b.flat_text.is_empty() {
                    // Not breaking: emit flat text
                    self.flush_indent();
                    self.output.push_str(&b.flat_text);
                    self.column += b.flat_text.chars().count();
                }
            }

            DocToken::Begin(b) => {
                // Decide whether to break this group
                // For chains, use chain_wrap_column (default 80) instead of max_line_width (default 120)
                // For builders, always break (they look better broken)
                //
                // When uses_structural_size is true, use flat_structural instead of flat.
                // This allows groups containing internal hardlines (like lambdas) to stay
                // on one line if they fit structurally.
                let decision_size = if b.uses_structural_size {
                    size.flat_structural
                } else {
                    size.flat
                };

                let broken = match b.break_type {
                    BreakType::Builder => true, // Builders always break
                    BreakType::Chain => {
                        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                        let fits = self.column as i32 + decision_size
                            <= i32::try_from(self.config.chain_wrap_column).unwrap_or(80);
                        !fits || decision_size >= Size::INFINITY
                    }
                    _ => {
                        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                        let fits = self.column as i32 + decision_size
                            <= i32::try_from(self.config.max_line_width).unwrap_or(100);
                        !fits || decision_size >= Size::INFINITY
                    }
                };

                #[allow(clippy::cast_sign_loss)]
                let new_indent =
                    (i32::try_from(self.indent).unwrap_or(0) + b.offset).max(0) as usize;

                self.group_stack.push(GroupState {
                    indent: self.indent,
                    broken,
                    ambient_offset: b.ambient_offset,
                });

                self.indent = new_indent;
            }

            DocToken::End => {
                if let Some(state) = self.group_stack.pop() {
                    self.indent = state.indent;
                }
            }

            DocToken::Conditional(c) => {
                let broken = self.is_broken();
                let text = if broken { &c.broken_text } else { &c.flat_text };
                if !text.is_empty() {
                    self.flush_indent();
                    self.output.push_str(text);
                    self.column += text.chars().count();
                }
            }

            DocToken::Hardline => {
                // Drop any indent scheduled for this line: it would become
                // trailing whitespace on a blank line.
                self.pending_indent = None;
                self.output.push('\n');

                // Calculate cumulative ambient offset from BROKEN parent groups only
                // Only broken groups contribute their ambient offset (break-aware)
                let ambient = self
                    .group_stack
                    .iter()
                    .filter(|g| g.broken)
                    .map(|g| g.ambient_offset)
                    .sum::<i32>();

                #[allow(clippy::cast_sign_loss)]
                let indent_with_ambient =
                    (i32::try_from(self.indent).unwrap_or(0) + ambient).max(0) as usize;

                self.pending_indent = Some(indent_with_ambient);
                self.column = indent_with_ambient;
            }

            DocToken::Literal(s) => {
                // Output literal text as-is, updating column for the last line.
                // A literal that begins with a newline (blank line) discards the
                // scheduled indent instead of flushing it as trailing whitespace.
                if s.starts_with('\n') {
                    self.pending_indent = None;
                } else {
                    self.flush_indent();
                }
                self.output.push_str(s);
                if let Some(last_newline) = s.rfind('\n') {
                    self.column = s[last_newline + 1..].chars().count();
                } else {
                    self.column += s.chars().count();
                }
            }

            DocToken::WidthReservation(_) => {}
        }
    }

    /// Check if we're currently in a broken group
    fn is_broken(&self) -> bool {
        self.group_stack.last().is_some_and(|g| g.broken)
    }

    fn finish(self) -> String {
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::DocumentBuilder;

    #[test]
    fn test_print_simple_text() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.text("hello world");

        let printer = Printer::new(FormatConfig::default());
        let doc = builder.finish();
        let output = printer.print(&doc);

        assert_eq!(output, "hello world\n");
    }

    #[test]
    fn test_width_counts_characters_not_bytes() {
        // "ééééé ab" is 8 characters but 13 bytes. It fits in 10 columns.
        let config = FormatConfig::default().with_max_line_width(10);
        let mut builder = DocumentBuilder::new(config.clone());
        builder.group(|b| {
            b.text("ééééé");
            b.space();
            b.text("ab");
        });

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        assert_eq!(output, "ééééé ab\n");
    }

    #[test]
    fn test_print_group_fits() {
        let config = FormatConfig::default().with_max_line_width(80);
        let mut builder = DocumentBuilder::new(config.clone());
        builder.group(|b| {
            b.text("short");
            b.space();
            b.text("text");
        });

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        // Should fit on one line
        assert_eq!(output, "short text\n");
    }

    #[test]
    fn test_print_group_breaks() {
        let config = FormatConfig::default().with_max_line_width(10);
        let mut builder = DocumentBuilder::new(config.clone());
        builder.group(|b| {
            b.text("this");
            b.space();
            b.text("is");
            b.space();
            b.text("long");
        });

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        // Should break because "this is long" (12 chars) > 10
        assert!(output.contains('\n'));
    }

    #[test]
    fn test_print_hardline() {
        let config = FormatConfig::default();
        let mut builder = DocumentBuilder::new(config.clone());
        builder.text("line1");
        builder.hardline();
        builder.text("line2");

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        assert_eq!(output, "line1\nline2\n");
    }

    #[test]
    fn test_print_indent() {
        let config = FormatConfig::default()
            .with_max_line_width(10)
            .with_indent_size(4);
        let mut builder = DocumentBuilder::new(config.clone());
        builder.text("if");
        builder.space();
        builder.text("(x)");
        builder.indent(|b| {
            b.hardline(); // hardline inside indent to get indented
            b.text("body");
        });

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        assert!(output.contains("    body")); // 4-space indent
    }

    #[test]
    fn test_print_chain_breaks_at_80() {
        // Chain should break at 80 cols even though max_line_width is 120
        let config = FormatConfig::default()
            .with_max_line_width(120)
            .with_chain_wrap_column(40); // Low threshold for testing

        let mut builder = DocumentBuilder::new(config.clone());
        builder.chain_group(|b| {
            b.text("receiver");
            b.chain_break();
            b.text(".veryLongMethodName()");
            b.chain_break();
            b.text(".anotherMethod()");
        });

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        // Should break because chain exceeds 40 cols
        assert!(output.contains('\n'), "Chain should break: {output}");
    }

    #[test]
    fn test_print_chain_fits_under_threshold() {
        // Short chain should not break
        let config = FormatConfig::default()
            .with_max_line_width(120)
            .with_chain_wrap_column(80);

        let mut builder = DocumentBuilder::new(config.clone());
        builder.chain_group(|b| {
            b.text("a");
            b.chain_break();
            b.text(".b()");
        });

        let printer = Printer::new(config);
        let doc = builder.finish();
        let output = printer.print(&doc);

        // Should not break - fits under 80
        assert!(
            !output.contains('\n') || output == "a.b()\n",
            "Short chain should not break: {output}"
        );
    }

    #[test]
    fn width_reservation_affects_fit_without_rendering() {
        let config = FormatConfig::default().with_max_line_width(5);
        let mut builder = DocumentBuilder::new(config.clone());
        builder.group(|b| {
            b.text("abcd");
            b.softline();
            b.reserve_width(2);
        });

        let output = Printer::new(config).print(&builder.finish());
        assert_eq!(output, "abcd\n");
    }
}
