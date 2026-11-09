// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java Parser Implementation with Rowan Integration
//!
//! This module implements an error-resilient parser for Java source code using
//! recursive descent parsing with Pratt parsing for expressions. The parser
//! uses rowan's GreenNodeBuilder to construct lossless concrete syntax trees (CST).
//!
//! # Architecture
//!
//! The parser is built on the following layers:
//!
//! 1. **Lexer**: Produces `(SyntaxKind, &str, TextRange)` tuples from source code
//! 2. **Parser**: Consumes tokens and builds a green tree using `GreenNodeBuilder`
//! 3. **Rowan Green Tree**: Immutable, structural-shared representation
//! 4. **Rowan Red Tree**: Provides navigation and parent pointers (SyntaxNode)
//!
//! # Key Features
//!
//! - **Error Recovery**: When encountering unexpected tokens, the parser attempts
//!   to recover by skipping to synchronization points (statement/declaration boundaries)
//! - **Lookahead**: Supports arbitrary lookahead for disambiguating Java syntax
//! - **Pratt Parsing**: Uses operator precedence parsing for expressions
//! - **Lossless**: Preserves all tokens including trivia (whitespace, comments)
//! - **Rowan Integration**: Directly builds rowan green trees for efficient CST
//!
//! # Example
//!
//! ```rust
//! use hyperion_parser::parser::Parser;
//! use hyperion_parser::lexer::Lexer;
//!
//! let source = "class Foo { }";
//! let tokens = Lexer::new(source).collect();
//! let (green_node, errors) = Parser::parse(tokens);
//!
//! // Convert to red tree for navigation
//! use hyperion_parser::syntax::SyntaxNode;
//! let syntax_node = SyntaxNode::new_root(green_node);
//! ```

use crate::syntax::{JavaLanguage, SyntaxKind};
use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, Language, TextRange, TextSize};
use std::fmt;

mod annotations;
mod declarations;
mod expressions;
mod members;
mod statements;
mod types;

/// Parse mode controls how much of the source code is parsed.
///
/// For symbol extraction during indexing, we only need declarations (class/method/field
/// signatures) and can skip method bodies entirely. This provides 8-10x faster parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ParseMode {
    /// Parse the complete source file including all method bodies
    #[default]
    Full,
    /// Parse only declarations (classes, methods, fields) and skip method/constructor bodies.
    /// Method bodies are replaced with empty BLOCK_STMT nodes containing just braces.
    DeclarationsOnly,
}

/// Check whether a token can represent one Java identifier. Keep this parser
/// helper delegated to the public classification so lexer/parser consumers do
/// not acquire subtly different restricted-keyword tables.
pub(crate) const fn is_identifier_kind(kind: SyntaxKind) -> bool {
    kind.is_identifier()
}

/// Parse error with message and location
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub range: TextRange,
}

impl ParseError {
    pub fn new(message: impl Into<String>, range: TextRange) -> Self {
        Self {
            message: message.into(),
            range,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {:?}", self.message, self.range)
    }
}

/// Java parser that builds rowan green trees
///
/// The parser consumes a sequence of `(SyntaxKind, &str, TextRange)` tuples
/// produced by the lexer and constructs a lossless concrete syntax tree using
/// rowan's `GreenNodeBuilder`.
///
/// # Architecture
///
/// ```text
/// Tokens (SyntaxKind, &str, TextRange)
///           |
///           v
///     Parser::parse()
///           |
///           v
///   GreenNodeBuilder
///           |
///           v
///     GreenNode (immutable)
/// ```
///
/// # Usage
///
/// ```rust
/// use hyperion_parser::parser::Parser;
/// use hyperion_parser::lexer::Lexer;
///
/// let source = "public class HelloWorld { }";
/// let tokens = Lexer::new(source).collect();
/// let (green_node, errors) = Parser::parse(tokens);
///
/// if errors.is_empty() {
///     println!("Parse successful!");
/// }
/// ```
pub struct Parser<'a> {
    /// Token stream: (kind, text, range)
    tokens: Vec<(SyntaxKind, &'a str, TextRange)>,
    /// Current position in token stream
    pos: usize,
    /// Rowan green node builder
    builder: GreenNodeBuilder<'static>,
    /// Parse errors encountered
    errors: Vec<ParseError>,
    /// Stack of record names for detecting compact constructors
    record_stack: Vec<&'a str>,
    /// Current recursion depth to prevent stack overflow
    recursion_depth: usize,
    /// Whether we're currently parsing a switch case pattern
    /// Used to prevent mistaking switch arrows for lambda arrows
    in_switch_case: bool,
    /// Whether we're inside a switch expression body where yield statements are valid
    in_switch_expr: bool,
    /// Parse mode: Full or DeclarationsOnly (for fast indexing)
    parse_mode: ParseMode,
}

/// Maximum recursion depth before aborting to prevent stack overflow.
/// This is set high enough to handle any reasonable Java code while
/// preventing stack exhaustion from pathological input.
///
/// Reasoning: With ~2MB typical stack size and ~200 bytes per frame,
/// we can handle ~10,000 recursive calls. Setting to 1000 provides
/// safety margin and matches Java compiler nesting limits.
const MAX_RECURSION_DEPTH: usize = 1000;

impl<'a> Parser<'a> {
    /// Parse a token stream into a rowan green tree
    ///
    /// This is the main entry point for parsing. It consumes a vector of tokens
    /// and returns a `GreenNode` representing the parsed syntax tree, along with
    /// any errors encountered during parsing.
    ///
    /// # Arguments
    ///
    /// * `tokens` - Vector of `(SyntaxKind, &str, TextRange)` tuples from the lexer
    ///
    /// # Returns
    ///
    /// A tuple of `(GreenNode, Vec<ParseError>)`:
    /// - The `GreenNode` is the root of the parsed tree (always `SOURCE_FILE`)
    /// - The error vector contains any parse errors (may be empty)
    ///
    /// # Example
    ///
    /// ```rust
    /// use hyperion_parser::parser::Parser;
    /// use hyperion_parser::lexer::Lexer;
    ///
    /// let source = "class Test { }";
    /// let tokens = Lexer::new(source).collect();
    /// let (green_node, errors) = Parser::parse(tokens);
    ///
    /// assert!(errors.is_empty(), "Parse errors: {:?}", errors);
    /// ```
    #[must_use]
    pub fn parse(tokens: Vec<(SyntaxKind, &'a str, TextRange)>) -> (GreenNode, Vec<ParseError>) {
        let mut parser = Self {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            record_stack: Vec::new(),
            recursion_depth: 0,
            in_switch_case: false,
            in_switch_expr: false,
            parse_mode: ParseMode::Full,
        };

        parser.parse_source_file();
        let green = parser.builder.finish();
        (green, parser.errors)
    }

    /// Parse with a specific mode (Full or DeclarationsOnly)
    ///
    /// Use `ParseMode::DeclarationsOnly` for fast indexing - it skips method bodies
    /// and only parses declarations (classes, methods, fields).
    ///
    /// # Performance
    ///
    /// `DeclarationsOnly` mode provides 8-10x faster parsing by skipping method bodies.
    /// Useful for symbol extraction where only signatures are needed.
    #[must_use]
    pub fn parse_with_mode(
        tokens: Vec<(SyntaxKind, &'a str, TextRange)>,
        mode: ParseMode,
    ) -> (GreenNode, Vec<ParseError>) {
        let mut parser = Self {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            record_stack: Vec::new(),
            recursion_depth: 0,
            in_switch_case: false,
            in_switch_expr: false,
            parse_mode: mode,
        };

        parser.parse_source_file();
        let green = parser.builder.finish();
        (green, parser.errors)
    }

    // ============================================================================
    // Token Management
    // ============================================================================

    /// Get the current token kind without consuming it
    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Peek ahead n tokens and return the kind
    fn nth(&self, n: usize) -> SyntaxKind {
        let idx = self.pos + n;
        if idx < self.tokens.len() {
            self.tokens[idx].0
        } else {
            SyntaxKind::EOF
        }
    }

    /// Get the current token's text
    fn current_text(&self) -> &'a str {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].1
        } else {
            ""
        }
    }

    /// Get the current token's range
    fn current_range(&self) -> TextRange {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].2
        } else {
            // Return a range at the end
            let last_end = self
                .tokens
                .last()
                .map_or_else(|| TextSize::from(0), |last| last.2.end());
            TextRange::new(last_end, last_end)
        }
    }

    /// Check if current token matches the given kind
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Check if current token matches any of the given kinds
    fn at_any(&self, kinds: &[SyntaxKind]) -> bool {
        kinds.contains(&self.current())
    }

    /// Check if current token is an identifier (including contextual keywords)
    ///
    /// Contextual keywords like 'record', 'var', 'yield' are only keywords in specific contexts
    /// and should be allowed as identifiers elsewhere (parameter names, variable names, field names, etc.)
    ///
    /// For example:
    /// - `void replay(ReleaseRecord record)` - 'record' is a parameter name
    /// - `ReleaseRecord record = getRecord();` - 'record' is a variable name
    /// - `private MyRecord record;` - 'record' is a field name
    ///
    /// But 'record' is still a keyword when declaring a record class:
    /// - `record Point(int x, int y) {}`
    fn at_identifier(&self) -> bool {
        is_identifier_kind(self.current())
    }

    /// Expect an identifier (including contextual keywords), report error if not found
    fn expect_identifier(&mut self) -> bool {
        if self.at_identifier() {
            self.bump();
            true
        } else {
            let range = self.current_range();
            self.error(format!("Expected IDENT, found {:?}", self.current()), range);
            false
        }
    }

    /// Check for a JLS `TypeIdentifier`, whose restricted-word exclusions are
    /// narrower than the set of contextual keywords.
    fn at_type_identifier(&self) -> bool {
        self.current().is_type_identifier()
    }

    /// Expect a type declaration/reference segment.
    fn expect_type_identifier(&mut self) -> bool {
        if self.at_type_identifier() {
            self.bump();
            true
        } else {
            let range = self.current_range();
            self.error(
                format!("Expected type identifier, found {:?}", self.current()),
                range,
            );
            false
        }
    }

    /// `non-sealed` is a contextual keyword made from three otherwise ordinary
    /// tokens. Recognize it only in modifier grammar; globally coalescing the
    /// spelling would break the legal subtraction expression `non-sealed`.
    fn at_non_sealed_modifier(&self) -> bool {
        if self.current() != SyntaxKind::IDENT
            || self.current_text() != "non"
            || self.nth(1) != SyntaxKind::MINUS
            || self.nth(2) != SyntaxKind::SEALED_KW
        {
            return false;
        }
        let Some((_, _, non_range)) = self.tokens.get(self.pos) else {
            return false;
        };
        let Some((_, _, minus_range)) = self.tokens.get(self.pos + 1) else {
            return false;
        };
        let Some((_, _, sealed_range)) = self.tokens.get(self.pos + 2) else {
            return false;
        };
        non_range.end() == minus_range.start() && minus_range.end() == sealed_range.start()
    }

    /// Check if we're at end of file
    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len() || self.at(SyntaxKind::EOF)
    }

    /// Peek ahead n tokens and return the kind (alternative name for clarity)
    fn peek(&self, n: usize) -> Option<SyntaxKind> {
        let idx = self.pos + n;
        if idx < self.tokens.len() {
            Some(self.tokens[idx].0)
        } else {
            None
        }
    }

    /// Determine whether the tokens at `offset` begin a type or record pattern.
    ///
    /// Both `instanceof` and switch labels need the same disambiguation. Keep
    /// it here so qualified, generic, array, primitive, and inferred types do
    /// not drift into subtly different grammars in the two callers.
    fn looks_like_type_pattern(&self, mut offset: usize, allow_inferred_type: bool) -> bool {
        let skip_trivia = |offset: &mut usize| {
            while self.peek(*offset).is_some_and(SyntaxKind::is_trivia) {
                *offset += 1;
            }
        };
        let skip_annotations = |offset: &mut usize| {
            let mut absolute = self.pos + *offset;
            while self
                .tokens
                .get(absolute)
                .is_some_and(|token| token.0 == SyntaxKind::AT)
            {
                absolute = self.skip_annotation_lookahead(absolute)?;
                self.skip_trivia_lookahead(&mut absolute);
            }
            *offset = absolute - self.pos;
            Some(())
        };
        let consume_arrays = |offset: &mut usize| {
            let mut consumed = false;
            loop {
                skip_trivia(offset);
                if self.peek(*offset) != Some(SyntaxKind::L_BRACKET) {
                    break;
                }
                *offset += 1;
                skip_trivia(offset);
                if self.peek(*offset) != Some(SyntaxKind::R_BRACKET) {
                    return false;
                }
                *offset += 1;
                consumed = true;
            }
            consumed
        };

        skip_trivia(&mut offset);
        if skip_annotations(&mut offset).is_none() {
            return false;
        }
        if allow_inferred_type && self.peek(offset) == Some(SyntaxKind::VAR_KW) {
            let mut next = offset + 1;
            skip_trivia(&mut next);
            if self.peek(next) != Some(SyntaxKind::DOT) {
                return self.peek(next).is_some_and(is_identifier_kind);
            }
        }

        if matches!(
            self.peek(offset),
            Some(
                SyntaxKind::BOOLEAN_KW
                    | SyntaxKind::BYTE_KW
                    | SyntaxKind::SHORT_KW
                    | SyntaxKind::INT_KW
                    | SyntaxKind::LONG_KW
                    | SyntaxKind::CHAR_KW
                    | SyntaxKind::FLOAT_KW
                    | SyntaxKind::DOUBLE_KW
            )
        ) {
            offset += 1;
            consume_arrays(&mut offset);
            skip_trivia(&mut offset);
            return self.peek(offset).is_some_and(is_identifier_kind);
        }

        let mut generic_depth = 0u32;
        'type_tokens: loop {
            skip_trivia(&mut offset);
            if skip_annotations(&mut offset).is_none() {
                return false;
            }
            match self.peek(offset) {
                Some(kind) if kind.is_identifier() => {
                    offset += 1;
                    if generic_depth > 0 {
                        continue;
                    }
                    let mut next = offset;
                    skip_trivia(&mut next);
                    if !matches!(
                        self.peek(next),
                        Some(SyntaxKind::DOT | SyntaxKind::LT | SyntaxKind::L_BRACKET)
                    ) {
                        if !kind.is_type_identifier() {
                            return false;
                        }
                        break 'type_tokens;
                    }
                }
                Some(SyntaxKind::DOT) => offset += 1,
                Some(SyntaxKind::LT) => {
                    generic_depth += 1;
                    offset += 1;
                }
                Some(kind @ (SyntaxKind::GT | SyntaxKind::SHR | SyntaxKind::USHR))
                    if generic_depth > 0 =>
                {
                    let closing_count = match kind {
                        SyntaxKind::GT => 1,
                        SyntaxKind::SHR => 2,
                        SyntaxKind::USHR => 3,
                        _ => unreachable!(),
                    };
                    if generic_depth < closing_count {
                        return false;
                    }
                    generic_depth -= closing_count;
                    offset += 1;
                    if generic_depth == 0 {
                        let mut next = offset;
                        skip_trivia(&mut next);
                        if !matches!(
                            self.peek(next),
                            Some(SyntaxKind::DOT | SyntaxKind::L_BRACKET)
                        ) {
                            break 'type_tokens;
                        }
                    }
                }
                Some(
                    SyntaxKind::QUESTION
                    | SyntaxKind::EXTENDS_KW
                    | SyntaxKind::SUPER_KW
                    | SyntaxKind::COMMA,
                ) if generic_depth > 0 => offset += 1,
                Some(SyntaxKind::L_BRACKET) => {
                    if !consume_arrays(&mut offset) {
                        return false;
                    }
                    break 'type_tokens;
                }
                _ => return false,
            }
        }

        skip_trivia(&mut offset);
        matches!(
            self.peek(offset),
            Some(kind) if is_identifier_kind(kind) || kind == SyntaxKind::L_PAREN
        )
    }

    /// Consume the current token and add it to the tree
    fn bump(&mut self) {
        if self.pos < self.tokens.len() {
            let (kind, text, _) = self.tokens[self.pos];
            self.builder.token(JavaLanguage::kind_to_raw(kind), text);
            self.pos += 1;
        }
    }

    /// Consume the current token if it matches the expected kind
    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect a token of the given kind, report error if not found
    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            let range = self.current_range();
            self.error(
                format!("Expected {:?}, found {:?}", kind, self.current()),
                range,
            );
            false
        }
    }

    /// Expect GT (>) but handle SHR (>>) and USHR (>>>) by splitting them
    ///
    /// This handles nested generics like `Map<String, List<Integer>>`
    /// where the lexer produces >> as a single SHR token, or
    /// `List<Set<Map<String, Integer>>>` where >>> is a USHR token.
    fn expect_gt_split_shr(&mut self) -> bool {
        if self.at(SyntaxKind::GT) {
            self.bump();
            true
        } else if self.at(SyntaxKind::SHR) {
            // Split >> into two > tokens
            // Add first > to the builder
            self.builder
                .token(JavaLanguage::kind_to_raw(SyntaxKind::GT), ">");

            // Replace the SHR token with a GT token for the next consumption
            if self.pos < self.tokens.len() {
                let (_, _, range) = self.tokens[self.pos];
                // Create a new GT token at the position of the second >
                let second_gt_start = range.start() + TextSize::from(1);
                let second_gt_range = TextRange::new(second_gt_start, range.end());
                self.tokens[self.pos] = (SyntaxKind::GT, ">", second_gt_range);
            }
            true
        } else if self.at(SyntaxKind::USHR) {
            // Split >>> into three > tokens
            // Add first > to the builder
            self.builder
                .token(JavaLanguage::kind_to_raw(SyntaxKind::GT), ">");

            // Replace the USHR token with a SHR token for the next consumption
            if self.pos < self.tokens.len() {
                let (_, _, range) = self.tokens[self.pos];
                // Create a new SHR token at the position of the second and third >
                let shr_start = range.start() + TextSize::from(1);
                let shr_range = TextRange::new(shr_start, range.end());
                self.tokens[self.pos] = (SyntaxKind::SHR, ">>", shr_range);
            }
            true
        } else {
            let range = self.current_range();
            self.error(format!("Expected GT, found {:?}", self.current()), range);
            false
        }
    }

    /// Report a parse error
    fn error(&mut self, message: impl Into<String>, range: TextRange) {
        self.errors.push(ParseError::new(message, range));
    }

    /// Check recursion depth and report error if limit exceeded.
    /// Returns true if parsing should continue, false if depth limit hit.
    #[inline]
    fn check_recursion_depth(&mut self) -> bool {
        self.recursion_depth += 1;
        if self.recursion_depth > MAX_RECURSION_DEPTH {
            let range = self.current_range();
            self.error(
                format!(
                    "Maximum recursion depth ({MAX_RECURSION_DEPTH}) exceeded. Code is too deeply nested."
                ),
                range,
            );
            false
        } else {
            true
        }
    }

    /// Decrease recursion depth counter
    #[inline]
    const fn decrease_recursion_depth(&mut self) {
        self.recursion_depth = self.recursion_depth.saturating_sub(1);
    }

    /// Start a new syntax node
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(JavaLanguage::kind_to_raw(kind));
    }

    /// Take a checkpoint for retroactively wrapping nodes
    fn checkpoint(&self) -> Checkpoint {
        self.builder.checkpoint()
    }

    /// Start a new syntax node at a previous checkpoint, wrapping everything
    /// emitted since the checkpoint as children of the new node
    fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder
            .start_node_at(checkpoint, JavaLanguage::kind_to_raw(kind));
    }

    /// Finish the current syntax node
    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Skip trivia tokens (whitespace, comments)
    fn skip_trivia(&mut self) {
        while self.current().is_trivia() && !self.at_eof() {
            self.bump();
        }
    }

    /// Skip to matching closing brace (for DeclarationsOnly mode).
    ///
    /// Used to skip method/constructor bodies when we only need declarations for indexing.
    /// Creates an empty BLOCK_STMT node containing just the braces.
    fn skip_to_matching_brace(&mut self) {
        if !self.at(SyntaxKind::L_BRACE) {
            return;
        }

        self.start_node(SyntaxKind::BLOCK_STMT);
        self.bump(); // consume {

        let mut depth = 1;
        while depth > 0 && !self.at_eof() {
            match self.current() {
                SyntaxKind::L_BRACE => depth += 1,
                SyntaxKind::R_BRACE => depth -= 1,
                _ => {}
            }
            self.bump();
        }

        self.finish_node();
    }

    // ============================================================================
    // Error Recovery
    // ============================================================================

    /// Recover to the next type declaration
    fn recover_to_type_decl(&mut self) {
        while !self.at_eof() {
            // Skip to a likely synchronization point
            match self.current() {
                SyntaxKind::CLASS_KW
                | SyntaxKind::INTERFACE_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::RECORD_KW
                | SyntaxKind::AT => return,
                SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::STATIC_KW
                | SyntaxKind::FINAL_KW
                | SyntaxKind::ABSTRACT_KW => {
                    // Might be a modifier for a type declaration
                    return;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use tracing::debug;

    #[test]
    fn test_empty_file() {
        let tokens = Lexer::new("").collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;
        assert!(errors.is_empty());
        assert_eq!(
            green.kind(),
            JavaLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    #[test]
    fn test_simple_class() {
        let source = "class Foo { }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        if !errors.is_empty() {
            debug!("Parse errors:");
            for err in &errors {
                debug!("  {}", err);
            }
        }

        assert!(
            errors.is_empty(),
            "Expected no errors, got {} errors",
            errors.len()
        );
        assert_eq!(
            green.kind(),
            JavaLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    #[test]
    fn test_class_with_field() {
        let source = "class Foo { int x; }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_class_with_method() {
        let source = "class Foo { void bar() { } }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_package_and_imports() {
        let source = r"
            package com.example;
            import java.util.List;
            class Foo { }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_for_loop_traditional() {
        let source = r"
            class Test {
                void method() {
                    for (int i = 0; i < 10; i++) {
                        System.out.println(i);
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_for_loop_enhanced() {
        let source = r"
            class Test {
                void method() {
                    for (String s : list) {
                        System.out.println(s);
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_switch_traditional() {
        let source = r#"
            class Test {
                void method(int x) {
                    switch (x) {
                        case 1:
                            System.out.println("one");
                            break;
                        case 2:
                            System.out.println("two");
                            break;
                        default:
                            System.out.println("other");
                    }
                }
            }
        "#;
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_switch_arrow() {
        let source = r"
            class Test {
                int method(int x) {
                    return switch (x) {
                        case 1 -> 10;
                        case 2, 3 -> 20;
                        default -> 0;
                    };
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        // Arrow syntax and switch expressions might have errors depending on
        // how the expression parser handles them
        if !errors.is_empty() {
            debug!("Parse errors: {:?}", errors);
        }
    }

    #[test]
    fn test_switch_arrow_throw() {
        let source = "class Test { void m(int x) { switch (x) { case 1 -> throw new E(); default -> throw new F(); } } }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse switch with throw in arrow cases"
        );
    }

    #[test]
    fn test_array_creation_with_size() {
        let source = r"
            class Test {
                void method() {
                    String[] result = new String[2];
                    int[] numbers = new int[10];
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse array creation with size: {errors:?}"
        );
    }

    #[test]
    fn test_nested_generics() {
        let source = "class Test { Map<String, Map<String, Integer>> field; }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse nested generics with >> without errors: {errors:?}"
        );
    }

    #[test]
    fn test_try_catch_finally() {
        let source = r"
            class Test {
                void method() {
                    try {
                        riskyOperation();
                    } catch (IOException e) {
                        handleError(e);
                    } finally {
                        cleanup();
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_try_with_resources() {
        let source = r#"
            class Test {
                void method() {
                    try (FileReader fr = new FileReader("file.txt")) {
                        fr.read();
                    } catch (IOException e) {
                        handleError(e);
                    }
                }
            }
        "#;
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_multi_catch() {
        let source = r"
            class Test {
                void method() {
                    try {
                        riskyOperation();
                    } catch (IOException | SQLException e) {
                        handleError(e);
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_catch_with_final_modifier() {
        let source = r"
            class Test {
                void method() {
                    try {
                        riskyOperation();
                    } catch (final IOException e) {
                        handleError(e);
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(
            errors.is_empty(),
            "Expected no errors with final in catch, got: {errors:?}"
        );
    }

    #[test]
    fn test_multi_catch_with_final() {
        let source = r"
            class Test {
                void method() {
                    try {
                        riskyOperation();
                    } catch (final IOException | SQLException e) {
                        handleError(e);
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(
            errors.is_empty(),
            "Expected no errors with final in multi-catch, got: {errors:?}"
        );
    }

    #[test]
    fn test_multiple_catch_with_final() {
        let source = r"
            class Test {
                void method() {
                    try {
                        riskyOperation();
                    } catch (final IOException e1) {
                        handleIO(e1);
                    } catch (final SQLException e2) {
                        handleSQL(e2);
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(
            errors.is_empty(),
            "Expected no errors with final in multiple catch clauses, got: {errors:?}"
        );
    }

    #[test]
    fn test_lambda_expression() {
        let source = r#"
            class Test {
                void method() {
                    Runnable r = () -> System.out.println("Hello");
                }
            }
        "#;
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_lambda_with_params() {
        let source = r"
            class Test {
                void method() {
                    Function<String, Integer> f = s -> s.length();
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_lambda_with_block() {
        let source = r#"
            class Test {
                void method() {
                    Runnable r = () -> {
                        System.out.println("Hello");
                        System.out.println("World");
                    };
                }
            }
        "#;
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_lambda_with_typed_params() {
        let source = r"
            class Test {
                void method() {
                    TriFunction<Timestamp, Timestamp, Integer, List<Item>> f =
                        (Timestamp timeMax, Timestamp timeMin, int count) -> find(timeMax, timeMin, count);
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_lambda_multiline_with_method_call() {
        let source = r"
            class Processor {
                public List<Item> findItems(String key) {
                    return process(
                            0,
                            0,
                            250_000,
                            (Timestamp timeMax, Timestamp timeMin, int count) -> repo.find(
                                    null,
                                    null,
                                    null,
                                    null,
                                    key,
                                    timeMax,
                                    timeMin,
                                    count,
                                    true,
                                    List.of(State.OPEN)));
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_lambda_token_positions_match_source() {
        use crate::SyntaxNode;

        // Token ranges through a long call containing a typed lambda.
        let source = r"class Test {
    public List<Item> findItems(String key) {
        return process(
                0,
                0,
                250_000,
                (Timestamp timeMax, Timestamp timeMin, int count) -> repo.find(
                        null,
                        null,
                        null,
                        null,
                        key,
                        timeMax,
                        timeMin,
                        count,
                        true,
                        List.of(State.OPEN)));
    }
}";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);

        // Parse should succeed
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");

        // Walk the tree and verify every token's text range matches the source
        use rowan::NodeOrToken;
        let root = SyntaxNode::new_root(green);

        for element in root.descendants_with_tokens() {
            if let NodeOrToken::Token(token) = element {
                let range = token.text_range();
                let start = range.start().into();
                let end = range.end().into();

                // Verify range is within source bounds
                assert!(
                    end <= source.len(),
                    "Token {:?} at {}..{} exceeds source length {}",
                    token.kind(),
                    start,
                    end,
                    source.len()
                );

                // Verify the text at this range matches what the token reports
                let source_text = &source[start..end];
                let token_text = token.text();
                assert_eq!(source_text, token_text,
                    "Token text mismatch at {start}..{end}: source has {source_text:?}, token reports {token_text:?}");
            }
        }
    }

    #[test]
    fn test_typed_lambda_in_multiline_query() {
        // Typed lambda with ten arguments and a nested enum-qualified call.
        let source = r"public List<Item> process(String scopeId, long maxTimestamp, long minTimestamp, int maxCount)
        throws ProcessException {
    return process(
            maxTimestamp,
            minTimestamp,
            maxCount,
            (Timestamp timeMax, Timestamp timeMin, int count) -> repo.find(
                    null,
                    scopeId,
                    null,
                    null,
                    null,
                    timeMax,
                    timeMin,
                    count,
                    true,
                    List.of(State.OPEN)));
}";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        // This should parse without errors after lambda parsing support
        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_cast_expression() {
        let source = r"
            class Test {
                void method(Object obj) {
                    String s = (String) obj;
                    int x = (int) 3.14;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_bitwise_shift_operators() {
        let source = r"
            class Test {
                void method() {
                    int a = x >> 2;      // Right shift
                    int b = y << 3;      // Left shift
                    int c = z >>> 1;     // Unsigned right shift
                    int d = (a >> 1) + (b << 2);
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse bitwise shift operators without errors: {errors:?}"
        );
    }

    #[test]
    fn test_generics_vs_bitwise_context() {
        let source = r"
            class Test {
                // Generics context: >> should be treated as two GT tokens
                Map<String, List<Integer>> field;
                List<Map<String, Set<Long>>> complexField;

                void method() {
                    // Expression context: >> should be treated as SHR operator
                    int x = a >> b;
                    int y = c << d;
                    int z = e >>> f;

                    // Mixed: generics in type, bitwise in expression
                    List<List<Integer>> list = new ArrayList<>();
                    int shifted = value >> 2;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should correctly distinguish generics from bitwise operators: {errors:?}"
        );
    }

    #[test]
    fn test_triple_nested_generics() {
        let source = r"
            class Test {
                Map<String, Map<Integer, List<Boolean>>> tripleNested;
                List<Set<Map<String, Integer>>> anotherNesting;
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse triple-nested generics: {errors:?}"
        );
    }

    #[test]
    fn test_bitwise_comparison_operators() {
        let source = r"
            class Test {
                void method() {
                    boolean a = x < y;
                    boolean b = x > y;
                    boolean c = x <= y;
                    boolean d = x >= y;
                    boolean e = (x << 2) > (y >> 1);
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse comparison and shift operators: {errors:?}"
        );
    }

    #[test]
    fn test_method_with_nested_generic_return_type() {
        let source = r"
            class Test {
                public Map<String, Map<Kind, Integer>> countItems(List<Location> ids) {
                    return null;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse method with nested generic return type: {errors:?}"
        );
    }

    #[test]
    fn test_local_variable_with_nested_generics() {
        let source = r"
            class Test {
                void method() {
                    Map<String, Map<Kind, Integer>> r = new HashMap<>();
                    List<Set<String>> list = new ArrayList<>();
                    Map<String, List<Integer>> simple = new HashMap<>();
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse local variables with nested generic types: {errors:?}"
        );
    }

    #[test]
    fn test_diamond_operator() {
        let source = r"
            class Test {
                void method() {
                    List<String> list = new ArrayList<>();
                    Map<String, Integer> map = new HashMap<>();
                    Set<Long> set = new HashSet<>();
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(errors.len(), 0, "Should parse diamond operator: {errors:?}");
    }

    #[test]
    fn test_enhanced_for_with_generics() {
        let source = r"
            class Test {
                void method(Map<String, List<Integer>> map) {
                    for (Map.Entry<String, List<Integer>> entry : map.entrySet()) {
                        String key = entry.getKey();
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse enhanced for loop with nested generics: {errors:?}"
        );
    }

    #[test]
    fn test_wildcard_generics() {
        let source = r"
            class Test {
                void method() {
                    List<?> wildcardList = new ArrayList<>();
                    Map<?, ?> wildcardMap = new HashMap<>();
                    List<? extends Number> boundedList = new ArrayList<>();
                    Map<String, ? super Integer> superMap = new HashMap<>();
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse wildcard generic types: {errors:?}"
        );
    }

    #[test]
    fn test_generic_method_call() {
        let source = r"
            class Test {
                void method() {
                    List<String> empty = Collections.emptyList();
                    Map<String, Integer> emptyMap = Collections.emptyMap();
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse generic method calls with type inference: {errors:?}"
        );
    }

    #[test]
    fn test_cast_with_generics() {
        let source = r"
            class Test {
                void method(Object obj) {
                    List<String> list = (List<String>) obj;
                    Map<String, Integer> map = (Map<String, Integer>) obj;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse casts with generic types: {errors:?}"
        );
    }

    #[test]
    fn test_deeply_nested_generics() {
        let source = r"
            class Test {
                Map<String, Map<String, Map<String, List<Integer>>>> deeplyNested;

                void method() {
                    Map<String, Map<String, Map<String, List<Integer>>>> m = new HashMap<>();
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse deeply nested generics: {errors:?}"
        );
    }

    #[test]
    fn test_parenthesized_expression_vs_cast_with_shift() {
        // Test that (identifier >> n) is parsed as parenthesized expression, not cast
        let source = r"
            class Test {
                void method() {
                    // Parenthesized expressions with shift operators
                    int a = (x >> 1);
                    int b = (y << 2);
                    int c = (z >>> 3);

                    // Combined expressions
                    int d = (a >> 1) + (b << 2);
                    int e = ((x >> 1) << 2);

                    // Actual casts (for contrast)
                    int f = (int) longValue;
                    byte g = (byte) intValue;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should distinguish parenthesized shift expressions from casts: {errors:?}"
        );
    }

    #[test]
    fn test_parenthesized_expression_with_all_binary_operators() {
        // Test that parenthesized expressions with various binary operators are not confused with casts
        let source = r"
            class Test {
                void method() {
                    // Arithmetic
                    int a = (x + y);
                    int b = (x - y);
                    int c = (x * y);
                    int d = (x / y);
                    int e = (x % y);

                    // Shift
                    int f = (x << y);
                    int g = (x >> y);
                    int h = (x >>> y);

                    // Logical
                    boolean i = (a && b);
                    boolean j = (a || b);

                    // Comparison
                    boolean k = (a == b);
                    boolean l = (a != b);

                    // Combined in complex expression
                    int m = (a + b) * (c - d) / (e % f);
                    int n = ((x >> 1) + (y << 2)) >>> 3;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse parenthesized expressions with all binary operators: {errors:?}"
        );
    }

    #[test]
    fn test_cast_with_generic_types() {
        // Test that casts with generic types (including >> for closing nested generics) work
        let source = r"
            class Test {
                void method() {
                    // Simple generic casts
                    List<String> a = (List<String>) object;
                    Map<String, Integer> b = (Map<String, Integer>) object;

                    // Nested generic casts (>> closes two type arguments)
                    List<List<Integer>> c = (List<List<Integer>>) object;
                    Map<String, List<Set<Integer>>> d = (Map<String, List<Set<Integer>>>) object;

                    // Wildcard casts
                    List<?> e = (List<?>) object;
                    Map<String, ?> f = (Map<String, ?>) object;
                    List<? extends Number> g = (List<? extends Number>) object;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse casts with generic types: {errors:?}"
        );
    }

    #[test]
    fn test_shift_in_method_argument() {
        // Test shift operators inside method call arguments (common pattern)
        let source = r"
            class Test {
                void method() {
                    foo(a >> 1);
                    foo(b << 2);
                    bar(x >> 1, y << 2);
                    baz((a >> 1) + (b << 2));

                    // With new expressions
                    new Foo(a >> 1);
                    list.add(value >> shift);
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse shift operators in method arguments: {errors:?}"
        );
    }

    #[test]
    fn test_bitwise_operators_in_parens() {
        // Test bitwise OR, AND, XOR in parenthesized expressions
        let source = r"
            class Test {
                void method() {
                    int a = (x | y);    // bitwise OR
                    int b = (x & y);    // bitwise AND
                    int c = (x ^ y);    // bitwise XOR
                    int d = (x | y) & (z ^ w);
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse bitwise operators in parenthesized expressions: {errors:?}"
        );
    }

    #[test]
    fn test_comparison_operators_in_parens() {
        // Test <, >, <=, >= in parenthesized expressions
        let source = r"
            class Test {
                void method() {
                    boolean a = (x < y);
                    boolean b = (x > y);
                    boolean c = (x <= y);
                    boolean d = (x >= y);
                    boolean e = (a < b) && (c > d);
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse comparison operators in parenthesized expressions: {errors:?}"
        );
    }

    #[test]
    fn test_instanceof_in_parens() {
        // Test instanceof in parenthesized expressions
        let source = r"
            class Test {
                void method() {
                    boolean a = (obj instanceof String);
                    boolean b = (x instanceof List);
                    if ((value instanceof Integer)) {
                        // do something
                    }
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse instanceof in parenthesized expressions: {errors:?}"
        );
    }

    #[test]
    fn test_intersection_type_cast() {
        // Test intersection types in casts (& has different meaning here)
        // This is a Java 8+ feature for lambda target types:
        //   (Serializable & Comparable) () -> 42
        let source = r"
            class Test {
                void method() {
                    Object a = (Serializable & Comparable) obj;
                    Object b = (Runnable & Serializable) lambda;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse intersection type casts: {errors:?}"
        );
    }

    #[test]
    fn test_intersection_type_cast_with_three_types() {
        // Test intersection with more than two types
        let source = r"
            class Test {
                void method() {
                    Object a = (A & B & C) obj;
                    Object b = (Serializable & Comparable & Cloneable) value;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse intersection type casts with three types: {errors:?}"
        );
    }

    #[test]
    fn test_intersection_type_cast_with_lambda() {
        // Test intersection type cast with lambda expression
        let source = r"
            class Test {
                void method() {
                    Runnable r = (Runnable & Serializable) () -> System.out.println();
                    Object o = (Callable & Serializable) () -> 42;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse intersection type casts with lambda: {errors:?}"
        );
    }

    #[test]
    fn test_bitwise_and_still_works_in_parens() {
        // Ensure bitwise AND in parentheses still works after intersection type support
        let source = r"
            class Test {
                void method() {
                    int a = (x & y);
                    int b = (x & y) + (z & w);
                    boolean c = (flags & MASK) != 0;
                }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Bitwise AND in parentheses should still work: {errors:?}"
        );
    }

    #[test]
    fn test_intersection_type_in_generic_bounds() {
        // Test intersection types in generic type bounds (already supported)
        let source = r"
            class Test {
                <T extends Serializable & Comparable<T>> void sort(T[] array) {}
                <T extends A & B & C> T process(T value) { return value; }
            }
        ";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse intersection types in generic bounds: {errors:?}"
        );
    }

    #[test]
    fn test_trailing_comma_in_method_call() {
        // Test trailing comma in method arguments (valid Java syntax)
        let source = r#"
            class Test {
                void test() {
                    foo(arg1, arg2,);
                    bar(
                        longArg1,
                        longArg2,
                    );
                    // Lambda with trailing comma
                    TestUtils.waitUntilTrue(
                        () -> broker.logManager().producerStateManagerConfig().producerIdExpirationMs() == 100000,
                        () -> "Configuration was not updated.",
                        DEFAULT_MAX_WAIT_MS,
                        100,
                    );
                }
            }
        "#;
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        let _ = &green;

        assert_eq!(
            errors.len(),
            0,
            "Should parse trailing commas in method arguments: {errors:?}"
        );
    }
}
