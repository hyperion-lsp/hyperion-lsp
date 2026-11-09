// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Expression visitors: binary, ternary, lambda, field access, new, cast, etc.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::SyntaxElement;

use super::JavaFormatter;

impl JavaFormatter<'_> {
    /// Visit a method call expression
    /// Handles both simple calls like `foo()` and chained calls like `a.b().c()`
    pub(super) fn visit_method_call_expr(&mut self, node: &SyntaxNode) {
        // Check if this is part of a method chain
        let is_chain = Self::is_method_chain(node);

        if is_chain {
            self.visit_method_chain(node);
        } else {
            // Single call: palantir never breaks before the dot of a lone call —
            // overflow is handled inside the argument list. (The old long-receiver
            // heuristic force-broke fitting lines like `this.registry.remove(id);`.)
            self.builder.group(|b| {
                let mut formatter = JavaFormatter::new(b);
                formatter.visit_children(node);
            });
        }
    }

    /// Visit a binary expression (e.g., `a + b`, `x == y`)
    pub(super) fn visit_binary_expr(&mut self, node: &SyntaxNode) {
        // Binary expressions may break at operators when lines are too long
        // Use inconsistent group with continuation indent (+8)
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        self.builder.begin_inconsistent(continuation);

        // Note: Parser quirk - the left operand is often a sibling outside BINARY_EXPR,
        // not a child inside it. The left operand's NAME_EXPR contains trailing whitespace
        // which gets skipped by visit_simple_expr, so we always need to add space before
        // the operator here to compensate.
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if Self::is_binary_operator(t.kind()) {
                        // Break BEFORE operator (palantir style)
                        // When broken, the operator starts the new line
                        self.builder.space();
                        self.builder.text(t.text());
                        self.builder.text(" ");
                    } else if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace, we handle spacing ourselves
                    } else {
                        self.visit_token(&t);
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }

        self.builder.end();
    }

    /// Check if a token is a binary operator
    pub(super) const fn is_binary_operator(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PLUS
                | SyntaxKind::MINUS
                | SyntaxKind::STAR
                | SyntaxKind::SLASH
                | SyntaxKind::PERCENT
                | SyntaxKind::AMP
                | SyntaxKind::PIPE
                | SyntaxKind::CARET
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

    /// Visit a ternary expression (e.g., `cond ? a : b`)
    pub(super) fn visit_ternary_expr(&mut self, node: &SyntaxNode) {
        // Ternary expressions should use non-breaking spaces around ? and :
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::QUESTION => {
                        self.builder.text(" ? ");
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

    /// Visit a lambda expression (e.g., `x -> x * 2` or `(a, b) -> { return a + b; }`)
    pub(super) fn visit_lambda_expr(&mut self, node: &SyntaxNode) {
        // Lambda expressions can break after arrow when the body is a long expression
        // For block lambdas, keep () -> { on same line
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        let mut saw_arrow = false;

        // Check if this is a block lambda
        let has_block_body = node.children().any(|n| n.kind() == SyntaxKind::BLOCK_STMT);

        // Only wrap expression lambdas in a group for breaking
        // Use inconsistent (greedy) grouping to keep "x ->" with method call start
        // Consistent would force all-or-nothing breaking
        if !has_block_body {
            self.builder.begin_inconsistent(continuation);
        }

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::ARROW => {
                        // Keep "x -> body" together - let the body handle breaking internally
                        // Palantir keeps lambda param and start of body on same line
                        self.builder.text(" -> ");
                        saw_arrow = true;
                    }
                    SyntaxKind::COMMA => {
                        // Parameter separator: `(key, future) -> ...`
                        self.builder.text(", ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if saw_arrow {
                        // Body after arrow
                        if n.kind() == SyntaxKind::BLOCK_STMT {
                            self.visit_block(&n);
                        } else {
                            // Wrap expression body in inconsistent group to allow
                            // breaking after arrow without internal breaks taking precedence
                            self.builder.begin_inconsistent(0);
                            self.visit_node(&n);
                            self.builder.end();
                        }
                    } else {
                        // Parameters before arrow
                        self.visit_node(&n);
                    }
                }
            }
        }

        if !has_block_body {
            self.builder.end();
        }
    }

    /// Visit a field access expression (e.g., `obj.field`)
    pub(super) fn visit_field_access_expr(&mut self, node: &SyntaxNode) {
        // Check if part of a method chain
        let parent_is_method_call = node
            .parent()
            .is_some_and(|p| p.kind() == SyntaxKind::METHOD_CALL_EXPR);

        if parent_is_method_call {
            // Let the parent method call handle the chain
            self.visit_children(node);
        } else {
            self.builder.group(|b| {
                let mut formatter = JavaFormatter::new(b);
                formatter.visit_children(node);
            });
        }
    }

    /// Visit a new expression (e.g., `new Foo()`, `new int[10]`, `new Runnable() { ... }`)
    pub(super) fn visit_new_expr(&mut self, node: &SyntaxNode) {
        // new expressions may have an anonymous class body
        let mut in_body = false;
        let mut pending_modifier: Option<SyntaxNode> = None;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::NEW_KW => {
                        self.builder.text("new ");
                    }
                    SyntaxKind::L_BRACE => {
                        self.builder.text(" {");
                        in_body = true;
                    }
                    SyntaxKind::R_BRACE => {
                        self.builder.hardline();
                        self.builder.text("}");
                        in_body = false;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if in_body {
                        // Anonymous class body - handle like class body
                        if n.kind() == SyntaxKind::MODIFIER_LIST {
                            pending_modifier = Some(n);
                        } else {
                            self.builder.indent(|b| {
                                b.hardline();
                                let mut f = JavaFormatter::new(b);
                                if let Some(modifier) = pending_modifier.take() {
                                    f.visit_modifier_list(&modifier);
                                    f.builder.text(" ");
                                }
                                f.visit_node(&n);
                            });
                        }
                    } else if n.kind() == SyntaxKind::ARRAY_INIT_EXPR {
                        // Space before array initializer: new int[] { ... }
                        self.builder.text(" ");
                        self.visit_node(&n);
                    } else {
                        self.visit_node(&n);
                    }
                }
            }
        }
    }

    /// Visit an array initializer (e.g., `{1, 2, 3}`)
    pub(super) fn visit_array_init_expr(&mut self, node: &SyntaxNode) {
        // Array initializers use fill-style wrapping: keep as many elements as
        // fit on each line, then continue at one normal indentation level. A
        // consistent group would break after every comma once the initializer
        // overflowed, which is unlike palantir-java-format for large tables.
        let indent = i32::try_from(self.builder.config().indent_size).unwrap_or(4);
        self.builder.begin_inconsistent(indent);

        let mut group_open = true;
        let mut last_was_comma = false;
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::L_BRACE => self.builder.text("{"),
                    SyntaxKind::R_BRACE => {
                        // A trailing comma already supplied the line break in
                        // a broken group; avoid adding a blank line before `}`.
                        if !last_was_comma {
                            self.builder.softline();
                        }
                        self.builder.end();
                        group_open = false;
                        self.builder.text("}");
                    }
                    SyntaxKind::COMMA => {
                        self.builder.text(",");
                        self.builder.soft_break_with(" ");
                        last_was_comma = true;
                    }
                    SyntaxKind::WHITESPACE => {}
                    _ => {
                        self.visit_token(&t);
                        last_was_comma = false;
                    }
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                    last_was_comma = false;
                }
            }
        }

        if group_open {
            self.builder.end();
        }
    }

    /// Visit a parenthesized expression (e.g., `(a + b)`)
    pub(super) fn visit_paren_expr(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);
            formatter.visit_children(node);
        });
    }

    /// Visit a unary expression (e.g., `++i`, `!flag`, `-x`, `~bits`)
    pub(super) fn visit_unary_expr(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);
            let mut saw_operand = false;

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            SyntaxKind::PLUS_PLUS | SyntaxKind::MINUS_MINUS => {
                                // Pre/post increment/decrement - no space
                                formatter.builder.text(t.text());
                            }
                            SyntaxKind::BANG | SyntaxKind::TILDE => {
                                // Logical/bitwise NOT - no space before operand
                                formatter.builder.text(t.text());
                            }
                            SyntaxKind::PLUS | SyntaxKind::MINUS => {
                                // Unary plus/minus - no space before operand
                                formatter.builder.text(t.text());
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        saw_operand = true;
                        formatter.visit_node(&n);
                    }
                }
            }

            // Handle postfix operators (operand comes first)
            let _ = saw_operand; // suppress unused warning
        });
    }

    /// Visit an assignment expression (e.g., `x = 5`, `y += 10`)
    pub(super) fn visit_assignment_expr(&mut self, node: &SyntaxNode) {
        // Assignment expressions should use non-breaking spaces around operators
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if Self::is_assignment_operator(t.kind()) {
                        self.builder.text(" ");
                        self.builder.text(t.text());
                        self.builder.text(" ");
                    } else if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace, we handle spacing ourselves
                    } else {
                        self.visit_token(&t);
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Check if a token is an assignment operator
    pub(super) const fn is_assignment_operator(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::EQ
                | SyntaxKind::PLUS_EQ
                | SyntaxKind::MINUS_EQ
                | SyntaxKind::STAR_EQ
                | SyntaxKind::SLASH_EQ
                | SyntaxKind::PERCENT_EQ
                | SyntaxKind::AMP_EQ
                | SyntaxKind::PIPE_EQ
                | SyntaxKind::CARET_EQ
        )
    }

    /// Visit a cast expression (e.g., `(String) obj`)
    pub(super) fn visit_cast_expr(&mut self, node: &SyntaxNode) {
        // Cast expressions: (Type) expr - use non-breaking space after cast
        let mut after_cast = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::L_PAREN => self.builder.text("("),
                    SyntaxKind::R_PAREN => {
                        self.builder.text(")");
                        after_cast = true;
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if after_cast {
                        // Non-breaking space between cast and expression
                        self.builder.text(" ");
                    }
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit an instanceof expression (e.g., `obj instanceof String`, `obj instanceof String s`)
    pub(super) fn visit_instanceof_expr(&mut self, node: &SyntaxNode) {
        // Use non-breaking spaces around instanceof keyword
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::INSTANCEOF_KW => {
                        self.builder.text(" instanceof ");
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

    /// Visit an array access expression (e.g., `arr[i]`, `matrix[row][col]`)
    pub(super) fn visit_array_access_expr(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::L_BRACKET | SyntaxKind::R_BRACKET => {
                                // No space around brackets
                                formatter.builder.text(t.text());
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

    /// Visit a method reference expression (e.g., `String::length`, `ArrayList::new`)
    pub(super) fn visit_method_ref_expr(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::COLON_COLON => {
                                // No space around ::
                                formatter.builder.text("::");
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

    /// Visit a this expression (e.g., `this`)
    pub(super) fn visit_this_expr(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            if let SyntaxElement::Token(t) = child {
                if t.kind() != SyntaxKind::WHITESPACE {
                    self.builder.text(t.text());
                }
            }
        }
    }

    /// Visit a super expression (e.g., `super`)
    pub(super) fn visit_super_expr(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            if let SyntaxElement::Token(t) = child {
                if t.kind() != SyntaxKind::WHITESPACE {
                    self.builder.text(t.text());
                }
            }
        }
    }

    /// Visit a simple expression (NAME_EXPR, LITERAL_EXPR) - just tokens, skip whitespace
    pub(super) fn visit_simple_expr(&mut self, node: &SyntaxNode) {
        // Nodes can sit in a name expression (an annotation on `@A String::new`), so they
        // are visited, not skipped. Comments go through the token visitor, which ends a
        // line comment's line.
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                    // An annotation is followed by the name it annotates, which needs a space.
                    if n.kind() == SyntaxKind::ANNOTATION {
                        self.builder.text(" ");
                    }
                }
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE => {}
                    SyntaxKind::LINE_COMMENT
                    | SyntaxKind::BLOCK_COMMENT
                    | SyntaxKind::DOC_COMMENT => {
                        self.visit_token(&t);
                    }
                    _ => self.builder.text(t.text()),
                },
            }
        }
    }

    /// Visit a class literal expression (e.g., `String.class`, `int.class`)
    pub(super) fn visit_class_literal_expr(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::DOT => {
                                // No space around dot
                                formatter.builder.text(".");
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
}
