// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Expression Parsing
//!
//! This module contains all expression parsing methods for the Java parser.
//! Expression parsing uses a simplified precedence climbing approach.
//!
//! Expression precedence (lowest to highest):
//! 1. Assignment (=, +=, -=, etc.) - right associative
//! 2. Ternary (?:)
//! 3. Logical OR (||)
//! 4. Logical AND (&&)
//! 5. Bitwise OR (|)
//! 6. Bitwise XOR (^)
//! 7. Bitwise AND (&)
//! 8. Equality (==, !=)
//! 9. Relational (<, >, <=, >=, instanceof)
//! 10. Shift (<<, >>, >>>)
//! 11. Additive (+, -)
//! 12. Multiplicative (*, /, %)
//! 13. Unary (!, ~, +, -, ++, --, cast)
//! 14. Postfix (++, --, [], ., ::)
//! 15. Primary (literals, identifiers, parenthesized, new, lambda)

use crate::syntax::SyntaxKind;

use super::is_identifier_kind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LambdaParameterKind {
    Concise,
    InferredWithVar,
    Declared,
}

/// Syntactic facts about a type-argument list needed before committing to
/// parsing `<` as generics rather than as the relational operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TypeArgumentsLookahead {
    pub(super) end: usize,
    pub(super) empty: bool,
    pub(super) all_unbounded_wildcards: bool,
    pub(super) has_top_level_wildcard: bool,
    pub(super) contains_bare_primitive: bool,
    pub(super) contains_annotation: bool,
}

impl super::Parser<'_> {
    /// Parse an expression. Operator precedence is handled by precedence climbing;
    /// see the module docs for the levels.
    pub(super) fn parse_expression(&mut self) {
        // Check recursion depth for deeply nested expressions
        if !self.check_recursion_depth() {
            // Depth limit exceeded - create a minimal error node
            self.start_node(SyntaxKind::ERROR);
            while !self.at_eof()
                && !self.at(SyntaxKind::SEMICOLON)
                && !self.at(SyntaxKind::COMMA)
                && !self.at(SyntaxKind::R_PAREN)
                && !self.at(SyntaxKind::R_BRACE)
                && !self.at(SyntaxKind::R_BRACKET)
            {
                self.bump();
            }
            self.finish_node();
            self.decrease_recursion_depth();
            return;
        }

        self.parse_assignment_expr();
        self.decrease_recursion_depth();
    }

    /// Parse assignment expression (=, +=, -=, etc.)
    /// Assignment is right-associative and has lowest precedence
    pub(super) fn parse_assignment_expr(&mut self) {
        // Parse the left-hand side
        self.parse_ternary_expr();

        self.skip_trivia();

        // Check for assignment operators
        if matches!(
            self.current(),
            SyntaxKind::EQ
                | SyntaxKind::PLUS_EQ
                | SyntaxKind::MINUS_EQ
                | SyntaxKind::STAR_EQ
                | SyntaxKind::SLASH_EQ
                | SyntaxKind::PERCENT_EQ
                | SyntaxKind::AMP_EQ
                | SyntaxKind::PIPE_EQ
                | SyntaxKind::CARET_EQ
                | SyntaxKind::SHL_EQ
                | SyntaxKind::SHR_EQ
                | SyntaxKind::USHR_EQ
        ) {
            self.start_node(SyntaxKind::ASSIGNMENT_EXPR);
            self.skip_trivia();
            self.bump(); // consume assignment operator
            self.skip_trivia();
            self.parse_assignment_expr(); // right-associative
            self.finish_node();
        }
    }

    /// Parse ternary expression (? :)
    /// Grammar: ConditionalOrExpression ? Expression : ConditionalExpression (JLS §15.25)
    pub(super) fn parse_ternary_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_logical_or_expr();

        self.skip_trivia();

        if self.at(SyntaxKind::QUESTION) {
            self.start_node_at(checkpoint, SyntaxKind::TERNARY_EXPR);
            self.bump();
            self.skip_trivia();
            self.parse_expression(); // True branch - full expression
            self.skip_trivia();
            self.expect(SyntaxKind::COLON);
            self.skip_trivia();
            // False branch should parse ConditionalExpression (JLS §15.25)
            // not full Expression. This preserves context when inside parentheses.
            self.parse_ternary_expr(); // Recursive call for false branch
            self.finish_node();
        }
    }

    /// Parse logical OR expression (||)
    pub(super) fn parse_logical_or_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_logical_and_expr();

        while self.at(SyntaxKind::PIPE_PIPE) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_logical_and_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse logical AND expression (&&)
    pub(super) fn parse_logical_and_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_bitwise_or_expr();

        while self.at(SyntaxKind::AMP_AMP) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_bitwise_or_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse bitwise OR expression (|)
    pub(super) fn parse_bitwise_or_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_bitwise_xor_expr();

        while self.at(SyntaxKind::PIPE) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_bitwise_xor_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse bitwise XOR expression (^)
    pub(super) fn parse_bitwise_xor_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_bitwise_and_expr();

        while self.at(SyntaxKind::CARET) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_bitwise_and_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse bitwise AND expression (&)
    pub(super) fn parse_bitwise_and_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_equality_expr();

        while self.at(SyntaxKind::AMP) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_equality_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse equality expression (== !=)
    pub(super) fn parse_equality_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_relational_expr();

        while matches!(self.current(), SyntaxKind::EQ_EQ | SyntaxKind::BANG_EQ) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_relational_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse relational expression (< > <= >=, instanceof)
    pub(super) fn parse_relational_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_shift_expr();

        while matches!(
            self.current(),
            SyntaxKind::LT | SyntaxKind::GT | SyntaxKind::LT_EQ | SyntaxKind::GT_EQ
        ) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_shift_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }

        // instanceof (regular or pattern matching)
        if self.at(SyntaxKind::INSTANCEOF_KW) {
            // Determine if this is a pattern instanceof by checking if identifier follows type
            let is_pattern = self.is_pattern_instanceof();

            if is_pattern {
                self.start_node_at(checkpoint, SyntaxKind::PATTERN_INSTANCEOF_EXPR);
            } else {
                self.start_node_at(checkpoint, SyntaxKind::INSTANCEOF_EXPR);
            }

            self.skip_trivia();
            self.bump(); // consume 'instanceof'
            self.skip_trivia();

            // If pattern instanceof, parse as pattern (Java 16+)
            // Otherwise just parse the type
            if is_pattern {
                // Parse type pattern: Type identifier
                self.parse_type_pattern();
            } else {
                self.parse_lambda_parameter_type();
            }

            self.finish_node();

            // Skip trailing trivia so that higher precedence levels can find operators
            self.skip_trivia();
        }
    }

    /// Check if the current instanceof is a pattern instanceof (Java 16+)
    ///
    /// Pattern instanceof: `obj instanceof String s`
    /// Regular instanceof: `obj instanceof String`
    pub(super) fn is_pattern_instanceof(&self) -> bool {
        if !self.at(SyntaxKind::INSTANCEOF_KW) {
            return false;
        }
        self.looks_like_type_pattern(1, false)
    }

    /// Parse shift expression (<< >> >>>)
    pub(super) fn parse_shift_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_additive_expr();

        while matches!(
            self.current(),
            SyntaxKind::SHL | SyntaxKind::SHR | SyntaxKind::USHR
        ) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_additive_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse additive expression (+ -)
    pub(super) fn parse_additive_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_multiplicative_expr();

        while matches!(self.current(), SyntaxKind::PLUS | SyntaxKind::MINUS) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_multiplicative_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse multiplicative expression (* / %)
    pub(super) fn parse_multiplicative_expr(&mut self) {
        let mut checkpoint = self.checkpoint();
        self.parse_unary_expr();

        while matches!(
            self.current(),
            SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT
        ) {
            self.start_node_at(checkpoint, SyntaxKind::BINARY_EXPR);
            self.skip_trivia();
            self.bump();
            self.skip_trivia();
            self.parse_unary_expr();
            self.finish_node();
            checkpoint = self.checkpoint();
        }
    }

    /// Parse unary expression (! ~ - + ++ --)
    pub(super) fn parse_unary_expr(&mut self) {
        match self.current() {
            SyntaxKind::BANG
            | SyntaxKind::TILDE
            | SyntaxKind::MINUS
            | SyntaxKind::PLUS
            | SyntaxKind::PLUS_PLUS
            | SyntaxKind::MINUS_MINUS => {
                // Check recursion depth for prefix operators (can chain: !!!value)
                if !self.check_recursion_depth() {
                    // M5 FIX: Depth limit exceeded - DON'T call decrease_recursion_depth() before parsing
                    // check_recursion_depth() already incremented the counter, and we need to keep it
                    // incremented while parsing, then decrement after we're done.
                    self.parse_postfix_expr();
                    self.decrease_recursion_depth();
                    return;
                }

                self.start_node(SyntaxKind::UNARY_EXPR);
                self.bump();
                self.skip_trivia();
                self.parse_unary_expr();
                self.finish_node();
                self.decrease_recursion_depth();
            }
            _ => self.parse_postfix_expr(),
        }
    }

    /// Parse postfix expression (x++, x--, field access, method calls, array access)
    ///
    /// Uses rowan checkpoints to produce nested expression trees. For example,
    /// `user.getName()` produces:
    /// ```text
    /// METHOD_CALL_EXPR
    ///   NAME_EXPR("user")     ← child (receiver)
    ///   DOT                    ← child
    ///   IDENT("getName")       ← child (method name)
    ///   ARG_LIST(...)          ← child
    /// ```
    ///
    /// Chained calls `a.b().c()` nest naturally:
    /// ```text
    /// METHOD_CALL_EXPR                    ← outer call .c()
    ///   METHOD_CALL_EXPR                  ← inner call .b()
    ///     NAME_EXPR("a")
    ///     DOT
    ///     IDENT("b")
    ///     ARG_LIST
    ///   DOT
    ///   IDENT("c")
    ///   ARG_LIST
    /// ```
    pub(super) fn parse_postfix_expr(&mut self) {
        let checkpoint = self.checkpoint();
        let mut unannotated_base = self.pos;
        while self
            .tokens
            .get(unannotated_base)
            .is_some_and(|token| token.0 == SyntaxKind::AT)
        {
            let Some(after_annotation) = self.skip_annotation_lookahead(unannotated_base) else {
                break;
            };
            unannotated_base = after_annotation;
            self.skip_trivia_lookahead(&mut unannotated_base);
        }
        let initial_kind = self
            .tokens
            .get(unannotated_base)
            .map_or(self.current(), |token| token.0);
        let initial_is_primitive = matches!(
            initial_kind,
            SyntaxKind::BOOLEAN_KW
                | SyntaxKind::BYTE_KW
                | SyntaxKind::SHORT_KW
                | SyntaxKind::INT_KW
                | SyntaxKind::LONG_KW
                | SyntaxKind::CHAR_KW
                | SyntaxKind::FLOAT_KW
                | SyntaxKind::DOUBLE_KW
        );
        let mut can_be_reference_type = initial_kind.is_identifier() || initial_is_primitive;
        let mut final_type_segment_is_valid =
            initial_kind.is_type_identifier() || initial_is_primitive;
        let mut is_primitive_type = initial_is_primitive;
        let mut is_void_type = initial_kind == SyntaxKind::VOID_KW;
        let mut has_type_arguments = false;
        let mut type_is_reifiable = true;
        let mut type_has_top_level_wildcard = false;
        let mut type_has_invalid_argument = false;
        let mut has_type_annotations = unannotated_base != self.pos;
        let mut is_array_type = false;
        let mut completed_method_reference = false;
        let mut can_be_method_reference_receiver =
            !matches!(initial_kind, SyntaxKind::VOID_KW) && !initial_is_primitive;
        self.parse_primary_expr();

        loop {
            self.skip_trivia();

            match self.current() {
                // Postfix increment/decrement
                SyntaxKind::PLUS_PLUS | SyntaxKind::MINUS_MINUS => {
                    self.start_node_at(checkpoint, SyntaxKind::UNARY_EXPR);
                    self.bump();
                    self.finish_node();
                    can_be_reference_type = false;
                    can_be_method_reference_receiver = false;
                }
                // Field access or method call or class literal
                SyntaxKind::DOT => {
                    self.bump(); // DOT
                    self.skip_trivia();

                    if matches!(self.current(), SyntaxKind::THIS_KW | SyntaxKind::SUPER_KW) {
                        let qualified_receiver_kind = self.current();
                        if !can_be_reference_type
                            || !final_type_segment_is_valid
                            || is_primitive_type
                            || is_void_type
                            || has_type_arguments
                            || is_array_type
                        {
                            self.error(
                                "A qualified this/super expression requires a type name",
                                self.current_range(),
                            );
                        }
                        self.start_node_at(
                            checkpoint,
                            if qualified_receiver_kind == SyntaxKind::THIS_KW {
                                SyntaxKind::THIS_EXPR
                            } else {
                                SyntaxKind::SUPER_EXPR
                            },
                        );
                        self.bump();
                        self.finish_node();
                        can_be_reference_type = false;
                        can_be_method_reference_receiver = true;
                        final_type_segment_is_valid = false;
                        is_primitive_type = false;
                        is_void_type = false;
                    } else if self.at(SyntaxKind::NEW_KW) {
                        // Qualified class instance creation: expr.new InnerClass()
                        // JLS: Primary.new [TypeArguments] Identifier [TypeArguments] (Arguments) [ClassBody]
                        self.start_node_at(checkpoint, SyntaxKind::NEW_EXPR);
                        self.bump(); // new keyword
                        self.skip_trivia();

                        // Optional type arguments before class name
                        if self.at(SyntaxKind::LT) && self.is_type_args_ahead() {
                            self.parse_type_args();
                            self.skip_trivia();
                        }

                        // Class name (identifier)
                        if self.at_identifier() {
                            self.parse_type();
                            self.skip_trivia();
                        }

                        // Constructor arguments
                        if self.at(SyntaxKind::L_PAREN) {
                            self.parse_arg_list();
                            self.skip_trivia();

                            // Anonymous class body (optional)
                            if self.at(SyntaxKind::L_BRACE) {
                                self.parse_class_body();
                            }
                        }

                        self.finish_node();
                        can_be_reference_type = false;
                        can_be_method_reference_receiver = true;
                    } else if self.at(SyntaxKind::CLASS_KW) {
                        // Class literal: Type.class, Type[].class, int.class, etc.
                        // JLS §15.8.2: ClassLiteral
                        let valid_receiver = if is_void_type {
                            !is_array_type && !has_type_annotations
                        } else {
                            can_be_reference_type
                                && final_type_segment_is_valid
                                && type_is_reifiable
                                && !type_has_invalid_argument
                                && (!has_type_arguments || is_array_type)
                                && !has_type_annotations
                        };
                        if !valid_receiver {
                            self.error(
                                "A class literal requires a reifiable type",
                                self.current_range(),
                            );
                        }
                        self.start_node_at(checkpoint, SyntaxKind::CLASS_LITERAL_EXPR);
                        self.bump(); // class keyword
                        self.finish_node();
                        // Any type annotation has already produced the specific
                        // javac-compatible class-literal diagnostic above.
                        has_type_annotations = false;
                        can_be_reference_type = false;
                        // A class literal is itself a primary expression, so
                        // `String.class::getName` remains a legal method ref.
                        can_be_method_reference_receiver = true;
                    } else if self.at(SyntaxKind::LT) {
                        // Explicit type arguments: expr.<Type>methodName()
                        let pos_checkpoint = self.pos;
                        if self.is_type_args_ahead() {
                            if is_primitive_type || is_void_type {
                                self.error(
                                    "Primitive and void types cannot be qualified",
                                    self.current_range(),
                                );
                            }
                            self.parse_type_args();
                            self.skip_trivia();

                            // After type arguments, we should have the method name
                            if self.at_identifier() {
                                self.bump(); // consume method name
                                self.skip_trivia();

                                if self.at(SyntaxKind::L_PAREN) {
                                    // Method call with explicit type arguments
                                    self.start_node_at(checkpoint, SyntaxKind::METHOD_CALL_EXPR);
                                    self.parse_arg_list();
                                    self.finish_node();
                                } else {
                                    self.error(
                                        "Explicit type arguments must be followed by a method invocation",
                                        self.current_range(),
                                    );
                                }
                            } else {
                                // Error: type arguments without method name
                                self.error(
                                    "Expected a method name after explicit type arguments",
                                    self.current_range(),
                                );
                            }

                            // Whatever syntax preceded the dot, the result is
                            // now a method-invocation expression. It may be a
                            // method-reference receiver, but it cannot remain
                            // a class-literal or constructor-reference type.
                            can_be_reference_type = false;
                            final_type_segment_is_valid = false;
                            is_primitive_type = false;
                            is_void_type = false;
                            is_array_type = false;
                            can_be_method_reference_receiver = true;
                        } else {
                            // Not type arguments, restore and error
                            self.pos = pos_checkpoint;
                            let range = self.current_range();
                            self.error("Expected field or method name after '.'", range);
                            continue;
                        }
                    } else if self.at_identifier() {
                        // Allow contextual keywords as field/method names
                        let member_kind = self.current();
                        if is_primitive_type || is_void_type {
                            self.error(
                                "Primitive and void types cannot be qualified",
                                self.current_range(),
                            );
                            can_be_reference_type = false;
                            final_type_segment_is_valid = false;
                            is_primitive_type = false;
                            is_void_type = false;
                        }
                        self.bump(); // identifier
                        self.skip_trivia();

                        if self.at(SyntaxKind::L_PAREN) {
                            // Method call without type arguments
                            self.start_node_at(checkpoint, SyntaxKind::METHOD_CALL_EXPR);
                            self.parse_arg_list();
                            self.finish_node();
                            can_be_reference_type = false;
                        } else {
                            // Field access
                            self.start_node_at(checkpoint, SyntaxKind::FIELD_ACCESS_EXPR);
                            self.finish_node();
                            if can_be_reference_type {
                                final_type_segment_is_valid = member_kind.is_type_identifier();
                                is_primitive_type = false;
                                is_void_type = false;
                            }
                        }
                        can_be_method_reference_receiver = true;
                    } else {
                        // DOT followed by something unexpected
                        if !self.at_eof() {
                            let range = self.current_range();
                            self.error("Expected field or method name after '.'", range);
                        }
                    }
                }
                // Parameterized reference types in class literals and method
                // references. A relational `<` is left to the precedence
                // parser unless the matching `>` is followed by a type tail.
                SyntaxKind::LT if can_be_reference_type && !is_primitive_type => {
                    let Some(type_args) = self.inspect_type_arguments() else {
                        break;
                    };
                    let Some(after_type_args) = self.significant_kind_at(type_args.end) else {
                        break;
                    };
                    if !matches!(
                        after_type_args,
                        SyntaxKind::DOT | SyntaxKind::L_BRACKET | SyntaxKind::COLON_COLON
                    ) {
                        break;
                    }
                    if type_args.empty {
                        self.error(
                            "The diamond operator is not allowed in a type used by a class literal or method reference",
                            self.current_range(),
                        );
                    }
                    has_type_arguments = true;
                    type_is_reifiable &= type_args.all_unbounded_wildcards;
                    type_has_top_level_wildcard |= type_args.has_top_level_wildcard;
                    type_has_invalid_argument |= type_args.contains_bare_primitive;
                    has_type_annotations |= type_args.contains_annotation;
                    self.parse_type_args();
                }
                // Type-use annotations are permitted in an array constructor
                // reference's reference type (for example `String @A []::new`).
                // javac rejects them in class literals and after a parameterized
                // receiver, which are diagnosed by the surrounding state.
                SyntaxKind::AT if can_be_reference_type => {
                    if has_type_arguments {
                        self.error(
                            "Type annotations cannot follow type arguments in a method-reference receiver",
                            self.current_range(),
                        );
                    }
                    has_type_annotations = true;
                    while self.at(SyntaxKind::AT) {
                        self.parse_annotation();
                        self.skip_trivia();
                    }
                    if !self.at(SyntaxKind::L_BRACKET) {
                        self.error(
                            "A type annotation in a method-reference receiver must annotate an array dimension",
                            self.current_range(),
                        );
                        can_be_reference_type = false;
                    }
                }
                // Array access or array type dimensions (for array constructor references and class literals)
                SyntaxKind::L_BRACKET => {
                    // Look ahead to distinguish:
                    // - Type[]::new - array constructor reference (empty brackets before ::)
                    // - Type[].class - array class literal (empty brackets before .class)
                    // - array[expr] - array access (expression inside brackets)

                    // Peek ahead to see if this is an empty bracket followed by ::, .class, or more []
                    let is_array_type_dimensions = self.array_type_dimension_ahead();

                    if is_array_type_dimensions {
                        if is_void_type {
                            self.error(
                                "The void type cannot have array dimensions",
                                self.current_range(),
                            );
                        }
                        if !can_be_reference_type || !final_type_segment_is_valid {
                            self.error(
                                "Array constructor references and class literals require a type",
                                self.current_range(),
                            );
                        }
                        if !type_is_reifiable {
                            self.error(
                                "An array constructor reference or class literal requires a reifiable component type",
                                self.current_range(),
                            );
                        }
                        if type_has_invalid_argument {
                            self.error(
                                "Generic type arguments must be reference types",
                                self.current_range(),
                            );
                        }
                        // Array dimensions for constructor reference or class literal - consume brackets
                        // but don't wrap in ARRAY_ACCESS_EXPR. The :: or .class handler will create
                        // the appropriate node. We need to consume all contiguous []
                        while self.at(SyntaxKind::L_BRACKET) {
                            // Peek to ensure this bracket is empty
                            let mut lookahead = 1;
                            while let Some(kind) = self.peek(lookahead) {
                                if kind.is_trivia() {
                                    lookahead += 1;
                                } else {
                                    break;
                                }
                            }

                            if self.peek(lookahead) != Some(SyntaxKind::R_BRACKET) {
                                // Not empty, stop here
                                break;
                            }

                            self.bump(); // [
                            self.skip_trivia();
                            self.expect(SyntaxKind::R_BRACKET); // ]
                            self.skip_trivia();
                        }
                        is_array_type = true;
                        is_primitive_type = false;
                        is_void_type = false;
                        can_be_method_reference_receiver = true;
                    } else {
                        // Regular array access with index expression
                        self.start_node_at(checkpoint, SyntaxKind::ARRAY_ACCESS_EXPR);
                        self.bump();
                        self.skip_trivia();
                        self.parse_expression();
                        self.skip_trivia();
                        self.expect(SyntaxKind::R_BRACKET);
                        self.finish_node();
                        can_be_reference_type = false;
                        can_be_method_reference_receiver = false;
                        is_array_type = false;
                    }
                }
                // Method reference: ClassName::methodName, object::method, ClassName::new
                SyntaxKind::COLON_COLON => {
                    let mut reference_member = self.pos + 1;
                    self.skip_trivia_lookahead(&mut reference_member);
                    if self
                        .tokens
                        .get(reference_member)
                        .is_some_and(|token| token.0 == SyntaxKind::LT)
                    {
                        if let Some(type_args) = self.inspect_type_arguments_at(reference_member) {
                            reference_member = type_args.end;
                            self.skip_trivia_lookahead(&mut reference_member);
                        }
                    }
                    let ref_kind = if self
                        .tokens
                        .get(reference_member)
                        .is_some_and(|token| token.0 == SyntaxKind::NEW_KW)
                    {
                        SyntaxKind::CONSTRUCTOR_REF_EXPR
                    } else {
                        SyntaxKind::METHOD_REF_EXPR
                    };
                    self.start_node_at(checkpoint, ref_kind);
                    self.bump(); // consume ::
                    self.skip_trivia();

                    let mut has_explicit_type_arguments = false;
                    if self.at(SyntaxKind::LT) {
                        has_explicit_type_arguments = true;
                        if let Some(type_args) = self.inspect_type_arguments() {
                            if type_args.empty
                                || type_args.has_top_level_wildcard
                                || type_args.contains_bare_primitive
                            {
                                self.error(
                                    "Method-reference type arguments must be explicit reference types",
                                    self.current_range(),
                                );
                            }
                        }
                        self.parse_type_args();
                        self.skip_trivia();
                    }

                    if self.at(SyntaxKind::NEW_KW) {
                        // Constructor reference: ClassName::new, Type[]::new
                        if is_array_type {
                            if has_explicit_type_arguments {
                                self.error(
                                    "An array constructor reference cannot have type arguments",
                                    self.current_range(),
                                );
                            }
                        } else if !can_be_reference_type
                            || !final_type_segment_is_valid
                            || is_primitive_type
                            || is_void_type
                            || type_has_top_level_wildcard
                            || type_has_invalid_argument
                        {
                            self.error(
                                "A constructor reference requires a non-wildcard class or array type",
                                self.current_range(),
                            );
                        }
                        self.bump(); // consume 'new'
                    } else if self.at_identifier() {
                        // Method reference: ClassName::methodName, object::method
                        // Allow contextual keywords as method names
                        if !can_be_method_reference_receiver || type_has_invalid_argument {
                            self.error(
                                "A method reference requires an expression or reference type receiver",
                                self.current_range(),
                            );
                        }
                        self.bump(); // consume method name
                    } else {
                        let range = self.current_range();
                        self.error("Expected method name or 'new' after '::'", range);
                    }
                    self.finish_node();
                    completed_method_reference = true;
                    break;
                }
                _ => break,
            }
        }

        if has_type_annotations && !completed_method_reference {
            self.error(
                "A type annotation in an expression requires a method-reference receiver",
                self.current_range(),
            );
        }
    }

    pub(super) fn previous_significant_kind(&self) -> Option<SyntaxKind> {
        self.tokens[..self.pos]
            .iter()
            .rev()
            .find_map(|token| (!token.0.is_trivia()).then_some(token.0))
    }

    /// Inspect a type-argument list without changing the token stream. Besides
    /// locating its matching close, record the narrow JLS reifiability case:
    /// every top-level argument is exactly an unbounded wildcard.
    pub(super) fn inspect_type_arguments(&self) -> Option<TypeArgumentsLookahead> {
        self.inspect_type_arguments_at(self.pos)
    }

    fn inspect_type_arguments_at(&self, start: usize) -> Option<TypeArgumentsLookahead> {
        if self
            .tokens
            .get(start)
            .is_none_or(|token| token.0 != SyntaxKind::LT)
        {
            return None;
        }

        let mut pos = start + 1;
        let mut depth = 1usize;
        let mut argument_has_token = false;
        let mut argument_is_unbounded_wildcard = true;
        let mut all_unbounded_wildcards = true;
        let mut argument_count = 0usize;
        let mut has_top_level_wildcard = false;
        let mut contains_bare_primitive = false;
        let mut contains_annotation = false;

        while pos < self.tokens.len() {
            let kind = self.tokens[pos].0;
            if kind.is_trivia() {
                pos += 1;
                continue;
            }

            if kind == SyntaxKind::AT {
                contains_annotation = true;
                pos = self.skip_annotation_lookahead(pos)?;
                continue;
            }

            match kind {
                SyntaxKind::LT => {
                    if depth == 1 {
                        argument_has_token = true;
                        argument_is_unbounded_wildcard = false;
                    }
                    depth += 1;
                    pos += 1;
                }
                SyntaxKind::GT | SyntaxKind::SHR | SyntaxKind::USHR => {
                    let closing_count = match kind {
                        SyntaxKind::GT => 1,
                        SyntaxKind::SHR => 2,
                        SyntaxKind::USHR => 3,
                        _ => unreachable!(),
                    };
                    if closing_count > depth {
                        return None;
                    }
                    if closing_count == depth {
                        if argument_has_token {
                            argument_count += 1;
                            all_unbounded_wildcards &= argument_is_unbounded_wildcard;
                        } else if argument_count != 0 {
                            return None;
                        }
                        return Some(TypeArgumentsLookahead {
                            end: pos + 1,
                            empty: argument_count == 0,
                            all_unbounded_wildcards: argument_count != 0 && all_unbounded_wildcards,
                            has_top_level_wildcard,
                            contains_bare_primitive,
                            contains_annotation,
                        });
                    }
                    depth -= closing_count;
                    pos += 1;
                }
                SyntaxKind::COMMA if depth == 1 => {
                    if !argument_has_token {
                        return None;
                    }
                    argument_count += 1;
                    all_unbounded_wildcards &= argument_is_unbounded_wildcard;
                    argument_has_token = false;
                    argument_is_unbounded_wildcard = true;
                    pos += 1;
                }
                SyntaxKind::QUESTION => {
                    if depth == 1 {
                        has_top_level_wildcard = true;
                        if argument_has_token {
                            argument_is_unbounded_wildcard = false;
                        }
                        argument_has_token = true;
                    }
                    pos += 1;
                }
                SyntaxKind::BOOLEAN_KW
                | SyntaxKind::BYTE_KW
                | SyntaxKind::SHORT_KW
                | SyntaxKind::INT_KW
                | SyntaxKind::LONG_KW
                | SyntaxKind::CHAR_KW
                | SyntaxKind::FLOAT_KW
                | SyntaxKind::DOUBLE_KW => {
                    if !self.primitive_array_dimensions_follow(pos) {
                        contains_bare_primitive = true;
                    }
                    if depth == 1 {
                        argument_has_token = true;
                        argument_is_unbounded_wildcard = false;
                    }
                    pos += 1;
                }
                SyntaxKind::VOID_KW => {
                    contains_bare_primitive = true;
                    if depth == 1 {
                        argument_has_token = true;
                        argument_is_unbounded_wildcard = false;
                    }
                    pos += 1;
                }
                _ => {
                    if depth == 1 {
                        argument_has_token = true;
                        argument_is_unbounded_wildcard = false;
                    }
                    pos += 1;
                }
            }
        }

        None
    }

    fn primitive_array_dimensions_follow(&self, primitive_pos: usize) -> bool {
        let mut pos = primitive_pos + 1;
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
        if self
            .tokens
            .get(pos)
            .is_none_or(|token| token.0 != SyntaxKind::L_BRACKET)
        {
            return false;
        }
        pos += 1;
        self.skip_trivia_lookahead(&mut pos);
        self.tokens
            .get(pos)
            .is_some_and(|token| token.0 == SyntaxKind::R_BRACKET)
    }

    pub(super) fn significant_kind_at(&self, mut pos: usize) -> Option<SyntaxKind> {
        while let Some(token) = self.tokens.get(pos) {
            if !token.0.is_trivia() {
                return Some(token.0);
            }
            pos += 1;
        }
        None
    }

    fn array_type_dimension_ahead(&self) -> bool {
        if !self.at(SyntaxKind::L_BRACKET) {
            return false;
        }
        let mut pos = self.pos + 1;
        self.skip_trivia_lookahead(&mut pos);
        if self
            .tokens
            .get(pos)
            .is_none_or(|token| token.0 != SyntaxKind::R_BRACKET)
        {
            return false;
        }
        pos += 1;
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
        self.tokens.get(pos).is_some_and(|token| {
            matches!(
                token.0,
                SyntaxKind::COLON_COLON | SyntaxKind::DOT | SyntaxKind::L_BRACKET
            )
        })
    }

    /// Parse primary expression (literals, identifiers, this, new, etc.)
    pub(super) fn parse_primary_expr(&mut self) {
        match self.current() {
            // Literals
            SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::TEXT_BLOCK_LITERAL
            | SyntaxKind::BOOLEAN_LITERAL
            | SyntaxKind::NULL_LITERAL => {
                self.start_node(SyntaxKind::LITERAL_EXPR);
                self.bump();
                self.finish_node();
            }
            // Parenthesized expression, lambda, or cast
            SyntaxKind::L_PAREN => {
                // Disambiguate between:
                // 1. Lambda: (params) -> body OR () -> body
                // 2. Cast: (Type) expr
                // 3. Parenthesized expression: (expr)

                if self.is_lambda() {
                    self.parse_lambda_expr();
                } else if self.is_cast() {
                    self.parse_cast_expr();
                } else {
                    // Parenthesized expression. Inside the parens we are no longer
                    // at switch-case-label level, so nested lambdas parse normally.
                    let prev_in_switch_case = self.in_switch_case;
                    self.in_switch_case = false;
                    self.start_node(SyntaxKind::PAREN_EXPR);
                    self.bump();
                    self.skip_trivia();
                    self.parse_expression();
                    self.skip_trivia();
                    self.expect(SyntaxKind::R_PAREN);
                    self.finish_node();
                    self.in_switch_case = prev_in_switch_case;
                }
            }
            // this
            SyntaxKind::THIS_KW => {
                self.start_node(SyntaxKind::THIS_EXPR);
                self.bump();
                self.finish_node();
            }
            // super - can be super(), super.field, super.method(), or just super
            SyntaxKind::SUPER_KW => {
                let super_checkpoint = self.checkpoint();
                self.start_node(SyntaxKind::SUPER_EXPR);
                self.bump();
                self.finish_node();
                self.skip_trivia();

                // Check for super() constructor call — wrap SUPER_EXPR as child
                if self.at(SyntaxKind::L_PAREN) {
                    self.start_node_at(super_checkpoint, SyntaxKind::METHOD_CALL_EXPR);
                    self.parse_arg_list();
                    self.finish_node();
                }
            }
            // new expression
            SyntaxKind::NEW_KW => {
                self.parse_new_expr();
            }
            // switch expression
            SyntaxKind::SWITCH_KW => {
                self.parse_switch_expr();
            }
            // Identifier, method call, or single-parameter lambda
            // Use at_identifier() to handle all contextual keywords
            _ if self.at_identifier() => {
                // Check if it's a single-parameter lambda: ident -> expr
                if self.is_single_param_lambda() {
                    self.parse_lambda_expr();
                } else {
                    let name_checkpoint = self.checkpoint();
                    let unqualified_yield_call = self.at(SyntaxKind::YIELD_KW) && {
                        let mut lookahead = 1;
                        while self.peek(lookahead).is_some_and(SyntaxKind::is_trivia) {
                            lookahead += 1;
                        }
                        self.peek(lookahead) == Some(SyntaxKind::L_PAREN)
                    };
                    if unqualified_yield_call {
                        self.error(
                            "An invocation of a method named 'yield' must be qualified",
                            self.current_range(),
                        );
                    }
                    self.start_node(SyntaxKind::NAME_EXPR);
                    self.bump();
                    self.finish_node();
                    self.skip_trivia();

                    // Check for method call — wrap NAME_EXPR as child
                    if self.at(SyntaxKind::L_PAREN) {
                        self.start_node_at(name_checkpoint, SyntaxKind::METHOD_CALL_EXPR);
                        self.parse_arg_list();
                        self.finish_node();
                    }
                }
            }
            // Array initializer
            SyntaxKind::L_BRACE => {
                self.parse_array_initializer();
            }
            // Primitive type class literals: int.class, boolean.class, etc.
            SyntaxKind::INT_KW
            | SyntaxKind::LONG_KW
            | SyntaxKind::SHORT_KW
            | SyntaxKind::BYTE_KW
            | SyntaxKind::CHAR_KW
            | SyntaxKind::FLOAT_KW
            | SyntaxKind::DOUBLE_KW
            | SyntaxKind::BOOLEAN_KW
            | SyntaxKind::VOID_KW => {
                // Parse primitive type, then expect .class
                self.start_node(SyntaxKind::NAME_EXPR);
                self.bump(); // consume primitive keyword
                self.finish_node();
            }
            // A reference type in an array constructor reference may carry a
            // leading type-use annotation: `@A String[]::new`.
            SyntaxKind::AT => {
                self.start_node(SyntaxKind::NAME_EXPR);
                while self.at(SyntaxKind::AT) {
                    self.parse_annotation();
                    self.skip_trivia();
                }
                if self.at_identifier()
                    || matches!(
                        self.current(),
                        SyntaxKind::BOOLEAN_KW
                            | SyntaxKind::BYTE_KW
                            | SyntaxKind::SHORT_KW
                            | SyntaxKind::INT_KW
                            | SyntaxKind::LONG_KW
                            | SyntaxKind::CHAR_KW
                            | SyntaxKind::FLOAT_KW
                            | SyntaxKind::DOUBLE_KW
                    )
                {
                    self.bump();
                } else {
                    self.error(
                        "Expected a type after type-use annotations",
                        self.current_range(),
                    );
                }
                self.finish_node();
            }
            _ => {
                self.error(
                    format!("Unexpected token in expression: {:?}", self.current()),
                    self.current_range(),
                );
                // Error recovery: create an error node and skip
                self.start_node(SyntaxKind::ERROR);
                self.bump();
                self.finish_node();
            }
        }
    }

    /// Parse new expression
    pub(super) fn parse_new_expr(&mut self) {
        self.start_node(SyntaxKind::NEW_EXPR);

        self.expect(SyntaxKind::NEW_KW);
        self.skip_trivia();

        // Type
        self.parse_type();

        self.skip_trivia();

        // Constructor arguments or array dimensions
        if self.at(SyntaxKind::L_PAREN) {
            self.parse_arg_list();
            self.skip_trivia();

            // Anonymous class body (optional)
            if self.at(SyntaxKind::L_BRACE) {
                self.parse_class_body();
            }
        } else if self.at(SyntaxKind::L_BRACKET) {
            // Array creation with explicit dimensions: new int[5] or new int[5][]
            while self.at(SyntaxKind::L_BRACKET) {
                self.bump();
                self.skip_trivia();

                // Dimension expression (optional)
                if !self.at(SyntaxKind::R_BRACKET) {
                    self.parse_expression();
                    self.skip_trivia();
                }

                self.expect(SyntaxKind::R_BRACKET);
                self.skip_trivia();
            }

            // Array initializer (optional)
            if self.at(SyntaxKind::L_BRACE) {
                self.parse_array_initializer();
            }
        } else if self.at(SyntaxKind::L_BRACE) {
            // Array initializer when type already includes array dimensions
            // e.g., `new byte[]{1, 2}` where parse_type() consumed `byte[]`
            self.parse_array_initializer();
        }

        self.finish_node();
    }

    /// Parse array initializer
    pub(super) fn parse_array_initializer(&mut self) {
        self.start_node(SyntaxKind::ARRAY_INIT_EXPR);

        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        if !self.at(SyntaxKind::R_BRACE) {
            loop {
                let start_pos = self.pos;

                // Element can be expression or nested array initializer
                if self.at(SyntaxKind::L_BRACE) {
                    self.parse_array_initializer();
                } else {
                    self.parse_expression();
                }

                // BUGFIX: If no tokens were consumed, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }

                self.skip_trivia();

                if self.at(SyntaxKind::COMMA) {
                    self.bump();
                    self.skip_trivia();

                    // JLS §10.6: Array initializers allow trailing commas
                    // { elem1, elem2, } is valid
                    if self.at(SyntaxKind::R_BRACE) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
    }

    /// Parse argument list
    pub(super) fn parse_arg_list(&mut self) {
        self.start_node(SyntaxKind::ARG_LIST);

        // Arguments are no longer at switch-case-label level, so nested lambdas
        // (e.g. `when foo((a, b) -> a + b) ->`) must be recognized normally.
        let prev_in_switch_case = self.in_switch_case;
        self.in_switch_case = false;

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        if !self.at(SyntaxKind::R_PAREN) {
            loop {
                let start_pos = self.pos;
                self.parse_expression();

                // BUGFIX: If parse_expression didn't consume any tokens, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }

                self.skip_trivia();

                if self.at(SyntaxKind::COMMA) {
                    self.bump();
                    self.skip_trivia();

                    // Support trailing comma
                    if self.at(SyntaxKind::R_PAREN) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.expect(SyntaxKind::R_PAREN);

        self.in_switch_case = prev_in_switch_case;

        self.finish_node();
    }

    // ============================================================================
    // Lambda Expressions
    // ============================================================================

    /// Check if we're at a lambda expression
    pub(super) fn is_lambda(&self) -> bool {
        // Look ahead for arrow (->) pattern
        // Patterns:
        // 1. () -> ...
        // 2. (param) -> ...
        // 3. (Type param) -> ...
        // 4. (param1, param2) -> ...

        // At the top level of a switch case label / pattern guard, a `(...) ->`
        // is the case arrow, not a lambda (e.g. `case Integer i when (i > 5) -> 1`).
        // Nested contexts (arg lists, parenthesized sub-expressions) clear this
        // flag so genuine nested lambdas still parse.
        if self.in_switch_case {
            return false;
        }

        if !self.at(SyntaxKind::L_PAREN) {
            return false;
        }

        let mut lookahead = 1;
        let mut depth = 1;
        let max_lookahead = 50;

        // Scan to matching R_PAREN
        while lookahead < max_lookahead && depth > 0 {
            match self.peek(lookahead) {
                Some(SyntaxKind::L_PAREN) => depth += 1,
                Some(SyntaxKind::R_PAREN) => depth -= 1,
                Some(SyntaxKind::EOF) | None => return false,
                _ => {}
            }
            lookahead += 1;
        }

        if depth != 0 {
            return false;
        }

        // Check for arrow after R_PAREN (skip trivia)
        while lookahead < max_lookahead {
            match self.peek(lookahead) {
                Some(SyntaxKind::ARROW) => return true,
                Some(kind) if kind.is_trivia() => lookahead += 1,
                _ => return false,
            }
        }

        false
    }

    /// Check if we're at a single-parameter lambda (without parentheses)
    pub(super) fn is_single_param_lambda(&self) -> bool {
        // If we're parsing a switch case pattern, don't interpret arrows as lambda arrows
        if self.in_switch_case {
            return false;
        }

        // Pattern: ident -> ... (including contextual keywords)
        if !self.at_identifier() {
            return false;
        }

        let mut lookahead = 1;
        let max_lookahead = 10;

        // Skip trivia and look for arrow
        while lookahead < max_lookahead {
            match self.peek(lookahead) {
                Some(SyntaxKind::ARROW) => return true,
                Some(kind) if kind.is_trivia() => lookahead += 1,
                _ => return false,
            }
        }

        false
    }

    /// Parse a lambda expression
    pub(super) fn parse_lambda_expr(&mut self) {
        self.start_node(SyntaxKind::LAMBDA_EXPR);

        // Parse parameters
        if self.at(SyntaxKind::L_PAREN) {
            // Multiple parameters or explicit parameter list
            self.bump(); // (
            self.skip_trivia();

            if !self.at(SyntaxKind::R_PAREN) {
                let first_kind = self.parse_lambda_parameter();
                self.skip_trivia();

                while self.eat(SyntaxKind::COMMA) {
                    self.skip_trivia();
                    let start_pos = self.pos;
                    let parameter_range = self.current_range();
                    let next_kind = self.parse_lambda_parameter();
                    if first_kind.is_some() && next_kind != first_kind {
                        self.error(
                            "Lambda parameters cannot mix concise, `var`, and declared-type forms",
                            parameter_range,
                        );
                    }
                    // BUGFIX: If parse_lambda_parameter didn't consume any tokens, break to prevent infinite loop
                    if self.pos == start_pos {
                        break;
                    }
                    self.skip_trivia();
                }
            }

            self.expect(SyntaxKind::R_PAREN);
            self.skip_trivia();
        } else if self.at_identifier() {
            // Single parameter without parentheses (including contextual keywords)
            self.start_node(SyntaxKind::PARAM);
            self.bump();
            self.finish_node();
            self.skip_trivia();
        }

        // Arrow
        self.expect(SyntaxKind::ARROW);
        self.skip_trivia();

        // A yield statement cannot target a switch expression across a lambda
        // boundary. A switch expression inside this body establishes its own
        // context in the usual way.
        let enclosing_switch_expression = self.in_switch_expr;
        self.in_switch_expr = false;

        // Body: expression or block
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_block();
        } else {
            self.parse_expression();
        }
        self.in_switch_expr = enclosing_switch_expression;

        self.finish_node();
    }

    /// Parse one lambda parameter and classify which list grammar it uses.
    fn parse_lambda_parameter(&mut self) -> Option<LambdaParameterKind> {
        self.start_node(SyntaxKind::PARAM);

        // Lambda parameters can be:
        // 1. Just an identifier: x
        // 2. Type and identifier: String x
        // 3. Generic type and identifier: List<String> x
        // 4. final Type identifier: final String x
        // 5. Annotations on types: @NonNull String x

        // VariableModifier is a repeated choice of annotation or `final`.
        // Keeping it as a loop preserves legal source ordering such as
        // `@A final var value`.
        let mut has_modifiers = false;
        loop {
            if self.at(SyntaxKind::FINAL_KW) {
                has_modifiers = true;
                self.start_node(SyntaxKind::MODIFIER);
                self.bump();
                self.finish_node();
                self.skip_trivia();
            } else if self.at(SyntaxKind::AT) {
                has_modifiers = true;
                self.parse_annotation();
                self.skip_trivia();
            } else {
                break;
            }
        }

        let kind = if has_modifiers {
            // Typed parameter required: final Type name or @Anno Type name
            let inferred = self.at(SyntaxKind::VAR_KW) && !self.identifier_starts_qualified_name();
            self.parse_lambda_parameter_type();
            self.skip_trivia();
            self.expect_identifier();
            Some(if inferred {
                LambdaParameterKind::InferredWithVar
            } else {
                LambdaParameterKind::Declared
            })
        }
        // For IDENTs, need to distinguish between type names and parameter names
        // For primitives (int, long, etc.), always a typed parameter
        else if self.at_identifier() {
            // Could be either "Type name" or just "name"
            // Look ahead to check if followed by another identifier or generic type args
            let mut lookahead = 1;
            // Skip trivia to find the next non-trivia token
            while let Some(kind) = self.peek(lookahead) {
                if kind.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }

            // Check if this is a typed parameter
            let is_typed = self.peek(lookahead).is_some_and(|kind| {
                matches!(
                    kind,
                    SyntaxKind::DOT | SyntaxKind::LT | SyntaxKind::L_BRACKET
                ) || is_identifier_kind(kind)
            });

            if is_typed {
                // Typed parameter: Type name (could be generic or array)
                let inferred =
                    self.at(SyntaxKind::VAR_KW) && !self.identifier_starts_qualified_name();
                self.parse_lambda_parameter_type();
                self.skip_trivia();
                self.expect_identifier();
                Some(if inferred {
                    LambdaParameterKind::InferredWithVar
                } else {
                    LambdaParameterKind::Declared
                })
            } else {
                // Untyped parameter: just name
                self.bump();
                Some(LambdaParameterKind::Concise)
            }
        } else if self.is_type_start() {
            // Primitive type (int, long, etc.) - always typed
            self.parse_lambda_parameter_type();
            self.skip_trivia();
            self.expect_identifier();
            Some(LambdaParameterKind::Declared)
        } else {
            None
        };

        self.finish_node();
        kind
    }

    // ============================================================================
    // Cast Expressions
    // ============================================================================

    /// Check if we're at a cast expression
    pub(super) fn is_cast(&self) -> bool {
        // Look ahead for pattern: (Type) expr or (@Annotation Type) expr
        // Must distinguish from: (expr)

        if !self.at(SyntaxKind::L_PAREN) {
            return false;
        }

        let mut lookahead = 1;
        let max_lookahead = 100; // Increased from 20 to handle complex fully-qualified generic types

        // Skip trivia after (
        while lookahead < max_lookahead {
            if let Some(kind) = self.peek(lookahead) {
                if kind.is_trivia() {
                    lookahead += 1;
                    continue;
                }
                break;
            }
            return false;
        }

        // Skip type annotations (JLS §9.7.4)
        // Pattern: (@NonNull String) obj, (@NonNull @Immutable List) obj
        while lookahead < max_lookahead {
            if self.peek(lookahead) == Some(SyntaxKind::AT) {
                lookahead += 1;

                // Skip trivia after @
                while lookahead < max_lookahead {
                    if let Some(kind) = self.peek(lookahead) {
                        if kind.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    } else {
                        return false;
                    }
                }

                // Skip annotation name (IDENT or qualified.name.IDENT)
                // Pattern: NonNull or java.lang.NonNull
                loop {
                    if self.peek(lookahead).is_some_and(SyntaxKind::is_identifier) {
                        lookahead += 1;

                        // Skip trivia after IDENT
                        while lookahead < max_lookahead {
                            if let Some(kind) = self.peek(lookahead) {
                                if kind.is_trivia() {
                                    lookahead += 1;
                                } else {
                                    break;
                                }
                            } else {
                                return false;
                            }
                        }

                        // Check for DOT (qualified name)
                        if self.peek(lookahead) == Some(SyntaxKind::DOT) {
                            lookahead += 1;

                            // Skip trivia after DOT
                            while lookahead < max_lookahead {
                                if let Some(kind) = self.peek(lookahead) {
                                    if kind.is_trivia() {
                                        lookahead += 1;
                                    } else {
                                        break;
                                    }
                                } else {
                                    return false;
                                }
                            }
                            // Continue loop to parse next IDENT
                        } else {
                            // No DOT, annotation name complete
                            break;
                        }
                    } else {
                        // Not an IDENT, malformed annotation
                        return false;
                    }
                }

                // Skip annotation arguments if present: @NonNull() or @SuppressWarnings("unchecked")
                if self.peek(lookahead) == Some(SyntaxKind::L_PAREN) {
                    lookahead += 1;
                    let mut paren_depth = 1;
                    while lookahead < max_lookahead && paren_depth > 0 {
                        match self.peek(lookahead) {
                            Some(SyntaxKind::L_PAREN) => {
                                paren_depth += 1;
                                lookahead += 1;
                            }
                            Some(SyntaxKind::R_PAREN) => {
                                paren_depth -= 1;
                                lookahead += 1;
                            }
                            Some(SyntaxKind::EOF) | None => return false,
                            _ => {
                                lookahead += 1;
                            }
                        }
                    }
                }
                // Skip trivia after annotation
                while lookahead < max_lookahead {
                    if let Some(kind) = self.peek(lookahead) {
                        if kind.is_trivia() {
                            lookahead += 1;
                            continue;
                        }
                        break;
                    }
                    return false;
                }
            } else {
                break;
            }
        }

        // Check if it starts with a type
        if let Some(kind) = self.peek(lookahead) {
            let primitive = matches!(
                kind,
                SyntaxKind::BOOLEAN_KW
                    | SyntaxKind::BYTE_KW
                    | SyntaxKind::SHORT_KW
                    | SyntaxKind::INT_KW
                    | SyntaxKind::LONG_KW
                    | SyntaxKind::CHAR_KW
                    | SyntaxKind::FLOAT_KW
                    | SyntaxKind::DOUBLE_KW
            );
            if !primitive && !kind.is_type_identifier() {
                if !kind.is_identifier() {
                    return false;
                }
                let mut after_identifier = lookahead + 1;
                while self
                    .peek(after_identifier)
                    .is_some_and(SyntaxKind::is_trivia)
                {
                    after_identifier += 1;
                }
                if self.peek(after_identifier) != Some(SyntaxKind::DOT) {
                    return false;
                }
            }

            lookahead += 1;

            // Look for R_PAREN followed by something that looks like an expression start
            // Also check that there are no binary operators inside (which would make it an expression)
            let mut depth = 1;
            let mut type_arg_depth = 0; // Track type argument < > nesting depth persistently

            while lookahead < max_lookahead && depth > 0 {
                match self.peek(lookahead) {
                    // Track type argument nesting for wildcard disambiguation
                    Some(SyntaxKind::LT) if depth >= 1 => {
                        type_arg_depth += 1;
                    }
                    Some(SyntaxKind::GT) if depth >= 1 && type_arg_depth > 0 => {
                        type_arg_depth -= 1;
                    }
                    Some(SyntaxKind::SHR) if depth >= 1 && type_arg_depth > 0 => {
                        // >> counts as two > (only relevant inside generics)
                        if type_arg_depth >= 2 {
                            type_arg_depth -= 2;
                        } else {
                            type_arg_depth = 0;
                        }
                    }
                    Some(SyntaxKind::USHR) if depth >= 1 && type_arg_depth > 0 => {
                        // >>> counts as three > (only relevant inside generics)
                        if type_arg_depth >= 3 {
                            type_arg_depth -= 3;
                        } else {
                            type_arg_depth = 0;
                        }
                    }

                    Some(SyntaxKind::L_PAREN) => depth += 1,
                    Some(SyntaxKind::R_PAREN) => {
                        depth -= 1;
                        if depth == 0 {
                            // Found matching R_PAREN, check what follows
                            lookahead += 1;
                            // Skip trivia
                            while lookahead < max_lookahead {
                                if let Some(kind) = self.peek(lookahead) {
                                    if kind.is_trivia() {
                                        lookahead += 1;
                                        continue;
                                    }
                                    // Cast if followed by expression-like tokens
                                    // Use is_identifier() to include contextual keywords (record, when, var, etc.)
                                    return kind.is_identifier()
                                        || matches!(
                                            kind,
                                            SyntaxKind::INT_LITERAL
                                                | SyntaxKind::LONG_LITERAL
                                                | SyntaxKind::FLOAT_LITERAL
                                                | SyntaxKind::DOUBLE_LITERAL
                                                | SyntaxKind::STRING_LITERAL
                                                | SyntaxKind::CHAR_LITERAL
                                                | SyntaxKind::BOOLEAN_LITERAL
                                                | SyntaxKind::NULL_LITERAL
                                                | SyntaxKind::THIS_KW
                                                | SyntaxKind::SUPER_KW
                                                | SyntaxKind::NEW_KW
                                                | SyntaxKind::L_PAREN
                                                | SyntaxKind::BANG
                                                | SyntaxKind::TILDE
                                                | SyntaxKind::PLUS
                                                | SyntaxKind::MINUS
                                                | SyntaxKind::PLUS_PLUS
                                                | SyntaxKind::MINUS_MINUS
                                        );
                                }
                                return false;
                            }
                        }
                    }
                    // If we find binary operators at depth 1, it's an expression, not a cast
                    // NOTE: We intentionally do NOT include QUESTION here because:
                    // - QUESTION can be a ternary operator (expression): (a < b ? c : d)
                    // - QUESTION can also be a wildcard type argument (cast): (List<?>) x
                    // We handle QUESTION separately below with context-aware logic
                    Some(kind)
                        if depth == 1
                            && matches!(
                                kind,
                                SyntaxKind::SHL
                                    | SyntaxKind::SHR
                                    | SyntaxKind::USHR
                                    | SyntaxKind::PLUS
                                    | SyntaxKind::MINUS
                                    | SyntaxKind::STAR
                                    | SyntaxKind::SLASH
                                    | SyntaxKind::PERCENT
                                    | SyntaxKind::PIPE_PIPE
                                    | SyntaxKind::AMP_AMP
                                    | SyntaxKind::EQ_EQ
                                    | SyntaxKind::BANG_EQ
                            ) =>
                    {
                        return false
                    }
                    // Handle AMP (&): distinguish intersection type from bitwise AND
                    // Intersection types in casts: (Serializable & Comparable) obj
                    // Bitwise AND in expressions: (a & b)
                    // If a type identifier follows the &, it's an intersection type
                    Some(SyntaxKind::AMP) if depth == 1 && type_arg_depth == 0 => {
                        // Check if a type name follows the &
                        let mut peek_ahead = lookahead + 1;
                        // Skip trivia after &
                        while peek_ahead < max_lookahead {
                            if let Some(kind) = self.peek(peek_ahead) {
                                if kind.is_trivia() {
                                    peek_ahead += 1;
                                    continue;
                                }
                                break;
                            }
                            return false;
                        }
                        // Check if what follows looks like a type (identifier or primitive)
                        if let Some(kind) = self.peek(peek_ahead) {
                            if kind.is_type_identifier()
                                || matches!(
                                    kind,
                                    SyntaxKind::BOOLEAN_KW
                                        | SyntaxKind::BYTE_KW
                                        | SyntaxKind::SHORT_KW
                                        | SyntaxKind::INT_KW
                                        | SyntaxKind::LONG_KW
                                        | SyntaxKind::CHAR_KW
                                        | SyntaxKind::FLOAT_KW
                                        | SyntaxKind::DOUBLE_KW
                                )
                            {
                                // Looks like an intersection type, continue scanning
                            } else {
                                // Not followed by a type, treat as bitwise AND expression
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    // Handle DOT: distinguish qualified inner types from expressions
                    // Pattern C: Graph<?>.Edge - qualified inner type in cast
                    // After closing type arguments (type_arg_depth==0), DOT followed by
                    // identifier indicates a qualified inner type
                    Some(SyntaxKind::DOT) if depth == 1 && type_arg_depth == 0 => {
                        // Look ahead to see if this is a qualified inner type
                        let mut peek_ahead = lookahead + 1;
                        // Skip trivia after DOT
                        while peek_ahead < max_lookahead {
                            if let Some(kind) = self.peek(peek_ahead) {
                                if kind.is_trivia() {
                                    peek_ahead += 1;
                                    continue;
                                }
                                break;
                            }
                            return false;
                        }

                        // Check if followed by identifier (qualified inner type)
                        if let Some(kind) = self.peek(peek_ahead) {
                            if kind.is_type_identifier() {
                                // DOT followed by identifier at top level after generics
                                // This is a qualified inner type: Outer<T>.Inner
                                // The inner type may also have type arguments: Outer<T>.Inner<U>
                                // Continue scanning to include potential type arguments
                                // NOTE: We don't increment type_arg_depth here because we're
                                // just passing through the DOT. When we encounter the next <,
                                // the LT handler will increment type_arg_depth appropriately.
                            } else {
                                // DOT not followed by identifier - likely member access
                                // This makes it an expression, not a cast
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    // Handle QUESTION: distinguish ternary operator from wildcard type
                    // If ? is inside type arguments (type_arg_depth > 0), it's a wildcard
                    // If not, it's a ternary operator
                    Some(SyntaxKind::QUESTION) if depth == 1 && type_arg_depth == 0 => {
                        // Use the persistent type_arg_depth counter
                        // If we're inside type arguments (unmatched <), this ? is a wildcard
                        // If not, it's likely a ternary operator
                        return false; // Ternary operator, not a cast
                    }
                    Some(SyntaxKind::EOF) | None => return false,
                    _ => {}
                }
                lookahead += 1;
            }
        }

        false
    }

    /// Parse a cast expression
    pub(super) fn parse_cast_expr(&mut self) {
        self.start_node(SyntaxKind::CAST_EXPR);

        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Parse type annotations (JLS §9.7.4, Java 8+)
        // Examples: (@NonNull String) obj, (@NonNull @Immutable List<String>) items
        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }

        // Parse the type (possibly an intersection type: Type & Type & ...)
        self.parse_type();
        self.skip_trivia();

        // Check for intersection type: (Serializable & Comparable) obj
        if self.at(SyntaxKind::AMP) {
            // We have an intersection type - wrap everything in INTERSECTION_TYPE
            // Note: The first type is already parsed, we need to continue with more
            self.start_node(SyntaxKind::INTERSECTION_TYPE);

            while self.at(SyntaxKind::AMP) {
                self.bump(); // consume &
                self.skip_trivia();

                // Parse annotations on intersection type components
                while self.at(SyntaxKind::AT) {
                    self.parse_annotation();
                    self.skip_trivia();
                }

                let start_pos = self.pos;
                self.parse_type();

                // Prevent infinite loop if parse_type didn't consume anything
                if self.pos == start_pos {
                    break;
                }

                self.skip_trivia();
            }

            self.finish_node();
        }

        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.parse_unary_expr();

        self.finish_node();
    }

    /// Check if the upcoming < token starts type arguments or is a comparison operator
    ///
    /// This helps disambiguate patterns like:
    /// - `.method.<String>call()` - type arguments (return true)
    /// - `x < y` - comparison (return false)
    ///
    /// Returns true if what follows looks like valid type arguments followed by a method name and (.
    /// Pattern: `.<Type, Type2>methodName(`
    pub(super) fn is_type_args_ahead(&self) -> bool {
        if !self.at(SyntaxKind::LT) {
            return false;
        }

        let mut lookahead = 1;
        let max_lookahead = 50;
        // We're currently AT the opening <, and lookahead=1 means we're looking at the first
        // token INSIDE the type arguments. So we start at depth 1.
        let mut depth = 1;

        // Scan through the type arguments
        while lookahead < max_lookahead {
            if let Some(kind) = self.peek(lookahead) {
                if kind.is_trivia() {
                    lookahead += 1;
                    continue;
                }

                if kind.is_identifier() {
                    lookahead += 1;
                    continue;
                }

                match kind {
                    // Type-like starts (primitives and wildcards)
                    SyntaxKind::BOOLEAN_KW
                    | SyntaxKind::BYTE_KW
                    | SyntaxKind::SHORT_KW
                    | SyntaxKind::INT_KW
                    | SyntaxKind::LONG_KW
                    | SyntaxKind::CHAR_KW
                    | SyntaxKind::FLOAT_KW
                    | SyntaxKind::DOUBLE_KW
                    | SyntaxKind::QUESTION => {
                        // Valid type tokens - just consume them
                        lookahead += 1;
                    }
                    // Nested generic
                    SyntaxKind::LT => {
                        lookahead += 1;
                        depth += 1;
                    }
                    // Closing brackets. The lexer keeps adjacent closers as shift tokens, so one
                    // token can close one, two, or three generic nesting levels.
                    SyntaxKind::GT | SyntaxKind::SHR | SyntaxKind::USHR => {
                        let closing_count = match kind {
                            SyntaxKind::GT => 1,
                            SyntaxKind::SHR => 2,
                            SyntaxKind::USHR => 3,
                            _ => unreachable!(),
                        };
                        if depth < closing_count {
                            return false;
                        }
                        depth -= closing_count;
                        lookahead += 1;
                        if depth == 0 {
                            return self.is_method_call_after_type_args(lookahead, max_lookahead);
                        }
                    }
                    // Comma separates type arguments, EXTENDS/SUPER for wildcard bounds
                    SyntaxKind::COMMA | SyntaxKind::EXTENDS_KW | SyntaxKind::SUPER_KW
                        if depth >= 1 =>
                    {
                        lookahead += 1;
                    }
                    // Punctuation that might appear in types
                    SyntaxKind::L_BRACKET | SyntaxKind::R_BRACKET | SyntaxKind::DOT => {
                        lookahead += 1;
                    }
                    // If we see something unexpected at depth 1, not type arguments
                    _ if depth >= 1 => return false,
                    // Otherwise not valid
                    _ => return false,
                }
            } else {
                return false;
            }
        }

        false
    }

    /// Check for the `methodName(` suffix required after explicit type arguments.
    fn is_method_call_after_type_args(&self, mut lookahead: usize, max_lookahead: usize) -> bool {
        while lookahead < max_lookahead && self.peek(lookahead).is_some_and(SyntaxKind::is_trivia) {
            lookahead += 1;
        }

        let Some(method_name) = self.peek(lookahead) else {
            return false;
        };
        if !method_name.is_identifier() {
            return false;
        }
        lookahead += 1;

        while lookahead < max_lookahead && self.peek(lookahead).is_some_and(SyntaxKind::is_trivia) {
            lookahead += 1;
        }
        self.peek(lookahead) == Some(SyntaxKind::L_PAREN)
    }

    // ============================================================================
    // Names and Utilities
    // ============================================================================

    /// Parse a qualified name (e.g., java.util.List)
    pub(super) fn parse_qualified_name(&mut self) {
        self.start_node(SyntaxKind::QUALIFIED_NAME);

        loop {
            // Use expect_identifier to handle contextual keywords like `record`
            if !self.expect_identifier() {
                break;
            }
            self.skip_trivia();

            if self.at(SyntaxKind::DOT) {
                // Look ahead past potential type-use annotations (JSR 308, Java 8+)
                // Examples: Map.@Nullable Entry, com.example.@NonNull User
                let mut lookahead_pos = self.pos + 1; // skip DOT
                self.skip_trivia_lookahead(&mut lookahead_pos);

                // Skip past any annotations
                while lookahead_pos < self.tokens.len()
                    && self.tokens[lookahead_pos].0 == SyntaxKind::AT
                {
                    if let Some(next_pos) = self.skip_annotation_lookahead(lookahead_pos) {
                        lookahead_pos = next_pos;
                        self.skip_trivia_lookahead(&mut lookahead_pos);
                    } else {
                        break;
                    }
                }

                // Check if there's an identifier after the annotations
                if lookahead_pos < self.tokens.len()
                    && is_identifier_kind(self.tokens[lookahead_pos].0)
                {
                    self.bump(); // consume DOT
                    self.skip_trivia();

                    // Consume any annotations between DOT and identifier
                    while self.at(SyntaxKind::AT) {
                        self.parse_annotation();
                        self.skip_trivia();
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        self.finish_node();
    }

    /// Whether the current identifier is followed by a qualified-name dot.
    /// This distinguishes the restricted local-variable type `var` from a
    /// legal package qualifier such as `var.example.Widget`.
    pub(super) fn identifier_starts_qualified_name(&self) -> bool {
        let mut lookahead = self.pos + 1;
        self.skip_trivia_lookahead(&mut lookahead);
        self.tokens
            .get(lookahead)
            .is_some_and(|token| token.0 == SyntaxKind::DOT)
    }

    /// Parse a qualified type name. Package qualifiers use `Identifier`, while
    /// the final segment must satisfy the narrower JLS `TypeIdentifier` rule.
    pub(super) fn parse_qualified_type_name(&mut self) {
        self.start_node(SyntaxKind::QUALIFIED_NAME);

        loop {
            let has_qualified_segment = self.identifier_starts_qualified_name();
            let accepted = if has_qualified_segment {
                self.expect_identifier()
            } else {
                self.expect_type_identifier()
            };
            if !accepted {
                break;
            }
            self.skip_trivia();

            if !has_qualified_segment || !self.at(SyntaxKind::DOT) {
                break;
            }
            self.bump();
            self.skip_trivia();

            // JSR 308 permits annotations between a qualifier and the next
            // type segment: `pkg.Outer.@Nullable Inner`.
            while self.at(SyntaxKind::AT) {
                self.parse_annotation();
                self.skip_trivia();
            }
        }

        self.finish_node();
    }
}
