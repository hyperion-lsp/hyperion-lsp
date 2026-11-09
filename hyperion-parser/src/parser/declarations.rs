// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Declaration Parsing
//!
//! This module contains parsing methods for top-level and nested type declarations:
//! - Source file structure (package, imports, type declarations)
//! - Class, interface, enum, record, and annotation type declarations
//! - Class body parsing

use crate::syntax::SyntaxKind;

impl super::Parser<'_> {
    // ============================================================================
    // Source File (Root)
    // ============================================================================

    /// Parse the entire source file
    ///
    /// Grammar:
    /// ```text
    /// SourceFile:
    ///     [PackageDecl] {ImportDecl} {TypeDecl}
    /// ```
    pub(super) fn parse_source_file(&mut self) {
        self.start_node(SyntaxKind::SOURCE_FILE);

        // Skip leading trivia and consume it
        while self.current().is_trivia() {
            self.bump();
        }

        // Parse package declaration (optional). Package annotations precede
        // the `package` token, so recognizing only a leading PACKAGE_KW made
        // the parser's own annotation support unreachable.
        let has_package = self.at_annotated_declaration_keyword(SyntaxKind::PACKAGE_KW);
        if has_package {
            self.parse_package_decl();
            // Skip trivia after package declaration
            self.skip_trivia();
        }

        // Parse import declarations
        while self.at(SyntaxKind::IMPORT_KW) {
            let start_pos = self.pos;
            self.parse_import_decl();
            // BUGFIX: If parse_import_decl didn't consume any tokens, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }
            // Skip trivia after import declaration
            self.skip_trivia();
        }

        // A modular compilation unit has imports followed by one (possibly
        // annotated/open) module declaration instead of type declarations.
        if self.at_module_declaration_start() {
            if has_package {
                self.error(
                    "A module declaration cannot appear in a packaged compilation unit",
                    self.current_range(),
                );
            }
            self.parse_module_decl();
            self.skip_trivia();
            if !self.at_eof() {
                self.error(
                    "A module declaration must be the only top-level declaration",
                    self.current_range(),
                );
            }
        }

        // Parse type declarations (classes, interfaces, enums, records)
        while !self.at_eof() {
            // Skip trivia between declarations
            while self.current().is_trivia() {
                self.bump();
            }

            if self.at_eof() {
                break;
            }

            if !self.parse_type_decl() {
                // Error recovery: skip to next likely declaration point
                self.recover_to_type_decl();
            }
        }

        // Consume any trailing trivia
        while !self.at_eof() {
            self.bump();
        }

        self.finish_node();
    }

    fn after_leading_annotations(&self) -> Option<usize> {
        let mut lookahead = self.pos;
        loop {
            self.skip_trivia_lookahead(&mut lookahead);
            if self
                .tokens
                .get(lookahead)
                .is_none_or(|token| token.0 != SyntaxKind::AT)
            {
                break;
            }
            lookahead = self.skip_annotation_lookahead(lookahead)?;
        }
        Some(lookahead)
    }

    fn at_annotated_declaration_keyword(&self, expected: SyntaxKind) -> bool {
        let Some(lookahead) = self.after_leading_annotations() else {
            return false;
        };
        self.tokens
            .get(lookahead)
            .is_some_and(|token| token.0 == expected)
    }

    fn at_module_declaration_start(&self) -> bool {
        let Some(mut lookahead) = self.after_leading_annotations() else {
            return false;
        };
        if self
            .tokens
            .get(lookahead)
            .is_some_and(|token| token.0 == SyntaxKind::OPEN_KW)
        {
            lookahead += 1;
            self.skip_trivia_lookahead(&mut lookahead);
        }
        self.tokens
            .get(lookahead)
            .is_some_and(|token| token.0 == SyntaxKind::MODULE_KW)
    }

    // ============================================================================
    // Package and Import Declarations
    // ============================================================================

    /// Parse a package declaration
    ///
    /// Grammar:
    /// ```text
    /// PackageDecl:
    ///     {Annotation} package QualifiedName ;
    /// ```
    pub(super) fn parse_package_decl(&mut self) {
        self.start_node(SyntaxKind::PACKAGE_DECL);

        // Parse annotations (if any)
        while self.at(SyntaxKind::AT) {
            let start_pos = self.pos;
            self.parse_annotation();
            // BUGFIX: If parse_annotation didn't consume any tokens, break to prevent infinite loop
            if self.pos == start_pos {
                break;
            }
            self.skip_trivia();
        }

        // package keyword
        self.expect(SyntaxKind::PACKAGE_KW);

        // Consume trivia
        self.skip_trivia();

        // qualified name
        self.parse_qualified_name();

        // semicolon
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    // ============================================================================
    // Module Declarations (Java 9+)
    // ============================================================================

    fn parse_module_decl(&mut self) {
        self.start_node(SyntaxKind::MODULE_DECL);

        while self.at(SyntaxKind::AT) {
            self.parse_annotation();
            self.skip_trivia();
        }

        if self.at(SyntaxKind::OPEN_KW) {
            self.bump();
            self.skip_trivia();
        }
        self.expect(SyntaxKind::MODULE_KW);
        self.skip_trivia();
        self.parse_qualified_name();
        self.skip_trivia();
        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let start = self.pos;
            match self.current() {
                SyntaxKind::REQUIRES_KW => self.parse_requires_directive(),
                SyntaxKind::EXPORTS_KW => {
                    self.parse_package_directive(SyntaxKind::EXPORTS_DIRECTIVE);
                }
                SyntaxKind::OPENS_KW => self.parse_package_directive(SyntaxKind::OPENS_DIRECTIVE),
                SyntaxKind::USES_KW => self.parse_uses_directive(),
                SyntaxKind::PROVIDES_KW => self.parse_provides_directive(),
                _ => {
                    self.error("Expected a module directive", self.current_range());
                    self.start_node(SyntaxKind::ERROR);
                    while !self.at_eof()
                        && !self.at(SyntaxKind::SEMICOLON)
                        && !self.at(SyntaxKind::R_BRACE)
                    {
                        self.bump();
                    }
                    self.eat(SyntaxKind::SEMICOLON);
                    self.finish_node();
                }
            }
            if self.pos == start {
                self.bump();
            }
            self.skip_trivia();
        }

        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    fn parse_requires_directive(&mut self) {
        self.start_node(SyntaxKind::REQUIRES_DIRECTIVE);
        self.bump();
        self.skip_trivia();

        let mut seen_static = false;
        let mut seen_transitive = false;
        loop {
            let modifier = if self.at(SyntaxKind::STATIC_KW) && !seen_static {
                seen_static = true;
                true
            } else if self.at(SyntaxKind::TRANSITIVE_KW) && !seen_transitive {
                // `transitive` is also a legal module-name identifier. It is a
                // modifier only when another module-name token must follow.
                let mut after = self.pos + 1;
                self.skip_trivia_lookahead(&mut after);
                let followed_by_name_or_modifier = self.tokens.get(after).is_some_and(|token| {
                    token.0.is_identifier() || token.0 == SyntaxKind::STATIC_KW
                });
                if followed_by_name_or_modifier {
                    seen_transitive = true;
                }
                followed_by_name_or_modifier
            } else {
                false
            };
            if !modifier {
                break;
            }
            self.start_node(SyntaxKind::MODIFIER);
            self.bump();
            self.finish_node();
            self.skip_trivia();
        }

        self.parse_qualified_name();
        self.skip_trivia();
        self.expect(SyntaxKind::SEMICOLON);
        self.finish_node();
    }

    fn parse_package_directive(&mut self, node_kind: SyntaxKind) {
        self.start_node(node_kind);
        self.bump();
        self.skip_trivia();
        self.parse_qualified_name();
        self.skip_trivia();

        if self.at(SyntaxKind::TO_KW) {
            self.bump();
            self.skip_trivia();
            loop {
                let start = self.pos;
                self.parse_qualified_name();
                if self.pos == start {
                    break;
                }
                self.skip_trivia();
                if !self.eat(SyntaxKind::COMMA) {
                    break;
                }
                self.skip_trivia();
            }
        }

        self.expect(SyntaxKind::SEMICOLON);
        self.finish_node();
    }

    fn parse_uses_directive(&mut self) {
        self.start_node(SyntaxKind::USES_DIRECTIVE);
        self.bump();
        self.skip_trivia();
        self.parse_qualified_type_name();
        self.skip_trivia();
        self.expect(SyntaxKind::SEMICOLON);
        self.finish_node();
    }

    fn parse_provides_directive(&mut self) {
        self.start_node(SyntaxKind::PROVIDES_DIRECTIVE);
        self.bump();
        self.skip_trivia();
        self.parse_qualified_type_name();
        self.skip_trivia();
        self.expect(SyntaxKind::WITH_KW);
        self.skip_trivia();

        loop {
            let start = self.pos;
            self.parse_qualified_type_name();
            if self.pos == start {
                break;
            }
            self.skip_trivia();
            if !self.eat(SyntaxKind::COMMA) {
                break;
            }
            self.skip_trivia();
        }

        self.expect(SyntaxKind::SEMICOLON);
        self.finish_node();
    }

    /// Parse an import declaration
    ///
    /// Grammar:
    /// ```text
    /// ImportDecl:
    ///     import [static] QualifiedName [. *] ;
    /// ```
    pub(super) fn parse_import_decl(&mut self) {
        self.start_node(SyntaxKind::IMPORT_DECL);

        // import keyword
        self.bump();

        self.skip_trivia();

        // static keyword (optional)
        self.eat(SyntaxKind::STATIC_KW);

        self.skip_trivia();

        // qualified name
        self.parse_qualified_name();

        // .* for wildcard imports (optional)
        if self.at(SyntaxKind::DOT) {
            self.bump();
            self.skip_trivia();
            self.eat(SyntaxKind::STAR);
        }

        self.skip_trivia();

        // semicolon
        self.expect(SyntaxKind::SEMICOLON);

        self.finish_node();
    }

    // ============================================================================
    // Type Declarations (Classes, Interfaces, Enums, Records)
    // ============================================================================

    /// Parse a type declaration
    ///
    /// Returns true if a declaration was parsed, false otherwise
    pub(super) fn parse_type_decl(&mut self) -> bool {
        // Parse modifiers
        self.parse_modifiers();

        // Determine type kind and delegate
        match self.current() {
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
            SyntaxKind::RECORD_KW => {
                self.parse_record_decl();
                true
            }
            SyntaxKind::AT if self.nth(1) == SyntaxKind::INTERFACE_KW => {
                self.parse_annotation_type_decl();
                true
            }
            _ => false,
        }
    }

    /// Parse a class declaration
    ///
    /// Grammar:
    /// ```text
    /// ClassDecl:
    ///     {Modifier} class Identifier [TypeParams] [extends Type] [implements TypeList] ClassBody
    /// ```
    pub(super) fn parse_class_decl(&mut self) {
        self.start_node(SyntaxKind::CLASS_DECL);

        // class keyword
        self.expect(SyntaxKind::CLASS_KW);

        self.skip_trivia();

        // class name
        self.expect_type_identifier();

        self.skip_trivia();

        // type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }

        self.skip_trivia();

        // extends clause (optional)
        if self.at(SyntaxKind::EXTENDS_KW) {
            self.bump();
            self.skip_trivia();
            self.parse_type();
        }

        self.skip_trivia();

        // implements / permits clauses (optional, order-insensitive)
        let mut seen_implements = false;
        let mut seen_permits = false;
        loop {
            if !seen_implements && self.at(SyntaxKind::IMPLEMENTS_KW) {
                self.bump();
                self.skip_trivia();
                self.parse_type_list();
                seen_implements = true;
                self.skip_trivia();
                continue;
            }

            if !seen_permits && self.at(SyntaxKind::PERMITS_KW) {
                self.bump();
                self.skip_trivia();
                self.parse_type_list();
                seen_permits = true;
                self.skip_trivia();
                continue;
            }

            break;
        }

        self.skip_trivia();

        // class body
        self.parse_class_body();

        self.finish_node();
    }

    /// Parse interface declaration: name, type parameters, `extends`, `permits` and body.
    pub(super) fn parse_interface_decl(&mut self) {
        self.start_node(SyntaxKind::INTERFACE_DECL);

        // interface keyword
        self.expect(SyntaxKind::INTERFACE_KW);

        self.skip_trivia();

        // interface name
        self.expect_type_identifier();

        self.skip_trivia();

        // type parameters (optional)
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }

        self.skip_trivia();

        // extends clause (optional)
        if self.at(SyntaxKind::EXTENDS_KW) {
            self.bump();
            self.skip_trivia();
            self.parse_type_list();
        }

        self.skip_trivia();

        // permits clause (optional for sealed interfaces)
        if self.at(SyntaxKind::PERMITS_KW) {
            self.bump();
            self.skip_trivia();
            self.parse_type_list();
        }

        self.skip_trivia();

        // interface body
        self.parse_class_body();

        self.finish_node();
    }

    /// Parse enum declaration with constants and body
    pub(super) fn parse_enum_decl(&mut self) {
        self.start_node(SyntaxKind::ENUM_DECL);

        // enum keyword
        self.expect(SyntaxKind::ENUM_KW);

        self.skip_trivia();

        // enum name
        self.expect_type_identifier();

        self.skip_trivia();

        // implements clause (optional)
        if self.at(SyntaxKind::IMPLEMENTS_KW) {
            self.bump();
            self.skip_trivia();
            self.parse_type_list();
        }

        self.skip_trivia();

        // enum body: { constants ; members }
        self.expect(SyntaxKind::L_BRACE);
        self.skip_trivia();

        // Parse enum constants (comma-separated list)
        // Constants can have annotations like @Deprecated
        while self.at_identifier() || self.at(SyntaxKind::AT) {
            self.parse_enum_constant();
            self.skip_trivia();

            if self.at(SyntaxKind::COMMA) {
                self.bump();
                self.skip_trivia();
            } else {
                break;
            }
        }

        // Optional semicolon after constants (required if there are methods/fields)
        if self.at(SyntaxKind::SEMICOLON) {
            self.bump();
            self.skip_trivia();
        }

        // Parse enum body members (methods, fields, constructors)
        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let start_pos = self.pos;
            if !self.parse_class_member() {
                // Error recovery
                self.bump();
            }
            if self.pos == start_pos {
                break;
            }
            self.skip_trivia();
        }

        self.expect(SyntaxKind::R_BRACE);

        self.finish_node();
    }

    /// Parse a single enum constant
    pub(super) fn parse_enum_constant(&mut self) {
        self.start_node(SyntaxKind::ENUM_CONSTANT);

        // Parse any annotations (e.g., @Deprecated)
        self.parse_modifiers();
        self.skip_trivia();

        // Constant name (can be contextual keyword)
        self.expect_identifier();
        self.skip_trivia();

        // Optional arguments: RED(1) or RED("red")
        if self.at(SyntaxKind::L_PAREN) {
            self.parse_arg_list();
            self.skip_trivia();
        }

        // Optional class body for constant-specific methods
        if self.at(SyntaxKind::L_BRACE) {
            self.parse_class_body();
        }

        self.finish_node();
    }

    /// Parse record declaration: name, type parameters, components, `implements` and body.
    pub(super) fn parse_record_decl(&mut self) {
        self.start_node(SyntaxKind::RECORD_DECL);

        // record keyword
        self.expect(SyntaxKind::RECORD_KW);

        self.skip_trivia();

        // record name
        let record_name = if self.at_type_identifier() {
            let text = self.current_text();
            self.bump();
            Some(text)
        } else {
            self.expect_type_identifier();
            None
        };

        self.skip_trivia();

        // Type parameters (optional) - e.g., <T, U>
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }

        self.skip_trivia();

        // A record header is mandatory, including for a componentless record.
        // Treating it as optional accepted the invalid declaration `record R {}`.
        self.parse_param_list();

        self.skip_trivia();

        // implements clause (optional)
        if self.at(SyntaxKind::IMPLEMENTS_KW) {
            self.bump();
            self.skip_trivia();
            self.parse_type_list();
        }

        self.skip_trivia();

        // record body
        if let Some(name) = record_name {
            self.record_stack.push(name);
        }
        self.parse_class_body();
        if record_name.is_some() {
            self.record_stack.pop();
        }

        self.finish_node();
    }

    /// Parse annotation type declaration (`@interface`): name and body.
    pub(super) fn parse_annotation_type_decl(&mut self) {
        self.start_node(SyntaxKind::ANNOTATION_DECL);

        // @ symbol
        self.expect(SyntaxKind::AT);

        // interface keyword
        self.expect(SyntaxKind::INTERFACE_KW);

        self.skip_trivia();

        // annotation name
        self.expect_type_identifier();

        self.skip_trivia();

        // annotation body
        self.parse_class_body();

        self.finish_node();
    }

    // ============================================================================
    // Class Body and Members
    // ============================================================================

    /// Parse class body (fields, methods, constructors, nested types)
    ///
    /// Grammar:
    /// ```text
    /// ClassBody:
    ///     { {ClassBodyDecl} }
    /// ```
    pub(super) fn parse_class_body(&mut self) {
        // Local and anonymous classes may occur lexically inside a switch
        // expression, but their methods, constructors, and initializers cannot
        // yield from that enclosing expression.
        let enclosing_switch_expression = self.in_switch_expr;
        self.in_switch_expr = false;

        self.expect(SyntaxKind::L_BRACE);

        self.skip_trivia();

        while !self.at(SyntaxKind::R_BRACE) && !self.at_eof() {
            let start_pos = self.pos;
            if !self.parse_class_member() {
                // Error recovery
                self.bump();
            }
            if self.pos == start_pos {
                break;
            }
            self.skip_trivia();
        }

        self.expect(SyntaxKind::R_BRACE);
        self.in_switch_expr = enclosing_switch_expression;
    }
}
