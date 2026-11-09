// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Statement Parsing
//!
//! This module contains all statement parsing methods for the Java parser.
//!
//! Statement types:
//! - Block statements ({ ... })
//! - Control flow: if, while, do-while, for, for-each
//! - Switch statements and expressions (Java 14+)
//! - Exception handling: try-catch-finally, try-with-resources
//! - Jump statements: return, throw, break, continue, yield
//! - Other: synchronized, assert, labeled, empty

use crate::parser::is_identifier_kind;
use crate::syntax::SyntaxKind;

impl super::Parser<'_> {
    // ============================================================================
    // Statements
    // ============================================================================

    /// Parse a block statement
    ///
    /// Grammar:
    /// ```text
    /// Block:
    ///     { {Statement} }
    /// ```
    pub(super) fn parse_block(&mut self) {
        self.start_node(SyntaxKind::BLOCK_STMT);

        self.expect(SyntaxKind::L_BRACE);

        self.skip_trivia();

        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let start_pos = self.pos;
            self.parse_statement();

            // BUGFIX: If parse_statement didn't consume any tokens, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }

            self.skip_trivia();
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
    }

    /// Parse a constructor body
    ///
    /// Constructor bodies are special blocks that can start with an explicit constructor
    /// invocation: this(...) or super(...).
    ///
    /// JLS §8.8.7: Constructor Body
    ///   `ConstructorBody`:
    ///     { [`ExplicitConstructorInvocation`] [`BlockStatements`] }
    ///
    ///   `ExplicitConstructorInvocation`:
    ///     [`TypeArguments`] this ( [`ArgumentList`] ) ;
    ///     [`TypeArguments`] super ( [`ArgumentList`] ) ;
    ///     `ExpressionName` . [`TypeArguments`] super ( [`ArgumentList`] ) ;
    ///     Primary . [`TypeArguments`] super ( [`ArgumentList`] ) ;
    pub(super) fn parse_constructor_body(&mut self) {
        self.start_node(SyntaxKind::BLOCK_STMT);

        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        // Check for explicit constructor invocation (this(...) or super(...))
        if self.is_explicit_constructor_invocation() {
            self.parse_explicit_constructor_invocation();
            self.skip_trivia();
        }

        // Parse remaining statements
        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let start_pos = self.pos;
            self.parse_statement();

            // BUGFIX: If parse_statement didn't consume any tokens, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }

            self.skip_trivia();
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
    }

    /// Check if we're at an explicit constructor invocation
    ///
    /// Patterns:
    ///   this(...)
    ///   super(...)
    ///   <TypeArgs> this(...)
    ///   <TypeArgs> super(...)
    ///   Primary.super(...)  (inner class)
    ///   ExpressionName.super(...)  (inner class)
    fn is_explicit_constructor_invocation(&self) -> bool {
        // Simple cases: this( or super(
        if self.at(SyntaxKind::THIS_KW) || self.at(SyntaxKind::SUPER_KW) {
            // Look ahead to check for '('
            let mut lookahead = 1;
            while lookahead < self.tokens.len() && self.pos + lookahead < self.tokens.len() {
                let kind = self.tokens[self.pos + lookahead].0;
                if kind.is_trivia() {
                    lookahead += 1;
                    continue;
                }
                return kind == SyntaxKind::L_PAREN;
            }
        }

        // Type arguments: <...> this( or <...> super(
        if self.at(SyntaxKind::LT) {
            let mut lookahead = 1;
            let mut depth = 1;

            // Skip over type arguments
            while lookahead < self.tokens.len()
                && self.pos + lookahead < self.tokens.len()
                && depth > 0
            {
                let kind = self.tokens[self.pos + lookahead].0;
                lookahead += 1;

                match kind {
                    SyntaxKind::LT => depth += 1,
                    SyntaxKind::GT => depth -= 1,
                    SyntaxKind::SHR => depth -= 2, // >> counts as two >
                    SyntaxKind::USHR => depth -= 3, // >>> counts as three >
                    _ => {}
                }
            }

            // Skip trivia after type arguments
            while lookahead < self.tokens.len() && self.pos + lookahead < self.tokens.len() {
                let kind = self.tokens[self.pos + lookahead].0;
                if kind.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }

            // Check for 'this' or 'super'
            if lookahead < self.tokens.len() && self.pos + lookahead < self.tokens.len() {
                let kind = self.tokens[self.pos + lookahead].0;
                if kind == SyntaxKind::THIS_KW || kind == SyntaxKind::SUPER_KW {
                    lookahead += 1;

                    // Skip trivia
                    while lookahead < self.tokens.len() && self.pos + lookahead < self.tokens.len()
                    {
                        let k = self.tokens[self.pos + lookahead].0;
                        if k.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    }

                    // Check for '('
                    if lookahead < self.tokens.len() && self.pos + lookahead < self.tokens.len() {
                        return self.tokens[self.pos + lookahead].0 == SyntaxKind::L_PAREN;
                    }
                }
            }
        }

        if self.is_qualified_super_invocation() {
            return true;
        }

        false
    }

    /// Parse an explicit constructor invocation
    ///
    /// Grammar:
    ///   `ExplicitConstructorInvocation`:
    ///     [`TypeArguments`] this ( [`ArgumentList`] ) ;
    ///     [`TypeArguments`] super ( [`ArgumentList`] ) ;
    fn parse_explicit_constructor_invocation(&mut self) {
        self.start_node(SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);

        // Type arguments (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_args();
            self.skip_trivia();
        }

        // 'this' or 'super' or qualified super
        if self.at(SyntaxKind::THIS_KW) || self.at(SyntaxKind::SUPER_KW) {
            self.bump();
        } else {
            self.parse_super_qualifier();
            self.skip_trivia();

            if self.at(SyntaxKind::DOT) {
                self.bump();
                self.skip_trivia();
            }

            // Optional type arguments before super
            if self.at(SyntaxKind::LT) && self.is_type_args_ahead() {
                self.parse_type_args();
                self.skip_trivia();
            }

            if self.is_super_token(self.pos) {
                self.bump();
            } else {
                let range = self.current_range();
                self.error("Expected 'super' after qualifier", range);
            }
        }

        self.skip_trivia();

        // Argument list
        if self.at(SyntaxKind::L_PAREN) {
            self.parse_arg_list();
        } else {
            let range = self.current_range();
            self.error("Expected argument list", range);
        }

        self.skip_trivia();

        // Semicolon
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    fn is_qualified_super_invocation(&self) -> bool {
        let mut pos = self.pos;
        let len = self.tokens.len();

        self.skip_trivia_lookahead(&mut pos);

        if pos >= len {
            return false;
        }

        loop {
            if pos >= len {
                return false;
            }

            let kind = self.tokens[pos].0;
            if is_identifier_kind(kind) || self.is_this_token(pos) {
                pos += 1;
            } else {
                return false;
            }

            self.skip_trivia_lookahead(&mut pos);

            if pos >= len || self.tokens[pos].0 != SyntaxKind::DOT {
                return false;
            }

            pos += 1;
            self.skip_trivia_lookahead(&mut pos);

            if self.is_this_token(pos) {
                pos += 1;
                self.skip_trivia_lookahead(&mut pos);

                if pos >= len || self.tokens[pos].0 != SyntaxKind::DOT {
                    return false;
                }

                pos += 1;
                self.skip_trivia_lookahead(&mut pos);
            }

            if pos < len && self.tokens[pos].0 == SyntaxKind::LT {
                if !self.skip_type_args_lookahead(&mut pos) {
                    return false;
                }
                self.skip_trivia_lookahead(&mut pos);
            }

            if self.is_super_token(pos) {
                pos += 1;
                self.skip_trivia_lookahead(&mut pos);
                return pos < len && self.tokens[pos].0 == SyntaxKind::L_PAREN;
            }

            if pos < len && is_identifier_kind(self.tokens[pos].0) {
                continue;
            }

            return false;
        }
    }

    fn skip_type_args_lookahead(&self, pos: &mut usize) -> bool {
        if *pos >= self.tokens.len() || self.tokens[*pos].0 != SyntaxKind::LT {
            return false;
        }

        let mut depth = 0;

        while *pos < self.tokens.len() {
            let kind = self.tokens[*pos].0;
            if kind.is_trivia() {
                *pos += 1;
                continue;
            }

            match kind {
                SyntaxKind::LT => depth += 1,
                SyntaxKind::GT => depth -= 1,
                SyntaxKind::SHR => depth -= 2,
                SyntaxKind::USHR => depth -= 3,
                _ => {}
            }

            *pos += 1;

            if depth <= 0 {
                return true;
            }
        }

        false
    }

    fn parse_super_qualifier(&mut self) {
        let len = self.tokens.len();
        let mut lookahead = self.pos;
        self.skip_trivia_lookahead(&mut lookahead);

        if lookahead >= len {
            let range = self.current_range();
            self.error("Expected qualifier before 'super'", range);
            return;
        }

        self.start_node(SyntaxKind::QUALIFIED_NAME);

        if self.is_this_token(self.pos) || self.at_identifier() {
            self.bump();
        } else {
            let range = self.current_range();
            self.error("Expected qualifier before 'super'", range);
        }

        loop {
            let mut dot_pos = self.pos;
            self.skip_trivia_lookahead(&mut dot_pos);

            if dot_pos >= len || self.tokens[dot_pos].0 != SyntaxKind::DOT {
                break;
            }

            let mut after_dot = dot_pos + 1;
            self.skip_trivia_lookahead(&mut after_dot);

            if self.is_super_token(after_dot) {
                break;
            }

            if self.at(SyntaxKind::DOT) {
                self.bump();
                self.skip_trivia();
            } else {
                while self.pos < dot_pos {
                    self.bump();
                }
                if self.at(SyntaxKind::DOT) {
                    self.bump();
                    self.skip_trivia();
                }
            }

            if self.is_this_token(self.pos) || self.at_identifier() {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        self.finish_node();
    }

    fn is_super_token(&self, pos: usize) -> bool {
        if pos >= self.tokens.len() {
            return false;
        }

        let (kind, text, _) = self.tokens[pos];
        kind == SyntaxKind::SUPER_KW || (kind == SyntaxKind::IDENT && text == "super")
    }

    fn is_this_token(&self, pos: usize) -> bool {
        if pos >= self.tokens.len() {
            return false;
        }

        let (kind, text, _) = self.tokens[pos];
        kind == SyntaxKind::THIS_KW || (kind == SyntaxKind::IDENT && text == "this")
    }

    /// Parse a statement
    pub(super) fn parse_statement(&mut self) {
        // Check recursion depth to prevent stack overflow
        if !self.check_recursion_depth() {
            // Depth limit exceeded - create an error node and skip to recovery point
            self.start_node(SyntaxKind::ERROR);
            while !self.at_eof() && !self.at(SyntaxKind::SEMICOLON) && !self.at(SyntaxKind::R_BRACE)
            {
                self.bump();
            }
            if self.at(SyntaxKind::SEMICOLON) {
                self.bump();
            }
            self.finish_node();
            self.decrease_recursion_depth();
            return;
        }

        match self.current() {
            SyntaxKind::L_BRACE => self.parse_block(),
            SyntaxKind::IF_KW => self.parse_if_stmt(),
            SyntaxKind::WHILE_KW => self.parse_while_stmt(),
            SyntaxKind::DO_KW => self.parse_do_while_stmt(),
            SyntaxKind::FOR_KW => self.parse_for_stmt(),
            SyntaxKind::SWITCH_KW => self.parse_switch_stmt(),
            SyntaxKind::TRY_KW => self.parse_try_stmt(),
            SyntaxKind::RETURN_KW => self.parse_return_stmt(),
            SyntaxKind::THROW_KW => self.parse_throw_stmt(),
            SyntaxKind::BREAK_KW => self.parse_break_stmt(),
            SyntaxKind::CONTINUE_KW => self.parse_continue_stmt(),
            SyntaxKind::YIELD_KW => {
                // `yield` is contextual even inside a switch expression. A
                // variable named `yield` may still be assigned, incremented,
                // selected, or indexed there; only the statement form consumes
                // it as the keyword.
                if self.in_switch_expr && self.at_yield_statement() {
                    self.parse_yield_stmt();
                } else {
                    self.parse_expression_stmt();
                }
            }
            SyntaxKind::SYNCHRONIZED_KW => self.parse_synchronized_stmt(),
            SyntaxKind::ASSERT_KW => self.parse_assert_stmt(),
            SyntaxKind::SEMICOLON => self.parse_empty_stmt(),
            // Local type declarations (JLS §14.3)
            // Classes, interfaces, enums, and records can be declared inside methods
            SyntaxKind::CLASS_KW => self.parse_local_class_decl(),
            SyntaxKind::INTERFACE_KW => self.parse_local_interface_decl(),
            SyntaxKind::ENUM_KW => self.parse_local_enum_decl(),
            SyntaxKind::RECORD_KW => {
                // 'record' is a contextual keyword - can be a type declaration OR a variable name
                // Check if this is a record declaration vs. a variable in an expression
                // Pattern: "record { ... }" or "record Foo(...) { ... }" = declaration
                //          "record = value;" or "record.method();" = expression/variable
                if self.is_record_type_decl() {
                    self.parse_local_record_decl();
                } else if self.is_labeled_stmt() {
                    // "record: statement" is a labeled statement
                    self.parse_labeled_stmt();
                } else if self.is_local_var_decl() {
                    // "record Type x = ..." is a local variable declaration (record is the type)
                    self.parse_local_var_decl();
                } else {
                    // "record = ..." or "record.method();" is an expression statement
                    self.parse_expression_stmt();
                }
            }
            // Handle local variable declarations vs expression statements
            _ => {
                // Check for modifiers that precede type declarations
                // e.g., "final class Foo", "abstract class Bar"
                if self.is_local_type_decl_with_modifiers() {
                    self.parse_type_decl();
                } else if self.is_labeled_stmt() {
                    // Check for labeled statement: identifier : statement
                    self.parse_labeled_stmt();
                } else if self.is_local_var_decl() {
                    // Check if this looks like a local variable declaration
                    // Pattern: Type name = value;
                    // Need to distinguish from expression statements
                    self.parse_local_var_decl();
                } else {
                    self.parse_expression_stmt();
                }
            }
        }

        self.decrease_recursion_depth();
    }

    fn at_yield_statement(&self) -> bool {
        let mut lookahead = 1;
        while self.peek(lookahead).is_some_and(SyntaxKind::is_trivia) {
            lookahead += 1;
        }

        !matches!(
            self.peek(lookahead),
            Some(
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
                    | SyntaxKind::PLUS_PLUS
                    | SyntaxKind::MINUS_MINUS
                    | SyntaxKind::DOT
                    | SyntaxKind::L_BRACKET
            )
        )
    }

    /// Check if we're at a local type declaration with modifiers
    /// e.g., "final class Foo", "abstract class Bar"
    fn is_local_type_decl_with_modifiers(&self) -> bool {
        // Check for modifiers that can precede type declarations
        match self.current() {
            SyntaxKind::FINAL_KW | SyntaxKind::ABSTRACT_KW | SyntaxKind::STRICTFP_KW => {
                // Look ahead to see what follows the modifier
                let mut lookahead = 1;
                // Skip trivia
                while let Some(kind) = self.peek(lookahead) {
                    if kind.is_trivia() {
                        lookahead += 1;
                    } else {
                        break;
                    }
                }
                // Check if followed by type declaration keywords
                matches!(
                    self.peek(lookahead),
                    Some(
                        SyntaxKind::CLASS_KW
                            | SyntaxKind::INTERFACE_KW
                            | SyntaxKind::ENUM_KW
                            | SyntaxKind::RECORD_KW
                    )
                )
            }
            _ => false,
        }
    }

    /// Check if 'record' keyword is being used as a type declaration
    /// vs. as a variable/identifier name
    ///
    /// Returns true for:
    ///   record Foo(...) { }
    ///   record { ... }  (illegal but parse as declaration to give better error)
    ///
    /// Returns false for:
    ///   record = value;
    ///   record.method();
    ///   record[0];
    ///   record Type x = ...;  (record used as type name)
    pub(super) fn is_record_type_decl(&self) -> bool {
        debug_assert!(self.at(SyntaxKind::RECORD_KW));

        let mut lookahead = 1;

        // Skip trivia after 'record'
        while let Some(kind) = self.peek(lookahead) {
            if kind.is_trivia() {
                lookahead += 1;
            } else {
                break;
            }
        }

        // Check what follows 'record'
        match self.peek(lookahead) {
            // record Foo(...) { } - type declaration
            Some(kind) if kind.is_type_identifier() => {
                // Could be:
                // 1. "record Foo(...) { }" - declaration
                // 2. "record Type x = ..." - variable declaration with record as type
                // 3. "record: statement" - labeled statement
                //
                // Look ahead further to distinguish
                lookahead += 1;

                // Skip trivia after identifier
                while let Some(k) = self.peek(lookahead) {
                    if k.is_trivia() {
                        lookahead += 1;
                    } else {
                        break;
                    }
                }

                // Generic records place their type-parameter list between the
                // name and the mandatory header: `record Pair<T>(T value) {}`.
                if self.peek(lookahead) == Some(SyntaxKind::LT) {
                    lookahead += 1;
                    let mut depth = 1u32;
                    while depth > 0 {
                        match self.peek(lookahead) {
                            Some(SyntaxKind::LT) => depth += 1,
                            Some(SyntaxKind::GT) => depth -= 1,
                            Some(SyntaxKind::SHR) if depth >= 2 => depth -= 2,
                            Some(SyntaxKind::USHR) if depth >= 3 => depth -= 3,
                            Some(SyntaxKind::EOF) | None => return false,
                            _ => {}
                        }
                        lookahead += 1;
                    }
                    while self.peek(lookahead).is_some_and(SyntaxKind::is_trivia) {
                        lookahead += 1;
                    }
                }

                // If followed by '(', it's a record declaration
                // If followed by identifier, it's a variable declaration (record Type name)
                // If followed by ':', it's a labeled statement
                // If followed by '.', '=', '[', etc., it's an expression
                matches!(self.peek(lookahead), Some(SyntaxKind::L_PAREN))
            }
            // record { ... } - illegal, but parse as declaration for better error messages
            Some(SyntaxKind::L_BRACE) => true,
            // record = ..., record.foo, record[0], etc. - expression/variable
            _ => false,
        }
    }

    /// Check if we're at a local variable declaration
    fn is_local_var_decl(&self) -> bool {
        // Local var declarations start with:
        // - Annotations (@Nullable, @SuppressWarnings, etc.) - Java 8+
        // - Type keywords (int, boolean, etc.)
        // - final Type
        // - Identifiers followed by identifier (Type name)
        // - var keyword

        // Check for annotations (Java 8+ allows annotations on local variables)
        if self.at(SyntaxKind::AT) {
            return true;
        }

        // Check for final modifier
        // But distinguish between:
        //   final String x = ...;  (local variable)
        //   final class Foo { }    (local class)
        if self.at(SyntaxKind::FINAL_KW) {
            // Look ahead to see what follows 'final'
            let mut lookahead = 1;
            // Skip trivia
            while let Some(kind) = self.peek(lookahead) {
                if kind.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }
            // If followed by class/interface/enum/record keywords, it's a local type declaration
            match self.peek(lookahead) {
                Some(
                    SyntaxKind::CLASS_KW
                    | SyntaxKind::INTERFACE_KW
                    | SyntaxKind::ENUM_KW
                    | SyntaxKind::RECORD_KW,
                ) => return false,
                _ => return true,
            }
        }

        if self.is_local_variable_type_start() {
            // Look ahead to see if followed by identifier
            // Skip past the type to check
            let mut lookahead = 1;

            // Skip trivia
            while let Some(kind) = self.peek(lookahead) {
                if kind.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }

            // Skip qualified name parts (Type.Inner.Inner)
            // BUGFIX: Handle explicit type arguments like Collections.<String>
            while let Some(kind) = self.peek(lookahead) {
                if kind == SyntaxKind::DOT {
                    lookahead += 1;
                    // Skip trivia after dot
                    while let Some(k) = self.peek(lookahead) {
                        if k.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    }

                    // Check if this is an explicit type argument (e.g., .<Type>methodName)
                    // rather than a qualified type (e.g., java.util.List)
                    if self.peek(lookahead) == Some(SyntaxKind::LT) {
                        // This looks like .<Type>, which is NOT a variable declaration
                        // It's a method call with explicit type arguments
                        return false;
                    }

                    // Expect identifier after dot (including contextual keywords)
                    if let Some(kind) = self.peek(lookahead) {
                        if is_identifier_kind(kind) {
                            lookahead += 1;
                            // Skip trivia after identifier
                            while let Some(k) = self.peek(lookahead) {
                                if k.is_trivia() {
                                    lookahead += 1;
                                } else {
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Check for array brackets in type
            while let Some(kind) = self.peek(lookahead) {
                if kind == SyntaxKind::L_BRACKET {
                    lookahead += 1;
                    // Skip to matching bracket
                    while let Some(k) = self.peek(lookahead) {
                        lookahead += 1;
                        if k == SyntaxKind::R_BRACKET {
                            break;
                        }
                    }
                    // Skip trivia after bracket
                    while let Some(k) = self.peek(lookahead) {
                        if k.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    }
                } else {
                    break;
                }
            }

            // Check for generic type arguments
            if self.peek(lookahead) == Some(SyntaxKind::LT) {
                lookahead += 1;
                let mut depth = 1;
                while depth > 0 && lookahead < 50 {
                    match self.peek(lookahead) {
                        Some(SyntaxKind::LT) => depth += 1,
                        Some(SyntaxKind::GT) => depth -= 1,
                        Some(SyntaxKind::SHR) => depth -= 2, // >> counts as two >
                        Some(SyntaxKind::USHR) => depth -= 3, // >>> counts as three >
                        Some(SyntaxKind::EOF) | None => return false,
                        _ => {}
                    }
                    lookahead += 1;
                }
                // Skip trivia after generic
                while let Some(k) = self.peek(lookahead) {
                    if k.is_trivia() {
                        lookahead += 1;
                    } else {
                        break;
                    }
                }

                // BUGFIX: Check for qualified inner types after generic type arguments
                // Pattern C: Graph<?>.Edge
                // After parsing type arguments, check for .Identifier pattern
                while self.peek(lookahead) == Some(SyntaxKind::DOT) {
                    lookahead += 1;
                    // Skip trivia after dot
                    while let Some(k) = self.peek(lookahead) {
                        if k.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    }

                    // Expect identifier after dot
                    if let Some(kind) = self.peek(lookahead) {
                        if is_identifier_kind(kind) {
                            lookahead += 1;
                            // Skip trivia after identifier
                            while let Some(k) = self.peek(lookahead) {
                                if k.is_trivia() {
                                    lookahead += 1;
                                } else {
                                    break;
                                }
                            }

                            // Check for inner type's own type arguments: Outer<T>.Inner<U>
                            if self.peek(lookahead) == Some(SyntaxKind::LT) {
                                lookahead += 1;
                                let mut inner_depth = 1;
                                while inner_depth > 0 && lookahead < 50 {
                                    match self.peek(lookahead) {
                                        Some(SyntaxKind::LT) => inner_depth += 1,
                                        Some(SyntaxKind::GT) => inner_depth -= 1,
                                        Some(SyntaxKind::SHR) => inner_depth -= 2,
                                        Some(SyntaxKind::USHR) => inner_depth -= 3,
                                        Some(SyntaxKind::EOF) | None => return false,
                                        _ => {}
                                    }
                                    lookahead += 1;
                                }
                                // Skip trivia after inner generics
                                while let Some(k) = self.peek(lookahead) {
                                    if k.is_trivia() {
                                        lookahead += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        } else {
                            // DOT not followed by identifier
                            break;
                        }
                    } else {
                        break;
                    }
                }

                // BUGFIX: Check for array brackets AFTER generic type arguments and qualified inner types
                // This handles patterns like Class<?>[] and Graph<?>.Edge[] where array dimensions come after generics
                while let Some(kind) = self.peek(lookahead) {
                    if kind == SyntaxKind::L_BRACKET {
                        lookahead += 1;
                        // Skip to matching bracket
                        while let Some(k) = self.peek(lookahead) {
                            lookahead += 1;
                            if k == SyntaxKind::R_BRACKET {
                                break;
                            }
                        }
                        // Skip trivia after bracket
                        while let Some(k) = self.peek(lookahead) {
                            if k.is_trivia() {
                                lookahead += 1;
                            } else {
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
            }

            // If followed by identifier (including contextual keywords), it's likely a variable declaration
            if let Some(kind) = self.peek(lookahead) {
                if is_identifier_kind(kind) {
                    return true;
                }
            }
        }

        false
    }

    /// Parse a local variable declaration
    pub(super) fn parse_local_var_decl(&mut self) {
        self.start_node(SyntaxKind::LOCAL_VAR_DECL);

        // Parse annotations (Java 8+ allows annotations on local variables)
        while self.at(SyntaxKind::AT) {
            let start_pos = self.pos;
            self.parse_annotation();
            // BUGFIX: If parse_annotation didn't consume any tokens, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }
            self.skip_trivia();
        }

        // Optional: final modifier
        if self.at(SyntaxKind::FINAL_KW) {
            self.bump();
            self.skip_trivia();
        }

        // Type
        self.parse_local_variable_type();
        self.skip_trivia();

        // Variable declarators (name, optional initializer)
        loop {
            // Variable name (allow contextual keywords like 'record')
            // BUGFIX: Check if expect succeeds, break if it fails to prevent infinite loop
            if !self.expect_identifier() {
                break;
            }
            self.skip_trivia();

            // Array dimensions (optional)
            while self.at(SyntaxKind::L_BRACKET) {
                self.bump();
                self.skip_trivia();
                self.expect(SyntaxKind::R_BRACKET);
                self.skip_trivia();
            }

            // Initializer (optional)
            if self.at(SyntaxKind::EQ) {
                self.bump();
                self.skip_trivia();
                self.parse_expression();
                self.skip_trivia();
            }

            // Check for more declarators
            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        // Semicolon
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse an if statement
    fn parse_if_stmt(&mut self) {
        self.start_node(SyntaxKind::IF_STMT);

        self.expect(SyntaxKind::IF_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.parse_statement();
        self.skip_trivia();

        // Else clause (optional)
        if self.at(SyntaxKind::ELSE_KW) {
            self.bump();
            self.skip_trivia();
            self.parse_statement();
        }

        self.finish_node();
    }

    /// Parse a while statement
    fn parse_while_stmt(&mut self) {
        self.start_node(SyntaxKind::WHILE_STMT);

        self.expect(SyntaxKind::WHILE_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.parse_statement();

        self.finish_node();
    }

    /// Parse a do-while statement
    fn parse_do_while_stmt(&mut self) {
        self.start_node(SyntaxKind::DO_WHILE_STMT);

        self.expect(SyntaxKind::DO_KW);
        self.skip_trivia();
        self.parse_statement();
        self.skip_trivia();
        self.expect(SyntaxKind::WHILE_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a for statement
    fn parse_for_stmt(&mut self) {
        // Peek ahead to determine if this is an enhanced for-loop BEFORE consuming tokens
        // We need to look past "for" and "(" to check the pattern
        let mut lookahead_pos = 0;

        // Skip past FOR_KW
        if self.peek(lookahead_pos) == Some(SyntaxKind::FOR_KW) {
            lookahead_pos += 1;
        }
        // Skip trivia after FOR_KW
        while matches!(
            self.peek(lookahead_pos),
            Some(SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT)
        ) {
            lookahead_pos += 1;
        }
        // Skip past L_PAREN
        if self.peek(lookahead_pos) == Some(SyntaxKind::L_PAREN) {
            lookahead_pos += 1;
        }
        // Skip trivia after L_PAREN
        while matches!(
            self.peek(lookahead_pos),
            Some(SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT)
        ) {
            lookahead_pos += 1;
        }

        // Now check if enhanced for from this position
        let is_enhanced = self.is_enhanced_for_from(lookahead_pos);

        // Start the correct node type based on what we detected
        if is_enhanced {
            self.start_node(SyntaxKind::FOREACH_STMT);
        } else {
            self.start_node(SyntaxKind::FOR_STMT);
        }

        self.expect(SyntaxKind::FOR_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Check if enhanced for (for-each)
        if is_enhanced {
            // for ([Annotations] [final] Type var : iterable)
            // Per JLS §14.14.2, annotations and modifiers are allowed on the variable

            // Parse annotations (Java 8+)
            while self.at(SyntaxKind::AT) {
                let start_pos = self.pos;
                self.parse_annotation();
                if self.pos == start_pos {
                    break;
                }
                self.skip_trivia();
            }

            // Optional: final modifier
            if self.at(SyntaxKind::FINAL_KW) {
                self.bump();
                self.skip_trivia();
            }

            self.parse_local_variable_type();
            self.skip_trivia();
            // Use expect_identifier() to handle contextual keywords like `record`, `var`, `yield`
            self.expect_identifier();
            self.skip_trivia();
            self.expect(SyntaxKind::COLON);
            self.skip_trivia();
            self.parse_expression();
            self.skip_trivia();
        } else {
            // Traditional for (init; condition; update)

            // Init: can be var declaration or expression list
            // JLS §14.14.1: ForInit can be LocalVariableDeclaration OR StatementExpressionList
            //
            // Examples:
            //   for (int i = 0; ...)        - LocalVariableDeclaration
            //   for (i = 0; ...)            - StatementExpressionList (Pattern H)
            //   for (i = 0, j = 0; ...)     - StatementExpressionList
            if !self.at(SyntaxKind::SEMICOLON) {
                // Pattern H Fix: Distinguish variable declarations from expression lists
                // Check if this looks like a declaration (Type varName) or an expression (ident =)
                if self.is_for_loop_var_decl() {
                    // Local variable declaration: for (int i = 0; ...)
                    self.start_node(SyntaxKind::LOCAL_VAR_DECL);
                    // JLS 14.14.1: ForInit is a LocalVariableDeclaration, which
                    // allows `final` and annotations: `for (final int n = a.length; ...)`.
                    loop {
                        if self.at(SyntaxKind::AT) {
                            let start_pos = self.pos;
                            self.parse_annotation();
                            if self.pos == start_pos {
                                break;
                            }
                            self.skip_trivia();
                        } else if self.at(SyntaxKind::FINAL_KW) {
                            self.bump();
                            self.skip_trivia();
                        } else {
                            break;
                        }
                    }
                    self.parse_local_variable_type();
                    self.skip_trivia();

                    // Variable declarators
                    loop {
                        // Use expect_identifier() to handle contextual keywords like `record`
                        if !self.expect_identifier() {
                            break;
                        }
                        self.skip_trivia();

                        // Initializer (optional)
                        if self.at(SyntaxKind::EQ) {
                            self.bump();
                            self.skip_trivia();
                            self.parse_expression();
                            self.skip_trivia();
                        }

                        if self.at(SyntaxKind::COMMA) {
                            self.bump();
                            self.skip_trivia();
                        } else {
                            break;
                        }
                    }

                    self.finish_node();
                } else {
                    // Expression list: for (i = 0; ...) or for (i = 0, j = 0; ...)
                    self.parse_expression();
                    self.skip_trivia();
                    while self.eat(SyntaxKind::COMMA) {
                        self.skip_trivia();
                        self.parse_expression();
                        self.skip_trivia();
                    }
                }
            }

            self.expect(SyntaxKind::SEMICOLON);
            self.skip_trivia();

            // Condition (optional)
            if !self.at(SyntaxKind::SEMICOLON) {
                self.parse_expression();
                self.skip_trivia();
            }

            self.expect(SyntaxKind::SEMICOLON);
            self.skip_trivia();

            // Update (optional)
            if !self.at(SyntaxKind::R_PAREN) {
                self.parse_expression();
                self.skip_trivia();
                while self.eat(SyntaxKind::COMMA) {
                    self.skip_trivia();
                    self.parse_expression();
                    self.skip_trivia();
                }
            }
        }

        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.parse_statement();

        self.finish_node();
    }

    /// Check if the for loop init clause is a variable declaration vs. an expression list
    ///
    /// Pattern H Fix: Distinguish these cases:
    ///   for (int i = 0; ...)      - Variable declaration (returns true)
    ///   for (i = 0; ...)          - Expression list (returns false)
    ///   for (List<String> x; ...) - Variable declaration (returns true)
    ///   for (map.get(k); ...)     - Expression (returns false)
    ///
    /// Strategy:
    /// - Primitive types (int, boolean, etc.) → always a declaration
    /// - Modifiers (final) → always a declaration
    /// - Identifier patterns:
    ///   - If followed by another identifier → declaration (Type varName)
    ///   - If followed by = or ; or , → expression (varName = value)
    ///   - If qualified (a.b) or has method call → expression
    fn is_for_loop_var_decl(&self) -> bool {
        // Primitive types are always declarations
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
            return true;
        }

        // final modifier indicates declaration
        if self.at(SyntaxKind::FINAL_KW) {
            return true;
        }

        // Annotations indicate declaration (Java 8+)
        if self.at(SyntaxKind::AT) {
            return true;
        }

        // var keyword (Java 10+)
        if self.at(SyntaxKind::VAR_KW) {
            return true;
        }

        // For identifiers, we need to look ahead
        if self.at_identifier() {
            let mut lookahead = 1;

            // Skip trivia
            while let Some(kind) = self.peek(lookahead) {
                if kind.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }

            match self.peek(lookahead) {
                // Followed by another identifier → declaration: Type varName
                Some(kind) if is_identifier_kind(kind) => return true,

                // Followed by < → generic type declaration: List<String> varName
                Some(SyntaxKind::LT) => {
                    // Skip to end of type arguments
                    lookahead += 1;
                    let mut depth = 1;
                    while depth > 0 && lookahead < 50 {
                        match self.peek(lookahead) {
                            Some(SyntaxKind::LT) => depth += 1,
                            Some(SyntaxKind::GT) => depth -= 1,
                            Some(SyntaxKind::SHR) => depth -= 2,
                            Some(SyntaxKind::USHR) => depth -= 3,
                            Some(SyntaxKind::EOF) | None => return false,
                            _ => {}
                        }
                        lookahead += 1;
                    }
                    // Skip trivia after >
                    while let Some(k) = self.peek(lookahead) {
                        if k.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    }

                    // Check for qualified inner types: Graph<?>.Edge varName
                    while self.peek(lookahead) == Some(SyntaxKind::DOT) {
                        lookahead += 1;
                        // Skip trivia
                        while let Some(k) = self.peek(lookahead) {
                            if k.is_trivia() {
                                lookahead += 1;
                            } else {
                                break;
                            }
                        }
                        // Expect identifier
                        if let Some(k) = self.peek(lookahead) {
                            if is_identifier_kind(k) {
                                lookahead += 1;
                                // Skip trivia
                                while let Some(k2) = self.peek(lookahead) {
                                    if k2.is_trivia() {
                                        lookahead += 1;
                                    } else {
                                        break;
                                    }
                                }
                                // Check for inner type arguments
                                if self.peek(lookahead) == Some(SyntaxKind::LT) {
                                    lookahead += 1;
                                    let mut inner_depth = 1;
                                    while inner_depth > 0 && lookahead < 50 {
                                        match self.peek(lookahead) {
                                            Some(SyntaxKind::LT) => inner_depth += 1,
                                            Some(SyntaxKind::GT) => inner_depth -= 1,
                                            Some(SyntaxKind::SHR) => inner_depth -= 2,
                                            Some(SyntaxKind::USHR) => inner_depth -= 3,
                                            Some(SyntaxKind::EOF) | None => return false,
                                            _ => {}
                                        }
                                        lookahead += 1;
                                    }
                                    // Skip trivia
                                    while let Some(k2) = self.peek(lookahead) {
                                        if k2.is_trivia() {
                                            lookahead += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }

                    // Check for array brackets
                    while self.peek(lookahead) == Some(SyntaxKind::L_BRACKET) {
                        lookahead += 1;
                        while let Some(k) = self.peek(lookahead) {
                            lookahead += 1;
                            if k == SyntaxKind::R_BRACKET {
                                break;
                            }
                        }
                        // Skip trivia
                        while let Some(k) = self.peek(lookahead) {
                            if k.is_trivia() {
                                lookahead += 1;
                            } else {
                                break;
                            }
                        }
                    }

                    // After generics and potential qualified types, check for identifier
                    if let Some(k) = self.peek(lookahead) {
                        return is_identifier_kind(k);
                    }
                    return false;
                }

                // Followed by . → Could be qualified type (java.util.List) or expression (obj.field)
                // We need more context
                Some(SyntaxKind::DOT) => {
                    // Look ahead past the dot
                    lookahead += 1;
                    // Skip trivia
                    while let Some(k) = self.peek(lookahead) {
                        if k.is_trivia() {
                            lookahead += 1;
                        } else {
                            break;
                        }
                    }
                    // Check what follows the dot
                    match self.peek(lookahead) {
                        // a.b followed by identifier → could be qualified type: java.util.List varName
                        Some(k) if is_identifier_kind(k) => {
                            lookahead += 1;
                            // Skip trivia
                            while let Some(k2) = self.peek(lookahead) {
                                if k2.is_trivia() {
                                    lookahead += 1;
                                } else {
                                    break;
                                }
                            }
                            // If followed by another identifier, likely a declaration
                            // Otherwise, it's an expression
                            if let Some(k2) = self.peek(lookahead) {
                                return is_identifier_kind(k2)
                                    || k2 == SyntaxKind::LT
                                    || k2 == SyntaxKind::DOT;
                            }
                            return false;
                        }
                        // a.(  → method call or anything else after dot → expression
                        _ => return false,
                    }
                }

                // Followed by =, ;, , ( [ or anything else → expression
                _ => return false,
            }
        }

        false
    }

    /// Check if this is an enhanced for loop starting from a specific lookahead position
    fn is_enhanced_for_from(&self, start_offset: usize) -> bool {
        // Look ahead to detect ":" pattern
        // Enhanced for: [Annotations] [final] Type ident :
        // Traditional for: Type ident = OR Type ident ;
        // Need larger lookahead to account for annotations like @SuppressWarnings("...")
        let mut lookahead = start_offset;
        let max_lookahead = start_offset + 50; // Increased to handle annotations

        while lookahead < max_lookahead {
            match self.peek(lookahead) {
                Some(SyntaxKind::COLON) => return true,
                Some(SyntaxKind::SEMICOLON | SyntaxKind::EQ | SyntaxKind::EOF) | None => {
                    return false
                }
                // Skip over parentheses (from annotations, type arguments, etc.)
                // Only stop at top-level R_PAREN (the for-loop's closing paren)
                // We detect this by tracking paren depth
                Some(SyntaxKind::L_PAREN) => {
                    lookahead += 1;
                    let mut depth = 1;
                    // Skip to matching R_PAREN
                    while depth > 0 && lookahead < max_lookahead {
                        match self.peek(lookahead) {
                            Some(SyntaxKind::L_PAREN) => depth += 1,
                            Some(SyntaxKind::R_PAREN) => depth -= 1,
                            Some(SyntaxKind::EOF) | None => return false,
                            _ => {}
                        }
                        lookahead += 1;
                    }
                }
                _ => lookahead += 1,
            }
        }

        false
    }

    /// Parse a switch statement
    pub(super) fn parse_switch_stmt(&mut self) {
        self.parse_switch_construct(SyntaxKind::SWITCH_STMT);
    }

    /// Parse a switch expression
    pub(super) fn parse_switch_expr(&mut self) {
        self.parse_switch_construct(SyntaxKind::SWITCH_EXPR);
    }

    /// Shared implementation for switch statements and expressions.
    fn parse_switch_construct(&mut self, node_kind: SyntaxKind) {
        self.start_node(node_kind);

        self.expect(SyntaxKind::SWITCH_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        // Track that we're inside a switch expression where yield statements are valid
        let prev_in_switch_expr = self.in_switch_expr;
        self.in_switch_expr = prev_in_switch_expr || node_kind == SyntaxKind::SWITCH_EXPR;

        // Parse switch cases
        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            if self.at(SyntaxKind::CASE_KW) {
                self.start_node(SyntaxKind::CASE_CLAUSE);
                self.bump(); // case
                self.skip_trivia();

                // Parse pattern(s) - can be multiple with comma (Java 14+)
                // Set flag to prevent lambda parsing in switch case patterns
                let prev_in_switch_case = self.in_switch_case;
                self.in_switch_case = true;

                // Parse first pattern (Java 21+) or expression (backward compat)
                let first_label_is_null = self.at(SyntaxKind::NULL_LITERAL);
                let first_label_is_pattern = self.is_type_pattern(false);
                let mut label_count = 1usize;
                self.parse_pattern();
                self.skip_trivia();

                // Multiple patterns separated by comma
                while self.eat(SyntaxKind::COMMA) {
                    self.skip_trivia();
                    // Java 21 permits the special combined label
                    // `case null, default`. `default` is a label element here,
                    // not an expression or type pattern.
                    if self.at(SyntaxKind::DEFAULT_KW) {
                        if !first_label_is_null || label_count != 1 {
                            self.error(
                                "'default' may only follow a sole 'null' case label",
                                self.current_range(),
                            );
                        }
                        self.bump();
                        self.skip_trivia();
                        break;
                    }
                    let start_pos = self.pos;
                    self.parse_pattern();
                    // BUGFIX: If parse_pattern didn't consume any tokens, break to prevent infinite loop
                    if self.pos == start_pos {
                        break;
                    }
                    label_count += 1;
                    self.skip_trivia();
                }

                // Restore previous state
                self.in_switch_case = prev_in_switch_case;

                // Parse an optional Java 21 pattern guard. `when` stays a
                // contextual token; this grammar position distinguishes it from
                // an ordinary invocation such as Mockito's `when(...)`.
                if self.at(SyntaxKind::WHEN_KW) {
                    if !first_label_is_pattern || label_count != 1 {
                        self.error(
                            "A 'when' guard requires one pattern case label",
                            self.current_range(),
                        );
                    }
                    self.parse_when_clause();
                    self.skip_trivia();
                }

                // Check for arrow syntax (Java 14+)
                if self.eat(SyntaxKind::ARROW) {
                    // Switch expression: case X -> Y
                    self.skip_trivia();
                    // Can be an expression, a block, or a throw statement
                    if self.at(SyntaxKind::L_BRACE) {
                        self.parse_block();
                    } else if self.at(SyntaxKind::THROW_KW) {
                        // Java 14+ allows throw as an expression in arrow cases
                        self.parse_throw_stmt();
                    } else {
                        self.parse_expression();
                        self.skip_trivia();
                        // Arrow cases might end with semicolon
                        self.eat(SyntaxKind::SEMICOLON);
                    }
                } else {
                    // Traditional: case X: statements
                    self.expect(SyntaxKind::COLON);
                    self.skip_trivia();

                    // Parse statements until next case/default/}
                    while !self.at_any(&[
                        SyntaxKind::CASE_KW,
                        SyntaxKind::DEFAULT_KW,
                        SyntaxKind::R_BRACE,
                    ]) && !self.at_eof()
                    {
                        let start_pos = self.pos;
                        self.parse_statement();
                        // BUGFIX: If parse_statement didn't consume any tokens, break to prevent infinite loop
                        if self.pos == start_pos {
                            break;
                        }
                        self.skip_trivia();
                    }
                }

                self.finish_node();
                self.skip_trivia();
            } else if self.at(SyntaxKind::DEFAULT_KW) {
                self.start_node(SyntaxKind::CASE_CLAUSE);
                self.bump(); // default
                self.skip_trivia();

                if self.eat(SyntaxKind::ARROW) {
                    // Arrow syntax
                    self.skip_trivia();
                    if self.at(SyntaxKind::L_BRACE) {
                        self.parse_block();
                    } else if self.at(SyntaxKind::THROW_KW) {
                        // Java 14+ allows throw as an expression in arrow cases
                        self.parse_throw_stmt();
                    } else {
                        self.parse_expression();
                        self.skip_trivia();
                        self.eat(SyntaxKind::SEMICOLON);
                    }
                } else {
                    // Traditional syntax
                    self.expect(SyntaxKind::COLON);
                    self.skip_trivia();

                    while !self.at_any(&[
                        SyntaxKind::CASE_KW,
                        SyntaxKind::DEFAULT_KW,
                        SyntaxKind::R_BRACE,
                    ]) && !self.at_eof()
                    {
                        let start_pos = self.pos;
                        self.parse_statement();
                        // BUGFIX: If parse_statement didn't consume any tokens, break to prevent infinite loop
                        if self.pos == start_pos {
                            break;
                        }
                        self.skip_trivia();
                    }
                }

                self.finish_node();
                self.skip_trivia();
            } else {
                // Unexpected token
                let range = self.current_range();
                self.error("Expected 'case' or 'default'", range);
                self.bump(); // Skip unexpected token
                self.skip_trivia();
            }
        }

        self.expect(SyntaxKind::R_BRACE);

        // Restore previous switch expression context
        self.in_switch_expr = prev_in_switch_expr;

        self.finish_node();
    }

    /// Parse a try statement
    fn parse_try_stmt(&mut self) {
        self.start_node(SyntaxKind::TRY_STMT);

        self.expect(SyntaxKind::TRY_KW);
        self.skip_trivia();

        // Try-with-resources (Java 7+)
        if self.at(SyntaxKind::L_PAREN) {
            self.start_node(SyntaxKind::RESOURCE_SPEC);
            self.bump(); // (
            self.skip_trivia();

            // Parse resources
            if !self.at(SyntaxKind::R_PAREN) {
                self.parse_resource();
                self.skip_trivia();

                while self.eat(SyntaxKind::SEMICOLON) {
                    self.skip_trivia();
                    if self.at(SyntaxKind::R_PAREN) {
                        break;
                    }
                    self.parse_resource();
                    self.skip_trivia();
                }
            }

            self.expect(SyntaxKind::R_PAREN);
            self.finish_node();
            self.skip_trivia();
        }

        // Try block
        self.parse_block();
        self.skip_trivia();

        // Catch clauses (optional, multiple)
        while self.at(SyntaxKind::CATCH_KW) {
            self.start_node(SyntaxKind::CATCH_CLAUSE);
            self.bump(); // catch
            self.skip_trivia();
            self.expect(SyntaxKind::L_PAREN);
            self.skip_trivia();

            // Optional final modifier (Java 10+)
            if self.eat(SyntaxKind::FINAL_KW) {
                self.skip_trivia();
            }

            // Parse exception types (can be multiple with | for multi-catch)
            self.parse_type();
            self.skip_trivia();

            while self.eat(SyntaxKind::PIPE) {
                self.skip_trivia();
                let start_pos = self.pos;
                self.parse_type();
                // BUGFIX: If parse_type didn't consume any tokens, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }
                self.skip_trivia();
            }

            // Exception variable name (use expect_identifier for contextual keywords)
            self.expect_identifier();
            self.skip_trivia();

            self.expect(SyntaxKind::R_PAREN);
            self.skip_trivia();

            // Catch block
            self.parse_block();
            self.finish_node();
            self.skip_trivia();
        }

        // Finally clause (optional)
        if self.at(SyntaxKind::FINALLY_KW) {
            self.start_node(SyntaxKind::FINALLY_CLAUSE);
            self.bump(); // finally
            self.skip_trivia();
            self.parse_block();
            self.finish_node();
        }

        self.finish_node();
    }

    /// Parse a resource in a try-with-resources statement
    fn parse_resource(&mut self) {
        // Resource can be:
        // 1. Type name = expression (declaration) - wrap in LOCAL_VAR_DECL for symbol extraction
        // 2. Just a variable name (Java 9+ JEP 213, effectively final variable)

        // Check if this is a resource declaration or variable reference
        // Need to distinguish:
        //   FileReader fr = new FileReader("file.txt")  (declaration)
        //   fr                                          (variable reference, Java 9+)
        //
        // Strategy: Look ahead to check for = sign after potential variable name
        // If we see annotations or 'final', it's definitely a declaration
        let is_declaration = self.at(SyntaxKind::AT)
            || self.at(SyntaxKind::FINAL_KW)
            || self.is_resource_declaration();

        if is_declaration {
            // Type name = expression
            // Wrap in LOCAL_VAR_DECL so symbol extraction and type inference work
            self.start_node(SyntaxKind::LOCAL_VAR_DECL);

            // Parse annotations (Java 8+ allows annotations on resource declarations)
            while self.at(SyntaxKind::AT) {
                let start_pos = self.pos;
                self.parse_annotation();
                // BUGFIX: If parse_annotation didn't consume any tokens, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }
                self.skip_trivia();
            }

            // Optional: final modifier (common in try-with-resources)
            if self.at(SyntaxKind::FINAL_KW) {
                self.bump();
                self.skip_trivia();
            }

            self.parse_local_variable_type();
            self.skip_trivia();
            // Use expect_identifier for contextual keywords like `record`
            self.expect_identifier();
            self.skip_trivia();
            self.expect(SyntaxKind::EQ);
            self.skip_trivia();
            self.parse_expression();

            self.finish_node();
        } else {
            // Just an expression (variable reference, Java 9+)
            self.parse_expression();
        }
    }

    /// Check if this looks like a resource declaration (has = after type and name)
    /// vs. a variable reference (no = sign)
    fn is_resource_declaration(&self) -> bool {
        if !self.is_local_variable_type_start() {
            return false;
        }

        let mut lookahead = 1;

        // Skip trivia after type
        while let Some(kind) = self.peek(lookahead) {
            if kind.is_trivia() {
                lookahead += 1;
            } else {
                break;
            }
        }

        // Skip qualified names (Type.Inner)
        while let Some(kind) = self.peek(lookahead) {
            if kind == SyntaxKind::DOT {
                lookahead += 1;
                // Skip trivia after dot
                while let Some(k) = self.peek(lookahead) {
                    if k.is_trivia() {
                        lookahead += 1;
                    } else {
                        break;
                    }
                }
                // Expect identifier after dot
                if self.peek(lookahead).is_some_and(SyntaxKind::is_identifier) {
                    lookahead += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Skip trivia
        while let Some(k) = self.peek(lookahead) {
            if k.is_trivia() {
                lookahead += 1;
            } else {
                break;
            }
        }

        // Skip generic type arguments <...>
        if self.peek(lookahead) == Some(SyntaxKind::LT) {
            lookahead += 1;
            let mut depth = 1;
            while depth > 0 && lookahead < 50 {
                match self.peek(lookahead) {
                    Some(SyntaxKind::LT) => depth += 1,
                    Some(SyntaxKind::GT) => depth -= 1,
                    Some(SyntaxKind::SHR) => depth -= 2,
                    Some(SyntaxKind::USHR) => depth -= 3,
                    Some(SyntaxKind::EOF) | None => return false,
                    _ => {}
                }
                lookahead += 1;
            }
            // Skip trivia after generic
            while let Some(k) = self.peek(lookahead) {
                if k.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }
        }

        // Skip array brackets []
        while self.peek(lookahead) == Some(SyntaxKind::L_BRACKET) {
            lookahead += 1;
            while let Some(k) = self.peek(lookahead) {
                lookahead += 1;
                if k == SyntaxKind::R_BRACKET {
                    break;
                }
            }
            // Skip trivia after bracket
            while let Some(k) = self.peek(lookahead) {
                if k.is_trivia() {
                    lookahead += 1;
                } else {
                    break;
                }
            }
        }

        // Check if followed by identifier (variable name)
        let has_identifier = self.peek(lookahead).is_some_and(SyntaxKind::is_identifier);

        if !has_identifier {
            return false;
        }

        lookahead += 1;

        // Skip trivia after identifier
        while let Some(k) = self.peek(lookahead) {
            if k.is_trivia() {
                lookahead += 1;
            } else {
                break;
            }
        }

        // The key distinction: check for = sign
        // If we see =, it's a declaration: Type name = expr
        // If we see ; or ) or something else, it's likely just an identifier
        self.peek(lookahead) == Some(SyntaxKind::EQ)
    }

    /// Parse a return statement
    fn parse_return_stmt(&mut self) {
        self.start_node(SyntaxKind::RETURN_STMT);

        self.expect(SyntaxKind::RETURN_KW);
        self.skip_trivia();

        // Expression (optional)
        if !self.at(SyntaxKind::SEMICOLON) {
            self.parse_expression();
            self.skip_trivia();
        }

        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a throw statement
    pub(super) fn parse_throw_stmt(&mut self) {
        self.start_node(SyntaxKind::THROW_STMT);

        self.expect(SyntaxKind::THROW_KW);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a yield statement (Java 13+, for switch expressions)
    fn parse_yield_stmt(&mut self) {
        self.start_node(SyntaxKind::YIELD_STMT);

        self.expect(SyntaxKind::YIELD_KW);
        self.skip_trivia();

        // Expression (required for yield)
        self.parse_expression();
        self.skip_trivia();

        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a break statement
    fn parse_break_stmt(&mut self) {
        self.start_node(SyntaxKind::BREAK_STMT);

        self.expect(SyntaxKind::BREAK_KW);
        self.skip_trivia();

        // Label (optional)
        if self.at_identifier() {
            self.bump();
            self.skip_trivia();
        }

        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a continue statement
    fn parse_continue_stmt(&mut self) {
        self.start_node(SyntaxKind::CONTINUE_STMT);

        self.expect(SyntaxKind::CONTINUE_KW);
        self.skip_trivia();

        // Label (optional)
        if self.at_identifier() {
            self.bump();
            self.skip_trivia();
        }

        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse a synchronized statement
    fn parse_synchronized_stmt(&mut self) {
        self.start_node(SyntaxKind::SYNCHRONIZED_STMT);

        self.expect(SyntaxKind::SYNCHRONIZED_KW);
        self.skip_trivia();
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::R_PAREN);
        self.skip_trivia();
        self.parse_block();

        self.finish_node();
    }

    /// Parse an assert statement
    fn parse_assert_stmt(&mut self) {
        self.start_node(SyntaxKind::ASSERT_STMT);

        self.expect(SyntaxKind::ASSERT_KW);
        self.skip_trivia();
        self.parse_expression();
        self.skip_trivia();

        // Message (optional)
        if self.at(SyntaxKind::COLON) {
            self.bump();
            self.skip_trivia();
            self.parse_expression();
            self.skip_trivia();
        }

        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    /// Parse an empty statement
    fn parse_empty_stmt(&mut self) {
        self.start_node(SyntaxKind::EMPTY_STMT);
        self.expect(SyntaxKind::SEMICOLON);
        self.finish_node();
    }

    /// Check if current position is a labeled statement
    fn is_labeled_stmt(&self) -> bool {
        // Pattern: identifier : statement
        if self.at_identifier() {
            // Look ahead to see if there's a colon
            let mut pos = self.pos + 1;
            // Skip trivia
            while pos < self.tokens.len() && self.tokens[pos].0.is_trivia() {
                pos += 1;
            }
            // Check for colon
            pos < self.tokens.len() && self.tokens[pos].0 == SyntaxKind::COLON
        } else {
            false
        }
    }

    /// Parse a labeled statement
    fn parse_labeled_stmt(&mut self) {
        self.start_node(SyntaxKind::LABELED_STMT);

        // Label identifier (can be contextual keyword)
        self.expect_identifier();
        self.skip_trivia();

        // Colon
        self.expect(SyntaxKind::COLON);
        self.skip_trivia();

        // The statement following the label
        self.parse_statement();

        self.finish_node();
    }

    /// Parse an expression statement
    fn parse_expression_stmt(&mut self) {
        self.start_node(SyntaxKind::EXPR_STMT);

        self.parse_expression();
        self.skip_trivia();
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    // ========================================================================
    // Pattern Parsing (Java 21+)
    // ========================================================================

    /// Parse a pattern in switch case or instanceof (Java 21+)
    ///
    /// Patterns can be:
    /// - Type patterns: `Integer i`, `String s`, `Integer _`
    /// - Record patterns: `Point(int x, int y)`
    /// - Nested patterns: `Circle(Point(var x, var y), var r)`
    /// - null literal: `null`
    ///
    /// Note: In switch cases, expressions are also allowed for backward compatibility
    pub(super) fn parse_pattern(&mut self) {
        self.parse_pattern_with_inferred_type(false);
    }

    fn parse_pattern_with_inferred_type(&mut self, allow_inferred_type: bool) {
        // Check for null literal pattern
        if self.at(SyntaxKind::NULL_LITERAL) {
            self.parse_expression(); // Parse as expression for now
            return;
        }

        // Try to detect if this is a type pattern by looking ahead
        if self.is_type_pattern(allow_inferred_type) {
            self.parse_type_pattern_with_inferred_type(allow_inferred_type);
        } else {
            // Fall back to expression parsing for constants/enums
            self.parse_expression();
        }
    }

    /// Check if the current position looks like a type pattern
    fn is_type_pattern(&self, allow_inferred_type: bool) -> bool {
        self.looks_like_type_pattern(0, allow_inferred_type)
    }

    /// Parse a type pattern: `Integer i`, `String s`, `Integer _`
    pub(super) fn parse_type_pattern(&mut self) {
        self.parse_type_pattern_with_inferred_type(false);
    }

    fn parse_type_pattern_with_inferred_type(&mut self, allow_inferred_type: bool) {
        self.start_node(SyntaxKind::TYPE_PATTERN);

        // A top-level type pattern may not use `var`. A nested type pattern
        // directly inside a record pattern may infer its component type.
        if allow_inferred_type {
            self.parse_local_variable_type();
        } else {
            self.parse_type();
        }
        self.skip_trivia();

        // Check for record pattern components: Type(...)
        if self.at(SyntaxKind::L_PAREN) {
            // This is actually a record pattern, finish type pattern and start record
            self.finish_node(); // End TYPE_PATTERN
            self.start_node(SyntaxKind::RECORD_PATTERN);
            self.parse_record_pattern_components();
            self.finish_node(); // End RECORD_PATTERN
            return;
        }

        // Parse pattern variable: identifier or _ (unnamed pattern)
        // Note: _ is lexed as IDENT, so expect_identifier handles both
        self.expect_identifier();

        self.finish_node();
    }

    /// Parse record pattern components: ( pattern, pattern, ... )
    fn parse_record_pattern_components(&mut self) {
        self.expect(SyntaxKind::L_PAREN);
        self.skip_trivia();

        // Parse pattern list
        if !self.at(SyntaxKind::R_PAREN) {
            loop {
                let start_pos = self.pos;

                // Recursively parse patterns (allows nesting)
                self.parse_pattern_with_inferred_type(true);
                self.skip_trivia();

                // BUGFIX: If parse_pattern didn't consume any tokens, break to prevent infinite loop
                if self.pos == start_pos {
                    break;
                }

                if !self.eat(SyntaxKind::COMMA) {
                    break;
                }
                self.skip_trivia();
            }
        }

        self.expect(SyntaxKind::R_PAREN);
    }

    /// Parse a when clause (pattern guard): when x > 0
    fn parse_when_clause(&mut self) {
        self.start_node(SyntaxKind::WHEN_CLAUSE);

        self.expect(SyntaxKind::WHEN_KW);
        self.skip_trivia();

        // Parse the guard expression. Keep `in_switch_case` set so a top-level
        // `(...) ->` is treated as the case arrow rather than a lambda; nested
        // arg-lists / parens clear the flag so genuine nested lambdas still parse.
        let prev_in_switch_case = self.in_switch_case;
        self.in_switch_case = true;
        self.parse_expression();
        self.in_switch_case = prev_in_switch_case;

        self.finish_node();
    }

    // ========================================================================
    // Local Type Declarations (JLS §14.3)
    // ========================================================================

    /// Parse a local class declaration inside a method
    ///
    /// Per JLS §14.3, local classes cannot be public, protected, private, or static.
    /// But they can be final, abstract, strictfp, and have annotations.
    fn parse_local_class_decl(&mut self) {
        // Delegate to parse_type_decl which handles modifiers + class parsing
        self.parse_type_decl();
    }

    /// Parse a local interface declaration inside a method
    ///
    /// Per JLS §14.3, local interfaces are implicitly static.
    fn parse_local_interface_decl(&mut self) {
        // Delegate to parse_type_decl which handles modifiers + interface parsing
        self.parse_type_decl();
    }

    /// Parse a local enum declaration inside a method
    ///
    /// Per JLS §14.3, local enums are implicitly static.
    fn parse_local_enum_decl(&mut self) {
        // Delegate to parse_type_decl which handles modifiers + enum parsing
        self.parse_type_decl();
    }

    /// Parse a local record declaration inside a method
    ///
    /// Per JLS §14.3, local records are implicitly static.
    fn parse_local_record_decl(&mut self) {
        // Delegate to parse_type_decl which handles modifiers + record parsing
        self.parse_type_decl();
    }
}
