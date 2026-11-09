// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Document IR types for Oppen-style pretty printing

/// A document is a sequence of tokens that represent formatted output
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub tokens: Vec<DocToken>,
}

impl Document {
    /// Create a new empty document
    #[must_use]
    pub const fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    /// Check if the document is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Get the number of tokens
    #[must_use]
    pub const fn len(&self) -> usize {
        self.tokens.len()
    }
}

/// Document IR token types (Oppen-style)
#[derive(Debug, Clone)]
pub enum DocToken {
    /// Non-whitespace content (text that must appear in output)
    Text(String),

    /// Potential line-break point
    Break(BreakToken),

    /// Start of a group (controls how breaks within are handled)
    Begin(BeginToken),

    /// End of a group
    End,

    /// Conditional content (different text based on whether group is broken)
    Conditional(ConditionalToken),

    /// Hard line break (always breaks, resets indentation)
    Hardline,

    /// Literal newline (preserves exact whitespace, used for blank lines)
    Literal(String),

    /// Width that participates in fit decisions without emitting text.
    ///
    /// This lets a nested group account for punctuation that is emitted by its
    /// parent, such as the semicolon following a method-call argument list.
    WidthReservation(usize),
}

/// A potential line break point
#[derive(Debug, Clone)]
pub struct BreakToken {
    /// Text to print if NOT breaking (usually " " for space)
    pub flat_text: String,

    /// Text to print if breaking (usually "" for nothing)
    pub break_text: String,

    /// Additional indentation offset when breaking (relative to current indent)
    pub offset: i32,
}

impl BreakToken {
    /// Create a space that can become a newline
    #[must_use]
    pub fn space() -> Self {
        Self {
            flat_text: " ".to_string(),
            break_text: String::new(),
            offset: 0,
        }
    }

    /// Create a break with no flat text (only breaks)
    #[must_use]
    pub const fn soft() -> Self {
        Self {
            flat_text: String::new(),
            break_text: String::new(),
            offset: 0,
        }
    }

    /// Create a break with custom offset
    #[must_use]
    pub const fn with_offset(mut self, offset: i32) -> Self {
        self.offset = offset;
        self
    }
}

/// Start of a group, controlling break behavior
#[derive(Debug, Clone)]
pub struct BeginToken {
    /// How breaks within this group should be handled
    pub break_type: BreakType,

    /// Indentation offset for content within this group
    pub offset: i32,

    /// If true, use `flat_structural` size instead of `flat` for break decisions.
    /// This allows groups containing internal hardlines (like lambda bodies) to
    /// stay on one line if they fit structurally, rather than always breaking.
    pub uses_structural_size: bool,

    /// Ambient offset applied to ALL breaks within this group (including nested groups)
    /// Unlike `offset`, this doesn't force breaks - it just adds indent when breaks happen.
    /// Used for assignment RHS continuation context where binary ops need +4 extra.
    pub ambient_offset: i32,
}

impl BeginToken {
    /// Create a consistent group (all-or-nothing breaks)
    #[must_use]
    pub const fn consistent(offset: i32) -> Self {
        Self {
            break_type: BreakType::Consistent,
            offset,
            uses_structural_size: false,
            ambient_offset: 0,
        }
    }

    /// Create an inconsistent group (greedy line filling)
    #[must_use]
    pub const fn inconsistent(offset: i32) -> Self {
        Self {
            break_type: BreakType::Inconsistent,
            offset,
            uses_structural_size: false,
            ambient_offset: 0,
        }
    }

    /// Create a consistent group that uses structural sizing
    /// (ignores internal hardlines when deciding whether to break)
    #[must_use]
    pub const fn consistent_structural(offset: i32) -> Self {
        Self {
            break_type: BreakType::Consistent,
            offset,
            uses_structural_size: true,
            ambient_offset: 0,
        }
    }

    /// Create an inconsistent group that uses structural sizing
    #[must_use]
    pub const fn inconsistent_structural(offset: i32) -> Self {
        Self {
            break_type: BreakType::Inconsistent,
            offset,
            uses_structural_size: true,
            ambient_offset: 0,
        }
    }

    /// Create a group with ambient offset (applies to all nested breaks)
    /// Used for assignment RHS where we want nested binary ops to get +4 indent
    #[must_use]
    pub const fn ambient(offset: i32, ambient_offset: i32) -> Self {
        Self {
            break_type: BreakType::Inconsistent,
            offset,
            uses_structural_size: false,
            ambient_offset,
        }
    }
}

/// Break type determines how breaks within a group are handled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakType {
    /// If the group doesn't fit on one line, break at ALL break points
    /// Used for things like parameter lists where partial breaking looks bad
    Consistent,

    /// Greedy line filling - only break when necessary
    /// Used for things like array initializers where partial breaking is fine
    Inconsistent,

    /// Special handling for method chains
    /// Breaks are aligned to the chain start, with special 80-char dot limit
    Chain,

    /// Builder pattern - always breaks with continuation indent (8 spaces)
    /// Used for Foo.builder().x().y().build() patterns
    Builder,
}

/// Conditional content based on whether the enclosing group is broken
#[derive(Debug, Clone)]
pub struct ConditionalToken {
    /// Text to print when the group is broken (spans multiple lines)
    pub broken_text: String,

    /// Text to print when the group fits on one line
    pub flat_text: String,
}

impl ConditionalToken {
    /// Create a trailing comma (appears only when broken)
    #[must_use]
    pub fn trailing_comma() -> Self {
        Self {
            broken_text: ",".to_string(),
            flat_text: String::new(),
        }
    }

    /// Create conditional content
    #[must_use]
    pub fn new(broken: impl Into<String>, flat: impl Into<String>) -> Self {
        Self {
            broken_text: broken.into(),
            flat_text: flat.into(),
        }
    }
}
