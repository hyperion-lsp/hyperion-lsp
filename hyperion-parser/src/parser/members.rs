// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Class Member Parsing
//!
//! This module contains methods for parsing class members: fields, methods,
//! constructors, nested types, and initializer blocks.

use crate::syntax::SyntaxKind;

use super::is_identifier_kind;
use super::ParseMode;

impl super::Parser<'_> {
    /// Parse a class member (field, method, constructor, nested type, initializer)
    ///
    /// Returns true if a member was parsed
    pub(super) fn parse_class_member(&mut self) -> bool {
        // Parse modifiers
        self.parse_modifiers();

        self.skip_trivia();

        // Check for various member types
        match self.current() {
            // Nested type declarations
            SyntaxKind::CLASS_KW => {
                self.parse_class_decl();
                true
            }
            SyntaxKind::INTERFACE_KW => {
                self.parse_interface_decl();
                true
            }
            SyntaxKind::ENUM_KW => {
                self.parse_enum_decl();
                true
            }
            SyntaxKind::RECORD_KW if self.is_record_type_decl() => {
                self.parse_record_decl();
                true
            }
            // Annotation type declaration (@interface)
            SyntaxKind::AT => {
                // Check if this is @interface (annotation type declaration)
                let mut lookahead = self.pos + 1;
                while lookahead < self.tokens.len() && self.tokens[lookahead].0.is_trivia() {
                    lookahead += 1;
                }
                if lookahead < self.tokens.len()
                    && self.tokens[lookahead].0 == SyntaxKind::INTERFACE_KW
                {
                    self.parse_annotation_type_decl();
                    return true;
                }
                // Otherwise fall through to default case (error recovery)
                false
            }
            // Static or instance initializer block
            SyntaxKind::L_BRACE => {
                self.parse_initializer();
                true
            }
            // Type parameters indicate a method
            SyntaxKind::LT => {
                self.parse_method_or_constructor();
                true
            }
            // Otherwise, need to look ahead to disambiguate field/method/constructor
            _ => {
                if self.at_eof() || self.at(SyntaxKind::R_BRACE) {
                    false
                } else {
                    // Compact record constructors omit the parameter list. Detect IDENT followed by `{`.
                    if self.at_type_identifier() {
                        if let Some(&record_name) = self.record_stack.last() {
                            if self.current_text() == record_name {
                                let mut lookahead = self.pos + 1;
                                while lookahead < self.tokens.len()
                                    && self.tokens[lookahead].0.is_trivia()
                                {
                                    lookahead += 1;
                                }
                                if lookahead < self.tokens.len()
                                    && self.tokens[lookahead].0 == SyntaxKind::L_BRACE
                                {
                                    self.parse_constructor_decl();
                                    return true;
                                }
                            }
                        }
                    }

                    self.parse_method_or_field();
                    true
                }
            }
        }
    }

    /// Parse method or field declaration
    ///
    /// This requires lookahead to disambiguate:
    /// - Field: Type name [= expr] ;
    /// - Method: Type name ( params ) body
    /// - Constructor: name ( params ) body
    pub(super) fn parse_method_or_field(&mut self) {
        // BUGFIX: Use lookahead without adding tokens to the builder to avoid duplication
        // We need to determine if this is a method or field WITHOUT parsing tokens yet

        // Check if this is a constructor: TypeIdentifier directly followed by (
        // Constructors have no return type, just name(params)
        if self.at_type_identifier() {
            // Look ahead to find the next significant token after the identifier
            let mut lookahead = self.pos + 1;
            while lookahead < self.tokens.len() && self.tokens[lookahead].0.is_trivia() {
                lookahead += 1;
            }

            if lookahead < self.tokens.len() {
                let next = self.tokens[lookahead].0;
                if next == SyntaxKind::L_PAREN || next == SyntaxKind::L_BRACE {
                    // This is a constructor (regular or compact record constructor)
                    self.parse_constructor_decl();
                    return;
                }
            }
        }

        // For method vs field disambiguation, we need to look ahead to find if there's a (
        // after the return type and name. We'll do this by scanning forward in the token stream
        // WITHOUT consuming tokens or adding to the builder.

        let mut lookahead_pos = self.pos;
        let mut found_paren = false;

        // Skip type parameters if present
        if lookahead_pos < self.tokens.len() && self.tokens[lookahead_pos].0 == SyntaxKind::LT {
            // Skip to matching > (handles >>, >>> from nested generics)
            let mut depth = 1;
            lookahead_pos += 1;
            while lookahead_pos < self.tokens.len() && depth > 0 {
                match self.tokens[lookahead_pos].0 {
                    SyntaxKind::LT => depth += 1,
                    SyntaxKind::GT => depth -= 1,
                    SyntaxKind::SHR => {
                        // >> counts as two >
                        depth -= 2;
                    }
                    SyntaxKind::USHR => {
                        // >>> counts as three >
                        depth -= 3;
                    }
                    _ => {}
                }
                lookahead_pos += 1;
            }
        }

        // Skip return type/type - just advance past type-like tokens
        while lookahead_pos < self.tokens.len() {
            let kind = self.tokens[lookahead_pos].0;
            if kind.is_trivia() ||
               is_identifier_kind(kind) ||  // IDENT or contextual keywords (for method names)
               kind == SyntaxKind::DOT ||
               kind == SyntaxKind::LT || kind == SyntaxKind::GT ||
               kind == SyntaxKind::SHR || kind == SyntaxKind::USHR ||  // For nested generics
               kind == SyntaxKind::COMMA ||
               kind == SyntaxKind::L_BRACKET || kind == SyntaxKind::R_BRACKET ||
               kind == SyntaxKind::EXTENDS_KW || kind == SyntaxKind::SUPER_KW ||
               kind == SyntaxKind::QUESTION ||
               // Primitive types
               kind == SyntaxKind::BOOLEAN_KW || kind == SyntaxKind::BYTE_KW ||
               kind == SyntaxKind::SHORT_KW || kind == SyntaxKind::INT_KW ||
               kind == SyntaxKind::LONG_KW || kind == SyntaxKind::CHAR_KW ||
               kind == SyntaxKind::FLOAT_KW || kind == SyntaxKind::DOUBLE_KW ||
               kind == SyntaxKind::VOID_KW
            {
                lookahead_pos += 1;
            } else {
                break;
            }
        }

        // Check what follows the type+name
        if lookahead_pos < self.tokens.len() && self.tokens[lookahead_pos].0 == SyntaxKind::L_PAREN
        {
            found_paren = true;
        }

        // Now actually parse based on what we found
        if found_paren {
            self.parse_method_decl();
        } else {
            self.parse_field_decl();
        }
    }

    /// Parse a method declaration
    ///
    /// Grammar:
    /// ```text
    /// MethodDecl:
    ///     {Modifier} [TypeParams] ReturnType Ident ParamList [throws TypeList] (Block | ;)
    /// ```
    pub(super) fn parse_method_decl(&mut self) {
        self.start_node(SyntaxKind::METHOD_DECL);

        // Type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
            self.skip_trivia();
        }

        // Return type
        self.parse_result_type();

        self.skip_trivia();

        // Method name (can be contextual keyword)
        self.expect_identifier();

        self.skip_trivia();

        // Parameter list (optional for record compact constructors)
        if self.at(SyntaxKind::L_PAREN) {
            self.parse_param_list();
        }

        self.skip_trivia();

        // Throws clause (optional)
        if self.at(SyntaxKind::THROWS_KW) {
            self.start_node(SyntaxKind::THROWS_CLAUSE);
            self.bump();
            self.skip_trivia();
            self.parse_type_list();
            self.finish_node();
            self.skip_trivia();
        }

        // Default value (annotation methods only)
        // Grammar: default Value
        if self.at(SyntaxKind::DEFAULT_KW) {
            self.start_node(SyntaxKind::DEFAULT_VALUE);
            self.bump(); // consume 'default'
            self.skip_trivia();
            self.parse_annotation_value();
            self.finish_node();
            self.skip_trivia();
        }

        // Method body or semicolon (abstract/interface methods)
        if self.at(SyntaxKind::L_BRACE) {
            if self.parse_mode == ParseMode::DeclarationsOnly {
                self.skip_to_matching_brace();
            } else {
                self.parse_block();
            }
        } else {
            self.expect(SyntaxKind::SEMICOLON);
        }

        self.finish_node();
    }

    /// Parse a constructor declaration
    ///
    /// Grammar:
    /// ```text
    /// ConstructorDecl:
    ///     {Modifier} [TypeParams] Ident ParamList [throws TypeList] Block
    /// ```
    pub(super) fn parse_constructor_decl(&mut self) {
        self.start_node(SyntaxKind::CONSTRUCTOR_DECL);

        // Type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
            self.skip_trivia();
        }

        // Constructor name (same as class name)
        self.expect_type_identifier();

        self.skip_trivia();

        // Parameter list (optional for compact record constructors)
        if self.at(SyntaxKind::L_PAREN) {
            self.parse_param_list();
        }

        self.skip_trivia();

        // Throws clause (optional)
        if self.at(SyntaxKind::THROWS_KW) {
            self.start_node(SyntaxKind::THROWS_CLAUSE);
            self.bump();
            self.skip_trivia();
            self.parse_type_list();
            self.finish_node();
            self.skip_trivia();
        }

        // Constructor body (always has a body, cannot be abstract)
        if self.parse_mode == ParseMode::DeclarationsOnly {
            self.skip_to_matching_brace();
        } else {
            // Use parse_constructor_body() instead of parse_block() to handle
            // explicit constructor invocations (this(...) or super(...))
            self.parse_constructor_body();
        }

        self.finish_node();
    }

    /// Parse method or constructor (when type params are present)
    pub(super) fn parse_method_or_constructor(&mut self) {
        if self.lookahead_is_constructor_with_type_params() {
            self.parse_constructor_decl();
        } else {
            self.parse_method_decl();
        }
    }

    /// Lookahead helper to detect constructors that declare type parameters.
    ///
    /// We only call this when the current token is `<`. The logic peeks ahead
    /// without consuming tokens to determine whether the sequence corresponds
    /// to a constructor (i.e. the class name appears directly before the
    /// parameter list) or a regular method (where a return type and method name
    /// appear before the parameter list).
    pub(super) fn lookahead_is_constructor_with_type_params(&self) -> bool {
        let mut pos = self.pos;
        let len = self.tokens.len();

        if pos >= len || self.tokens[pos].0 != SyntaxKind::LT {
            return false;
        }

        // Skip the full type parameter list `<...>`
        pos += 1;
        let mut depth = 1;
        while pos < len && depth > 0 {
            match self.tokens[pos].0 {
                SyntaxKind::LT => depth += 1,
                SyntaxKind::GT => depth -= 1,
                SyntaxKind::SHR => depth -= 2,
                SyntaxKind::USHR => depth -= 3,
                _ => {}
            }
            pos += 1;
        }

        self.skip_trivia_lookahead(&mut pos);

        // Skip potential annotations between the type parameters and the name.
        while pos < len && self.tokens[pos].0 == SyntaxKind::AT {
            if let Some(next) = self.skip_annotation_lookahead(pos) {
                pos = next;
                self.skip_trivia_lookahead(&mut pos);
            } else {
                return false;
            }
        }

        // The next meaningful token must be an identifier (the class name)
        if pos >= len || !self.tokens[pos].0.is_type_identifier() {
            return false;
        }

        pos += 1;
        self.skip_trivia_lookahead(&mut pos);

        // Skip any type suffix that would belong to a return type (qualified
        // names, generics, arrays, or type-use annotations).
        loop {
            if pos >= len {
                break;
            }

            match self.tokens[pos].0 {
                SyntaxKind::DOT => {
                    pos += 1;
                    self.skip_trivia_lookahead(&mut pos);
                    if pos < len && self.tokens[pos].0.is_type_identifier() {
                        pos += 1;
                        self.skip_trivia_lookahead(&mut pos);
                    } else {
                        return false;
                    }
                }
                SyntaxKind::LT => {
                    pos += 1;
                    let mut depth = 1;
                    while pos < len && depth > 0 {
                        match self.tokens[pos].0 {
                            SyntaxKind::LT => depth += 1,
                            SyntaxKind::GT => depth -= 1,
                            SyntaxKind::SHR => depth -= 2,
                            SyntaxKind::USHR => depth -= 3,
                            _ => {}
                        }
                        pos += 1;
                    }
                    self.skip_trivia_lookahead(&mut pos);
                }
                SyntaxKind::L_BRACKET => {
                    pos += 1;
                    self.skip_trivia_lookahead(&mut pos);
                    if pos < len && self.tokens[pos].0 == SyntaxKind::R_BRACKET {
                        pos += 1;
                        self.skip_trivia_lookahead(&mut pos);
                    } else {
                        return false;
                    }
                }
                SyntaxKind::AT => {
                    if let Some(next) = self.skip_annotation_lookahead(pos) {
                        pos = next;
                        self.skip_trivia_lookahead(&mut pos);
                    } else {
                        return false;
                    }
                }
                _ => break,
            }
        }

        // If the next significant token is another identifier, we have a method
        // name and therefore a method declaration, not a constructor.
        if pos < len && self.tokens[pos].0.is_identifier() {
            return false;
        }

        pos < len && self.tokens[pos].0 == SyntaxKind::L_PAREN
    }

    /// Lookahead helper to skip trivia tokens.
    pub(super) fn skip_trivia_lookahead(&self, pos: &mut usize) {
        while *pos < self.tokens.len() && self.tokens[*pos].0.is_trivia() {
            *pos += 1;
        }
    }

    /// Lookahead helper to skip over an annotation starting at `@`.
    ///
    /// Returns the position immediately after the annotation or `None` if the
    /// token stream is malformed and the annotation cannot be skipped.
    pub(super) fn skip_annotation_lookahead(&self, mut pos: usize) -> Option<usize> {
        let len = self.tokens.len();
        if pos >= len || self.tokens[pos].0 != SyntaxKind::AT {
            return Some(pos);
        }

        pos += 1;
        self.skip_trivia_lookahead(&mut pos);

        // Qualified annotation name: Identifier ('.' Identifier)*. Keep this
        // aligned with `parse_annotation`, which accepts contextual keywords.
        if pos >= len || !is_identifier_kind(self.tokens[pos].0) {
            return None;
        }
        pos += 1;
        self.skip_trivia_lookahead(&mut pos);

        while pos < len && self.tokens[pos].0 == SyntaxKind::DOT {
            pos += 1;
            self.skip_trivia_lookahead(&mut pos);
            if pos < len && is_identifier_kind(self.tokens[pos].0) {
                pos += 1;
                self.skip_trivia_lookahead(&mut pos);
            } else {
                return None;
            }
        }

        // Optional annotation argument list
        if pos < len && self.tokens[pos].0 == SyntaxKind::L_PAREN {
            pos += 1;
            let mut depth = 1;
            while pos < len && depth > 0 {
                match self.tokens[pos].0 {
                    SyntaxKind::L_PAREN => depth += 1,
                    SyntaxKind::R_PAREN => depth -= 1,
                    _ => {}
                }
                pos += 1;
            }
            if depth != 0 {
                return None;
            }
        }

        Some(pos)
    }

    /// Parse a field declaration
    ///
    /// Grammar:
    /// ```text
    /// FieldDecl:
    ///     {Modifier} Type VariableDeclarators ;
    /// ```
    pub(super) fn parse_field_decl(&mut self) {
        self.start_node(SyntaxKind::FIELD_DECL);

        // Type
        self.parse_type();

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

    /// Parse an initializer block (static or instance)
    pub(super) fn parse_initializer(&mut self) {
        // Check if static initializer. We must scan back over trivia to the previous
        // significant token: `parse_class_member` (and `parse_modifiers`) skip any
        // whitespace/comments between the `static` modifier and the `{`, so a raw
        // `nth_back(1)` would land on the intervening trivia token and misclassify
        // every normally-formatted `static { ... }` block as an instance initializer.
        let is_static = {
            let mut i = self.pos;
            loop {
                if i == 0 {
                    break false;
                }
                i -= 1;
                let kind = self.tokens[i].0;
                if kind.is_trivia() {
                    continue;
                }
                break kind == SyntaxKind::STATIC_KW;
            }
        };

        if is_static {
            self.start_node(SyntaxKind::STATIC_INIT);
        } else {
            self.start_node(SyntaxKind::INSTANCE_INIT);
        }

        self.parse_block();

        self.finish_node();
    }
}
