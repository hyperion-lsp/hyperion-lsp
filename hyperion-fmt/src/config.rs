// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Formatter configuration following palantir-java-format defaults

/// Configuration options for the Java formatter
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // Config structs naturally have many boolean options
pub struct FormatConfig {
    /// Maximum line width (default: 120, matching palantir-java-format's
    /// PALANTIR style; google-java-format uses 100)
    pub max_line_width: usize,

    /// Indentation size in spaces (default: 4)
    pub indent_size: usize,

    /// Continuation indent for wrapped lines (default: 8, i.e., 2x indent_size)
    pub continuation_indent: usize,

    /// Maximum column for method chain last dot (default: 80)
    pub chain_wrap_column: usize,

    /// Whether to sort imports alphabetically within groups.
    ///
    /// Off by default: palantir-java-format's `formatSource` leaves import
    /// order untouched (import ordering is a separate opt-in pass there).
    pub sort_imports: bool,

    /// Whether to insert a final newline at end of file
    pub insert_final_newline: bool,

    /// Whether to use tabs instead of spaces for indentation (default: false)
    pub use_tabs: bool,

    /// Tab width when use_tabs is true (default: 4)
    pub tab_width: usize,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            max_line_width: 120,
            indent_size: 4,
            continuation_indent: 8,
            chain_wrap_column: 80,
            sort_imports: false,
            insert_final_newline: true,
            use_tabs: false,
            tab_width: 4,
        }
    }
}

impl FormatConfig {
    /// Create a new configuration with default palantir-java-format settings
    #[must_use]
    pub fn palantir() -> Self {
        Self::default()
    }

    /// Create a configuration matching google-java-format style
    #[must_use]
    pub const fn google() -> Self {
        Self {
            max_line_width: 100,
            indent_size: 2,
            continuation_indent: 4,
            chain_wrap_column: 80,
            sort_imports: false,
            insert_final_newline: true,
            use_tabs: false,
            tab_width: 2,
        }
    }

    /// Builder method to set max line width
    #[must_use]
    pub const fn with_max_line_width(mut self, width: usize) -> Self {
        self.max_line_width = width;
        self
    }

    /// Builder method to set indent size
    #[must_use]
    pub const fn with_indent_size(mut self, size: usize) -> Self {
        self.indent_size = size;
        self
    }

    /// Builder method to set continuation indent
    #[must_use]
    pub const fn with_continuation_indent(mut self, indent: usize) -> Self {
        self.continuation_indent = indent;
        self
    }

    /// Builder method to set chain wrap column
    #[must_use]
    pub const fn with_chain_wrap_column(mut self, column: usize) -> Self {
        self.chain_wrap_column = column;
        self
    }

    /// Builder method to enable/disable tabs
    #[must_use]
    pub const fn with_use_tabs(mut self, use_tabs: bool) -> Self {
        self.use_tabs = use_tabs;
        self
    }

    /// Builder method to set tab width
    #[must_use]
    pub const fn with_tab_width(mut self, width: usize) -> Self {
        self.tab_width = width;
        self
    }

    /// Get the indentation string (tabs or spaces)
    #[must_use]
    pub fn indent_string(&self) -> String {
        if self.use_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.indent_size)
        }
    }

    /// Get the continuation indentation string
    #[must_use]
    pub fn continuation_string(&self) -> String {
        if self.use_tabs {
            "\t".repeat(self.continuation_indent / self.tab_width)
        } else {
            " ".repeat(self.continuation_indent)
        }
    }
}
