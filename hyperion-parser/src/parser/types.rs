// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type and Generics Parsing
//!
//! This module contains type parsing methods for the Java parser, including:
//! - Primitive types, class types, array types
//! - Type parameters (generics declarations)
//! - Type arguments (generics usage)
//! - Parameter lists and individual parameters

use crate::syntax::SyntaxKind;

impl super::Parser<'_> {
    /// Parse an ordinary Java type.
    ///
    /// This deliberately does not recognize the restricted type name `var`.
    /// `var` is a terminal only in `LocalVariableType` and
    /// `LambdaParameterType`; callers in those grammar contexts must use the
    /// correspondingly named entry point below.
    pub(super) fn parse_type(&mut self) {
        // Parse leading type-use annotations (JSR 308, Java 8+)
        // Examples: @NonNull String, Function<@Nullable String>
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }

        // Primitive types
        if matches!(
            self.current(),
            SyntaxKind::BOOLEAN_KW
                | SyntaxKind::BYTE_KW
                | SyntaxKind::SHORT_KW
                | SyntaxKind::INT_KW
                | SyntaxKind::LONG_KW
                | SyntaxKind::CHAR_KW
                | SyntaxKind::FLOAT_KW
                | SyntaxKind::DOUBLE_KW
        ) {
            self.start_node(SyntaxKind::PRIMITIVE_TYPE);
            self.bump();
            self.finish_node();
        } else {
            // Class/interface type
            // JLS §4.5: ClassType can be qualified: Outer<T>.Inner<U>
            self.start_node(SyntaxKind::CLASS_TYPE);
            self.parse_qualified_type_name();

            self.skip_trivia();

            // Type arguments (optional)
            if self.at(SyntaxKind::LT) {
                self.parse_type_args();
                self.skip_trivia();
            }

            // Check for qualified inner types: Outer<T>.Inner<U>
            // After parsing type arguments, we may have more qualified segments
            while self.at(SyntaxKind::DOT) {
                // Look ahead to see if this is a type continuation (identifier follows DOT)
                // vs. a member access expression
                let mut lookahead_pos = self.pos + 1; // skip DOT
                self.skip_trivia_lookahead(&mut lookahead_pos);

                // Check if followed by identifier (inner type name)
                // Must exclude keywords that can't be type names
                if lookahead_pos < self.tokens.len()
                    && self.tokens[lookahead_pos].0.is_type_identifier()
                    && !matches!(
                        self.tokens[lookahead_pos].0,
                        SyntaxKind::CLASS_KW
                            | SyntaxKind::THIS_KW
                            | SyntaxKind::SUPER_KW
                            | SyntaxKind::NEW_KW
                    )
                {
                    self.bump(); // consume DOT
                    self.skip_trivia();

                    // Consume any type-use annotations (JSR 308, Java 8+)
                    // Example: Outer<T>.@NonNull Inner
                    while self.at(SyntaxKind::AT) {
                        self.parse_annotation();
                        self.skip_trivia();
                    }

                    // Parse inner type identifier
                    self.expect_type_identifier();
                    self.skip_trivia();

                    // Parse inner type's type arguments (if any)
                    // Example: Outer<T>.Inner<U>
                    if self.at(SyntaxKind::LT) {
                        self.parse_type_args();
                        self.skip_trivia();
                    }
                } else {
                    // Not a qualified type continuation - stop
                    break;
                }
            }

            self.finish_node();
        }

        self.skip_trivia();

        // Array dimensions (optional) - only consume empty brackets []
        // Brackets with size expressions like [2] are handled by array creation expressions
        while self.at(SyntaxKind::L_BRACKET) {
            // Lookahead to check if it's an empty bracket pair []
            // BUGFIX: Don't use bump() for lookahead - it adds tokens to the tree!
            // Use position-based lookahead instead.
            let mut lookahead_pos = self.pos + 1;
            self.skip_trivia_lookahead(&mut lookahead_pos);

            if lookahead_pos < self.tokens.len()
                && self.tokens[lookahead_pos].0 == SyntaxKind::R_BRACKET
            {
                // Empty brackets - this is an array type dimension
                self.start_node(SyntaxKind::ARRAY_TYPE);
                self.bump(); // [
                self.skip_trivia();
                self.expect(SyntaxKind::R_BRACKET); // ]
                self.finish_node();
                self.skip_trivia();
            } else {
                // Non-empty brackets - not an array type, stop
                break;
            }
        }
    }

    /// Parse a method result, the only type-like grammar position in which
    /// bare `void` is legal. Keeping it separate prevents `void` from leaking
    /// into fields, parameters, type arguments, casts, and local variables.
    pub(super) fn parse_result_type(&mut self) {
        if self.at(SyntaxKind::VOID_KW) {
            self.start_node(SyntaxKind::PRIMITIVE_TYPE);
            self.bump();
            self.finish_node();
        } else {
            self.parse_type();
        }
    }

    /// Parse a `LocalVariableType` (JLS §14.4).
    pub(super) fn parse_local_variable_type(&mut self) {
        self.parse_inferred_or_ordinary_type();
    }

    /// Parse a `LambdaParameterType` (JLS §15.27.1).
    pub(super) fn parse_lambda_parameter_type(&mut self) {
        self.parse_inferred_or_ordinary_type();
    }

    fn parse_inferred_or_ordinary_type(&mut self) {
        // A package may legally be named `var`; only a bare `var` denotes an
        // inferred type. For example, `var.example.Widget value` still has an
        // ordinary class type.
        if self.at(SyntaxKind::VAR_KW) && !self.identifier_starts_qualified_name() {
            self.start_node(SyntaxKind::VAR_TYPE);
            self.bump();
            self.finish_node();
        } else {
            self.parse_type();
        }
    }

    /// Parse a comma-separated list of types
    pub(super) fn parse_type_list(&mut self) {
        loop {
            let start_pos = self.pos;
            self.parse_type();

            // BUGFIX: If parse_type didn't consume any tokens, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }

            self.skip_trivia();

            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }
    }

    /// Parse type parameters
    ///
    /// Grammar:
    /// ```text
    /// TypeParams:
    ///     < TypeParam {, TypeParam} >
    /// ```
    pub(super) fn parse_type_params(&mut self) {
        self.start_node(SyntaxKind::TYPE_PARAM_LIST);

        self.expect(SyntaxKind::LT);

        self.skip_trivia();

        loop {
            self.start_node(SyntaxKind::TYPE_PARAM);

            // Type parameter name
            // BUGFIX: Check if expect succeeds, break if it fails to prevent infinite loop
            if !self.expect_type_identifier() {
                self.finish_node();
                break;
            }

            self.skip_trivia();

            // Bounds (optional)
            if self.at(SyntaxKind::EXTENDS_KW) {
                self.start_node(SyntaxKind::TYPE_BOUND);
                self.bump();
                self.skip_trivia();

                // Parse bounds (Type & Type & ...)
                loop {
                    let start_pos = self.pos;
                    self.parse_type();

                    // BUGFIX: If parse_type didn't consume any tokens, break to prevent infinite loop
                    if self.pos == start_pos {
                        break;
                    }

                    self.skip_trivia();

                    if self.at(SyntaxKind::AMP) {
                        self.bump();
                        self.skip_trivia();
                    } else {
                        break;
                    }
                }

                self.finish_node();
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
    ///
    /// Grammar:
    /// ```text
    /// TypeArgs:
    ///     < Type {, Type} >
    ///     < >  (diamond operator - Java 7+)
    /// ```
    pub(super) fn parse_type_args(&mut self) {
        self.start_node(SyntaxKind::TYPE_ARGS);

        self.expect(SyntaxKind::LT);

        self.skip_trivia();

        // Handle diamond operator: <>
        if self.at(SyntaxKind::GT) {
            self.bump(); // consume >
            self.finish_node();
            return;
        }

        loop {
            let start_pos = self.pos;

            // Wildcard type or regular type
            let annotated_wildcard = if self.at(SyntaxKind::AT) {
                let mut lookahead = self.pos;
                while self
                    .tokens
                    .get(lookahead)
                    .is_some_and(|token| token.0 == SyntaxKind::AT)
                {
                    let Some(after_annotation) = self.skip_annotation_lookahead(lookahead) else {
                        break;
                    };
                    lookahead = after_annotation;
                    self.skip_trivia_lookahead(&mut lookahead);
                }
                self.tokens
                    .get(lookahead)
                    .is_some_and(|token| token.0 == SyntaxKind::QUESTION)
            } else {
                false
            };
            if self.at(SyntaxKind::QUESTION) || annotated_wildcard {
                self.start_node(SyntaxKind::WILDCARD_TYPE);
                while self.at(SyntaxKind::AT) {
                    self.parse_annotation();
                    self.skip_trivia();
                }
                self.bump();
                self.skip_trivia();

                // extends or super bounds (optional)
                if self.at(SyntaxKind::EXTENDS_KW) || self.at(SyntaxKind::SUPER_KW) {
                    self.bump();
                    self.skip_trivia();
                    self.parse_type();
                }

                self.finish_node();
            } else {
                self.parse_type();
            }

            // BUGFIX: If no tokens were consumed, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }

            self.skip_trivia();

            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        self.expect_gt_split_shr();

        self.finish_node();
    }

    // ============================================================================
    // Parameters
    // ============================================================================

    /// Parse parameter list
    ///
    /// Grammar:
    /// ```text
    /// ParamList:
    ///     ( [Param {, Param}] )
    /// ```
    pub(super) fn parse_param_list(&mut self) {
        self.start_node(SyntaxKind::PARAM_LIST);

        self.expect(SyntaxKind::L_PAREN);

        self.skip_trivia();

        if !self.at(SyntaxKind::R_PAREN) {
            loop {
                let start_pos = self.pos;
                self.parse_param();

                // BUGFIX: If parse_param didn't consume any tokens, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }

                self.skip_trivia();

                if self.at(SyntaxKind::COMMA) {
                    self.bump();
                    self.skip_trivia();
                } else {
                    break;
                }
            }
        }

        self.expect(SyntaxKind::R_PAREN);

        self.finish_node();
    }

    /// Parse a single parameter
    ///
    /// Grammar:
    /// ```text
    /// Param:
    ///     {Modifier} Type [... ] Ident [{Dimension}]
    /// ```
    pub(super) fn parse_param(&mut self) {
        self.start_node(SyntaxKind::PARAM);

        // Modifiers (final, annotations)
        self.parse_modifiers();

        self.skip_trivia();

        // Type
        self.parse_type();

        self.skip_trivia();

        // Varargs ellipsis (optional)
        self.eat(SyntaxKind::ELLIPSIS);

        self.skip_trivia();

        // Parameter name (allow contextual keywords like 'record')
        self.expect_identifier();

        self.skip_trivia();

        // Array dimensions (optional)
        while self.at(SyntaxKind::L_BRACKET) {
            self.bump();
            self.skip_trivia();
            self.expect(SyntaxKind::R_BRACKET);
            self.skip_trivia();
        }

        self.finish_node();
    }
}
