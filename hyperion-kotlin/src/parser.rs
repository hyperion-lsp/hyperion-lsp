// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lossless Kotlin parser backed by Rowan.
//!
//! The recursive-descent parser consumes lexer tokens and uses Rowan's
//! `GreenNodeBuilder` to construct a concrete syntax tree (CST).
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
//!   to recover by skipping to synchronization points
//! - **Lossless**: Preserves all tokens including trivia (whitespace, comments)
//! - **Rowan Integration**: Directly builds rowan green trees for efficient CST
//!
//! The checked-in corpus exercises declarations, control flow, expressions,
//! null-safety syntax, lambdas, extensions, coroutines and modern class forms.
//! Passing that corpus does not imply full Kotlin compiler compatibility;
//! unsupported or malformed input is represented through parse errors while
//! retaining all source text for editor recovery.

use crate::syntax::{KotlinLanguage, SyntaxKind};
use rowan::{GreenNode, GreenNodeBuilder, Language, TextRange};
use std::fmt;

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

/// Kotlin parser that builds rowan green trees
///
/// The parser consumes a sequence of `(SyntaxKind, &str, TextRange)` tuples
/// produced by the lexer and constructs a lossless concrete syntax tree using
/// rowan's GreenNodeBuilder.
pub struct Parser<'a> {
    /// Token stream: (kind, text, range)
    tokens: Vec<(SyntaxKind, &'a str, TextRange)>,
    /// Current position in token stream
    pos: usize,
    /// Rowan green node builder
    builder: GreenNodeBuilder<'static>,
    /// Parse errors encountered
    errors: Vec<ParseError>,
    /// Nesting depth while parsing a non-block `when` entry result.
    when_entry_result_depth: usize,
    /// Whether trivia consumed since the last non-trivia token contained a
    /// line break. Nested parsers may consume that trivia before their caller
    /// decides whether a following identifier can start a named infix call.
    newline_since_last_significant_token: bool,
    /// Nesting depth of `(...)` / `[...]` in the expression being parsed. Newlines
    /// inside them are insignificant, so `x\n or y` continues the expression.
    paren_depth: u32,
    /// Set when the modifiers just parsed contained `enum`; consumed by the
    /// class declaration they precede.
    pending_enum: bool,
}

impl<'a> Parser<'a> {
    /// Parse a token stream into a rowan green tree
    ///
    /// This is the main entry point for parsing. It consumes a vector of tokens
    /// and returns a GreenNode representing the parsed syntax tree, along with
    /// any errors encountered during parsing.
    ///
    /// # Arguments
    ///
    /// * `tokens` - Vector of `(SyntaxKind, &str, TextRange)` tuples from the lexer
    ///
    /// # Returns
    ///
    /// A tuple of `(GreenNode, Vec<ParseError>)`:
    /// - The GreenNode is the root of the parsed tree (always SOURCE_FILE)
    /// - The error vector contains any parse errors (may be empty)
    pub fn parse(tokens: Vec<(SyntaxKind, &'a str, TextRange)>) -> (GreenNode, Vec<ParseError>) {
        let mut parser = Self {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            when_entry_result_depth: 0,
            newline_since_last_significant_token: false,
            paren_depth: 0,
            pending_enum: false,
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
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].0
        } else {
            SyntaxKind::EOF
        }
    }

    /// Check if current token matches the given kind
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Check if we're at end of file
    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len() || self.at(SyntaxKind::EOF)
    }

    /// Return the next non-trivia token at or after `pos`, without consuming
    /// it. Declaration prefixes need this for `fun interface`, where `fun`
    /// otherwise looks like the start of an ordinary function declaration.
    fn next_non_trivia_kind(&self, mut pos: usize) -> SyntaxKind {
        while let Some((kind, _, _)) = self.tokens.get(pos) {
            if !kind.is_trivia() {
                return *kind;
            }
            pos += 1;
        }
        SyntaxKind::EOF
    }

    /// Whether the trivia at the current position contains a line break and
    /// is followed by the supplied kind. Used to delimit bare `when` results:
    /// the next line's `is Type ->` starts a new entry, not an infix `is` on
    /// the preceding result expression.
    fn trivia_has_newline_before(&self, wanted: SyntaxKind) -> bool {
        let mut pos = self.pos;
        let mut has_newline = false;
        while let Some((kind, text, _)) = self.tokens.get(pos) {
            if !kind.is_trivia() {
                return has_newline && *kind == wanted;
            }
            has_newline |= text.contains('\n') || text.contains('\r');
            pos += 1;
        }
        false
    }

    /// Whether the trivia at the current position contains a line break.
    /// Kotlin does not permit a named infix call to begin on the next line:
    /// `a\nplus b` is two statements, not `a plus b`.
    fn current_trivia_has_newline(&self) -> bool {
        let mut pos = self.pos;
        let mut has_newline = false;
        while let Some((kind, text, _)) = self.tokens.get(pos) {
            if !kind.is_trivia() {
                return has_newline;
            }
            has_newline |= text.contains('\n') || text.contains('\r');
            pos += 1;
        }
        has_newline
    }

    /// Whether the current `<` starts a balanced type-argument list followed
    /// immediately by call arguments.
    ///
    /// In expression position `<` is normally a comparison operator, but
    /// `Pair<String, Int>(...)` is unambiguously a generic call once the
    /// matching `>` is followed by `(`.
    fn at_generic_call_suffix(&self, allow_trailing_lambda: bool) -> Option<SyntaxKind> {
        if !self.at(SyntaxKind::LT) {
            return None;
        }

        // Everything between `<` and its matching `>` must be able to occur in
        // a type-argument list; `a < b) { ... x > (y)` is a comparison whose
        // "arguments" run into unrelated code, and must not be read as a call.
        let mut angle = 0usize;
        let mut paren = 0usize;
        let mut index = self.pos;
        while index < self.tokens.len() {
            match self.tokens[index].0 {
                SyntaxKind::LT => angle += 1,
                SyntaxKind::GT => {
                    if angle == 0 {
                        return None;
                    }
                    angle -= 1;
                    if angle == 0 {
                        index += 1;
                        while index < self.tokens.len()
                            && self.tokens[index].0.is_trivia()
                            && !self.tokens[index].1.contains('\n')
                        {
                            index += 1;
                        }
                        return match self.tokens.get(index).map(|token| token.0) {
                            Some(SyntaxKind::L_PAREN) => Some(SyntaxKind::L_PAREN),
                            // `assertFailsWith<E> { ... }`: type arguments straight
                            // into a trailing lambda.
                            Some(SyntaxKind::L_BRACE) if allow_trailing_lambda => {
                                Some(SyntaxKind::L_BRACE)
                            }
                            // `Array<String>::class`
                            Some(SyntaxKind::COLONCOLON) => Some(SyntaxKind::COLONCOLON),
                            _ => None,
                        };
                    }
                }
                SyntaxKind::L_PAREN => paren += 1,
                SyntaxKind::R_PAREN => {
                    if paren == 0 {
                        return None;
                    }
                    paren -= 1;
                }
                SyntaxKind::IDENT
                | SyntaxKind::BACKTICK_IDENT
                | SyntaxKind::DOT
                | SyntaxKind::COMMA
                | SyntaxKind::QUESTION
                | SyntaxKind::STAR
                | SyntaxKind::COLONCOLON
                | SyntaxKind::COLON
                | SyntaxKind::ARROW
                | SyntaxKind::AT
                | SyntaxKind::IN_KW
                | SyntaxKind::OUT_KW => {}
                kind if kind.is_trivia() => {}
                kind if self.token_is_soft_keyword(kind) => {}
                _ => return None,
            }
            index += 1;
        }

        None
    }

    /// Parse an extension receiver type up to and including its separating dot.
    /// `separator` is the token index of that dot (see `receiver_separator_index`).
    fn parse_receiver_type(&mut self, separator: usize) {
        self.start_node(SyntaxKind::TYPE_REF);

        // Qualified receiver: `Outer.Builder`, `Map.Entry<K, V>`
        loop {
            if self.at(SyntaxKind::BACKTICK_IDENT) {
                self.bump();
            } else {
                self.expect_identifier_or_soft_keyword();
            }
            self.skip_trivia();

            if self.at(SyntaxKind::LT) {
                self.parse_type_arguments();
                self.skip_trivia();
            }

            if self.pos < separator && self.at(SyntaxKind::DOT) {
                self.bump();
                self.skip_trivia();
                continue;
            }
            break;
        }

        // Logos tokenizes `?.` as one SAFE_CALL token. In an extension
        // declaration it instead means nullable receiver `?` followed by
        // the receiver/member separator `.`, so consume that combined
        // token exactly once and do not require a second DOT below.
        let combined_nullable_receiver_separator = self.at(SyntaxKind::SAFE_CALL);
        if combined_nullable_receiver_separator {
            self.bump();
            self.skip_trivia();
        } else if self.at(SyntaxKind::QUESTION) {
            self.bump();
            self.skip_trivia();
        }

        self.finish_node();

        if !combined_nullable_receiver_separator {
            self.expect(SyntaxKind::DOT);
            self.skip_trivia();
        }
    }

    /// For an extension property (`val List<T>.last: T`), the token index of the
    /// dot that ends the receiver type, if the declaration has one.
    fn property_receiver_separator_index(&self) -> Option<usize> {
        let mut i = self.pos;
        let mut angle: i32 = 0;
        let mut last_dot = None;
        while i < self.tokens.len() {
            let (kind, text, _) = &self.tokens[i];
            match *kind {
                SyntaxKind::LT => angle += 1,
                SyntaxKind::GT => {
                    angle -= 1;
                    if angle < 0 {
                        return None;
                    }
                }
                SyntaxKind::DOT | SyntaxKind::SAFE_CALL if angle == 0 => last_dot = Some(i),
                SyntaxKind::COMMA | SyntaxKind::STAR | SyntaxKind::AT | SyntaxKind::COLON
                    if angle > 0 => {}
                SyntaxKind::WHITESPACE if text.contains('\n') && angle == 0 => return last_dot,
                SyntaxKind::WHITESPACE
                | SyntaxKind::LINE_COMMENT
                | SyntaxKind::BLOCK_COMMENT
                | SyntaxKind::IDENT
                | SyntaxKind::BACKTICK_IDENT
                | SyntaxKind::QUESTION => {}
                k if self.token_is_soft_keyword(k) && k != SyntaxKind::BY_KW => {}
                _ => return last_dot,
            }
            i += 1;
        }
        last_dot
    }

    /// For an extension function declaration, the token index of the `.` (or
    /// `?.`) separating the receiver type from the function name.
    ///
    /// Scans a type-shaped prefix: identifiers, dots, balanced `<...>` type
    /// arguments (commas, `*`, `?`, variance, annotations and nested function
    /// types inside), and stops at the `(` of the parameter list. The receiver
    /// ends at the last top-level dot.
    fn receiver_separator_index(&self) -> Option<usize> {
        let mut i = self.pos;
        let mut angle: i32 = 0;
        let mut paren: i32 = 0;
        let mut last_dot = None;

        while i < self.tokens.len() {
            let kind = self.tokens[i].0;
            match kind {
                SyntaxKind::LT => angle += 1,
                SyntaxKind::GT => {
                    angle -= 1;
                    if angle < 0 {
                        return None;
                    }
                }
                SyntaxKind::L_PAREN if angle > 0 => paren += 1,
                SyntaxKind::R_PAREN if angle > 0 => paren -= 1,
                SyntaxKind::L_PAREN if paren == 0 => return last_dot,
                SyntaxKind::DOT | SyntaxKind::SAFE_CALL if angle == 0 => last_dot = Some(i),
                SyntaxKind::DOT | SyntaxKind::COMMA | SyntaxKind::COLON | SyntaxKind::AT
                    if angle > 0 => {}
                SyntaxKind::ARROW | SyntaxKind::STAR | SyntaxKind::IN_KW | SyntaxKind::OUT_KW
                    if angle > 0 => {}
                SyntaxKind::WHITESPACE
                | SyntaxKind::LINE_COMMENT
                | SyntaxKind::BLOCK_COMMENT
                | SyntaxKind::IDENT
                | SyntaxKind::BACKTICK_IDENT
                | SyntaxKind::QUESTION => {}
                k if angle == 0 && paren == 0 && self.token_is_soft_keyword(k) => {}
                k if angle > 0 && self.token_is_soft_keyword(k) => {}
                _ => return None,
            }
            i += 1;
        }

        None
    }

    /// After `is`/`!is`: whether the type carries type arguments or a
    /// nullable marker (`List<*>`, `Foo?`) and so needs real type parsing.
    fn type_operand_needs_type_ref(&self) -> bool {
        let mut i = self.pos;
        while self.tokens.get(i).is_some_and(|t| t.0.is_trivia()) {
            i += 1;
        }
        // (annotation-free) qualified name
        loop {
            match self.tokens.get(i).map(|t| t.0) {
                Some(kind) if kind == SyntaxKind::IDENT || self.token_is_soft_keyword(kind) => {
                    i += 1;
                }
                _ => return false,
            }
            match self.tokens.get(i).map(|t| t.0) {
                Some(SyntaxKind::DOT) => i += 1,
                Some(SyntaxKind::LT | SyntaxKind::QUESTION) => return true,
                _ => return false,
            }
        }
    }

    const fn token_is_soft_keyword(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::DATA_KW
                | SyntaxKind::SEALED_KW
                | SyntaxKind::OUT_KW
                | SyntaxKind::VARARG_KW
                | SyntaxKind::INLINE_KW
                | SyntaxKind::OPEN_KW
                | SyntaxKind::BY_KW
                | SyntaxKind::WHERE_KW
                | SyntaxKind::INIT_KW
                | SyntaxKind::ACTUAL_KW
                | SyntaxKind::EXPECT_KW
                | SyntaxKind::COMPANION_KW
                | SyntaxKind::SUSPEND_KW
                | SyntaxKind::OPERATOR_KW
                | SyntaxKind::INFIX_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::ANNOTATION_KW
        )
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
                .map_or_else(|| rowan::TextSize::from(0), |last| last.2.end());
            TextRange::new(last_end, last_end)
        }
    }

    /// Get the current token's text
    fn current_text(&self) -> &str {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].1
        } else {
            ""
        }
    }

    /// Consume the current token and add it to the tree
    fn bump(&mut self) {
        if self.pos < self.tokens.len() {
            let (kind, text, _) = self.tokens[self.pos];
            self.builder.token(KotlinLanguage::kind_to_raw(kind), text);
            if kind.is_trivia() {
                self.newline_since_last_significant_token |=
                    text.contains('\n') || text.contains('\r');
            } else {
                self.newline_since_last_significant_token = false;
            }
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

    /// Report a parse error
    fn error(&mut self, message: impl Into<String>, range: TextRange) {
        self.errors.push(ParseError::new(message, range));
    }

    /// Start a new syntax node
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(KotlinLanguage::kind_to_raw(kind));
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

    /// Defensive progress guard for parser loops.
    ///
    /// Every parser loop that delegates to a sub-parser can, in principle, spin
    /// forever: if the sub-parser bails out without consuming a token (because
    /// the current token is neither handled nor a recognised synchronisation
    /// point) the loop re-enters with an identical parser state. That is a
    /// *class* of bug, not a one-off, so every such loop calls this guard once
    /// per iteration with the position it started at.
    ///
    /// Returns `true` when the iteration made no progress. In that case an
    /// error is recorded and — unless we are at EOF — exactly one token is
    /// consumed inside an `ERROR` node, which both guarantees termination and
    /// keeps the tree lossless. At EOF nothing is consumed; callers are
    /// responsible for breaking out (loops conditioned on `!self.at_eof()` do
    /// so automatically).
    fn stalled(&mut self, start_pos: usize, context: &str) -> bool {
        if self.pos > start_pos {
            return false;
        }

        let range = self.current_range();
        self.error(
            format!(
                "Unexpected {:?} while parsing {context}; skipping token",
                self.current()
            ),
            range,
        );

        if !self.at_eof() {
            self.start_node(SyntaxKind::ERROR);
            self.bump();
            self.finish_node();
        }

        true
    }

    /// Consume balanced parentheses including their contents
    /// Used for constructor calls in supertype lists, etc.
    fn consume_balanced_parens(&mut self) {
        if !self.at(SyntaxKind::L_PAREN) {
            return;
        }
        self.bump(); // consume L_PAREN
        let mut depth = 1;
        while depth > 0 && !self.at_eof() {
            match self.current() {
                SyntaxKind::L_PAREN => {
                    depth += 1;
                    self.bump();
                }
                SyntaxKind::R_PAREN => {
                    depth -= 1;
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    /// Check if current token is a keyword that can be used as an identifier
    fn at_keyword(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::COMPANION_KW
                | SyntaxKind::OBJECT_KW
                | SyntaxKind::CLASS_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::ANNOTATION_KW
                | SyntaxKind::INTERFACE_KW
                | SyntaxKind::FUN_KW
                | SyntaxKind::VAL_KW
                | SyntaxKind::VAR_KW
                | SyntaxKind::PACKAGE_KW
                | SyntaxKind::IMPORT_KW
                | SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::INTERNAL_KW
                | SyntaxKind::OPEN_KW
                | SyntaxKind::ABSTRACT_KW
                | SyntaxKind::OVERRIDE_KW
                | SyntaxKind::DATA_KW
                | SyntaxKind::SEALED_KW
                | SyntaxKind::SUSPEND_KW
                | SyntaxKind::INLINE_KW
                | SyntaxKind::OPERATOR_KW
                | SyntaxKind::INFIX_KW
                | SyntaxKind::IF_KW
                | SyntaxKind::ELSE_KW
                | SyntaxKind::WHEN_KW
                | SyntaxKind::FOR_KW
                | SyntaxKind::WHILE_KW
                | SyntaxKind::RETURN_KW
                | SyntaxKind::BREAK_KW
                | SyntaxKind::CONTINUE_KW
                | SyntaxKind::TRY_KW
                | SyntaxKind::CATCH_KW
                | SyntaxKind::FINALLY_KW
                | SyntaxKind::THROW_KW
                | SyntaxKind::AS_KW
                | SyntaxKind::IS_KW
                | SyntaxKind::IN_KW
                | SyntaxKind::OUT_KW
                | SyntaxKind::THIS_KW
                | SyntaxKind::SUPER_KW
                | SyntaxKind::TYPEALIAS_KW
                | SyntaxKind::CONSTRUCTOR_KW
                | SyntaxKind::INIT_KW
                | SyntaxKind::BY_KW
                | SyntaxKind::LATEINIT_KW
                | SyntaxKind::EXPECT_KW
                | SyntaxKind::ACTUAL_KW
                | SyntaxKind::TRUE_KW
                | SyntaxKind::FALSE_KW
                | SyntaxKind::NULL_KW
                | SyntaxKind::REIFIED_KW
        )
    }

    /// Check if we're at the start of a property accessor (get or set)
    ///
    /// Property accessors are contextual - "get" and "set" are identifiers, not keywords.
    /// We detect them by checking if the current token is an IDENT with text "get" or "set",
    /// or if we're at a visibility modifier (private/protected/etc.) followed by get/set.
    /// Token index after any annotations starting at `index`
    /// (`@Foo`, `@field:Foo("x")`, `@a.b.C(...)`), skipping trivia between them.
    fn skip_annotations_lookahead(&self, mut index: usize) -> usize {
        loop {
            while self.tokens.get(index).is_some_and(|t| t.0.is_trivia()) {
                index += 1;
            }
            if self.tokens.get(index).map(|t| t.0) != Some(SyntaxKind::AT) {
                return index;
            }
            index += 1;
            // use-site target and (qualified) name
            while matches!(
                self.tokens.get(index).map(|t| t.0),
                Some(SyntaxKind::IDENT | SyntaxKind::DOT | SyntaxKind::COLON)
            ) {
                index += 1;
            }
            // arguments directly after the name
            if self.tokens.get(index).map(|t| t.0) == Some(SyntaxKind::L_PAREN) {
                let mut depth = 0usize;
                while let Some((kind, _, _)) = self.tokens.get(index) {
                    match kind {
                        SyntaxKind::L_PAREN => depth += 1,
                        SyntaxKind::R_PAREN => {
                            depth -= 1;
                            if depth == 0 {
                                index += 1;
                                break;
                            }
                        }
                        SyntaxKind::EOF => return index,
                        _ => {}
                    }
                    index += 1;
                }
            }
        }
    }

    fn at_property_accessor(&self) -> bool {
        // Direct check: current token is "get" or "set"
        if self.at(SyntaxKind::IDENT) && matches!(self.current_text(), "get" | "set") {
            return true;
        }

        // Annotated accessor: `@SuppressLint("x") get() = ...`
        if self.at(SyntaxKind::AT) {
            let after = self.skip_annotations_lookahead(self.pos);
            let mut probe = after;
            while matches!(
                self.tokens.get(probe).map(|t| t.0),
                Some(
                    SyntaxKind::PRIVATE_KW
                        | SyntaxKind::PROTECTED_KW
                        | SyntaxKind::INTERNAL_KW
                        | SyntaxKind::PUBLIC_KW
                        | SyntaxKind::INLINE_KW
                        | SyntaxKind::WHITESPACE
                )
            ) {
                probe += 1;
            }
            return self
                .tokens
                .get(probe)
                .is_some_and(|t| t.0 == SyntaxKind::IDENT && matches!(t.1, "get" | "set"));
        }

        // Look ahead for modifiers + "get"/"set"
        // This handles cases like "private set" or "protected get"
        if matches!(
            self.current(),
            SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::INTERNAL_KW
                | SyntaxKind::PUBLIC_KW
                | SyntaxKind::INLINE_KW
        ) {
            // Look ahead past the modifier(s) to see if there's "get" or "set"
            let mut lookahead = 1;
            while lookahead < 10 && self.pos + lookahead < self.tokens.len() {
                let kind = self.tokens[self.pos + lookahead].0;
                if kind.is_trivia() {
                    lookahead += 1;
                    continue;
                }
                // Check if this is another modifier
                if matches!(
                    kind,
                    SyntaxKind::PRIVATE_KW
                        | SyntaxKind::PROTECTED_KW
                        | SyntaxKind::INTERNAL_KW
                        | SyntaxKind::PUBLIC_KW
                        | SyntaxKind::INLINE_KW
                ) {
                    lookahead += 1;
                    continue;
                }
                // Check if this is "get" or "set"
                if kind == SyntaxKind::IDENT {
                    let text = self.tokens[self.pos + lookahead].1;
                    return matches!(text, "get" | "set");
                }
                // Not a modifier or accessor - not an accessor pattern
                return false;
            }
        }

        false
    }

    /// Check if current token is an identifier or a soft keyword that can be used as an identifier
    /// In Kotlin, soft keywords like `data`, `catch`, `finally`, etc. can be used as identifiers
    /// in most contexts (e.g., `val data = 5` is valid Kotlin).
    fn at_identifier_or_soft_keyword(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::IDENT
                | SyntaxKind::BACKTICK_IDENT
                | SyntaxKind::DATA_KW      // data class vs val data
                | SyntaxKind::SEALED_KW    // sealed class vs val sealed
                | SyntaxKind::CATCH_KW     // exception handling but can be identifier
                | SyntaxKind::FINALLY_KW   // exception handling but can be identifier
                | SyntaxKind::CONSTRUCTOR_KW // class constructor but can be identifier
                | SyntaxKind::INIT_KW      // initializer but can be identifier
                | SyntaxKind::BY_KW        // delegation but can be identifier
                | SyntaxKind::EXPECT_KW    // multiplatform but can be identifier
                | SyntaxKind::ACTUAL_KW    // multiplatform but can be identifier
                | SyntaxKind::COMPANION_KW // companion object but can be identifier
                | SyntaxKind::SUSPEND_KW   // coroutine but can be identifier
                | SyntaxKind::INLINE_KW    // inline function but can be identifier
                | SyntaxKind::NOINLINE_KW  // noinline param but can be identifier
                | SyntaxKind::CROSSINLINE_KW // crossinline param but can be identifier
                | SyntaxKind::OPERATOR_KW  // operator but can be identifier
                | SyntaxKind::INFIX_KW     // infix but can be identifier
                | SyntaxKind::LATEINIT_KW  // lateinit but can be identifier
                | SyntaxKind::VARARG_KW    // vararg but can be identifier
                | SyntaxKind::OPEN_KW      // open class but can be identifier
                | SyntaxKind::ABSTRACT_KW  // abstract but can be identifier
                | SyntaxKind::OVERRIDE_KW  // override but can be identifier
                | SyntaxKind::TYPEALIAS_KW // typealias but can be identifier
                | SyntaxKind::REIFIED_KW // reified type param modifier but can be identifier
                | SyntaxKind::WHERE_KW // where clause keyword but valid callable name
                | SyntaxKind::OUT_KW // variance modifier but a common identifier (`out`)
                | SyntaxKind::ENUM_KW // modifier keyword only before `class`
                | SyntaxKind::ANNOTATION_KW // modifier keyword only before `class`
                | SyntaxKind::IMPORT_KW // `suspend fun import()`; hard only at file start
        )
    }

    /// Expect and consume an identifier or backtick-escaped identifier.
    fn expect_identifier_or_backtick(&mut self) -> bool {
        if self.at(SyntaxKind::IDENT) || self.at(SyntaxKind::BACKTICK_IDENT) {
            self.bump();
            true
        } else {
            let range = self.current_range();
            self.error(
                format!("Expected identifier, found {:?}", self.current()),
                range,
            );
            false
        }
    }

    /// Expect and consume an identifier or soft keyword
    fn expect_identifier_or_soft_keyword(&mut self) -> bool {
        if self.at_identifier_or_soft_keyword() {
            self.bump();
            true
        } else {
            let range = self.current_range();
            self.error(
                format!("Expected identifier, found {:?}", self.current()),
                range,
            );
            false
        }
    }

    // ============================================================================
    // Source File (Root)
    // ============================================================================

    /// Parse the entire source file
    ///
    /// Grammar:
    /// ```text
    /// SourceFile:
    ///     [PackageDecl] {ImportDecl} {TopLevelDecl}
    /// ```
    fn parse_source_file(&mut self) {
        self.start_node(SyntaxKind::SOURCE_FILE);

        // Skip leading trivia
        while self.current().is_trivia() {
            self.bump();
        }

        // File annotations precede the package declaration: `@file:JvmName("X")`.
        // They must not be routed through top-level declaration parsing, which
        // would leave the package and import lines as loose tokens.
        while self.at(SyntaxKind::AT)
            && self.tokens.get(self.pos + 1).is_some_and(|t| t.1 == "file")
            && self.next_non_trivia_kind(self.pos + 2) == SyntaxKind::COLON
        {
            self.parse_annotation();
            while self.current().is_trivia() {
                self.bump();
            }
        }

        // Parse package declaration (optional)
        if self.at(SyntaxKind::PACKAGE_KW) {
            self.parse_package_decl();
        }

        // Parse import declarations
        while self.at(SyntaxKind::IMPORT_KW) {
            self.parse_import_decl();
        }

        // Parse top-level declarations
        while !self.at_eof() {
            // Skip trivia between declarations
            while self.current().is_trivia() {
                self.bump();
            }

            if self.at_eof() {
                break;
            }

            let decl_start = self.pos;
            if !self.parse_top_level_decl() {
                // Error recovery: skip to next likely declaration point
                self.bump();
            }
            self.stalled(decl_start, "top-level declaration");
        }

        // Consume any trailing trivia
        while !self.at_eof() {
            self.bump();
        }

        self.finish_node();
    }

    // ============================================================================
    // Package and Import Declarations
    // ============================================================================

    /// Parse a package declaration
    fn parse_package_decl(&mut self) {
        self.start_node(SyntaxKind::PACKAGE_DECL);

        self.expect(SyntaxKind::PACKAGE_KW);
        self.skip_trivia();

        // Package name (qualified identifier)
        self.parse_qualified_name();

        self.skip_trivia();

        // Optional semicolon (Kotlin doesn't require it)
        self.eat(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse an import declaration
    fn parse_import_decl(&mut self) {
        self.start_node(SyntaxKind::IMPORT_DECL);

        self.expect(SyntaxKind::IMPORT_KW);
        self.skip_trivia();

        // Import name (qualified identifier)
        self.parse_qualified_name();

        self.skip_trivia();

        // Check for wildcard import (e.g., import kotlin.collections.*)
        // parse_qualified_name stops before the final DOT when it sees STAR ahead
        if self.at(SyntaxKind::DOT) {
            self.bump(); // Consume the DOT
            self.skip_trivia();
            if self.at(SyntaxKind::STAR) {
                self.bump(); // Consume the STAR
                self.skip_trivia();
            }
        }

        // Optional alias (as Name) - soft keywords can be aliases
        if self.at(SyntaxKind::AS_KW) {
            self.bump();
            self.skip_trivia();
            self.expect_identifier_or_soft_keyword();
            self.skip_trivia();
        }

        // Optional semicolon
        self.eat(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a qualified name (e.g., com.example.Package)
    ///
    /// In Kotlin, keywords can be used as identifiers in qualified names
    /// (e.g., package com.example.companion)
    fn parse_qualified_name(&mut self) {
        loop {
            // Accept either IDENT or any keyword as identifier
            if self.at(SyntaxKind::IDENT)
                || self.at(SyntaxKind::BACKTICK_IDENT)
                || self.at_keyword()
            {
                self.bump();
            } else {
                let range = self.current_range();
                self.error(
                    format!("Expected identifier, found {:?}", self.current()),
                    range,
                );
                break;
            }
            self.skip_trivia();

            if self.at(SyntaxKind::DOT) {
                // Look ahead to see if this is a receiver type in a function type (Type.() -> ...)
                // or a wildcard import (import kotlin.collections.*)
                // Use position-based lookahead WITHOUT consuming tokens to avoid position drift
                let mut lookahead_pos = self.pos + 1;

                // Skip trivia in lookahead
                while lookahead_pos < self.tokens.len() && self.tokens[lookahead_pos].0.is_trivia()
                {
                    lookahead_pos += 1;
                }

                // Check if next token is L_PAREN (receiver type) or STAR (wildcard import)
                // If so, don't consume the dot - let the parent handle it
                if lookahead_pos < self.tokens.len() {
                    let next_kind = self.tokens[lookahead_pos].0;
                    if next_kind == SyntaxKind::L_PAREN || next_kind == SyntaxKind::STAR {
                        break;
                    }
                }

                // Safe to consume the DOT now
                self.bump();
            } else {
                break;
            }
        }
    }

    // ============================================================================
    // Top-Level Declarations
    // ============================================================================

    /// Parse a top-level declaration
    ///
    /// Returns true if a declaration was parsed
    fn parse_top_level_decl(&mut self) -> bool {
        // Parse modifiers
        self.parse_modifiers();

        // Determine declaration type
        match self.current() {
            SyntaxKind::FUN_KW => {
                if self.next_non_trivia_kind(self.pos + 1) == SyntaxKind::INTERFACE_KW {
                    self.parse_fun_interface_decl();
                } else {
                    self.parse_function_decl();
                }
                true
            }
            SyntaxKind::VAL_KW | SyntaxKind::VAR_KW => {
                self.parse_property_decl();
                true
            }
            SyntaxKind::CLASS_KW => {
                self.parse_class_decl();
                true
            }
            SyntaxKind::OBJECT_KW => {
                self.parse_object_decl();
                true
            }
            SyntaxKind::INTERFACE_KW => {
                self.parse_interface_decl();
                true
            }
            SyntaxKind::TYPEALIAS_KW => {
                self.parse_typealias();
                true
            }
            _ => false,
        }
    }

    /// Parse a type alias declaration.
    ///
    /// Grammar:
    /// ```text
    /// TypeAlias:
    ///     [Modifiers] 'typealias' SimpleIdentifier [TypeParameters] '=' Type
    /// ```
    ///
    /// `typealias` is file-scoped only in Kotlin — it is never valid inside a
    /// class body — but this parser is still called from `parse_class_member`
    /// so that position produces a real error instead of the member loop
    /// silently swallowing the tokens one at a time (see `parse_class_member`).
    fn parse_typealias(&mut self) {
        self.start_node(SyntaxKind::TYPEALIAS_DECL);

        self.expect(SyntaxKind::TYPEALIAS_KW);
        self.skip_trivia();

        // Alias name
        self.expect_identifier_or_backtick();
        self.skip_trivia();

        // Optional type parameters: `typealias Handler<T> = ...`
        if self.at(SyntaxKind::LT) {
            self.parse_type_parameters();
            self.skip_trivia();
        }

        self.expect(SyntaxKind::EQ);
        self.skip_trivia();

        // Aliased type. Guard against the truncated `typealias Name =` case,
        // where there is nothing left for `parse_type_ref` to consume.
        if !self.at_eof() {
            self.parse_type_ref();
        }

        self.finish_node();
    }

    // ============================================================================
    // Modifiers
    // ============================================================================

    /// Parse modifiers (public, private, abstract, etc.)
    fn parse_modifiers(&mut self) {
        self.start_node(SyntaxKind::MODIFIERS);

        loop {
            self.skip_trivia();

            // Check for annotation
            if self.at(SyntaxKind::AT) {
                self.parse_annotation();
                continue;
            }

            // Context receivers: `context(scope: LazyItemScope) fun ...`
            if self.at(SyntaxKind::IDENT)
                && self.current_text() == "context"
                && self.next_non_trivia_kind(self.pos + 1) == SyntaxKind::L_PAREN
            {
                let mut after = self.pos + 1;
                while self.tokens.get(after).is_some_and(|t| t.0.is_trivia()) {
                    after += 1;
                }
                let mut depth = 0usize;
                while let Some((kind, _, _)) = self.tokens.get(after) {
                    match kind {
                        SyntaxKind::L_PAREN => depth += 1,
                        SyntaxKind::R_PAREN => {
                            depth -= 1;
                            if depth == 0 {
                                after += 1;
                                break;
                            }
                        }
                        SyntaxKind::EOF => break,
                        _ => {}
                    }
                    after += 1;
                }
                if self.starts_declaration_after(after) {
                    self.bump(); // context
                    self.skip_trivia();
                    self.consume_balanced_parens();
                    continue;
                }
            }

            // Check for modifier keyword
            match self.current() {
                SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::INTERNAL_KW
                | SyntaxKind::OPEN_KW
                | SyntaxKind::ABSTRACT_KW
                | SyntaxKind::DATA_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::ANNOTATION_KW
                | SyntaxKind::SEALED_KW
                | SyntaxKind::OVERRIDE_KW
                | SyntaxKind::SUSPEND_KW
                | SyntaxKind::INLINE_KW
                | SyntaxKind::OPERATOR_KW
                | SyntaxKind::INFIX_KW
                | SyntaxKind::COMPANION_KW => {
                    if self.at(SyntaxKind::ENUM_KW) {
                        self.pending_enum = true;
                    }
                    self.bump();
                }
                // Soft modifiers that only count as modifiers when a
                // declaration (or another modifier) follows.
                SyntaxKind::LATEINIT_KW
                | SyntaxKind::EXPECT_KW
                | SyntaxKind::ACTUAL_KW
                | SyntaxKind::NOINLINE_KW
                | SyntaxKind::CROSSINLINE_KW
                    if self.starts_declaration_after(self.pos + 1) =>
                {
                    self.bump();
                }
                // `const`, `tailrec`, `external`, `final`, `inner`, `value` are
                // ordinary identifiers to the lexer.
                SyntaxKind::IDENT
                    if matches!(
                        self.current_text(),
                        "const" | "tailrec" | "external" | "final" | "inner" | "value"
                    ) && self.starts_declaration_after(self.pos + 1) =>
                {
                    self.bump();
                }
                _ => break,
            }
        }

        self.finish_node();
    }

    /// Whether the first significant token at or after `index` starts a
    /// declaration or is a modifier that precedes one.
    fn starts_declaration_after(&self, mut index: usize) -> bool {
        while self.tokens.get(index).is_some_and(|t| t.0.is_trivia()) {
            index += 1;
        }
        let Some((kind, text, _)) = self.tokens.get(index) else {
            return false;
        };
        match kind {
            SyntaxKind::VAL_KW
            | SyntaxKind::VAR_KW
            | SyntaxKind::FUN_KW
            | SyntaxKind::CLASS_KW
            | SyntaxKind::OBJECT_KW
            | SyntaxKind::INTERFACE_KW
            | SyntaxKind::TYPEALIAS_KW
            | SyntaxKind::CONSTRUCTOR_KW
            | SyntaxKind::PUBLIC_KW
            | SyntaxKind::PRIVATE_KW
            | SyntaxKind::PROTECTED_KW
            | SyntaxKind::INTERNAL_KW
            | SyntaxKind::OPEN_KW
            | SyntaxKind::ABSTRACT_KW
            | SyntaxKind::DATA_KW
            | SyntaxKind::ENUM_KW
            | SyntaxKind::ANNOTATION_KW
            | SyntaxKind::SEALED_KW
            | SyntaxKind::OVERRIDE_KW
            | SyntaxKind::SUSPEND_KW
            | SyntaxKind::INLINE_KW
            | SyntaxKind::OPERATOR_KW
            | SyntaxKind::INFIX_KW
            | SyntaxKind::COMPANION_KW
            | SyntaxKind::LATEINIT_KW
            | SyntaxKind::EXPECT_KW
            | SyntaxKind::ACTUAL_KW
            | SyntaxKind::AT => true,
            SyntaxKind::IDENT => matches!(
                *text,
                "const" | "tailrec" | "external" | "final" | "inner" | "value"
            ),
            _ => false,
        }
    }

    /// Parse an annotation
    fn parse_annotation(&mut self) {
        self.start_node(SyntaxKind::ANNOTATION);

        self.expect(SyntaxKind::AT);
        self.skip_trivia();

        // Check for use-site target (e.g., @param:Ann, @field:Ann, @setparam:Ann).
        // Use-site targets are identifiers or keywords followed by COLON.
        //
        // This must be probed with non-consuming lookahead: `bump()` appends the
        // token text to the green-tree builder, and rewinding `self.pos` does not
        // un-emit it. A speculative bump+rewind would therefore duplicate the
        // identifier (and its trailing trivia) in the lossless tree, corrupting
        // every subsequent token offset.
        let mut file_target = false;
        if self.at(SyntaxKind::IDENT) || self.at_keyword() {
            let mut lookahead_pos = self.pos + 1;
            while lookahead_pos < self.tokens.len() && self.tokens[lookahead_pos].0.is_trivia() {
                lookahead_pos += 1;
            }
            if lookahead_pos < self.tokens.len()
                && self.tokens[lookahead_pos].0 == SyntaxKind::COLON
            {
                // Confirmed use-site target: consume the target identifier and colon.
                file_target = self.current_text() == "file";
                self.bump();
                self.skip_trivia();
                self.bump();
                self.skip_trivia();
            }
        }

        // Annotation name (required after optional use-site target)
        self.parse_qualified_name();

        self.skip_trivia();

        // Type arguments: `@Destination<RootGraph>(...)`
        if self.at(SyntaxKind::LT) {
            self.parse_type_arguments();
            self.skip_trivia();
        }

        // Arguments (optional). They must directly follow the name: in
        // `@Composable () -> Unit` the parentheses belong to the function type.
        // A `@file:` annotation is never a type, so its arguments may be separated.
        let args_adjacent = file_target
            || self
                .pos
                .checked_sub(1)
                .and_then(|previous| self.tokens.get(previous))
                .is_some_and(|token| !token.0.is_trivia());
        if self.at(SyntaxKind::L_PAREN) && args_adjacent {
            self.parse_annotation_args();
        }

        self.finish_node();
    }

    /// Parse annotation arguments: `(value)` or `(key = value, key2 = value2)`
    fn parse_annotation_args(&mut self) {
        self.start_node(SyntaxKind::ANNOTATION_ARG_LIST);
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        if !self.at(SyntaxKind::R_PAREN) {
            // Parse first argument
            self.parse_annotation_arg();

            // Parse remaining arguments
            while self.at(SyntaxKind::COMMA) {
                self.bump(); // consume comma
                self.skip_trivia();

                if self.at(SyntaxKind::R_PAREN) {
                    break; // trailing comma
                }

                self.parse_annotation_arg();
            }
        }

        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// Parse a single annotation argument: `value` or `key = value`
    fn parse_annotation_arg(&mut self) {
        self.start_node(SyntaxKind::ANNOTATION_ARG);

        // Check if this is a key-value pair (key = value) or just a value
        // Look ahead to see if there's an '=' after the identifier
        if self.at(SyntaxKind::IDENT) || self.at(SyntaxKind::BACKTICK_IDENT) {
            let ahead_pos = self.pos + 1;
            // Skip whitespace to find '='
            let mut check_pos = ahead_pos;
            while check_pos < self.tokens.len() {
                let kind = self.tokens[check_pos].0;
                if kind == SyntaxKind::WHITESPACE
                    || kind == SyntaxKind::LINE_COMMENT
                    || kind == SyntaxKind::BLOCK_COMMENT
                {
                    check_pos += 1;
                } else {
                    break;
                }
            }

            // If we found '=', this is a key-value pair
            if check_pos < self.tokens.len() && self.tokens[check_pos].0 == SyntaxKind::EQ {
                // Parse key
                self.bump(); // IDENT
                self.skip_trivia();
                self.expect(SyntaxKind::EQ);
                self.skip_trivia();
            }
        }

        // Parse value
        self.parse_annotation_value();

        // Constant expressions: `"a" + "b"`, `BASE + 1`
        loop {
            self.skip_trivia();
            if !matches!(
                self.current(),
                SyntaxKind::PLUS
                    | SyntaxKind::MINUS
                    | SyntaxKind::STAR
                    | SyntaxKind::SLASH
                    | SyntaxKind::PERCENT
            ) {
                break;
            }
            self.bump();
            self.skip_trivia();
            self.parse_annotation_value();
        }

        self.skip_trivia();
        self.finish_node();
    }

    /// Parse an annotation value (literal, annotation, array, identifier, etc.)
    fn parse_annotation_value(&mut self) {
        match self.current() {
            // Nested annotation
            SyntaxKind::AT => {
                self.parse_annotation();
            }
            // Parenthesized value: `[(Index(value = ["a"]))]`
            SyntaxKind::L_PAREN => {
                self.bump();
                self.skip_trivia();
                self.parse_annotation_value();
                self.skip_trivia();
                self.expect(SyntaxKind::R_PAREN);
            }
            // Array value using square brackets: [val1, val2]
            SyntaxKind::L_BRACKET => {
                self.start_node(SyntaxKind::ANNOTATION_ARRAY_VALUE);
                self.bump(); // '['
                self.skip_trivia();

                if !self.at(SyntaxKind::R_BRACKET) {
                    self.parse_annotation_value();

                    while self.at(SyntaxKind::COMMA) {
                        self.bump();
                        self.skip_trivia();

                        if self.at(SyntaxKind::R_BRACKET) {
                            break; // trailing comma
                        }

                        self.parse_annotation_value();
                    }
                }

                self.skip_trivia();
                self.expect(SyntaxKind::R_BRACKET);
                self.finish_node();
            }
            // Literals
            SyntaxKind::STRING_LITERAL
            | SyntaxKind::RAW_STRING_LITERAL
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::TRUE_KW
            | SyntaxKind::FALSE_KW
            | SyntaxKind::NULL_KW => {
                self.bump();
            }
            // Identifier (could be enum constant, class reference, etc.)
            SyntaxKind::IDENT => {
                // Parse as qualified name (could be Enum.VALUE, ClassName::class,
                // or an annotation-constructor value such as ReplaceWith("…")).
                self.parse_qualified_name();

                // Handle ::class suffix (Kotlin's class literal syntax)
                self.skip_trivia();
                if self.at(SyntaxKind::COLONCOLON) {
                    self.bump(); // ::
                    self.skip_trivia();
                    if self.at(SyntaxKind::CLASS_KW) {
                        self.bump();
                    }
                }

                self.skip_trivia();
                if self.at(SyntaxKind::L_PAREN) {
                    // Annotation values can invoke another annotation class's
                    // constructor without an `@`, e.g. ReplaceWith("newApi").
                    self.parse_annotation_args();
                }
            }
            // Unary minus for negative numbers
            SyntaxKind::MINUS => {
                self.bump(); // -
                self.skip_trivia();
                // Expect a numeric literal
                if matches!(
                    self.current(),
                    SyntaxKind::INT_LITERAL
                        | SyntaxKind::LONG_LITERAL
                        | SyntaxKind::FLOAT_LITERAL
                        | SyntaxKind::DOUBLE_LITERAL
                ) {
                    self.bump();
                }
            }
            // Unary plus (rare but valid)
            SyntaxKind::PLUS => {
                self.bump(); // +
                self.skip_trivia();
                if matches!(
                    self.current(),
                    SyntaxKind::INT_LITERAL
                        | SyntaxKind::LONG_LITERAL
                        | SyntaxKind::FLOAT_LITERAL
                        | SyntaxKind::DOUBLE_LITERAL
                ) {
                    self.bump();
                }
            }
            // Unknown - skip to delimiter
            _ => {
                // Skip unknown tokens until we hit a delimiter
                while !self.at_eof()
                    && !matches!(
                        self.current(),
                        SyntaxKind::COMMA
                            | SyntaxKind::R_PAREN
                            | SyntaxKind::R_BRACKET
                            | SyntaxKind::R_BRACE
                    )
                {
                    self.bump();
                }
            }
        }
    }

    // ============================================================================
    // Function Declaration
    // ============================================================================

    /// Parse a function declaration
    ///
    /// Grammar:
    /// ```text
    /// FunctionDecl:
    ///     {Modifier} fun [TypeParams] Identifier ParamList [: ReturnType] [FunctionBody]
    /// ```
    fn parse_function_decl(&mut self) {
        self.start_node(SyntaxKind::FUNCTION_DECL);

        self.expect(SyntaxKind::FUN_KW);
        self.skip_trivia();

        // Type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_parameters();
            self.skip_trivia();
        }

        // Check for receiver type (extension function): a possibly qualified,
        // generic, nullable type followed by the separating dot.
        if let Some(separator) = self.receiver_separator_index() {
            self.parse_receiver_type(separator);
        }

        // Function name can be a soft keyword (`where`, `by`, ...), or an
        // arbitrary backtick-escaped spelling.
        if self.at(SyntaxKind::BACKTICK_IDENT) {
            self.bump();
        } else {
            self.expect_identifier_or_soft_keyword();
        }
        self.skip_trivia();

        // Parameter list
        self.parse_parameter_list();

        self.skip_trivia();

        // Return type (optional)
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            self.parse_type_ref();
        }

        self.skip_trivia();

        // Where clause (optional)
        if self.at(SyntaxKind::WHERE_KW) {
            self.parse_where_clause();
            self.skip_trivia();
        }

        // Function body (optional for abstract/interface functions)
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        } else if self.at(SyntaxKind::EQ) {
            // Single-expression body
            self.bump();
            self.skip_trivia();
            self.parse_expression();
        }

        self.finish_node();
    }

    /// Parse a where clause: `where T : Comparable<T>, U : Serializable`
    fn parse_where_clause(&mut self) {
        self.start_node(SyntaxKind::WHERE_CLAUSE);
        self.expect(SyntaxKind::WHERE_KW);
        self.skip_trivia();

        loop {
            // Each constraint: T : Bound
            let constraint_start = self.pos;
            self.start_node(SyntaxKind::TYPE_CONSTRAINT);

            // Type parameter name
            if self.at(SyntaxKind::IDENT) {
                self.bump();
                self.skip_trivia();
            }

            // Colon
            if self.at(SyntaxKind::COLON) {
                self.bump();
                self.skip_trivia();
            }

            // Upper bound type
            self.parse_type_ref();
            self.skip_trivia();

            self.finish_node(); // TYPE_CONSTRAINT

            if self.stalled(constraint_start, "where clause constraint") {
                break;
            }

            // Comma for next constraint
            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        self.finish_node(); // WHERE_CLAUSE
    }

    /// Parse a parameter list
    fn parse_parameter_list(&mut self) {
        self.start_node(SyntaxKind::PARAMETER_LIST);

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        if !self.at(SyntaxKind::R_PAREN) {
            loop {
                self.parse_parameter();
                self.skip_trivia();

                if self.at(SyntaxKind::COMMA) {
                    self.bump();
                    self.skip_trivia();
                    if self.at(SyntaxKind::R_PAREN) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.expect(SyntaxKind::R_PAREN);

        self.finish_node();
    }

    /// Parse a single parameter
    fn parse_parameter(&mut self) {
        self.start_node(SyntaxKind::PARAMETER);

        // Optional annotations (e.g., @JvmField)
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }

        // Optional parameter modifiers. Primary-constructor property parameters
        // commonly combine visibility/override with `val`/`var`, e.g.
        // `class User(private val id: Id)` and
        // `class Named(override val name: String) : HasName`.
        while (self.at(SyntaxKind::VARARG_KW)
            || self.at(SyntaxKind::NOINLINE_KW)
            || self.at(SyntaxKind::CROSSINLINE_KW)
            || self.at(SyntaxKind::PUBLIC_KW)
            || self.at(SyntaxKind::PRIVATE_KW)
            || self.at(SyntaxKind::PROTECTED_KW)
            || self.at(SyntaxKind::INTERNAL_KW)
            || self.at(SyntaxKind::OPEN_KW)
            || self.at(SyntaxKind::ABSTRACT_KW)
            || self.at(SyntaxKind::LATEINIT_KW)
            || self.at(SyntaxKind::OVERRIDE_KW))
            // `private: Boolean` names a parameter `private`
            && self.next_non_trivia_kind(self.pos + 1) != SyntaxKind::COLON
        {
            self.bump();
            self.skip_trivia();
        }

        // Optional val/var for primary constructor parameters
        if self.at(SyntaxKind::VAL_KW) || self.at(SyntaxKind::VAR_KW) {
            self.bump();
            self.skip_trivia();
        }

        // Parameter name - soft keywords can be parameter names, and a
        // visibility modifier followed by `:` is itself the name (`private: Boolean`).
        if matches!(
            self.current(),
            SyntaxKind::PRIVATE_KW
                | SyntaxKind::PUBLIC_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::INTERNAL_KW
        ) {
            self.bump();
        } else {
            self.expect_identifier_or_soft_keyword();
        }
        self.skip_trivia();

        // Type annotation
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            self.parse_type_ref();
        }

        self.skip_trivia();

        // Default value (optional)
        if self.at(SyntaxKind::EQ) {
            self.bump();
            self.skip_trivia();
            self.parse_expression();
        }

        self.finish_node();
    }

    // ============================================================================
    // Property Declaration
    // ============================================================================

    /// Parse a property declaration
    ///
    /// Grammar:
    /// ```text
    /// PropertyDecl:
    ///     {Modifier} (val|var) Identifier [: Type] [= Expression]
    /// ```
    fn parse_property_decl(&mut self) {
        self.start_node(SyntaxKind::PROPERTY_DECL);

        // val or var
        if self.at(SyntaxKind::VAL_KW) || self.at(SyntaxKind::VAR_KW) {
            self.bump();
        } else {
            self.error("Expected 'val' or 'var'", self.current_range());
        }

        self.skip_trivia();

        // Generic extension property: `val <T> List<T>.last: T`
        if self.at(SyntaxKind::LT) {
            self.parse_type_parameters();
            self.skip_trivia();
        }

        // Extension property receiver: `val Project.sourceSets: X`
        if let Some(separator) = self.property_receiver_separator_index() {
            self.parse_receiver_type(separator);
        }

        // Property name - accept identifiers or soft keywords (e.g., val data: Int)
        self.expect_identifier_or_soft_keyword();
        self.skip_trivia();

        // Type annotation (optional)
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            self.parse_type_ref();
            self.skip_trivia();
        }

        // Initializer (optional) or delegation
        if self.at(SyntaxKind::EQ) {
            self.bump();
            self.skip_trivia();
            self.parse_expression();
        } else if self.at(SyntaxKind::BY_KW) {
            self.bump(); // consume 'by'
            self.skip_trivia();
            self.parse_expression();
        }

        // Parse property accessors (get/set)
        self.skip_trivia();
        while self.at_property_accessor() {
            let accessor_start = self.pos;
            self.parse_property_accessor();
            self.skip_trivia();
            // `at_property_accessor` is a pure lookahead: if the accessor parser
            // ever returned without consuming, this would re-enter identically.
            if self.stalled(accessor_start, "property accessor") {
                break;
            }
        }

        self.finish_node();
    }

    /// Parse a property accessor (getter or setter)
    ///
    /// Grammar:
    /// ```text
    /// PropertyAccessor:
    ///     [Modifiers] (get|set) [FunctionBody]
    ///
    /// Getter:
    ///     [Modifiers] get() [FunctionBody]
    ///
    /// Setter:
    ///     [Modifiers] set([Modifiers] Identifier [: Type]) [FunctionBody]
    ///
    /// FunctionBody:
    ///     Block              // { ... }
    ///     = Expression       // = expr
    /// ```
    fn parse_property_accessor(&mut self) {
        self.start_node(SyntaxKind::PROPERTY_ACCESSOR);

        // Optional modifiers (visibility, inline, etc.)
        self.parse_modifiers();
        self.skip_trivia();

        // Accessor name: "get" or "set" (as IDENT, not keyword)
        let is_setter = self.current_text() == "set";
        self.bump(); // Consume "get" or "set"
        self.skip_trivia();

        // Parameter list for setter, empty parens for getter
        if self.at(SyntaxKind::L_PAREN) {
            if is_setter {
                self.parse_setter_parameter_list();
            } else {
                // Getter: just empty parentheses
                self.bump(); // L_PAREN
                self.skip_trivia();
                self.expect(SyntaxKind::R_PAREN);
            }
            self.skip_trivia();
        }

        // Optional function body
        if self.at(SyntaxKind::L_BRACE) {
            // Block body: { ... }
            self.parse_block();
        } else if self.at(SyntaxKind::EQ) {
            // Expression body: = expr
            self.bump(); // EQ
            self.skip_trivia();
            self.parse_expression();
        }
        // No body is also valid (for interface properties or accessors with only visibility)

        self.finish_node();
    }

    /// Parse setter parameter list: (param) or ([modifiers] param: Type)
    ///
    /// Setters have exactly one parameter, but it can have modifiers and type annotations.
    fn parse_setter_parameter_list(&mut self) {
        self.start_node(SyntaxKind::PARAMETER_LIST);

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Setter has exactly one parameter
        if !self.at(SyntaxKind::R_PAREN) {
            self.start_node(SyntaxKind::PARAMETER);

            // Optional modifiers (rarely used but valid)
            self.parse_modifiers();
            self.skip_trivia();

            // Parameter name
            self.expect_identifier_or_soft_keyword();
            self.skip_trivia();

            // Optional type annotation
            if self.at(SyntaxKind::COLON) {
                self.bump();
                self.skip_trivia();
                self.parse_type_ref();
                self.skip_trivia();
            }

            self.finish_node(); // PARAMETER
        }

        self.expect(SyntaxKind::R_PAREN);

        self.finish_node(); // PARAMETER_LIST
    }

    /// Parse a local property declaration (inside a block)
    ///
    /// The *declaration head* (`val`/`var`, name, optional `: Type`) is scanned
    /// with newline sensitivity: Kotlin has no statement terminator, so a
    /// newline before the name/type/`=` means the declaration was truncated and
    /// the following line is a separate statement. Bailing out there keeps a
    /// half-written `val` from swallowing the rest of the block.
    ///
    /// The *initializer* is parsed by the real expression parser
    /// (`parse_expression`), exactly as statement, return and call-argument
    /// position do. It used to be a flat token scan delimited by newlines and
    /// `val`/`var`/`fun` keywords, which is why structured expressions never
    /// appeared here: `val r = when (x) { ... }` produced bare tokens plus a
    /// flat `LAMBDA_EXPR` for the braces instead of `WHEN_EXPR`. The scan's
    /// statement-termination job is already done by the Pratt loop, which stops
    /// at any token that is neither an infix nor a postfix operator — so
    /// `val x = a` followed by `f()` on the next line still parses as two
    /// statements.
    ///
    /// Grammar:
    /// ```text
    /// LocalPropertyDecl:
    ///     (val|var) (Identifier | DestructuringPattern) [: Type] [(= | by) Expression]
    /// ```
    fn parse_local_property_decl(&mut self) {
        self.start_node(SyntaxKind::PROPERTY_DECL);

        // val or var
        if self.at(SyntaxKind::VAL_KW) || self.at(SyntaxKind::VAR_KW) {
            self.bump();
        } else {
            self.error("Expected 'val' or 'var'", self.current_range());
        }

        // Skip whitespace but not newlines (important for statement boundary detection)
        while self.at(SyntaxKind::WHITESPACE) {
            let text = self.current_text();
            if text.contains('\n') {
                // Hit newline - end of property name area
                self.bump();
                self.finish_node();
                return;
            }
            self.bump();
        }

        // Check for destructuring declaration: val (a, b) = ...
        if self.at(SyntaxKind::L_PAREN) {
            // Consume the entire destructuring pattern
            self.bump(); // L_PAREN
            let mut paren_depth = 1;
            while paren_depth > 0 && !self.at_eof() {
                match self.current() {
                    SyntaxKind::L_PAREN => {
                        paren_depth += 1;
                        self.bump();
                    }
                    SyntaxKind::R_PAREN => {
                        paren_depth -= 1;
                        self.bump();
                    }
                    _ => {
                        self.bump();
                    }
                }
            }
        } else {
            // Property name - accept identifiers or soft keywords
            self.expect_identifier_or_soft_keyword();
        }

        // Skip whitespace but not newlines
        while self.at(SyntaxKind::WHITESPACE) {
            let text = self.current_text();
            if text.contains('\n') {
                self.bump();
                self.finish_node();
                return;
            }
            self.bump();
        }

        // Type annotation (optional)
        if self.at(SyntaxKind::COLON) {
            self.bump();
            // Skip whitespace but not newlines
            while self.at(SyntaxKind::WHITESPACE) {
                let text = self.current_text();
                if text.contains('\n') {
                    self.bump();
                    self.finish_node();
                    return;
                }
                self.bump();
            }
            self.parse_type_ref();
            // Skip whitespace but not newlines
            while self.at(SyntaxKind::WHITESPACE) {
                let text = self.current_text();
                if text.contains('\n') {
                    self.bump();
                    self.finish_node();
                    return;
                }
                self.bump();
            }
        }

        // Initializer (optional) or delegation
        if self.at(SyntaxKind::BY_KW) {
            self.bump(); // consume 'by'
                         // Skip whitespace but not newlines
            while self.at(SyntaxKind::WHITESPACE) {
                let text = self.current_text();
                if text.contains('\n') {
                    self.bump();
                    self.finish_node();
                    return;
                }
                self.bump();
            }
            self.parse_local_initializer_expr();
        } else if self.at(SyntaxKind::EQ) {
            self.bump();
            // Skip whitespace but not newlines
            while self.at(SyntaxKind::WHITESPACE) {
                let text = self.current_text();
                if text.contains('\n') {
                    self.bump();
                    self.finish_node();
                    return;
                }
                self.bump();
            }

            self.parse_local_initializer_expr();
        }

        self.finish_node();
    }

    /// Parse the initializer / delegate expression of a local `val`/`var`.
    ///
    /// This is the real expression parser, so the initializer gets the same tree
    /// shape it would in statement position (`WHEN_EXPR`, `TRY_EXPR`,
    /// `IF_EXPR`, `DO_WHILE_EXPR`, structured `LAMBDA_EXPR`, …).
    ///
    /// The one thing the Pratt parser cannot do on its own is refuse an empty
    /// initializer: `val x =` at a block boundary leaves the parser looking at
    /// `}` / `val` / EOF, and `parse_primary_expr`'s fallback would either
    /// consume nothing (fine) or, for tokens outside `at_expression_end`,
    /// swallow the next statement's first token. Guarding on
    /// `at_expression_end` up front keeps the truncated forms
    /// (`val x =`, `val x = }`) as pure no-ops.
    fn parse_local_initializer_expr(&mut self) {
        if self.at_expression_end() || self.at_eof() {
            return;
        }
        self.parse_expression();
    }

    // ============================================================================
    // Class Declaration
    // ============================================================================

    /// Parse a class declaration
    ///
    /// Grammar:
    /// ```text
    /// ClassDecl:
    ///     {Modifier} class Identifier [TypeParams] [PrimaryConstructor] [: SuperTypes] ClassBody
    /// ```
    fn parse_class_decl(&mut self) {
        let is_enum = std::mem::take(&mut self.pending_enum);
        self.start_node(SyntaxKind::CLASS_DECL);

        self.expect(SyntaxKind::CLASS_KW);
        self.skip_trivia();

        // Class name
        self.expect_identifier_or_backtick();
        self.skip_trivia();

        // Type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_parameters();
        }

        // A primary constructor may carry annotations and visibility between
        // the class name/type parameters and the `constructor` keyword:
        // `class User private constructor(...)`.
        // Leaving those tokens unconsumed ends CLASS_DECL before its body and
        // promotes every nested declaration to file scope.
        self.skip_trivia();
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }
        // A visibility keyword belongs to the primary constructor only when it
        // is followed by the explicit `constructor` keyword. Without that
        // boundary, a body-less declaration such as `class Base` would consume
        // the visibility of the next top-level declaration after `skip_trivia`
        // crossed the newline (`class Base\nprivate data class User`).
        if matches!(
            self.current(),
            SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::INTERNAL_KW
        ) && self.next_non_trivia_kind(self.pos + 1) == SyntaxKind::CONSTRUCTOR_KW
        {
            self.bump();
            self.skip_trivia();
        }

        // Primary constructor (optional)
        if self.at(SyntaxKind::L_PAREN) || self.at(SyntaxKind::CONSTRUCTOR_KW) {
            // Modifiers, annotations and defaults in the parameter list are covered by
            // primary_constructors_with_modifiers_annotations_and_defaults_parse_cleanly.
            if self.at(SyntaxKind::CONSTRUCTOR_KW) {
                self.bump();
                self.skip_trivia();
            }
            if self.at(SyntaxKind::L_PAREN) {
                self.parse_parameter_list();
            }
        }

        self.skip_trivia();

        // Supertype list (optional): class Foo : Bar, Baz, Interface1
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();

            self.parse_supertype_specifier();

            // Parse additional supertypes (comma-separated)
            while self.at(SyntaxKind::COMMA) {
                self.bump(); // consume comma
                self.skip_trivia();
                self.parse_supertype_specifier();
            }
        }

        self.skip_trivia();

        // Where clause (optional)
        if self.at(SyntaxKind::WHERE_KW) {
            self.parse_where_clause();
            self.skip_trivia();
        }

        // Class body
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_class_body_with(is_enum);
        }

        self.finish_node();
    }

    /// Parse one class-header supertype specifier, including constructor and
    /// interface-delegation forms (`Base(args)` and `I by delegate`).
    fn parse_supertype_specifier(&mut self) {
        self.parse_type_ref();
        self.skip_trivia();
        if self.at(SyntaxKind::L_PAREN) {
            self.consume_balanced_parens();
            self.skip_trivia();
        }
        if self.at(SyntaxKind::BY_KW) {
            self.bump();
            self.skip_trivia();
            // In this grammar position a top-level postfix `{ ... }` is the
            // class body, not a trailing-lambda call on the delegate. Parse a
            // real expression tree while suppressing only that outer postfix
            // form. Parenthesized expressions and call arguments use their
            // normal expression context, so nested lambdas/object expressions
            // remain fully structured.
            self.parse_expr_bp_with_trailing_lambda(0, false);
            self.skip_trivia();
        }
    }

    /// Parse a class body
    fn parse_class_body(&mut self) {
        self.parse_class_body_with(false);
    }

    /// Whether an enum entry starts here: `A,` `A;` `A(1)` `A {` `A }`, optionally
    /// preceded by annotations.
    fn at_enum_entry(&self) -> bool {
        let start = self.skip_annotations_lookahead(self.pos);
        let Some((kind, _, _)) = self.tokens.get(start) else {
            return false;
        };
        if !matches!(kind, SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT) {
            return false;
        }
        let mut next = start + 1;
        while self.tokens.get(next).is_some_and(|t| t.0.is_trivia()) {
            next += 1;
        }
        matches!(
            self.tokens.get(next).map(|t| t.0),
            Some(
                SyntaxKind::COMMA
                    | SyntaxKind::SEMICOLON
                    | SyntaxKind::L_PAREN
                    | SyntaxKind::L_BRACE
                    | SyntaxKind::R_BRACE
            )
        )
    }

    /// Enum entries at the start of an enum body, up to the `;` (or `}`).
    fn parse_enum_entries(&mut self) {
        while self.at_enum_entry() {
            self.start_node(SyntaxKind::ENUM_ENTRY);
            while self.at(SyntaxKind::AT) {
                self.parse_annotation();
                self.skip_trivia();
            }
            self.bump(); // entry name
            self.skip_trivia();
            if self.at(SyntaxKind::L_PAREN) {
                self.parse_call_args(false);
                self.skip_trivia();
            }
            if self.at(SyntaxKind::L_BRACE) {
                self.parse_class_body();
            }
            self.finish_node();
            self.skip_trivia();
            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }
        if self.at(SyntaxKind::SEMICOLON) {
            self.bump();
            self.skip_trivia();
        }
    }

    fn parse_class_body_with(&mut self, is_enum: bool) {
        let outer_paren_depth = std::mem::replace(&mut self.paren_depth, 0);
        self.start_node(SyntaxKind::CLASS_BODY);

        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        if is_enum {
            self.parse_enum_entries();
        }

        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let member_start = self.pos;
            if !self.parse_class_member() {
                // Error recovery
                self.bump();
            }
            self.skip_trivia();
            self.stalled(member_start, "class member");
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
        self.paren_depth = outer_paren_depth;
    }

    /// Parse a class member
    fn parse_class_member(&mut self) -> bool {
        // Parse modifiers
        self.parse_modifiers();

        // Check for member type
        match self.current() {
            SyntaxKind::FUN_KW => {
                if self.next_non_trivia_kind(self.pos + 1) == SyntaxKind::INTERFACE_KW {
                    self.parse_fun_interface_decl();
                } else {
                    self.parse_function_decl();
                }
                true
            }
            SyntaxKind::VAL_KW | SyntaxKind::VAR_KW => {
                self.parse_property_decl();
                true
            }
            SyntaxKind::CLASS_KW => {
                self.parse_class_decl();
                true
            }
            SyntaxKind::INTERFACE_KW => {
                self.parse_interface_decl();
                true
            }
            SyntaxKind::OBJECT_KW => {
                self.parse_object_decl();
                true
            }
            SyntaxKind::INIT_KW => {
                self.parse_init_block();
                true
            }
            SyntaxKind::CONSTRUCTOR_KW => {
                self.parse_constructor_decl();
                true
            }
            SyntaxKind::TYPEALIAS_KW => {
                // `typealias` is file-scoped only in Kotlin; it cannot appear inside
                // a class body. Without an explicit arm here, `parse_class_member`
                // would return `false` and the `parse_class_body` loop would fall
                // back to its single-token error recovery (`self.bump()`), which
                // consumes tokens one at a time *without* recording an error - the
                // same "silently swallowed" bug this construct has at top level.
                // Instead, parse it properly (so the tree stays structured and the
                // loop still terminates in one step) and flag the position error.
                let range = self.current_range();
                self.error(
                    "'typealias' is not allowed inside a class body; declare it at file scope",
                    range,
                );
                self.parse_typealias();
                true
            }
            _ => false,
        }
    }

    /// Parse an initializer block
    fn parse_init_block(&mut self) {
        self.start_node(SyntaxKind::INIT_BLOCK);

        self.expect(SyntaxKind::INIT_KW);
        self.skip_trivia();

        self.parse_block();

        self.finish_node();
    }

    /// Parse a constructor declaration
    fn parse_constructor_decl(&mut self) {
        self.start_node(SyntaxKind::CONSTRUCTOR_DECL);

        self.expect(SyntaxKind::CONSTRUCTOR_KW);
        self.skip_trivia();

        self.parse_parameter_list();
        self.skip_trivia();

        // A secondary constructor may delegate to another constructor in the
        // same class or to the superclass before its body:
        // `constructor(x: Int) : this()` / `constructor() : super()`.
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            if self.at(SyntaxKind::THIS_KW) || self.at(SyntaxKind::SUPER_KW) {
                self.bump();
            } else {
                self.error(
                    "expected 'this' or 'super' in constructor delegation",
                    self.current_range(),
                );
            }
            self.skip_trivia();
            if self.at(SyntaxKind::L_PAREN) {
                self.consume_balanced_parens();
            } else {
                self.error(
                    "expected argument list in constructor delegation",
                    self.current_range(),
                );
            }
            self.skip_trivia();
        }

        // Constructor body (optional)
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        }

        self.finish_node();
    }

    // ============================================================================
    // Object Declaration
    // ============================================================================

    /// Parse an object declaration
    fn parse_object_decl(&mut self) {
        self.start_node(SyntaxKind::OBJECT_DECL);

        self.expect(SyntaxKind::OBJECT_KW);
        self.skip_trivia();

        // Object name (optional for object expressions)
        if self.at(SyntaxKind::IDENT) || self.at(SyntaxKind::BACKTICK_IDENT) {
            self.bump();
            self.skip_trivia();
        }

        // Supertype list (optional): object : Bar, Baz, Interface1
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();

            self.parse_supertype_specifier();

            // Parse additional supertypes (comma-separated)
            while self.at(SyntaxKind::COMMA) {
                self.bump(); // consume comma
                self.skip_trivia();
                self.parse_supertype_specifier();
            }
        }

        self.skip_trivia();

        // Object body
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_class_body();
        }

        self.finish_node();
    }

    // ============================================================================
    // Interface Declaration
    // ============================================================================

    /// Parse an interface declaration
    fn parse_interface_decl(&mut self) {
        self.start_node(SyntaxKind::INTERFACE_DECL);

        self.expect(SyntaxKind::INTERFACE_KW);
        self.skip_trivia();

        self.parse_interface_decl_tail();

        self.finish_node();
    }

    /// Parse the common part after `interface`, shared by regular and SAM
    /// (`fun interface`) declarations.
    fn parse_interface_decl_tail(&mut self) {
        // Interface name
        self.expect_identifier_or_backtick();
        self.skip_trivia();

        // Type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_parameters();
        }

        // Supertype list (optional)
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            self.parse_type_ref();
            self.skip_trivia();

            while self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
                self.parse_type_ref();
                self.skip_trivia();
            }
        }

        self.skip_trivia();

        // Interface body
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_class_body();
        }
    }

    /// Parse a functional (SAM) interface declaration: `fun interface Name`.
    fn parse_fun_interface_decl(&mut self) {
        self.start_node(SyntaxKind::INTERFACE_DECL);

        self.expect(SyntaxKind::FUN_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::INTERFACE_KW);
        self.skip_trivia();

        self.parse_interface_decl_tail();

        self.finish_node();
    }

    // ============================================================================
    // Type References
    // ============================================================================

    /// Parse a type reference
    ///
    /// Handles both regular types and function types:
    /// - `String`, `List<Int>`, `String?` - regular types
    /// - `(String) -> Int`, `() -> Unit`, `(String, Int) -> Boolean` - function types
    /// - `String.() -> Unit` - function type with receiver
    fn parse_type_ref(&mut self) {
        // Type annotations: `content: @Composable () -> Unit`
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }
        // `suspend` is part of a function *type*, not just a declaration
        // modifier: `suspend (T) -> R`.
        if self.at(SyntaxKind::SUSPEND_KW) {
            self.parse_function_type();
        // Check if this is a function type (starts with L_PAREN)
        } else if self.at(SyntaxKind::L_PAREN) {
            if self.lookahead_is_function_type() {
                self.parse_function_type();
            } else {
                // Parenthesized type reference, e.g. `(() -> Int)!!`.
                self.start_node(SyntaxKind::TYPE_REF);
                self.expect(SyntaxKind::L_PAREN);
                self.skip_trivia();
                self.parse_type_ref();
                self.skip_trivia();
                self.expect(SyntaxKind::R_PAREN);
                self.skip_trivia();

                while self.at(SyntaxKind::NOT_NULL) {
                    self.bump();
                    self.skip_trivia();
                }

                if self.at(SyntaxKind::QUESTION) {
                    self.bump();
                }

                self.finish_node();
            }
        } else {
            self.start_node(SyntaxKind::TYPE_REF);

            // Type name
            self.parse_qualified_name();

            self.skip_trivia();

            // Type arguments (optional)
            if self.at(SyntaxKind::LT) {
                self.parse_type_arguments();
            }

            self.skip_trivia();

            // Check for receiver type in function type (e.g., String.() -> Unit).
            // This lookahead must not consume the dot speculatively: pushing a
            // token into the green builder cannot be undone by rewinding `pos`.
            if self.at(SyntaxKind::DOT) {
                let mut lookahead_pos = self.pos + 1;
                while lookahead_pos < self.tokens.len() && self.tokens[lookahead_pos].0.is_trivia()
                {
                    lookahead_pos += 1;
                }

                if self
                    .tokens
                    .get(lookahead_pos)
                    .is_some_and(|token| token.0 == SyntaxKind::L_PAREN)
                {
                    // The receiver is its own TYPE_REF; FUNCTION_TYPE consumes
                    // the dot exactly once along with `() -> ReturnType`.
                    self.finish_node();
                    self.parse_function_type();
                    return;
                }
            }

            // Non-null assertion operator (optional) - e.g., String!! in generics
            // This handles cases like List<String!!> or fun <T: String!!> test()
            while self.at(SyntaxKind::NOT_NULL) {
                self.bump();
                self.skip_trivia();
            }

            // Nullable type marker (optional)
            if self.at(SyntaxKind::QUESTION) {
                self.bump();
            }

            self.finish_node();
        }
    }

    fn lookahead_is_function_type(&self) -> bool {
        let mut depth: i32 = 0;
        let mut i = self.pos;

        while i < self.tokens.len() {
            let kind = self.tokens[i].0;
            match kind {
                SyntaxKind::L_PAREN => depth += 1,
                SyntaxKind::R_PAREN => {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        while i < self.tokens.len() {
            let kind = self.tokens[i].0;
            if kind == SyntaxKind::WHITESPACE
                || kind == SyntaxKind::LINE_COMMENT
                || kind == SyntaxKind::BLOCK_COMMENT
            {
                i += 1;
                continue;
            }
            return kind == SyntaxKind::ARROW;
        }

        false
    }

    /// Parse a function type
    ///
    /// Grammar:
    /// ```text
    /// FunctionType:
    ///     [ReceiverType.] ( [FunctionTypeParams] ) -> Type [?]
    ///
    /// FunctionTypeParams:
    ///     Type ( , Type )*
    /// ```
    ///
    /// Examples:
    /// - `() -> String` - no parameters
    /// - `(String) -> Int` - single parameter
    /// - `(String, Int) -> Boolean` - multiple parameters
    /// - `String.() -> Unit` - with receiver type
    fn parse_function_type(&mut self) {
        self.start_node(SyntaxKind::FUNCTION_TYPE);

        if self.at(SyntaxKind::SUSPEND_KW) {
            self.bump();
            self.skip_trivia();
        }

        // `suspend CoroutineScope.() -> T`: the receiver follows `suspend`
        // directly, so it has not been parsed by `parse_type_ref` yet.
        if !self.at(SyntaxKind::L_PAREN) && !self.at(SyntaxKind::DOT) {
            self.start_node(SyntaxKind::TYPE_REF);
            self.parse_qualified_name();
            self.skip_trivia();
            if self.at(SyntaxKind::LT) {
                self.parse_type_arguments();
                self.skip_trivia();
            }
            if self.at(SyntaxKind::QUESTION) {
                self.bump();
                self.skip_trivia();
            }
            self.finish_node();
        }

        // Handle optional receiver type (e.g., in `String.() -> Unit`)
        if self.at(SyntaxKind::DOT) {
            self.bump(); // consume dot
            self.skip_trivia();
        }

        // Parse parameter list
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Parse function type parameters (types only, no names)
        if !self.at(SyntaxKind::R_PAREN) {
            loop {
                // Named parameter: `(chain: Interceptor.Chain) -> Response`
                if self.at_identifier_or_soft_keyword()
                    && self.next_non_trivia_kind(self.pos + 1) == SyntaxKind::COLON
                {
                    self.bump();
                    self.skip_trivia();
                    self.bump(); // ':'
                    self.skip_trivia();
                }
                self.parse_type_ref();
                self.skip_trivia();

                if self.at(SyntaxKind::COMMA) {
                    self.bump();
                    self.skip_trivia();
                    // Trailing comma is not allowed in function types
                    if self.at(SyntaxKind::R_PAREN) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();

        // Arrow
        self.expect(SyntaxKind::ARROW);
        self.skip_trivia();

        // Return type
        self.parse_type_ref();

        // Non-null assertion on function type (optional) - e.g., (() -> String)!!
        self.skip_trivia();
        while self.at(SyntaxKind::NOT_NULL) {
            self.bump();
            self.skip_trivia();
        }

        // Nullable function type (optional)
        if self.at(SyntaxKind::QUESTION) {
            self.bump();
        }

        self.finish_node();
    }

    /// Parse type parameters
    fn parse_type_parameters(&mut self) {
        self.start_node(SyntaxKind::TYPE_PARAMETER_LIST);

        self.expect(SyntaxKind::LT);
        self.skip_trivia();

        loop {
            self.start_node(SyntaxKind::TYPE_PARAMETER);

            // Optional type-parameter modifiers: variance (`in`/`out`) and/or the
            // reification modifier (`reified`, e.g. `inline fun <reified T> f()`).
            // Kotlin's grammar allows more than one (`typeParameterModifiers` is
            // `typeParameterModifier+`), so this loops rather than accepting a
            // single token. Each iteration consumes a token before looping, so
            // the loop is self-terminating and needs no `stalled()` guard.
            while let SyntaxKind::OUT_KW | SyntaxKind::IN_KW | SyntaxKind::REIFIED_KW =
                self.current()
            {
                self.bump();
                self.skip_trivia();
            }

            // Type parameter name
            self.expect(SyntaxKind::IDENT);
            self.skip_trivia();

            // Optional type bounds (e.g., T : Comparable<T>)
            if self.at(SyntaxKind::COLON) {
                self.bump(); // Consume colon
                self.skip_trivia();
                self.parse_type_ref(); // Parse the bound type
                self.skip_trivia();
            }

            self.finish_node();

            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        self.expect(SyntaxKind::GT);

        self.finish_node();
    }

    /// Parse type arguments
    fn parse_type_arguments(&mut self) {
        self.start_node(SyntaxKind::TYPE_ARGUMENT_LIST);

        self.expect(SyntaxKind::LT);
        self.skip_trivia();

        loop {
            // Handle star projections (e.g., List<*> or Map<String, *>)
            if self.at(SyntaxKind::STAR) {
                self.bump(); // consume STAR
            } else {
                // Use-site variance projections: `Array<out T>` /
                // `MutableList<in T>`. Unlike declaration-site variance,
                // this prefix belongs to an argument, not a parameter.
                if self.at(SyntaxKind::OUT_KW) || self.at(SyntaxKind::IN_KW) {
                    self.bump();
                    self.skip_trivia();
                }
                self.parse_type_ref();
            }
            self.skip_trivia();

            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        self.expect(SyntaxKind::GT);

        self.finish_node();
    }

    // ============================================================================
    // Statements
    // ============================================================================

    /// Parse a for expression: for (variable in iterable) { body }
    fn parse_for_expr(&mut self) {
        self.start_node(SyntaxKind::FOR_EXPR);

        self.expect(SyntaxKind::FOR_KW);
        self.skip_trivia();

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Parse loop variable binding: variable in iterable
        self.start_node(SyntaxKind::FOR_LOOP_VAR);

        // Loop variable - can be destructuring (a, b) or simple identifier
        if self.at(SyntaxKind::L_PAREN) {
            // Destructuring: (a, b) in pairs
            self.bump(); // consume L_PAREN
            while !self.at(SyntaxKind::R_PAREN) && !self.at_eof() {
                self.bump();
            }
            self.eat(SyntaxKind::R_PAREN);
            self.skip_trivia();
        } else {
            // Simple variable name
            self.expect_identifier_or_soft_keyword();
            self.skip_trivia();

            // Optional type annotation: for (x: Type in items)
            if self.at(SyntaxKind::COLON) {
                self.bump();
                self.skip_trivia();
                self.parse_type_ref();
                self.skip_trivia();
            }
        }

        // "in" keyword
        self.expect(SyntaxKind::IN_KW);
        self.skip_trivia();

        // Iterable expression: a real expression, so its names are
        // references like any other (`for (x in a.b().c)`).
        self.paren_depth += 1;
        let iterable_start = self.pos;
        if !self.at(SyntaxKind::R_PAREN) {
            self.parse_expr_bp(0);
            self.skip_trivia();
        }
        self.paren_depth -= 1;
        // Recovery: never leave tokens between the expression and `)` behind.
        if self.pos == iterable_start || !self.at(SyntaxKind::R_PAREN) {
            let mut depth = 0usize;
            while !self.at_eof() {
                if self.at(SyntaxKind::L_PAREN) {
                    depth += 1;
                } else if self.at(SyntaxKind::R_PAREN) {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                self.bump();
            }
        }

        self.finish_node(); // FOR_LOOP_VAR

        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();

        // Body
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        } else if !self.at_eof() {
            // Kotlin control structures also accept a single statement body.
            let body_start = self.pos;
            if !self.parse_statement() {
                self.stalled(body_start, "for body");
            }
        }

        self.finish_node(); // FOR_EXPR
    }

    /// Parse a while expression: while (condition) { body }
    fn parse_while_expr(&mut self) {
        self.start_node(SyntaxKind::WHILE_EXPR);

        self.expect(SyntaxKind::WHILE_KW);
        self.skip_trivia();

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Condition: a real expression.
        self.paren_depth += 1;
        let condition_start = self.pos;
        if !self.at(SyntaxKind::R_PAREN) {
            self.parse_expr_bp(0);
            self.skip_trivia();
        }
        self.paren_depth -= 1;
        if self.pos == condition_start || !self.at(SyntaxKind::R_PAREN) {
            let mut depth = 0usize;
            while !self.at_eof() {
                if self.at(SyntaxKind::L_PAREN) {
                    depth += 1;
                } else if self.at(SyntaxKind::R_PAREN) {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                self.bump();
            }
        }

        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();

        // Body
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        } else if !self.at_eof() {
            // Kotlin control structures also accept a single statement body.
            let body_start = self.pos;
            if !self.parse_statement() {
                self.stalled(body_start, "while body");
            }
        }

        self.finish_node(); // WHILE_EXPR
    }

    /// Parse a do-while expression: `do { body } while (condition)`
    ///
    /// Kotlin grammar:
    /// ```text
    /// DoWhileStatement:
    ///     'do' [ControlStructureBody] 'while' '(' Expression ')'
    /// ```
    ///
    /// The body is optional in the grammar (`do while (x)` is legal), and may be
    /// either a block or a single statement (`do x() while (cond)`).
    fn parse_do_while_expr(&mut self) {
        self.start_node(SyntaxKind::DO_WHILE_EXPR);

        self.expect(SyntaxKind::DO_KW);
        self.skip_trivia();

        // Body (optional per the grammar)
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        } else if !self.at(SyntaxKind::WHILE_KW) && !self.at_eof() {
            // Single-statement body: `do x() while (cond)`
            let body_start = self.pos;
            if !self.parse_statement() {
                self.stalled(body_start, "do-while body");
            }
        }
        self.skip_trivia();

        self.expect(SyntaxKind::WHILE_KW);
        self.skip_trivia();

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Condition
        if !self.at(SyntaxKind::R_PAREN) {
            let cond_start = self.pos;
            self.paren_depth += 1;
            self.parse_expr_bp(0);
            self.paren_depth -= 1;
            self.skip_trivia();
            // The condition may be unparseable (`do {} while (val)`); make sure we
            // still advance so the enclosing block loop cannot spin.
            if self.pos == cond_start {
                self.stalled(cond_start, "do-while condition");
            }
        }

        self.expect(SyntaxKind::R_PAREN);

        self.finish_node(); // DO_WHILE_EXPR
    }

    /// Parse a block statement
    /// Parse a single statement or declaration. Used by both parse_block and parse_lambda
    /// to ensure consistent handling of all statement types.
    ///
    /// Returns `true` if a statement was parsed, `false` if the token was unrecognized
    /// (caller should handle recovery).
    fn parse_statement(&mut self) -> bool {
        // Modifiers on local declarations: `lateinit var x`, `data class D(..)`,
        // `inline fun f()`, `private val v`.
        if matches!(
            self.current(),
            SyntaxKind::LATEINIT_KW
                | SyntaxKind::DATA_KW
                | SyntaxKind::SEALED_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::ANNOTATION_KW
                | SyntaxKind::OPEN_KW
                | SyntaxKind::ABSTRACT_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PUBLIC_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::INTERNAL_KW
                | SyntaxKind::INLINE_KW
                | SyntaxKind::SUSPEND_KW
                | SyntaxKind::OPERATOR_KW
                | SyntaxKind::INFIX_KW
        ) && self.starts_declaration_after(self.pos + 1)
        {
            self.parse_modifiers();
            self.skip_trivia();
            return self.parse_statement();
        }
        // Annotated statement/expression: `@Suppress("X") foo()`, `@Volatile var x`
        if self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
            return self.parse_statement();
        }
        if self.at_label_definition() {
            self.bump(); // label name
            self.bump(); // '@'
            self.skip_trivia();
            return self.parse_statement();
        }
        match self.current() {
            // Property declarations (val/var)
            SyntaxKind::VAL_KW | SyntaxKind::VAR_KW => {
                self.parse_local_property_decl();
                self.skip_trivia();
                true
            }
            // Nested blocks / lambda expressions
            SyntaxKind::L_BRACE => {
                self.parse_lambda();
                self.skip_trivia();
                true
            }
            // Function declarations (local functions)
            SyntaxKind::FUN_KW => {
                self.parse_function_decl();
                self.skip_trivia();
                true
            }
            // Local classes are legal declarations in a function/block body.
            // Route them through the normal class parser rather than block
            // recovery, which would otherwise consume the `class` keyword and
            // corrupt the declaration's remaining braces.
            SyntaxKind::CLASS_KW => {
                self.parse_class_decl();
                self.skip_trivia();
                true
            }
            // For loops
            SyntaxKind::FOR_KW => {
                self.parse_for_expr();
                self.skip_trivia();
                true
            }
            // While loops
            SyntaxKind::WHILE_KW => {
                self.parse_while_expr();
                self.skip_trivia();
                true
            }
            // Do-while loops
            SyntaxKind::DO_KW => {
                self.parse_do_while_expr();
                self.skip_trivia();
                true
            }
            // If expression
            SyntaxKind::IF_KW => {
                let checkpoint = self.builder.checkpoint();
                self.parse_if_expr();
                self.parse_expr_loop(checkpoint, 0, true, true);
                self.skip_trivia();
                true
            }
            // When expression
            SyntaxKind::WHEN_KW => {
                let checkpoint = self.builder.checkpoint();
                self.parse_when_expr();
                self.parse_expr_loop(checkpoint, 0, true, true);
                self.skip_trivia();
                true
            }
            // Try expression
            SyntaxKind::TRY_KW => {
                let checkpoint = self.builder.checkpoint();
                self.parse_try_expr();
                self.parse_expr_loop(checkpoint, 0, true, true);
                self.skip_trivia();
                true
            }
            // Return, throw, break, continue — expression statements
            SyntaxKind::RETURN_KW
            | SyntaxKind::THROW_KW
            | SyntaxKind::BREAK_KW
            | SyntaxKind::CONTINUE_KW => {
                self.parse_expression();
                self.skip_trivia();
                true
            }
            // Expression statements (identifiers, literals, this, super, etc.)
            SyntaxKind::IDENT
            | SyntaxKind::THIS_KW
            | SyntaxKind::SUPER_KW
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::RAW_STRING_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::TRUE_KW
            | SyntaxKind::FALSE_KW
            | SyntaxKind::NULL_KW
            | SyntaxKind::L_PAREN
            | SyntaxKind::BANG
            | SyntaxKind::MINUS
            | SyntaxKind::PLUS
            | SyntaxKind::PLUSPLUS
            | SyntaxKind::MINUSMINUS
            | SyntaxKind::OBJECT_KW => {
                self.start_node(SyntaxKind::EXPR_STMT);
                self.parse_expression();
                self.finish_node();
                self.skip_trivia();
                true
            }
            // Soft keywords used as plain identifiers: `out.write(x)`, `data.size`.
            // A modifier followed by a declaration keyword is not an expression.
            _ if self.at_identifier_or_soft_keyword()
                && !matches!(
                    self.next_non_trivia_kind(self.pos + 1),
                    SyntaxKind::CLASS_KW
                        | SyntaxKind::FUN_KW
                        | SyntaxKind::VAL_KW
                        | SyntaxKind::VAR_KW
                        | SyntaxKind::OBJECT_KW
                        | SyntaxKind::INTERFACE_KW
                ) =>
            {
                self.start_node(SyntaxKind::EXPR_STMT);
                self.parse_expression();
                self.finish_node();
                self.skip_trivia();
                true
            }
            // Skip semicolons (optional in Kotlin)
            SyntaxKind::SEMICOLON => {
                self.bump();
                self.skip_trivia();
                true
            }
            // Unknown token — caller handles recovery
            _ => false,
        }
    }

    /// Check if the current token starts a statement (for error recovery synchronization).
    fn at_statement_start(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::VAL_KW
                | SyntaxKind::VAR_KW
                | SyntaxKind::FUN_KW
                | SyntaxKind::CLASS_KW
                | SyntaxKind::OBJECT_KW
                | SyntaxKind::INTERFACE_KW
                | SyntaxKind::IF_KW
                | SyntaxKind::FOR_KW
                | SyntaxKind::WHILE_KW
                | SyntaxKind::WHEN_KW
                | SyntaxKind::TRY_KW
                | SyntaxKind::RETURN_KW
                | SyntaxKind::THROW_KW
                | SyntaxKind::BREAK_KW
                | SyntaxKind::CONTINUE_KW
                | SyntaxKind::DO_KW
        )
    }

    fn parse_block(&mut self) {
        let outer_paren_depth = std::mem::replace(&mut self.paren_depth, 0);
        self.start_node(SyntaxKind::BLOCK);

        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let stmt_start = self.pos;
            if !self.parse_statement() {
                // Error recovery: skip to next statement keyword or closing brace.
                // This prevents one syntax error from corrupting the rest of the block.
                self.start_node(SyntaxKind::ERROR);
                while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() && !self.at_statement_start()
                {
                    self.bump();
                }
                self.finish_node();
            }
            // The recovery scan above stops at statement-start keywords, so a
            // keyword that `parse_statement` does not handle would consume
            // nothing and spin here forever.
            self.stalled(stmt_start, "block statement");
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
        self.paren_depth = outer_paren_depth;
    }

    // ============================================================================
    // Expressions (Pratt Parser)
    // ============================================================================

    /// Get infix operator binding power (precedence)
    /// Returns (left_bp, right_bp) or None if not an infix operator
    /// Higher values = tighter binding
    const fn infix_binding_power(kind: SyntaxKind) -> Option<(u8, u8)> {
        match kind {
            // Assignment (right-associative)
            SyntaxKind::EQ
            | SyntaxKind::PLUSEQ
            | SyntaxKind::MINUSEQ
            | SyntaxKind::STAREQ
            | SyntaxKind::SLASHEQ
            | SyntaxKind::PERCENTEQ => Some((2, 1)),

            // Disjunction (or)
            SyntaxKind::PIPEPIPE => Some((3, 4)),

            // Conjunction (and)
            SyntaxKind::AMPAMP => Some((5, 6)),

            // Equality
            SyntaxKind::EQEQ | SyntaxKind::BANGEQ | SyntaxKind::EQEQEQ | SyntaxKind::BANGEQEQ => {
                Some((7, 8))
            }

            // Comparison, in/is/as checks and type casts
            SyntaxKind::LT
            | SyntaxKind::GT
            | SyntaxKind::LTEQ
            | SyntaxKind::GTEQ
            | SyntaxKind::IN_KW
            | SyntaxKind::IS_KW
            | SyntaxKind::AS_KW
            | SyntaxKind::SAFE_CAST
            | SyntaxKind::NOT_IN
            | SyntaxKind::NOT_IS => Some((9, 10)),

            // Elvis operator (?:)
            SyntaxKind::ELVIS => Some((11, 12)),

            // Range operators
            SyntaxKind::RANGE | SyntaxKind::RANGE_UNTIL => Some((13, 14)),

            // Additive
            SyntaxKind::PLUS | SyntaxKind::MINUS => Some((15, 16)),

            // Multiplicative
            SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => Some((17, 18)),

            // Member access (highest precedence for infix)
            SyntaxKind::DOT | SyntaxKind::SAFE_CALL => Some((21, 22)),

            _ => None,
        }
    }

    /// Get prefix operator binding power
    const fn prefix_binding_power(kind: SyntaxKind) -> Option<u8> {
        match kind {
            // `*` is the spread operator in argument position: `f(*args)`.
            SyntaxKind::BANG
            | SyntaxKind::MINUS
            | SyntaxKind::PLUS
            | SyntaxKind::STAR
            | SyntaxKind::PLUSPLUS
            | SyntaxKind::MINUSMINUS => Some(19),
            _ => None,
        }
    }

    /// Get postfix operator binding power
    const fn postfix_binding_power(kind: SyntaxKind) -> Option<u8> {
        match kind {
            // Postfix: !!, function call, array access, trailing lambda, ++, --
            SyntaxKind::NOT_NULL
            | SyntaxKind::L_PAREN
            | SyntaxKind::L_BRACKET
            | SyntaxKind::L_BRACE
            | SyntaxKind::PLUSPLUS
            | SyntaxKind::MINUSMINUS => Some(20),
            _ => None,
        }
    }

    /// Parse an expression using Pratt parsing with proper operator precedence
    fn parse_expression(&mut self) {
        self.parse_expr_bp(0);
    }

    /// Parse expression with binding power (Pratt parser core)
    fn parse_expr_bp(&mut self, min_bp: u8) {
        self.parse_expr_bp_with_trailing_lambda(min_bp, true);
    }

    /// Parse an expression at one delimiter level.
    ///
    /// `allow_trailing_lambda` is threaded through Pratt recursion because
    /// those recursive calls are an implementation detail of the same
    /// unparenthesized expression. Nested delimiter contexts call
    /// `parse_expr_bp` and therefore restore normal Kotlin trailing-lambda
    /// parsing.
    fn parse_expr_bp_with_trailing_lambda(&mut self, min_bp: u8, allow_trailing_lambda: bool) {
        // Handle prefix operators and primary expressions
        let checkpoint = self.builder.checkpoint();

        // Annotated expression: `else @Suppress("DEPRECATION") Foo.BAR`
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }

        // Check for prefix operators
        if let Some(prefix_bp) = Self::prefix_binding_power(self.current()) {
            self.start_node(SyntaxKind::UNARY_EXPR);
            self.bump(); // consume operator
            self.skip_trivia();
            self.parse_expr_bp_with_trailing_lambda(prefix_bp, allow_trailing_lambda);
            self.finish_node();
        } else {
            // Parse primary expression (atoms)
            self.parse_primary_expr();
        }

        self.parse_expr_loop(checkpoint, min_bp, allow_trailing_lambda, false);
    }

    /// The infix/postfix half of the Pratt parser, applied to whatever was
    /// parsed since `checkpoint`.
    ///
    /// With `require_dotted_start` it only continues an expression that was
    /// already complete when the loop was entered if the next token is `.`,
    /// `?.` or `?:` - the operators Kotlin lets a statement continue with on the
    /// next line (`when { .. }?.let { .. }`, `try { .. }.also { .. }`).
    fn parse_expr_loop(
        &mut self,
        checkpoint: rowan::Checkpoint,
        min_bp: u8,
        allow_trailing_lambda: bool,
        mut require_dotted_start: bool,
    ) {
        // Handle infix and postfix operators
        loop {
            if require_dotted_start {
                require_dotted_start = false;
                if !matches!(
                    self.next_non_trivia_kind(self.pos),
                    SyntaxKind::DOT | SyntaxKind::SAFE_CALL | SyntaxKind::ELVIS
                ) {
                    break;
                }
            }
            if self.when_entry_result_depth > 0
                && (self.trivia_has_newline_before(SyntaxKind::IS_KW)
                    || self.trivia_has_newline_before(SyntaxKind::NOT_IS)
                    || self.trivia_has_newline_before(SyntaxKind::ELSE_KW))
            {
                break;
            }
            let newline_before_next = self.paren_depth == 0
                && (self.newline_since_last_significant_token || self.current_trivia_has_newline());
            self.skip_trivia();

            // Class literals and callable references: `T::class` /
            // `String::length`. Treat the reference as a postfix member-like
            // expression so a following `.simpleName` continues through the
            // normal member-access path. This arm always consumes `::` (and,
            // when present, its target), so the Pratt loop keeps its progress
            // invariant.
            if self.at(SyntaxKind::COLONCOLON) {
                self.builder.start_node_at(
                    checkpoint,
                    KotlinLanguage::kind_to_raw(SyntaxKind::MEMBER_ACCESS_EXPR),
                );
                self.bump();
                self.skip_trivia();
                if self.at(SyntaxKind::CLASS_KW) || self.at_identifier_or_soft_keyword() {
                    self.bump();
                } else {
                    self.error(
                        format!(
                            "Expected callable reference target after '::', found {:?}",
                            self.current()
                        ),
                        self.current_range(),
                    );
                }
                self.finish_node();
                continue;
            }

            // A balanced `<...>` immediately followed by `(` belongs to the
            // callee, not to the comparison-operator grammar.
            if self.at_generic_call_suffix(allow_trailing_lambda) == Some(SyntaxKind::COLONCOLON) {
                self.parse_type_arguments();
                continue;
            }
            if let Some(next) = self.at_generic_call_suffix(allow_trailing_lambda) {
                self.builder.start_node_at(
                    checkpoint,
                    KotlinLanguage::kind_to_raw(SyntaxKind::CALL_EXPR),
                );
                self.parse_type_arguments();
                self.skip_trivia();
                if next == SyntaxKind::L_BRACE {
                    self.parse_lambda();
                } else {
                    self.parse_call_args(allow_trailing_lambda);
                }
                self.finish_node();
                continue;
            }

            // Check for postfix operators first
            if let Some(postfix_bp) = Self::postfix_binding_power(self.current()) {
                if postfix_bp < min_bp {
                    break;
                }

                // `(` and `[` on a new line start a new statement rather than
                // continuing the previous expression.
                if newline_before_next
                    && matches!(self.current(), SyntaxKind::L_PAREN | SyntaxKind::L_BRACKET)
                {
                    break;
                }

                match self.current() {
                    SyntaxKind::NOT_NULL => {
                        // !! postfix
                        self.builder.start_node_at(
                            checkpoint,
                            KotlinLanguage::kind_to_raw(SyntaxKind::UNARY_EXPR),
                        );
                        self.bump();
                        self.finish_node();
                    }
                    SyntaxKind::PLUSPLUS | SyntaxKind::MINUSMINUS => {
                        // i++ / i-- postfix
                        self.builder.start_node_at(
                            checkpoint,
                            KotlinLanguage::kind_to_raw(SyntaxKind::UNARY_EXPR),
                        );
                        self.bump();
                        self.finish_node();
                    }
                    SyntaxKind::L_PAREN => {
                        // Function call
                        self.builder.start_node_at(
                            checkpoint,
                            KotlinLanguage::kind_to_raw(SyntaxKind::CALL_EXPR),
                        );
                        self.parse_call_args(allow_trailing_lambda);
                        self.finish_node();
                    }
                    SyntaxKind::L_BRACKET => {
                        // Array access
                        self.builder.start_node_at(
                            checkpoint,
                            KotlinLanguage::kind_to_raw(SyntaxKind::ARRAY_ACCESS_EXPR),
                        );
                        self.bump(); // [
                        self.skip_trivia();
                        self.paren_depth += 1;
                        // `map["a"]`, `matrix[x, y]`, `instance["cls", "field"]`
                        loop {
                            self.parse_expr_bp(0);
                            self.skip_trivia();
                            if self.at(SyntaxKind::COMMA) {
                                self.bump();
                                self.skip_trivia();
                                if self.at(SyntaxKind::R_BRACKET) {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        self.paren_depth -= 1;
                        self.eat(SyntaxKind::R_BRACKET);
                        self.finish_node();
                    }
                    SyntaxKind::L_BRACE => {
                        if !allow_trailing_lambda {
                            break;
                        }
                        // Trailing lambda call (e.g., list.map { x -> x * 2 })
                        self.builder.start_node_at(
                            checkpoint,
                            KotlinLanguage::kind_to_raw(SyntaxKind::CALL_EXPR),
                        );
                        self.parse_lambda();
                        self.finish_node();
                    }
                    _ => break,
                }
                continue;
            }

            // Kotlin's `infixFunctionCall` grammar is
            // `rangeExpression (simpleIdentifier rangeExpression)*`. It binds
            // below ranges (13) and above Elvis (11) and `in`/`is` checks (9).
            // Its right binding power makes a chain left-associative:
            // `1 to 2 to 3` becomes `(1 to 2) to 3`.
            //
            // The grammar does not allow a newline before the infix name, so
            // never absorb an identifier beginning the following statement.
            // `by lazy @SuppressLint("x") { ... }`: an annotated trailing lambda.
            if self.at(SyntaxKind::AT) && !newline_before_next && allow_trailing_lambda {
                self.builder.start_node_at(
                    checkpoint,
                    KotlinLanguage::kind_to_raw(SyntaxKind::CALL_EXPR),
                );
                while self.at(SyntaxKind::AT) {
                    self.parse_annotation();
                    self.skip_trivia();
                }
                if self.at(SyntaxKind::L_BRACE) {
                    self.parse_lambda();
                }
                self.finish_node();
                continue;
            }

            // `forEach loop@{ ... }`: a labelled trailing lambda, not an infix call.
            if !newline_before_next
                && allow_trailing_lambda
                && self.at_label_definition()
                && self.next_non_trivia_kind(self.pos + 2) == SyntaxKind::L_BRACE
            {
                self.builder.start_node_at(
                    checkpoint,
                    KotlinLanguage::kind_to_raw(SyntaxKind::CALL_EXPR),
                );
                self.bump();
                self.bump();
                self.skip_trivia();
                self.parse_lambda();
                self.finish_node();
                continue;
            }

            const NAMED_INFIX_L_BP: u8 = 12;
            const NAMED_INFIX_R_BP: u8 = 13;
            if !newline_before_next && self.at_identifier_or_soft_keyword() {
                if NAMED_INFIX_L_BP < min_bp {
                    break;
                }

                self.builder.start_node_at(
                    checkpoint,
                    KotlinLanguage::kind_to_raw(SyntaxKind::BINARY_EXPR),
                );
                self.bump(); // simpleIdentifier infix function name
                self.skip_trivia();
                self.parse_expr_bp_with_trailing_lambda(NAMED_INFIX_R_BP, allow_trailing_lambda);
                self.finish_node();
                continue;
            }

            // Check for infix operators
            if let Some((l_bp, r_bp)) = Self::infix_binding_power(self.current()) {
                if l_bp < min_bp {
                    break;
                }

                let op = self.current();

                // Choose appropriate node type
                let node_kind = match op {
                    SyntaxKind::DOT => SyntaxKind::MEMBER_ACCESS_EXPR,
                    SyntaxKind::SAFE_CALL => SyntaxKind::SAFE_ACCESS_EXPR,
                    SyntaxKind::AS_KW | SyntaxKind::SAFE_CAST => SyntaxKind::AS_EXPR,
                    _ => SyntaxKind::BINARY_EXPR,
                };

                self.builder
                    .start_node_at(checkpoint, KotlinLanguage::kind_to_raw(node_kind));
                self.bump(); // consume operator
                self.skip_trivia();
                // The right operand of a cast or type test is a *type*, so
                // `x as B?` / `x as List<String>` / `x is Map<*, *>?` must not
                // go through expression parsing. A plain `x is Foo` keeps its
                // bare identifier tokens, which the scope visitor and smart-cast
                // analysis match on directly.
                if matches!(op, SyntaxKind::AS_KW | SyntaxKind::SAFE_CAST)
                    || (matches!(op, SyntaxKind::IS_KW | SyntaxKind::NOT_IS)
                        && self.type_operand_needs_type_ref())
                {
                    self.parse_type_ref();
                } else {
                    self.parse_expr_bp_with_trailing_lambda(r_bp, allow_trailing_lambda);
                }
                self.finish_node();
            } else {
                break;
            }
        }
    }

    /// Parse a primary expression (atoms: literals, identifiers, parenthesized, lambdas)
    fn parse_primary_expr(&mut self) {
        match self.current() {
            // Literals - wrap in LITERAL_EXPR for type inference
            SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::RAW_STRING_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::TRUE_KW
            | SyntaxKind::FALSE_KW
            | SyntaxKind::NULL_KW => {
                self.start_node(SyntaxKind::LITERAL_EXPR);
                self.bump();
                self.finish_node();
            }

            // Labelled expression: `foo@ { ... }`
            SyntaxKind::IDENT if self.at_label_definition() => {
                self.bump();
                self.bump();
                self.skip_trivia();
                self.parse_primary_expr();
            }

            // Identifier (variable reference)
            SyntaxKind::IDENT => {
                self.bump();
            }

            // `this@Outer` / `super@Outer` labelled receivers. The `@Label`
            // must be consumed as part of the primary expression; otherwise
            // class-member parsing mistakes it for an annotation and can eat
            // the enclosing class's closing brace during recovery.
            SyntaxKind::THIS_KW | SyntaxKind::SUPER_KW => {
                let is_super = self.at(SyntaxKind::SUPER_KW);
                self.bump();
                // `super<Interface>.member()`
                if is_super && self.at(SyntaxKind::LT) {
                    self.parse_type_arguments();
                }
                if self.at(SyntaxKind::AT) {
                    self.bump();
                    if self.at_identifier_or_soft_keyword() {
                        self.bump();
                    } else {
                        self.error(
                            format!(
                                "Expected label after {:?}, found {:?}",
                                SyntaxKind::AT,
                                self.current()
                            ),
                            self.current_range(),
                        );
                    }
                }
            }

            // Parenthesized expression
            SyntaxKind::L_PAREN => {
                self.start_node(SyntaxKind::PAREN_EXPR);
                self.bump(); // (
                self.skip_trivia();
                self.paren_depth += 1;
                self.parse_expr_bp(0);
                self.skip_trivia();
                self.paren_depth -= 1;
                self.eat(SyntaxKind::R_PAREN);
                self.finish_node();
            }

            // Lambda expression
            SyntaxKind::L_BRACE => {
                self.parse_lambda();
            }

            // if expression
            SyntaxKind::IF_KW => {
                self.parse_if_expr();
            }

            // when expression
            SyntaxKind::WHEN_KW => {
                self.parse_when_expr();
            }

            // return/throw/break/continue
            SyntaxKind::RETURN_KW => {
                self.start_node(SyntaxKind::RETURN_EXPR);
                self.bump();
                self.eat_jump_label();
                self.skip_trivia();
                // Optional return value
                if !self.at_expression_end() {
                    self.parse_expr_bp(0);
                }
                self.finish_node();
            }
            SyntaxKind::THROW_KW => {
                self.start_node(SyntaxKind::THROW_EXPR);
                self.bump();
                self.skip_trivia();
                self.parse_expr_bp(0);
                self.finish_node();
            }
            SyntaxKind::BREAK_KW => {
                self.start_node(SyntaxKind::BREAK_EXPR);
                self.bump();
                self.eat_jump_label();
                self.finish_node();
            }
            SyntaxKind::CONTINUE_KW => {
                self.start_node(SyntaxKind::CONTINUE_EXPR);
                self.bump();
                self.eat_jump_label();
                self.finish_node();
            }

            // Try expression
            SyntaxKind::TRY_KW => {
                self.parse_try_expr();
            }

            // do-while used in expression position (e.g. as a when-entry body)
            SyntaxKind::DO_KW => {
                self.parse_do_while_expr();
            }

            // Object literal / anonymous object expression:
            // `object : SomeInterface { ... }` or bare `object { ... }`.
            // `parse_object_decl` already supports an optional name, making it
            // reusable here for the unnamed expression form.
            SyntaxKind::OBJECT_KW => {
                self.parse_object_decl();
            }

            // Soft keywords used as identifiers (`out`, `data`, `enum`, ...)
            _ if self.at_identifier_or_soft_keyword() => {
                self.bump();
            }

            // For unknown tokens, just consume
            _ => {
                if !self.at_expression_end() && !self.at_eof() {
                    self.bump();
                }
            }
        }
    }

    /// `return@label`, `break@loop`, `continue@outer`: the `@label` directly
    /// after the keyword.
    fn eat_jump_label(&mut self) {
        if self.at(SyntaxKind::AT) {
            self.bump();
            if self.at_identifier_or_soft_keyword() {
                self.bump();
            }
        }
    }

    /// `loop@ for (...)`, `outer@ while`, `lit@{ ... }`: an identifier
    /// immediately followed by `@` defines a label.
    fn at_label_definition(&self) -> bool {
        self.at_identifier_or_soft_keyword()
            && self.tokens.get(self.pos + 1).map(|t| t.0) == Some(SyntaxKind::AT)
    }

    /// Parse a try expression: `try { } catch (e: Exception) { } finally { }`
    fn parse_try_expr(&mut self) {
        self.start_node(SyntaxKind::TRY_EXPR);

        self.expect(SyntaxKind::TRY_KW);
        self.skip_trivia();

        // Try body
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        }
        self.skip_trivia();

        // Catch clauses (zero or more)
        while self.at(SyntaxKind::CATCH_KW) {
            self.start_node(SyntaxKind::CATCH_CLAUSE);
            self.bump(); // consume 'catch'
            self.skip_trivia();

            // Parameter: (e: Exception)
            if self.at(SyntaxKind::L_PAREN) {
                self.bump(); // (
                self.skip_trivia();

                // Parameter name
                if self.at(SyntaxKind::IDENT) {
                    self.start_node(SyntaxKind::PARAMETER);
                    self.bump(); // parameter name
                    self.skip_trivia();

                    // Type annotation
                    if self.at(SyntaxKind::COLON) {
                        self.bump();
                        self.skip_trivia();
                        self.parse_type_ref();
                        self.skip_trivia();
                    }
                    self.finish_node(); // PARAMETER
                }

                self.eat(SyntaxKind::R_PAREN);
                self.skip_trivia();
            }

            // Catch body
            if self.at(SyntaxKind::L_BRACE) {
                self.parse_block();
            }

            self.finish_node(); // CATCH_CLAUSE
            self.skip_trivia();
        }

        // Finally clause (optional)
        if self.at(SyntaxKind::FINALLY_KW) {
            self.start_node(SyntaxKind::FINALLY_CLAUSE);
            self.bump(); // consume 'finally'
            self.skip_trivia();

            if self.at(SyntaxKind::L_BRACE) {
                self.parse_block();
            }

            self.finish_node(); // FINALLY_CLAUSE
        }

        self.finish_node(); // TRY_EXPR
    }

    /// Check if current position is at an expression terminator
    fn at_expression_end(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::SEMICOLON
                | SyntaxKind::R_PAREN
                | SyntaxKind::R_BRACE
                | SyntaxKind::R_BRACKET
                | SyntaxKind::COMMA
                | SyntaxKind::EOF
                | SyntaxKind::VAL_KW
                | SyntaxKind::VAR_KW
                | SyntaxKind::FUN_KW
                | SyntaxKind::CLASS_KW
        )
    }

    /// Parse function call arguments
    fn parse_call_args(&mut self, allow_trailing_lambda: bool) {
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.paren_depth += 1;

        while !self.at(SyntaxKind::R_PAREN) && !self.at_eof() {
            self.parse_expr_bp(0);
            self.skip_trivia();

            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        self.paren_depth -= 1;
        self.eat(SyntaxKind::R_PAREN);

        // Check for a trailing lambda without consuming trivia that belongs to
        // the outer Pratt loop. In particular, a newline after `call()` must
        // remain visible there so identifier-infix parsing cannot absorb the
        // first identifier of the following statement. Kotlin does allow a
        // trailing lambda after intervening newlines, so consume trivia only
        // once non-consuming lookahead confirms the lambda.
        if allow_trailing_lambda && self.next_non_trivia_kind(self.pos) == SyntaxKind::L_BRACE {
            self.skip_trivia();
            self.parse_lambda();
        }
    }

    /// Parse if expression
    fn parse_if_expr(&mut self) {
        self.start_node(SyntaxKind::IF_EXPR);

        self.expect(SyntaxKind::IF_KW);
        self.skip_trivia();

        // Condition
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.paren_depth += 1;
        self.parse_expr_bp(0);
        self.skip_trivia();
        self.paren_depth -= 1;
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();

        // Then branch
        self.parse_if_branch();

        self.skip_trivia();

        // Optional else branch. `else ->` is the next `when` entry, not this
        // `if`'s else.
        if self.at(SyntaxKind::ELSE_KW)
            && self.next_non_trivia_kind(self.pos + 1) != SyntaxKind::ARROW
        {
            self.bump();
            self.skip_trivia();
            self.parse_if_branch();
        }

        self.finish_node();
    }

    /// A branch of `if`: a block, a loop statement (`else for (..) ..`) or an
    /// expression.
    fn parse_if_branch(&mut self) {
        match self.current() {
            SyntaxKind::L_BRACE => self.parse_block(),
            SyntaxKind::FOR_KW => self.parse_for_expr(),
            SyntaxKind::WHILE_KW => self.parse_while_expr(),
            SyntaxKind::DO_KW => self.parse_do_while_expr(),
            _ => self.parse_expr_bp(0),
        }
    }

    /// Parse when expression
    fn parse_when_expr(&mut self) {
        self.start_node(SyntaxKind::WHEN_EXPR);

        self.expect(SyntaxKind::WHEN_KW);
        self.skip_trivia();

        // Optional subject: `when (expr)` or `when (val x = expr)`
        if self.at(SyntaxKind::L_PAREN) {
            self.bump();
            self.skip_trivia();

            self.paren_depth += 1;
            if self.at(SyntaxKind::VAL_KW) || self.at(SyntaxKind::VAR_KW) {
                self.parse_when_subject_binding();
            } else if !self.at(SyntaxKind::R_PAREN) {
                self.parse_expr_bp(0);
            }
            self.paren_depth -= 1;

            self.skip_trivia();
            self.expect(SyntaxKind::R_PAREN);
            self.skip_trivia();
        }
        let outer_paren_depth = std::mem::replace(&mut self.paren_depth, 0);

        // When body
        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            // Kotlin permits semicolons as separators between when entries.
            // Consuming one here is guaranteed progress, so it needs no stalled()
            // recovery path of its own.
            if self.eat(SyntaxKind::SEMICOLON) {
                self.skip_trivia();
                continue;
            }

            let entry_start = self.pos;
            self.parse_when_entry();
            self.skip_trivia();
            // A when entry that consumes nothing (unparseable condition and no
            // arrow) would otherwise spin forever.
            self.stalled(entry_start, "when entry");
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
        self.paren_depth = outer_paren_depth;
    }

    /// Parse a bound `when` subject: `when (val x = expr)` / `when (var x: T = expr)`.
    ///
    /// This is a full property declaration scoped to the when expression, so it
    /// is represented with the same `PROPERTY_DECL` node used elsewhere. It is
    /// deliberately *not* routed through `parse_local_property_decl`, whose
    /// initializer scan is delimited by newlines and statement keywords rather
    /// than by the subject's closing parenthesis.
    fn parse_when_subject_binding(&mut self) {
        self.start_node(SyntaxKind::PROPERTY_DECL);

        self.bump(); // val / var
        self.skip_trivia();

        self.expect_identifier_or_soft_keyword();
        self.skip_trivia();

        // Optional type annotation
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            self.parse_type_ref();
            self.skip_trivia();
        }

        // Initializer
        if self.at(SyntaxKind::EQ) {
            self.bump();
            self.skip_trivia();
            if !self.at(SyntaxKind::R_PAREN) {
                self.parse_expr_bp(0);
            }
        }

        self.finish_node(); // PROPERTY_DECL
    }

    /// Parse a when entry
    fn parse_when_entry(&mut self) {
        self.start_node(SyntaxKind::WHEN_ENTRY);

        // `@Suppress("DEPRECATION") TYPE_INT -> ...`
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }

        // Conditions (comma-separated) or 'else'
        if self.at(SyntaxKind::ELSE_KW) {
            self.bump();
        } else {
            // Parse conditions until ->
            while !self.at(SyntaxKind::ARROW) && !self.at_eof() && !self.at(SyntaxKind::R_BRACE) {
                self.parse_when_condition();
                self.skip_trivia();

                if self.at(SyntaxKind::COMMA) {
                    self.bump();
                    self.skip_trivia();
                } else {
                    break;
                }
            }
        }

        self.skip_trivia();
        self.expect(SyntaxKind::ARROW);
        self.skip_trivia();

        // Result
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        } else {
            self.when_entry_result_depth += 1;
            self.parse_expr_bp(0);
            self.when_entry_result_depth -= 1;
        }

        self.finish_node();
    }

    /// Parse a single `when` condition.
    ///
    /// Kotlin allows three forms: `is Type`, `in Range`, and a plain expression
    /// (plus the negated `!is` / `!in`). The Pratt parser treats `is`/`in` as
    /// *infix* operators, so it cannot parse them in the leading position a when
    /// condition puts them in; without this the `is` token was consumed as a
    /// bare atom and the following type name was left to collide with the
    /// expected `->`.
    fn parse_when_condition(&mut self) {
        match self.current() {
            SyntaxKind::IS_KW | SyntaxKind::NOT_IS => {
                self.bump();
                self.skip_trivia();
                self.parse_when_is_type();
            }
            SyntaxKind::IN_KW | SyntaxKind::NOT_IN => {
                self.bump();
                self.skip_trivia();
                self.parse_expr_bp(0);
            }
            _ => {
                self.parse_expr_bp(0);
            }
        }
    }

    /// Consume the type of an `is`/`!is` when condition.
    ///
    /// The tokens are emitted *flat* into the enclosing `WHEN_ENTRY` rather than
    /// wrapped in a `TYPE_REF`, because smart-cast analysis
    /// (`smart_cast::extract_when_entry_is_type`) scans the entry's direct
    /// children for `IS_KW` followed by `IDENT`.
    fn parse_when_is_type(&mut self) {
        if !self.at_identifier_or_soft_keyword() {
            let range = self.current_range();
            self.error(
                format!("Expected type after 'is', found {:?}", self.current()),
                range,
            );
            return;
        }
        self.bump();

        // Qualified names, generic arguments and nullability markers, consumed
        // without intervening trivia so that `is A -> 1` stops at the arrow.
        loop {
            match self.current() {
                SyntaxKind::DOT => {
                    self.bump();
                    if self.at_identifier_or_soft_keyword() {
                        self.bump();
                    } else {
                        break;
                    }
                }
                SyntaxKind::QUESTION => self.bump(),
                SyntaxKind::LT => {
                    // Balanced type-argument list.
                    let mut depth = 0usize;
                    while !self.at_eof() {
                        match self.current() {
                            SyntaxKind::LT => {
                                depth += 1;
                                self.bump();
                            }
                            SyntaxKind::GT => {
                                depth -= 1;
                                self.bump();
                                if depth == 0 {
                                    break;
                                }
                            }
                            // Never run past the end of the condition.
                            SyntaxKind::ARROW
                            | SyntaxKind::R_BRACE
                            | SyntaxKind::L_BRACE
                            | SyntaxKind::SEMICOLON => break,
                            _ => self.bump(),
                        }
                    }
                    if depth != 0 {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    /// Parse a lambda expression: { [params ->] body }
    /// Supports:
    /// - { } - no params, implicit 'it'
    /// - { -> body } - explicit no params
    /// - { x -> body } - single param
    /// - { x, y -> body } - multiple params
    /// - { x: Int -> body } - typed param
    /// - { x: Int, y: String -> body } - multiple typed params
    fn parse_lambda(&mut self) {
        let outer_paren_depth = std::mem::replace(&mut self.paren_depth, 0);
        self.start_node(SyntaxKind::LAMBDA_EXPR);

        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        // Check if there's an arrow ahead to detect lambda parameters
        if self.has_lambda_arrow_ahead() {
            self.parse_lambda_params();
        }

        // Lambda body — uses shared parse_statement for consistent handling
        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let stmt_start = self.pos;
            if !self.parse_statement() {
                // Error recovery: skip to next statement keyword or closing brace
                self.start_node(SyntaxKind::ERROR);
                while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() && !self.at_statement_start()
                {
                    self.bump();
                }
                self.finish_node();
            }
            // Same missed-consumption hazard as `parse_block`.
            self.stalled(stmt_start, "lambda body statement");
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
        self.paren_depth = outer_paren_depth;
    }

    /// Check if there's an arrow (->) ahead without nested braces
    /// This helps detect lambda parameters vs lambda body
    fn has_lambda_arrow_ahead(&self) -> bool {
        // Lambda parameters are `name[: Type]` items, `(a, b)` destructuring and
        // commas. Any other token before the first `->` (a declaration keyword,
        // `=`, a literal, a nested block...) means the arrow belongs to something
        // else, e.g. the function type in `{ val f: (Int) -> Unit = .. }`.
        let mut depth = 0usize;
        for i in self.pos..self.tokens.len() {
            let (kind, _, _) = &self.tokens[i];
            match kind {
                SyntaxKind::ARROW if depth == 0 => return true,
                SyntaxKind::L_PAREN => depth += 1,
                SyntaxKind::R_PAREN => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                SyntaxKind::COMMA
                | SyntaxKind::COLON
                | SyntaxKind::DOT
                | SyntaxKind::LT
                | SyntaxKind::GT
                | SyntaxKind::QUESTION
                | SyntaxKind::STAR
                | SyntaxKind::AT
                | SyntaxKind::IDENT
                | SyntaxKind::BACKTICK_IDENT
                | SyntaxKind::IN_KW
                | SyntaxKind::OUT_KW => {}
                // a function-typed parameter: `{ f: (Int) -> Unit -> .. }`
                SyntaxKind::ARROW => {}
                k if k.is_trivia() => {}
                k if self.token_is_soft_keyword(*k) => {}
                _ => return false,
            }
        }
        false
    }

    /// Parse lambda parameters: x, y -> or x: Int, y: String ->
    fn parse_lambda_params(&mut self) {
        self.start_node(SyntaxKind::LAMBDA_PARAMETERS);

        // Handle explicit empty params: { -> body }
        if self.at(SyntaxKind::ARROW) {
            self.bump();
            self.skip_trivia();
            self.finish_node();
            return;
        }

        // Parse parameters until we hit arrow
        loop {
            // Destructuring parameter: `{ (key, value) -> ... }`
            if self.at(SyntaxKind::L_PAREN) {
                self.bump();
                self.skip_trivia();
                while !self.at(SyntaxKind::R_PAREN) && !self.at_eof() {
                    self.bump();
                    self.skip_trivia();
                }
                self.eat(SyntaxKind::R_PAREN);
                self.skip_trivia();
            }
            // Parameter name (soft keywords such as `out` included)
            if self.at_identifier_or_soft_keyword() {
                self.bump();
                self.skip_trivia();

                // Optional type annotation
                if self.at(SyntaxKind::COLON) {
                    self.bump();
                    self.skip_trivia();
                    self.parse_type_ref();
                    self.skip_trivia();
                }
            }

            // Comma for next parameter or arrow to end
            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else if self.at(SyntaxKind::ARROW) {
                self.bump();
                self.skip_trivia();
                break;
            } else {
                // Unexpected token, just break
                break;
            }
        }

        self.finish_node();
    }
}

/// Parse result containing the green node and any errors
pub struct ParseTree {
    pub green_node: GreenNode,
    pub errors: Vec<ParseError>,
}

/// Convenience function to parse Kotlin source code
///
/// This function combines lexing and parsing in one call.
///
/// # Arguments
///
/// * `source` - Kotlin source code as a string
///
/// # Returns
///
/// A ParseTree containing the green node and any parse errors
pub fn parse(source: &str) -> ParseTree {
    let tokens = crate::lexer::Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    ParseTree { green_node, errors }
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
        assert!(errors.is_empty());
        assert_eq!(
            green.kind(),
            KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    /// The CST must be lossless: the concatenated token text of the green tree
    /// must equal the original source. A speculative bump during the annotation
    /// use-site-target probe previously duplicated the annotation name, so
    /// `@Deprecated fun foo() {}` produced `@Deprecated Deprecated fun foo() {}`.
    fn assert_lossless(source: &str) {
        let tree = crate::parser::parse(source);
        let root = crate::SyntaxNode::new_root(tree.green_node);
        assert_eq!(
            root.text().to_string(),
            source,
            "green tree text must round-trip the source exactly"
        );
    }

    #[test]
    fn test_annotation_without_use_site_target_is_lossless() {
        assert_lossless("@Deprecated fun foo() {}");
    }

    #[test]
    fn test_annotation_with_use_site_target_is_lossless() {
        assert_lossless("@field:Deprecated val x = 1");
    }

    #[test]
    fn test_qualified_annotation_is_lossless() {
        assert_lossless("@kotlin.Deprecated fun foo() {}");
    }

    #[test]
    fn test_annotation_with_args_is_lossless() {
        assert_lossless("@Deprecated(\"use bar\") fun foo() {}");
    }

    #[test]
    fn test_simple_function() {
        let source = "fun main() { }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);

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
            KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    #[test]
    fn test_property_decl() {
        let source = "val x: Int = 42";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_class_decl() {
        let source = "class MyClass { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_class_with_function() {
        let source = "class MyClass { fun foo() { } }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_package_and_imports() {
        let source = r"
            package com.example
            import kotlin.collections.List
            fun main() { }
        ";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_object_decl() {
        let source = "object Singleton { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_no_params() {
        let source = "fun foo(block: () -> String) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_single_param() {
        let source = "fun foo(block: (String) -> Int) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_multiple_params() {
        let source = "fun foo(block: (String, Int) -> Boolean) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_with_generics() {
        let source = "fun foo(block: (List<String>) -> String) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_nullable_return() {
        let source = "fun foo(block: () -> String?) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_with_receiver() {
        let source = "fun foo(block: String.() -> Unit) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_nested_function_types() {
        let source = "fun foo(block: ((String) -> Int) -> String) { }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }

    #[test]
    fn test_function_type_in_property() {
        let source = "val handler: (String, Int) -> Unit = { _, _ -> }";
        let tokens = Lexer::new(source).collect();
        let (_green, errors) = Parser::parse(tokens);

        assert!(errors.is_empty(), "Expected no errors, got: {errors:?}");
    }
}
