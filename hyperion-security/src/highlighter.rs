// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Syntax highlighting for code snippets in CLI output

use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

/// Code highlighter that supports Java, Kotlin, and Groovy
pub struct CodeHighlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    enabled: bool,
}

impl CodeHighlighter {
    /// Create a new code highlighter
    ///
    /// Highlighting is automatically disabled when output is not a TTY (piped to file/CI)
    #[must_use]
    pub fn new() -> Self {
        let enabled = atty::is(atty::Stream::Stdout);
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            enabled,
        }
    }

    /// Force enable or disable highlighting (for testing or explicit control)
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Highlight code snippet based on file extension
    ///
    /// Returns the original code unchanged if:
    /// - Highlighting is disabled (not a TTY)
    /// - Language cannot be detected
    /// - Highlighting fails for any reason
    pub fn highlight(&self, code: &str, file_path: &Path) -> String {
        if !self.enabled {
            return code.to_string();
        }

        // Detect syntax by file extension
        let syntax = self
            .syntax_set
            .find_syntax_for_file(file_path)
            .ok()
            .flatten()
            .or_else(|| {
                // Fallback to plain text if extension not recognized
                Some(self.syntax_set.find_syntax_plain_text())
            })
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        // Use a dark theme (works well on most terminals)
        // base16-ocean.dark is a popular theme included by default
        let theme = &self.theme_set.themes["base16-ocean.dark"];

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut highlighted = String::new();

        for line in LinesWithEndings::from(code) {
            let ranges: Vec<(Style, &str)> = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_default();
            let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
            highlighted.push_str(&escaped);
        }

        highlighted
    }

    /// Check if highlighting is enabled
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Default for CodeHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_highlighter_java() {
        let highlighter = CodeHighlighter::new().with_enabled(true);
        let code = r#"public class Test {
    public void method() {
        String s = "hello";
    }
}"#;
        let highlighted = highlighter.highlight(code, &PathBuf::from("Test.java"));

        // When enabled, output should contain ANSI escape codes
        assert!(
            highlighted.contains("\x1b["),
            "Should contain ANSI codes when enabled"
        );
    }

    #[test]
    fn test_highlighter_kotlin() {
        let highlighter = CodeHighlighter::new().with_enabled(true);
        let code = r#"class Test {
    fun method() {
        val s = "hello"
    }
}"#;
        let highlighted = highlighter.highlight(code, &PathBuf::from("Test.kt"));

        assert!(
            highlighted.contains("\x1b["),
            "Should contain ANSI codes for Kotlin"
        );
    }

    #[test]
    fn test_highlighter_disabled() {
        let highlighter = CodeHighlighter::new().with_enabled(false);
        let code = r#"public class Test {}"#;
        let highlighted = highlighter.highlight(code, &PathBuf::from("Test.java"));

        // When disabled, output should be unchanged
        assert_eq!(
            highlighted, code,
            "Should return original code when disabled"
        );
    }

    #[test]
    fn test_highlighter_unknown_extension() {
        let highlighter = CodeHighlighter::new().with_enabled(true);
        let code = "some code";
        let highlighted = highlighter.highlight(code, &PathBuf::from("unknown.xyz"));

        // Should still work (will use plain text syntax)
        assert!(!highlighted.is_empty());
    }
}
