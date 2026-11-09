// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Modifier and Annotation Parsing
//!
//! This module contains methods for parsing Java modifiers (public, static, final, etc.)
//! and annotations (@Override, @SuppressWarnings, etc.), including annotation arguments
//! and constant expressions per JLS §15.28.

use crate::syntax::SyntaxKind;
use rowan::TextRange;

impl super::Parser<'_> {
    /// Parse modifiers (public, static, final, etc.) and annotations
    pub(super) fn parse_modifiers(&mut self) {
        let mut started = false;
        loop {
            self.skip_trivia();

            // Check for annotation
            if self.at(SyntaxKind::AT) {
                // Do not consume the start of an annotation type declaration (`@interface`)
                let mut lookahead = self.pos + 1;
                while lookahead < self.tokens.len() && self.tokens[lookahead].0.is_trivia() {
                    lookahead += 1;
                }
                if lookahead < self.tokens.len()
                    && self.tokens[lookahead].0 == SyntaxKind::INTERFACE_KW
                {
                    break;
                }

                if !started {
                    self.start_node(SyntaxKind::MODIFIER_LIST);
                    started = true;
                }

                // Do not consume the start of an annotation type declaration (`@interface`)
                let start_pos = self.pos;
                self.parse_annotation();
                // BUGFIX: If parse_annotation didn't consume any tokens, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }
                continue;
            }

            if self.at_non_sealed_modifier() {
                if !started {
                    self.start_node(SyntaxKind::MODIFIER_LIST);
                    started = true;
                }
                self.start_node(SyntaxKind::MODIFIER);
                self.bump();
                self.bump();
                self.bump();
                self.finish_node();
                continue;
            }

            // Check for modifier keyword
            match self.current() {
                SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::STATIC_KW
                | SyntaxKind::FINAL_KW
                | SyntaxKind::ABSTRACT_KW
                | SyntaxKind::NATIVE_KW
                | SyntaxKind::SYNCHRONIZED_KW
                | SyntaxKind::TRANSIENT_KW
                | SyntaxKind::VOLATILE_KW
                | SyntaxKind::STRICTFP_KW
                | SyntaxKind::DEFAULT_KW
                | SyntaxKind::SEALED_KW
                | SyntaxKind::NON_SEALED_KW => {
                    if !started {
                        self.start_node(SyntaxKind::MODIFIER_LIST);
                        started = true;
                    }
                    self.start_node(SyntaxKind::MODIFIER);
                    self.bump();
                    self.finish_node();
                }
                _ => break,
            }
        }

        if started {
            self.finish_node();
        }
    }

    /// Parse an annotation
    ///
    /// Grammar:
    /// ```text
    /// Annotation:
    ///     @ QualifiedName [( [AnnotationArgs] )]
    /// ```
    pub(super) fn parse_annotation(&mut self) {
        self.start_node(SyntaxKind::ANNOTATION);

        // @ symbol
        self.bump();

        self.skip_trivia();

        // Annotation name
        if self.at_identifier() {
            self.parse_qualified_name();
        } else {
            let range = self.current_range();
            self.error("Expected annotation name", range);
            self.start_node(SyntaxKind::ERROR);
            if !self.at_eof() && !self.current().is_trivia() {
                self.bump();
            }
            self.finish_node();
        }

        self.skip_trivia();

        // Arguments (optional)
        if self.at(SyntaxKind::L_PAREN) {
            self.parse_annotation_args();
        }

        self.finish_node();
    }

    /// Parse annotation arguments: `(value)` or `(key = value, key2 = value2)`
    pub(super) fn parse_annotation_args(&mut self) {
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
    pub(super) fn parse_annotation_arg(&mut self) {
        self.start_node(SyntaxKind::ANNOTATION_ARG);

        // Check if this is a key-value pair (key = value) or just a value
        // Look ahead to see if there's an '=' after the identifier
        if self.at_identifier() {
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
                // Parse key (identifier)
                self.bump();
                self.skip_trivia();
                self.expect(SyntaxKind::EQ);
                self.skip_trivia();
            }
        }

        // Parse value
        self.parse_annotation_value();

        self.skip_trivia();
        self.finish_node();
    }

    /// Parse an annotation value (literal, annotation, array, etc.)
    pub(super) fn parse_annotation_value(&mut self) {
        match self.current() {
            // Nested annotation
            SyntaxKind::AT => {
                self.parse_annotation();
            }
            // Array
            SyntaxKind::L_BRACE => {
                self.start_node(SyntaxKind::ANNOTATION_ARRAY_VALUE);
                self.bump(); // '{'
                self.skip_trivia();

                if !self.at(SyntaxKind::R_BRACE) {
                    self.parse_annotation_value();

                    while self.at(SyntaxKind::COMMA) {
                        self.bump();
                        self.skip_trivia();

                        if self.at(SyntaxKind::R_BRACE) {
                            break;
                        }

                        self.parse_annotation_value();
                    }
                }

                self.skip_trivia();
                self.expect(SyntaxKind::R_BRACE);
                self.finish_node();
            }
            // For all other cases (literals, identifiers, expressions), parse as a constant expression
            // This handles:
            // - String literals
            // - Numeric literals
            // - Boolean literals
            // - Null literals
            // - Identifiers (enum constants, class references)
            // - Binary expressions (e.g., string concatenation with +)
            _ => {
                self.parse_annotation_constant_expr();
            }
        }
    }

    /// Parse a constant expression in annotation context.
    ///
    /// Supports the full set of JLS constant expressions (§15.28):
    /// - Literals (string, numeric, boolean, null)
    /// - Qualified names and .class
    /// - Unary: +, -, ~, !
    /// - Binary: +, -, *, /, %, &, |, ^, <<, >>, >>>, &&, ||, ==, !=, <, >, <=, >=
    /// - Ternary: ? :
    /// - Parenthesized expressions: (expr)
    pub(super) fn parse_annotation_constant_expr(&mut self) {
        self.parse_annotation_unary_or_primary();

        self.skip_trivia();

        // Check for binary operator
        if self.is_annotation_binary_op() {
            self.start_node(SyntaxKind::BINARY_EXPR);

            loop {
                self.skip_trivia();

                if !self.is_annotation_binary_op() {
                    break;
                }

                self.bump(); // consume operator
                self.skip_trivia();

                self.parse_annotation_unary_or_primary();
            }

            self.finish_node();
        }

        // Check for ternary operator: ? expr : expr
        self.skip_trivia();
        if self.at(SyntaxKind::QUESTION) {
            self.start_node(SyntaxKind::TERNARY_EXPR);
            self.bump(); // consume ?
            self.skip_trivia();
            self.parse_annotation_constant_expr(); // true branch
            self.skip_trivia();
            self.expect(SyntaxKind::COLON);
            self.skip_trivia();
            self.parse_annotation_constant_expr(); // false branch
            self.finish_node();
        }
    }

    /// Check if current token is a binary operator valid in annotation constant expressions.
    pub(super) fn is_annotation_binary_op(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::PLUS
                | SyntaxKind::MINUS
                | SyntaxKind::STAR
                | SyntaxKind::SLASH
                | SyntaxKind::PERCENT
                | SyntaxKind::AMP
                | SyntaxKind::PIPE
                | SyntaxKind::CARET
                | SyntaxKind::SHL
                | SyntaxKind::SHR
                | SyntaxKind::USHR
                | SyntaxKind::AMP_AMP
                | SyntaxKind::PIPE_PIPE
                | SyntaxKind::EQ_EQ
                | SyntaxKind::BANG_EQ
                | SyntaxKind::LT
                | SyntaxKind::GT
                | SyntaxKind::LT_EQ
                | SyntaxKind::GT_EQ
        )
    }

    /// Parse a unary expression or primary value in annotation context.
    pub(super) fn parse_annotation_unary_or_primary(&mut self) {
        // Handle unary operators: +, -, ~, !
        let has_unary = matches!(
            self.current(),
            SyntaxKind::MINUS | SyntaxKind::PLUS | SyntaxKind::TILDE | SyntaxKind::BANG
        );
        if has_unary {
            self.start_node(SyntaxKind::UNARY_EXPR);
            self.bump(); // consume unary operator
            self.skip_trivia();
            // UnaryExpression is recursive: annotation constants such as
            // `!!true` and `-(~+1)` are legal.
            self.parse_annotation_unary_or_primary();
            self.finish_node();
            return;
        }

        self.parse_annotation_primary();
    }

    /// Parse a primary value in annotation constant expression.
    pub(super) fn parse_annotation_primary(&mut self) {
        match self.current() {
            // Parenthesized expression
            SyntaxKind::L_PAREN => {
                if self.is_cast() {
                    self.parse_annotation_cast_expr();
                } else {
                    self.start_node(SyntaxKind::PAREN_EXPR);
                    self.bump(); // consume (
                    self.skip_trivia();
                    self.parse_annotation_constant_expr();
                    self.skip_trivia();
                    self.expect(SyntaxKind::R_PAREN);
                    self.finish_node();
                }
            }
            // Literals: string, text block, numeric, boolean, null
            SyntaxKind::STRING_LITERAL
            | SyntaxKind::TEXT_BLOCK_LITERAL
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::BOOLEAN_LITERAL => {
                self.bump();
            }
            // `null` is not an annotation element value, even when the
            // annotation element has a reference type.
            SyntaxKind::NULL_LITERAL => {
                self.error(
                    "null is not a legal annotation element value",
                    self.current_range(),
                );
                self.bump();
            }
            // Identifier (could be enum constant, class reference, etc.).
            // Module-system and restricted words remain identifiers here.
            _ if self.at_identifier() => {
                self.parse_qualified_name();
                let mut final_type_segment_is_valid = self
                    .previous_significant_kind()
                    .is_some_and(SyntaxKind::is_type_identifier);
                let mut has_type_arguments = false;
                let mut type_is_reifiable = true;
                let mut type_has_annotations = false;

                loop {
                    if self.at(SyntaxKind::LT) {
                        let Some(type_args) = self.inspect_type_arguments() else {
                            break;
                        };
                        if !matches!(
                            self.significant_kind_at(type_args.end),
                            Some(SyntaxKind::DOT | SyntaxKind::L_BRACKET | SyntaxKind::COLON_COLON)
                        ) {
                            break;
                        }
                        if type_args.empty {
                            self.error(
                                "The diamond operator is not allowed in a class literal",
                                self.current_range(),
                            );
                        }
                        has_type_arguments = true;
                        type_is_reifiable &= type_args.all_unbounded_wildcards;
                        type_has_annotations |= type_args.contains_annotation;
                        self.parse_type_args();
                        self.skip_trivia();
                        continue;
                    }

                    if self.annotation_inner_type_segment_ahead() {
                        self.bump(); // '.'
                        self.skip_trivia();
                        while self.at(SyntaxKind::AT) {
                            self.parse_annotation();
                            self.skip_trivia();
                        }
                        final_type_segment_is_valid = self.current().is_type_identifier();
                        self.expect_type_identifier();
                        self.skip_trivia();
                        continue;
                    }
                    break;
                }

                if self.at(SyntaxKind::DOT) || self.at(SyntaxKind::L_BRACKET) {
                    let has_dimensions = self.at(SyntaxKind::L_BRACKET);
                    if !final_type_segment_is_valid
                        || !type_is_reifiable
                        || type_has_annotations
                        || (has_type_arguments && !has_dimensions)
                    {
                        self.error(
                            "A class literal requires a reifiable type",
                            self.current_range(),
                        );
                    }
                    self.parse_annotation_class_literal_suffix(false, true);
                }
            }
            // Primitive and void names are not expressions by themselves; in
            // an annotation value they are legal only as class literals.
            SyntaxKind::BOOLEAN_KW
            | SyntaxKind::BYTE_KW
            | SyntaxKind::SHORT_KW
            | SyntaxKind::INT_KW
            | SyntaxKind::LONG_KW
            | SyntaxKind::CHAR_KW
            | SyntaxKind::FLOAT_KW
            | SyntaxKind::DOUBLE_KW
            | SyntaxKind::VOID_KW => {
                let dimensions_allowed = !self.at(SyntaxKind::VOID_KW);
                self.bump();
                self.skip_trivia();
                self.parse_annotation_class_literal_suffix(true, dimensions_allowed);
            }
            // Fallback: error
            _ => {
                let range = if self.pos < self.tokens.len() {
                    self.tokens[self.pos].2
                } else {
                    TextRange::new(0u32.into(), 0u32.into())
                };
                self.error(
                    format!("Expected annotation value, found {:?}", self.current()),
                    range,
                );
                if !self.at_eof() {
                    self.bump();
                }
            }
        }
    }

    fn parse_annotation_cast_expr(&mut self) {
        if !self.annotation_constant_cast_type_is_allowed() {
            self.error(
                "An annotation constant may only cast to a primitive type or String",
                self.current_range(),
            );
        }
        self.start_node(SyntaxKind::CAST_EXPR);
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.parse_type();
        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.parse_annotation_unary_or_primary();
        self.finish_node();
    }

    /// JLS constant expressions admit casts to primitive types and `String`,
    /// but not arbitrary reference or array types. Name resolution will decide
    /// whether an imported/simple `String` denotes `java.lang.String`; this
    /// check enforces the syntactic boundary without guessing symbol identity.
    fn annotation_constant_cast_type_is_allowed(&self) -> bool {
        if !self.at(SyntaxKind::L_PAREN) {
            return false;
        }
        let mut pos = self.pos + 1;
        self.skip_trivia_lookahead(&mut pos);
        while self
            .tokens
            .get(pos)
            .is_some_and(|token| token.0 == SyntaxKind::AT)
        {
            let Some(after_annotation) = self.skip_annotation_lookahead(pos) else {
                return false;
            };
            pos = after_annotation;
            self.skip_trivia_lookahead(&mut pos);
        }

        if self.tokens.get(pos).is_some_and(|token| {
            matches!(
                token.0,
                SyntaxKind::BOOLEAN_KW
                    | SyntaxKind::BYTE_KW
                    | SyntaxKind::SHORT_KW
                    | SyntaxKind::INT_KW
                    | SyntaxKind::LONG_KW
                    | SyntaxKind::CHAR_KW
                    | SyntaxKind::FLOAT_KW
                    | SyntaxKind::DOUBLE_KW
            )
        }) {
            pos += 1;
            self.skip_trivia_lookahead(&mut pos);
            return self
                .tokens
                .get(pos)
                .is_some_and(|token| token.0 == SyntaxKind::R_PAREN);
        }

        let Some(first_segment) = self.tokens.get(pos) else {
            return false;
        };
        if !first_segment.0.is_identifier() {
            return false;
        }
        let mut final_segment = first_segment.1;
        loop {
            pos += 1;
            self.skip_trivia_lookahead(&mut pos);
            if self
                .tokens
                .get(pos)
                .is_some_and(|token| token.0 == SyntaxKind::DOT)
            {
                pos += 1;
                self.skip_trivia_lookahead(&mut pos);
                let Some(segment) = self.tokens.get(pos) else {
                    return false;
                };
                if !segment.0.is_identifier() {
                    return false;
                }
                final_segment = segment.1;
                continue;
            }
            break;
        }

        final_segment == "String"
            && self
                .tokens
                .get(pos)
                .is_some_and(|token| token.0 == SyntaxKind::R_PAREN)
    }

    /// A dot after type arguments can continue a qualified class type. A dot
    /// immediately followed by `class` instead begins the class-literal tail.
    fn annotation_inner_type_segment_ahead(&self) -> bool {
        if !self.at(SyntaxKind::DOT) {
            return false;
        }
        let mut pos = self.pos + 1;
        self.skip_trivia_lookahead(&mut pos);
        while self
            .tokens
            .get(pos)
            .is_some_and(|token| token.0 == SyntaxKind::AT)
        {
            let Some(next) = self.skip_annotation_lookahead(pos) else {
                return false;
            };
            pos = next;
            self.skip_trivia_lookahead(&mut pos);
        }
        self.tokens
            .get(pos)
            .is_some_and(|token| token.0.is_type_identifier())
    }

    /// Parse the dimensions and `.class` tail shared by reference, primitive,
    /// and void class literals in annotation element values.
    fn parse_annotation_class_literal_suffix(&mut self, required: bool, dimensions_allowed: bool) {
        let mut had_dimensions = false;
        while self.at(SyntaxKind::L_BRACKET) {
            if !dimensions_allowed {
                self.error(
                    "The void type cannot have array dimensions",
                    self.current_range(),
                );
            }
            had_dimensions = true;
            self.bump();
            self.skip_trivia();
            self.expect(SyntaxKind::R_BRACKET);
            self.skip_trivia();
        }

        if self.at(SyntaxKind::DOT) {
            self.bump();
            self.skip_trivia();
            self.expect(SyntaxKind::CLASS_KW);
        } else if required || had_dimensions {
            self.error(
                "Expected '.class' after class-literal type",
                self.current_range(),
            );
        }
    }

    /// Check if current position starts a type
    pub(super) fn is_type_start(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::BOOLEAN_KW
                | SyntaxKind::BYTE_KW
                | SyntaxKind::SHORT_KW
                | SyntaxKind::INT_KW
                | SyntaxKind::LONG_KW
                | SyntaxKind::CHAR_KW
                | SyntaxKind::FLOAT_KW
                | SyntaxKind::DOUBLE_KW
        ) || self.at_type_identifier()
            || (self.at_identifier() && self.identifier_starts_qualified_name())
    }

    /// Check if current position starts a `LocalVariableType`.
    pub(super) fn is_local_variable_type_start(&self) -> bool {
        self.at(SyntaxKind::VAR_KW) || self.is_type_start()
    }
}
