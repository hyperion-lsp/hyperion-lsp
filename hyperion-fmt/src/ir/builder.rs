// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Document IR builder for constructing formatted output

use super::document::{BeginToken, BreakToken, BreakType, ConditionalToken, DocToken, Document};
use crate::config::FormatConfig;

/// Builder for constructing a Document IR
#[derive(Debug)]
pub struct DocumentBuilder {
    config: FormatConfig,
    tokens: Vec<DocToken>,
    /// Stack of group start indices for validation
    group_stack: Vec<usize>,
    /// Set right after `line_comment()` emits its forced newline, so an
    /// immediately following `hardline()` from the caller is absorbed instead
    /// of producing a stray blank line
    after_line_comment_break: bool,
}

impl DocumentBuilder {
    /// Create a new document builder with the given configuration
    #[must_use]
    pub const fn new(config: FormatConfig) -> Self {
        Self {
            config,
            tokens: Vec::new(),
            group_stack: Vec::new(),
            after_line_comment_break: false,
        }
    }

    /// Get the formatter configuration
    #[must_use]
    pub const fn config(&self) -> &FormatConfig {
        &self.config
    }

    /// Append a text token (non-whitespace content)
    pub fn text(&mut self, s: impl Into<String>) {
        let text = s.into();
        if !text.is_empty() {
            self.after_line_comment_break = false;
            self.tokens.push(DocToken::Text(text));
        }
    }

    /// Append a space that can become a newline
    pub fn space(&mut self) {
        self.tokens.push(DocToken::Break(BreakToken::space()));
    }

    /// Append a space that becomes a newline with continuation indent when broken
    pub fn space_with_continuation(&mut self) {
        let offset = i32::try_from(self.config.continuation_indent).unwrap_or(8);
        self.tokens.push(DocToken::Break(BreakToken {
            flat_text: " ".to_string(),
            break_text: String::new(),
            offset,
        }));
    }

    /// Append a soft break (no space when flat, newline when broken)
    pub fn softline(&mut self) {
        self.tokens.push(DocToken::Break(BreakToken::soft()));
    }

    /// Append a break with custom flat text
    pub fn break_with(&mut self, flat_text: impl Into<String>) {
        self.tokens.push(DocToken::Break(BreakToken {
            flat_text: flat_text.into(),
            break_text: String::new(),
            offset: 0,
        }));
    }

    /// Append a hard line break (always breaks)
    pub fn hardline(&mut self) {
        // A line comment already terminated this line; absorb the duplicate
        if self.after_line_comment_break {
            self.after_line_comment_break = false;
            return;
        }
        self.tokens.push(DocToken::Hardline);
    }

    /// Append a line comment (`// ...`) followed by a forced newline.
    ///
    /// A line comment swallows everything to end-of-line, so flattening an
    /// expression around one would comment out the rest of the statement —
    /// the forced break is a correctness requirement, not a style choice.
    pub fn line_comment(&mut self, s: impl Into<String>) {
        self.literal(s);
        self.tokens.push(DocToken::Hardline);
        self.after_line_comment_break = true;
    }

    /// Append a blank line (empty line with no indentation)
    /// Use this for visual separation between statements inside blocks
    pub fn blank_line(&mut self) {
        self.tokens.push(DocToken::Literal("\n".to_string()));
    }

    /// Append multiple hard line breaks (for blank lines)
    pub fn blank_lines(&mut self, count: usize) {
        for _ in 0..count {
            self.tokens.push(DocToken::Hardline);
        }
    }

    /// Append literal text (preserves exact content, used for comments)
    ///
    /// Trailing whitespace is trimmed from every line: palantir-java-format
    /// never emits it, even inside verbatim comment bodies.
    pub fn literal(&mut self, s: impl Into<String>) {
        let s = s.into();
        let s = if s.contains('\n') {
            // split('\n') keeps a final empty segment for a trailing newline,
            // so join() reconstructs it unchanged
            s.split('\n')
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            s
        };
        self.tokens.push(DocToken::Literal(s));
    }

    /// Start a consistent group (all-or-nothing breaks)
    pub fn begin_consistent(&mut self, offset: i32) {
        self.group_stack.push(self.tokens.len());
        self.tokens
            .push(DocToken::Begin(BeginToken::consistent(offset)));
    }

    /// Start an inconsistent group (greedy line filling)
    pub fn begin_inconsistent(&mut self, offset: i32) {
        self.group_stack.push(self.tokens.len());
        self.tokens
            .push(DocToken::Begin(BeginToken::inconsistent(offset)));
    }

    /// Start a method chain group
    pub fn begin_chain(&mut self, offset: i32) {
        self.group_stack.push(self.tokens.len());
        self.tokens.push(DocToken::Begin(BeginToken {
            break_type: BreakType::Chain,
            offset,
            uses_structural_size: false,
            ambient_offset: 0,
        }));
    }

    /// Start a builder pattern group (uses continuation indent, always breaks)
    pub fn begin_builder(&mut self, offset: i32) {
        self.group_stack.push(self.tokens.len());
        self.tokens.push(DocToken::Begin(BeginToken {
            break_type: BreakType::Builder,
            offset,
            uses_structural_size: false,
            ambient_offset: 0,
        }));
    }

    /// Start a group with ambient offset (applies to all nested breaks)
    /// Used for assignment RHS where nested expressions need extra indent
    pub fn begin_ambient(&mut self, offset: i32, ambient_offset: i32) {
        self.group_stack.push(self.tokens.len());
        self.tokens
            .push(DocToken::Begin(BeginToken::ambient(offset, ambient_offset)));
    }

    /// Start a consistent group with structural sizing
    /// (ignores internal hardlines when deciding whether to break)
    pub fn begin_consistent_structural(&mut self, offset: i32) {
        self.group_stack.push(self.tokens.len());
        self.tokens
            .push(DocToken::Begin(BeginToken::consistent_structural(offset)));
    }

    /// Group content with structural sizing (for assignments with lambdas)
    pub fn structural_group<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.begin_consistent_structural(0);
        f(self);
        self.end();
    }

    /// End the current group
    pub fn end(&mut self) {
        self.group_stack.pop();
        self.tokens.push(DocToken::End);
    }

    /// Add a trailing comma (only appears when group is broken)
    pub fn trailing_comma(&mut self) {
        self.tokens
            .push(DocToken::Conditional(ConditionalToken::trailing_comma()));
    }

    /// Add conditional content
    pub fn conditional(&mut self, broken: impl Into<String>, flat: impl Into<String>) {
        self.tokens
            .push(DocToken::Conditional(ConditionalToken::new(broken, flat)));
    }

    /// Reserve columns for fit decisions without emitting any text.
    pub fn reserve_width(&mut self, width: usize) {
        if width > 0 {
            self.tokens.push(DocToken::WidthReservation(width));
        }
    }

    /// Indent the following content by the configured indent size
    pub fn indent<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let offset = i32::try_from(self.config.indent_size).unwrap_or(4);
        self.begin_consistent(offset);
        f(self);
        self.end();
    }

    /// Group content with consistent breaking
    pub fn group<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.begin_consistent(0);
        f(self);
        self.end();
    }

    /// Group content with inconsistent (fill) breaking
    pub fn fill<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.begin_inconsistent(0);
        f(self);
        self.end();
    }

    /// Group content for method chain formatting (breaks at 80 chars, uses continuation indent)
    pub fn chain_group<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let offset = i32::try_from(self.config.continuation_indent).unwrap_or(8);
        self.begin_chain(offset);
        f(self);
        self.end();
    }

    /// Group content for builder pattern formatting (uses continuation indent, always breaks)
    pub fn builder_group<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let offset = i32::try_from(self.config.continuation_indent).unwrap_or(8);
        self.begin_builder(offset);
        f(self);
        self.end();
    }

    /// Add a chain break (appears before dot when chain exceeds line width)
    pub fn chain_break(&mut self) {
        self.tokens.push(DocToken::Break(BreakToken {
            flat_text: String::new(),
            break_text: String::new(),
            offset: 0,
        }));
    }

    /// Append a soft break with custom flat text
    pub fn soft_break_with(&mut self, flat_text: impl Into<String>) {
        self.tokens.push(DocToken::Break(BreakToken {
            flat_text: flat_text.into(),
            break_text: String::new(),
            offset: 0,
        }));
    }

    /// Format a comma-separated list with proper breaks
    pub fn comma_list<T, F>(&mut self, items: impl IntoIterator<Item = T>, mut format_item: F)
    where
        F: FnMut(&mut Self, T),
    {
        let items: Vec<_> = items.into_iter().collect();
        let len = items.len();

        for (i, item) in items.into_iter().enumerate() {
            format_item(self, item);
            if i + 1 < len {
                self.text(",");
                self.space();
            }
        }
    }

    /// Consume the builder and return the finished document
    #[must_use]
    pub fn finish(self) -> Document {
        // Close any unclosed groups (shouldn't happen in correct usage)
        let mut tokens = self.tokens;
        for _ in 0..self.group_stack.len() {
            tokens.push(DocToken::End);
        }

        Document { tokens }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.text("hello");
        builder.space();
        builder.text("world");

        let doc = builder.finish();
        assert_eq!(doc.len(), 3);
    }

    #[test]
    fn test_groups() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.group(|b| {
            b.text("foo");
            b.space();
            b.text("bar");
        });

        let doc = builder.finish();
        // Begin + text + break + text + End = 5 tokens
        assert_eq!(doc.len(), 5);
    }

    #[test]
    fn test_indent() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.indent(|b| {
            b.text("indented");
        });

        let doc = builder.finish();
        assert_eq!(doc.len(), 3); // Begin + text + End
    }

    #[test]
    fn test_width_reservation() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.reserve_width(2);

        let doc = builder.finish();
        assert!(matches!(
            doc.tokens.as_slice(),
            [DocToken::WidthReservation(2)]
        ));
    }

    #[test]
    fn test_comma_list() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.comma_list(["a", "b", "c"], |b, item| {
            b.text(item);
        });

        let doc = builder.finish();
        // a + , + space + b + , + space + c = 7 tokens
        assert_eq!(doc.len(), 7);
    }
}
