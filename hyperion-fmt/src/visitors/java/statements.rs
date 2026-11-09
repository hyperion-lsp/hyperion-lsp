// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Statement visitors: blocks, control flow, loops, try/catch, switch, etc.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::SyntaxElement;

use super::{is_identifier_or_contextual_keyword, is_statement_node, is_type_node, JavaFormatter};

impl JavaFormatter<'_> {
    /// Visit a block (e.g., method body)
    pub(super) fn visit_block(&mut self, node: &SyntaxNode) {
        self.builder.text("{");

        let has_content = node.children_with_tokens().any(|child| match &child {
            SyntaxElement::Node(_) => true,
            SyntaxElement::Token(t) => {
                t.kind() != SyntaxKind::L_BRACE
                    && t.kind() != SyntaxKind::R_BRACE
                    && t.kind() != SyntaxKind::WHITESPACE
            }
        });

        if has_content {
            self.builder.indent(|b| {
                let mut formatter = JavaFormatter::new(b);
                let mut pending_blank_line = false;
                let mut prev_whitespace_has_newline = false;

                let mut first_stmt = true;
                for child in node.children_with_tokens() {
                    match child {
                        SyntaxElement::Node(n) => {
                            // Blank lines can hide inside the previous statement's
                            // subtree (braceless if bodies), so also consult the
                            // raw token stream before this statement
                            if pending_blank_line
                                || (!first_stmt && Self::source_blank_line_before(&n))
                            {
                                formatter.builder.blank_line();
                                pending_blank_line = false;
                            }
                            formatter.builder.hardline();
                            formatter.visit_node(&n);
                            prev_whitespace_has_newline = false;
                            first_stmt = false;
                        }
                        SyntaxElement::Token(t) => {
                            match t.kind() {
                                SyntaxKind::L_BRACE | SyntaxKind::R_BRACE => {
                                    // Skip braces
                                }
                                SyntaxKind::WHITESPACE => {
                                    // Check for newlines/blank lines
                                    let text = t.text();
                                    let newline_count = text.chars().filter(|&c| c == '\n').count();
                                    if newline_count >= 2 {
                                        pending_blank_line = true;
                                    }
                                    prev_whitespace_has_newline = newline_count >= 1;
                                }
                                SyntaxKind::LINE_COMMENT
                                | SyntaxKind::BLOCK_COMMENT
                                | SyntaxKind::DOC_COMMENT => {
                                    if prev_whitespace_has_newline {
                                        // Standalone comment - put on its own line
                                        if pending_blank_line {
                                            formatter.builder.blank_line();
                                            pending_blank_line = false;
                                        }
                                        formatter.builder.hardline();
                                    } else {
                                        // Trailing comment - add space before it
                                        formatter.builder.text(" ");
                                    }
                                    formatter.builder.literal(t.text().to_string());
                                    prev_whitespace_has_newline = false;
                                }
                                _ => {
                                    formatter.visit_token(&t);
                                    prev_whitespace_has_newline = false;
                                }
                            }
                        }
                    }
                }
            });
            // Add newline before closing brace only for non-empty blocks
            self.builder.hardline();
        }
        // Empty blocks stay on one line: {}
        self.builder.text("}");
    }

    /// Visit a block comment
    pub(super) fn visit_block_comment(&mut self, node: &SyntaxNode) {
        // Preserve block comments as-is
        for child in node.children_with_tokens() {
            if let SyntaxElement::Token(t) = child {
                self.builder.literal(t.text().to_string());
            }
        }
    }

    /// Visit an if statement
    pub(super) fn visit_if_stmt(&mut self, node: &SyntaxNode) {
        let mut saw_else = false;
        let mut saw_r_paren = false; // Track if we're past the condition
        let mut saw_line_comment = false;

        // Collect children to enable lookahead for else formatting
        let children: Vec<_> = node.children_with_tokens().collect();

        for (i, child) in children.iter().enumerate() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::IF_KW => {
                        self.builder.text("if ");
                        saw_line_comment = false;
                    }
                    SyntaxKind::ELSE_KW => {
                        // Check what follows else to decide formatting:
                        // - else if: keep on same line
                        // - else { block }: keep on same line
                        // - else statement: break to new line (palantir style)
                        let next_is_block_or_if = children.get(i + 1..).is_some_and(|rest| {
                            rest.iter()
                                .find(|c| matches!(c, SyntaxElement::Node(_)))
                                .is_some_and(|n| {
                                    if let SyntaxElement::Node(n) = n {
                                        matches!(
                                            n.kind(),
                                            SyntaxKind::BLOCK_STMT | SyntaxKind::IF_STMT
                                        )
                                    } else {
                                        false
                                    }
                                })
                        });

                        if saw_line_comment {
                            // A line comment already ended the previous line;
                            // do not emit a leading space at the start of this
                            // line (for example `} // comment\nelse {`).
                            self.builder.text("else");
                        } else if next_is_block_or_if {
                            self.builder.text(" else");
                        } else {
                            // Single statement after else - break to new line
                            self.builder.hardline();
                            self.builder.text("else");
                        }
                        saw_else = true;
                        saw_r_paren = false; // Reset for else branch
                        saw_line_comment = false;
                    }
                    SyntaxKind::R_PAREN => {
                        self.builder.text(t.text());
                        saw_r_paren = true;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace. Keep the line-comment marker
                        // through the trivia before `else`.
                    }
                    SyntaxKind::LINE_COMMENT
                    | SyntaxKind::BLOCK_COMMENT
                    | SyntaxKind::DOC_COMMENT => {
                        if Self::comment_has_source_line_before(t) {
                            self.builder.hardline();
                        } else {
                            self.builder.text(" ");
                        }
                        if t.kind() == SyntaxKind::LINE_COMMENT {
                            self.builder.line_comment(t.text().to_string());
                            saw_line_comment = true;
                        } else {
                            self.builder.literal(t.text().to_string());
                            saw_line_comment = false;
                        }
                    }
                    _ => {
                        self.visit_token(t);
                        saw_line_comment = false;
                    }
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::BLOCK_STMT {
                        self.builder.text(" ");
                        self.visit_block(n);
                    } else if n.kind() == SyntaxKind::IF_STMT && saw_else {
                        // else if - keep on same line
                        self.builder.text(" ");
                        self.visit_if_stmt(n);
                    } else if saw_r_paren || saw_else {
                        // Non-block statement after condition or else
                        // Palantir style: keep on same line if it fits, break with indent if not
                        let continuation =
                            i32::try_from(self.builder.config().indent_size).unwrap_or(4);
                        self.builder.begin_consistent(continuation);
                        self.builder.space();
                        self.visit_node(n);
                        self.builder.end();
                    } else {
                        // Condition expression inside parentheses - no extra space
                        self.visit_node(n);
                    }
                    saw_line_comment = false;
                }
            }
        }
    }

    /// Visit a while statement
    pub(super) fn visit_while_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => match t.kind() {
                        SyntaxKind::WHILE_KW => {
                            formatter.builder.text("while ");
                        }
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        SyntaxKind::LINE_COMMENT
                        | SyntaxKind::BLOCK_COMMENT
                        | SyntaxKind::DOC_COMMENT => {
                            // Trailing comment - add space before it
                            formatter.builder.text(" ");
                            formatter.builder.literal(t.text().to_string());
                        }
                        _ => formatter.visit_token(&t),
                    },
                    SyntaxElement::Node(n) => {
                        if is_statement_node(&n) {
                            // Space before any statement body (block or single statement)
                            formatter.builder.text(" ");
                            if n.kind() == SyntaxKind::BLOCK_STMT {
                                formatter.visit_block(&n);
                            } else {
                                formatter.visit_node(&n);
                            }
                        } else {
                            formatter.visit_node(&n);
                        }
                    }
                }
            }
        });
    }

    /// Visit a do-while statement
    pub(super) fn visit_do_while_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);
            let mut in_while = false;

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::WHILE_KW => {
                                // Non-breaking space before while
                                formatter.builder.text(" while");
                                in_while = true;
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        if n.kind() == SyntaxKind::BLOCK_STMT && !in_while {
                            // Non-breaking space before block
                            formatter.builder.text(" ");
                            formatter.visit_block(&n);
                        } else {
                            formatter.visit_node(&n);
                        }
                    }
                }
            }
        });
    }

    /// Visit a for statement
    pub(super) fn visit_for_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => match t.kind() {
                        SyntaxKind::FOR_KW => {
                            formatter.builder.text("for ");
                        }
                        SyntaxKind::SEMICOLON => {
                            // Semicolons in for header need space after
                            formatter.builder.text("; ");
                        }
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        SyntaxKind::LINE_COMMENT
                        | SyntaxKind::BLOCK_COMMENT
                        | SyntaxKind::DOC_COMMENT => {
                            // Trailing comment - add space before it
                            formatter.builder.text(" ");
                            formatter.builder.literal(t.text().to_string());
                        }
                        _ => formatter.visit_token(&t),
                    },
                    SyntaxElement::Node(n) => {
                        if is_statement_node(&n) {
                            // Space before any statement body (block or single statement)
                            formatter.builder.text(" ");
                            if n.kind() == SyntaxKind::BLOCK_STMT {
                                formatter.visit_block(&n);
                            } else {
                                formatter.visit_node(&n);
                            }
                        } else {
                            formatter.visit_node(&n);
                        }
                    }
                }
            }
        });
    }

    /// Visit a foreach statement (enhanced for loop)
    pub(super) fn visit_foreach_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);
            let mut prev_was_type = false;

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => match t.kind() {
                        SyntaxKind::FOR_KW => {
                            formatter.builder.text("for ");
                        }
                        SyntaxKind::FINAL_KW => {
                            // Loop-variable modifier: `for (final Map.Entry<K, V> e : ...)`
                            formatter.builder.text("final ");
                        }
                        SyntaxKind::COLON => {
                            // Colon in enhanced for needs spaces: "for (int x : list)"
                            formatter.builder.text(" : ");
                        }
                        kind if is_identifier_or_contextual_keyword(kind) => {
                            // Variable name - needs space after type
                            // Note: `record` is a contextual keyword that can be used as identifier
                            if prev_was_type {
                                formatter.builder.text(" ");
                            }
                            formatter.builder.text(t.text());
                            prev_was_type = false;
                        }
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        SyntaxKind::LINE_COMMENT
                        | SyntaxKind::BLOCK_COMMENT
                        | SyntaxKind::DOC_COMMENT => {
                            // Trailing comment - add space before it
                            formatter.builder.text(" ");
                            formatter.builder.literal(t.text().to_string());
                        }
                        _ => formatter.visit_token(&t),
                    },
                    SyntaxElement::Node(n) => {
                        if is_statement_node(&n) {
                            // Space before any statement body (block or single statement)
                            formatter.builder.text(" ");
                            if n.kind() == SyntaxKind::BLOCK_STMT {
                                formatter.visit_block(&n);
                            } else {
                                formatter.visit_node(&n);
                            }
                        } else if is_type_node(&n) {
                            formatter.visit_node(&n);
                            prev_was_type = true;
                        } else {
                            // Add space after type before variable name (NAME_EXPR in foreach)
                            if prev_was_type && n.kind() == SyntaxKind::NAME_EXPR {
                                formatter.builder.text(" ");
                            }
                            formatter.visit_node(&n);
                            prev_was_type = false;
                        }
                    }
                }
            }
        });
    }

    /// Visit a return statement
    pub(super) fn visit_return_stmt(&mut self, node: &SyntaxNode) {
        // A bare return has no expression to separate from the semicolon, but
        // comments still need a separator (`return /* comment */;`).
        let has_expression = node.children().any(|child| {
            !matches!(
                child.kind(),
                SyntaxKind::RETURN_KW | SyntaxKind::SEMICOLON | SyntaxKind::WHITESPACE
            )
        });
        let has_comment = node.children_with_tokens().any(|child| {
            matches!(
                child,
                SyntaxElement::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::LINE_COMMENT
                            | SyntaxKind::BLOCK_COMMENT
                            | SyntaxKind::DOC_COMMENT
                    )
            )
        });
        if has_expression || has_comment {
            self.builder.text("return ");
        } else {
            self.builder.text("return");
        }

        // Find and visit the expression (if any) and semicolon
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::RETURN_KW | SyntaxKind::WHITESPACE => {
                        // Already handled above
                    }
                    SyntaxKind::SEMICOLON => {
                        self.builder.text(";");
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    // The expression can break internally
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a throw statement
    pub(super) fn visit_throw_stmt(&mut self, node: &SyntaxNode) {
        // Emit "throw " with hard space - never break between throw and expression
        self.builder.text("throw ");

        // Find and visit the expression and semicolon
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::THROW_KW | SyntaxKind::WHITESPACE => {
                        // Already handled above
                    }
                    SyntaxKind::SEMICOLON => {
                        self.builder.text(";");
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    // The expression can break internally
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a try statement
    pub(super) fn visit_try_stmt(&mut self, node: &SyntaxNode) {
        let mut prev_whitespace_has_newline = false;
        let mut pending_blank_line = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::TRY_KW => {
                        self.builder.text("try");
                        prev_whitespace_has_newline = false;
                    }
                    SyntaxKind::CATCH_KW => {
                        self.builder.text(" catch ");
                        prev_whitespace_has_newline = false;
                    }
                    SyntaxKind::FINALLY_KW => {
                        self.builder.text(" finally");
                        prev_whitespace_has_newline = false;
                    }
                    SyntaxKind::WHITESPACE => {
                        let text = t.text();
                        let newline_count = text.chars().filter(|&c| c == '\n').count();
                        if newline_count >= 2 {
                            pending_blank_line = true;
                        }
                        prev_whitespace_has_newline = newline_count >= 1;
                    }
                    SyntaxKind::LINE_COMMENT => {
                        if prev_whitespace_has_newline {
                            // Standalone comment - put on its own line
                            if pending_blank_line {
                                self.builder.blank_line();
                                pending_blank_line = false;
                            }
                            self.builder.hardline();
                        } else {
                            // Trailing comment - add space before it
                            self.builder.text(" ");
                        }
                        self.builder.literal(t.text().to_string());
                        prev_whitespace_has_newline = false;
                    }
                    SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                        if prev_whitespace_has_newline {
                            if pending_blank_line {
                                self.builder.blank_line();
                                pending_blank_line = false;
                            }
                            self.builder.hardline();
                        } else {
                            self.builder.text(" ");
                        }
                        self.builder.literal(t.text().to_string());
                        prev_whitespace_has_newline = false;
                    }
                    _ => {
                        self.builder.text(t.text());
                        prev_whitespace_has_newline = false;
                    }
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::BLOCK_STMT {
                        self.builder.text(" ");
                        self.visit_block(&n);
                    } else if n.kind() == SyntaxKind::CATCH_CLAUSE {
                        self.visit_catch_clause(&n);
                    } else if n.kind() == SyntaxKind::FINALLY_CLAUSE {
                        self.visit_finally_clause(&n);
                    } else if n.kind() == SyntaxKind::RESOURCE_SPEC {
                        // Try-with-resources: add space and visit resource spec
                        self.builder.text(" ");
                        self.visit_resource_spec(&n);
                    } else {
                        self.visit_node(&n);
                    }
                    prev_whitespace_has_newline = false;
                }
            }
        }
    }

    /// Visit a finally clause (finally { ... })
    pub(super) fn visit_finally_clause(&mut self, node: &SyntaxNode) {
        self.builder.text(" finally");

        // A comment between `finally` and the block belongs to the clause; the parser keeps
        // it here, so it is written out rather than dropped with the keyword.
        let mut after_line_comment = false;
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t)
                    if matches!(
                        t.kind(),
                        SyntaxKind::LINE_COMMENT
                            | SyntaxKind::BLOCK_COMMENT
                            | SyntaxKind::DOC_COMMENT
                    ) =>
                {
                    self.builder.text(" ");
                    self.visit_token(&t);
                    after_line_comment = t.kind() == SyntaxKind::LINE_COMMENT;
                }
                SyntaxElement::Node(n) if n.kind() == SyntaxKind::BLOCK_STMT => {
                    // A line comment has already ended its line, so the block starts there.
                    if !after_line_comment {
                        self.builder.text(" ");
                    }
                    self.visit_block(&n);
                    after_line_comment = false;
                }
                SyntaxElement::Node(n) => self.visit_node(&n),
                // The keyword and whitespace: `finally` is written above.
                SyntaxElement::Token(_) => {}
            }
        }
    }

    /// Visit a catch clause (catch (Exception e) { ... })
    pub(super) fn visit_catch_clause(&mut self, node: &SyntaxNode) {
        self.builder.text(" catch (");

        // Catch clause has: catch_kw, l_paren, type, ident, r_paren, block
        // The type and ident are direct children, not wrapped in PARAM
        let mut saw_type = false;
        let mut saw_r_paren = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::CATCH_KW | SyntaxKind::L_PAREN => {
                        // Skip
                    }
                    SyntaxKind::PIPE => {
                        // Union type separator - needs spaces
                        self.builder.text(" | ");
                    }
                    SyntaxKind::FINAL_KW => {
                        // Exception-parameter modifier: `catch (final IOException e)`
                        self.builder.text("final ");
                    }
                    SyntaxKind::IDENT if saw_type => {
                        // Exception parameter name
                        self.builder.text(" ");
                        self.builder.text(t.text());
                    }
                    SyntaxKind::R_PAREN => {
                        self.builder.text(")");
                        saw_r_paren = true;
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if is_type_node(&n) || n.kind() == SyntaxKind::UNION_TYPE {
                        self.visit_node(&n);
                        saw_type = true;
                    } else if n.kind() == SyntaxKind::BLOCK_STMT {
                        if !saw_r_paren {
                            self.builder.text(")");
                        }
                        self.builder.text(" ");
                        self.visit_block(&n);
                    } else {
                        self.visit_node(&n);
                    }
                }
            }
        }
    }

    /// Visit a switch statement
    pub(super) fn visit_switch_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);
            let mut in_body = false;

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::SWITCH_KW => {
                                formatter.builder.text("switch ");
                            }
                            SyntaxKind::L_BRACE => {
                                // Non-breaking space before brace
                                formatter.builder.text(" {");
                                in_body = true;
                            }
                            SyntaxKind::R_BRACE => {
                                formatter.builder.hardline();
                                formatter.builder.text("}");
                                in_body = false;
                            }
                            SyntaxKind::WHITESPACE if in_body => {
                                // Skip whitespace in body
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        if in_body {
                            formatter.builder.indent(|b2| {
                                b2.hardline();
                                let mut f2 = JavaFormatter::new(b2);
                                f2.visit_node(&n);
                            });
                        } else {
                            formatter.visit_node(&n);
                        }
                    }
                }
            }
        });
    }

    /// Visit a case clause (e.g., `case 1: ...` or `default: ...`)
    pub(super) fn visit_case_clause(&mut self, node: &SyntaxNode) {
        // Case clause format: "case value:" or "default:"
        // The label (case value:) stays on one line, then statements are indented
        let mut saw_colon = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::CASE_KW => {
                        self.builder.text("case ");
                    }
                    SyntaxKind::DEFAULT_KW => {
                        self.builder.text("default");
                    }
                    SyntaxKind::COLON => {
                        self.builder.text(":");
                        saw_colon = true;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if saw_colon {
                        // Statements after colon are indented
                        self.builder.indent(|b| {
                            b.hardline();
                            let mut f = JavaFormatter::new(b);
                            f.visit_node(&n);
                        });
                    } else {
                        // Value before colon (for case expressions)
                        self.visit_node(&n);
                    }
                }
            }
        }
    }

    /// Visit a synchronized statement (e.g., `synchronized (lock) { ... }`)
    pub(super) fn visit_synchronized_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::SYNCHRONIZED_KW => {
                                // Non-breaking space after synchronized
                                formatter.builder.text("synchronized ");
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        if n.kind() == SyntaxKind::BLOCK_STMT {
                            // Non-breaking space before block
                            formatter.builder.text(" ");
                            formatter.visit_block(&n);
                        } else {
                            formatter.visit_node(&n);
                        }
                    }
                }
            }
        });
    }

    /// Visit a break statement (e.g., `break;`, `break label;`)
    pub(super) fn visit_break_stmt(&mut self, node: &SyntaxNode) {
        // break statements must not break - "break\nlabel" is illegal Java
        let mut saw_break = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::BREAK_KW => {
                        self.builder.text("break");
                        saw_break = true;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    SyntaxKind::SEMICOLON => {
                        self.builder.text(";");
                    }
                    SyntaxKind::IDENT if saw_break => {
                        // Label after break - must use non-breaking space
                        self.builder.text(" ");
                        self.builder.text(t.text());
                    }
                    SyntaxKind::LINE_COMMENT
                    | SyntaxKind::BLOCK_COMMENT
                    | SyntaxKind::DOC_COMMENT => {
                        // Trailing comment - add space before it
                        self.builder.text(" ");
                        self.builder.literal(t.text().to_string());
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a continue statement (e.g., `continue;`, `continue label;`)
    pub(super) fn visit_continue_stmt(&mut self, node: &SyntaxNode) {
        // continue statements must not break - "continue\nlabel" is illegal Java
        let mut saw_continue = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::CONTINUE_KW => {
                        self.builder.text("continue");
                        saw_continue = true;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    SyntaxKind::SEMICOLON => {
                        self.builder.text(";");
                    }
                    SyntaxKind::IDENT if saw_continue => {
                        // Label after continue - must use non-breaking space
                        self.builder.text(" ");
                        self.builder.text(t.text());
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit an assert statement (e.g., `assert cond;`, `assert cond : msg;`)
    pub(super) fn visit_assert_stmt(&mut self, node: &SyntaxNode) {
        // assert statements should not break
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::ASSERT_KW => {
                        self.builder.text("assert ");
                    }
                    SyntaxKind::COLON => {
                        self.builder.text(" : ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a labeled statement (e.g., `label: statement`)
    pub(super) fn visit_labeled_stmt(&mut self, node: &SyntaxNode) {
        // Labeled statements: "label: statement" - keep on same line
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::COLON => {
                        self.builder.text(": ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a yield statement (e.g., `yield value;`)
    pub(super) fn visit_yield_stmt(&mut self, node: &SyntaxNode) {
        // yield statements should not break
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::YIELD_KW => {
                        self.builder.text("yield ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit an empty statement (just `;`)
    pub(super) fn visit_empty_stmt(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            if let SyntaxElement::Token(t) = child {
                if t.kind() == SyntaxKind::SEMICOLON {
                    self.builder.text(";");
                }
            }
        }
    }

    /// Check if a node contains structural breaks (lambdas with blocks, anonymous classes)
    /// that would cause hardlines in the output. This is used to decide whether to use
    /// structural sizing for assignments.
    pub(super) fn contains_structural_breaks(node: &SyntaxNode) -> bool {
        for descendant in node.descendants() {
            match descendant.kind() {
                SyntaxKind::LAMBDA_EXPR
                    if descendant
                        .children()
                        .any(|c| c.kind() == SyntaxKind::BLOCK_STMT) =>
                {
                    // Lambda with block body has hardlines
                    return true;
                }
                SyntaxKind::NEW_EXPR => {
                    // Anonymous class has hardlines (has an arg list followed by L_BRACE)
                    // Check for L_BRACE after ARG_LIST indicating anonymous class body
                    let mut saw_arg_list = false;
                    for child in descendant.children_with_tokens() {
                        match child {
                            SyntaxElement::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                                saw_arg_list = true;
                            }
                            SyntaxElement::Token(t)
                                if t.kind() == SyntaxKind::L_BRACE && saw_arg_list =>
                            {
                                return true;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Visit a try-with-resources statement (e.g., `try (InputStream is = ...) { ... }`)
    pub(super) fn visit_try_with_resources_stmt(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::CATCH_KW | SyntaxKind::FINALLY_KW => {
                                // Non-breaking space before catch/finally
                                formatter.builder.text(" ");
                                formatter.builder.text(t.text());
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        if n.kind() == SyntaxKind::BLOCK_STMT {
                            // Non-breaking space before block
                            formatter.builder.text(" ");
                            formatter.visit_block(&n);
                        } else if n.kind() == SyntaxKind::RESOURCE_SPEC {
                            // Non-breaking space before resource spec
                            formatter.builder.text(" ");
                            formatter.visit_resource_spec(&n);
                        } else {
                            formatter.visit_node(&n);
                        }
                    }
                }
            }
        });
    }

    /// Visit a resource specification in try-with-resources (e.g., `(InputStream is = new FileInputStream(...))`)
    pub(super) fn visit_resource_spec(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::L_PAREN => formatter.builder.text("("),
                            SyntaxKind::R_PAREN => formatter.builder.text(")"),
                            SyntaxKind::SEMICOLON => {
                                formatter.builder.text(";");
                                formatter.builder.soft_break_with(" ");
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        formatter.visit_node(&n);
                    }
                }
            }
        });
    }

    /// Visit a switch expression (Java 12+, e.g., `var x = switch (val) { case 1 -> "one"; ... }`)
    pub(super) fn visit_switch_expr(&mut self, node: &SyntaxNode) {
        // Switch expressions have the same structure as switch statements
        self.visit_switch_stmt(node);
    }

    /// Visit an expression statement (e.g., `x = 5;`, `this.field = value;`)
    /// Handles method chains with proper line breaking
    pub(super) fn visit_expr_stmt(&mut self, node: &SyntaxNode) {
        let (is_chain, is_builder, method_count, dot_count) = Self::analyze_flat_chain(node);

        // Use builder formatting for builder patterns with 3+ method calls
        if is_chain && is_builder && method_count >= 3 {
            self.visit_flat_chain(node, true);
            return;
        }
        if is_chain {
            self.visit_flat_chain(node, false);
            return;
        }

        if method_count == 1 {
            // Single method call: allow breaking before the top-level dot when receiver is long.
            // Pass the METHOD_CALL_EXPR so DOT tokens are visible at this level.
            if let Some(call) = node
                .descendants()
                .find(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
            {
                let top_level_dots = call
                    .children_with_tokens()
                    .filter(|c| matches!(c, SyntaxElement::Token(t) if t.kind() == SyntaxKind::DOT))
                    .count();
                if top_level_dots == 1 && dot_count >= 1 {
                    self.visit_single_method_call(&call);
                    return;
                }
            }
        }

        // Simple expression statement - keep on one line
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE => {}
                    SyntaxKind::LINE_COMMENT => {
                        self.builder.text(" ");
                        self.builder.line_comment(t.text().to_string());
                    }
                    SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                        self.builder.text(" ");
                        self.builder.literal(t.text().to_string());
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                        && n.children_with_tokens().count() == 0
                    {
                        continue;
                    }
                    self.visit_node(&n);
                }
            }
        }
    }
}
