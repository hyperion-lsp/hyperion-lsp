// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method chain formatting, flat chain analysis, builder patterns,
//! local variable declarations with chain values, and string concat helpers.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::SyntaxElement;

use crate::ir::DocumentBuilder;

use super::{is_identifier_or_contextual_keyword, is_type_node, JavaFormatter};

impl JavaFormatter<'_> {
    /// Analyze a flat method chain at the statement/declaration level
    /// Returns (is_chain, is_builder, method_count, dot_count)
    pub(super) fn analyze_flat_chain(node: &SyntaxNode) -> (bool, bool, usize, usize) {
        let mut is_builder = false;
        let mut method_count = 0;
        let mut dot_count = 0;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::DOT {
                        dot_count += 1;
                    } else if t.kind() == SyntaxKind::IDENT {
                        let name = t.text();
                        if name == "builder" || name == "newBuilder" {
                            is_builder = true;
                        }
                    }
                }
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::METHOD_CALL_EXPR {
                        method_count += 1;
                    }
                }
            }
        }

        // It's a chain if we have multiple dots and method calls
        let is_chain = dot_count >= 2 && method_count >= 2;
        (is_chain, is_builder, method_count, dot_count)
    }

    /// Visit a flat method chain with proper formatting
    ///
    /// Palantir-style: keep `receiver.firstMethod()` together, only break subsequent methods.
    /// This avoids breaking after receiver when chain exceeds 80 chars but first call fits.
    pub(super) fn visit_flat_chain(&mut self, node: &SyntaxNode, use_builder_style: bool) {
        let children: Vec<_> = node.children_with_tokens().collect();

        // Find the end of receiver.firstMethod() - after first ARG_LIST following first DOT
        let mut first_method_end = 0;
        let mut dot_count = 0;

        for (i, child) in children.iter().enumerate() {
            match child {
                SyntaxElement::Token(t) if t.kind() == SyntaxKind::DOT => {
                    dot_count += 1;
                }
                SyntaxElement::Node(n) if n.kind() == SyntaxKind::ARG_LIST && dot_count >= 1 => {
                    // Completed receiver.firstMethod()
                    first_method_end = i + 1;
                    break;
                }
                _ => {}
            }
        }

        // If we didn't find a clear first method or nothing remains, use original behavior
        if first_method_end == 0 || first_method_end >= children.len() {
            let format_chain = |b: &mut DocumentBuilder| {
                let mut formatter = JavaFormatter::new(b);
                let mut first_dot_done = false;

                for child in node.children_with_tokens() {
                    match child {
                        SyntaxElement::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {}
                            SyntaxKind::DOT => {
                                if first_dot_done {
                                    formatter.builder.chain_break();
                                }
                                formatter.builder.text(".");
                                first_dot_done = true;
                            }
                            SyntaxKind::LINE_COMMENT => {
                                formatter.builder.text(" ");
                                formatter.builder.line_comment(t.text().to_string());
                            }
                            SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                                formatter.builder.text(" ");
                                formatter.builder.literal(t.text().to_string());
                            }
                            _ => formatter.visit_token(&t),
                        },
                        SyntaxElement::Node(n) => {
                            if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                                && n.children_with_tokens().count() == 0
                            {
                                continue;
                            }
                            formatter.visit_node(&n);
                        }
                    }
                }
            };

            if use_builder_style {
                self.builder.builder_group(format_chain);
            } else {
                self.builder.chain_group(format_chain);
            }
            return;
        }

        // Phase 1: Emit receiver.firstMethod() directly (no group, no break opportunities)
        for child in &children[..first_method_end] {
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
                    _ => self.visit_token(t),
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                        && n.children_with_tokens().count() == 0
                    {
                        continue;
                    }
                    self.visit_node(n);
                }
            }
        }

        // Phase 2: Wrap remaining .method() calls in chain group with breaks before dots
        let remaining: Vec<_> = children[first_method_end..].to_vec();

        let format_remaining = |b: &mut DocumentBuilder| {
            let mut formatter = JavaFormatter::new(b);

            for child in &remaining {
                match child {
                    SyntaxElement::Token(t) => match t.kind() {
                        SyntaxKind::WHITESPACE => {}
                        SyntaxKind::DOT => {
                            // All dots in remaining portion get break opportunity
                            formatter.builder.chain_break();
                            formatter.builder.text(".");
                        }
                        SyntaxKind::LINE_COMMENT => {
                            formatter.builder.text(" ");
                            formatter.builder.line_comment(t.text().to_string());
                        }
                        SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                            formatter.builder.text(" ");
                            formatter.builder.literal(t.text().to_string());
                        }
                        _ => formatter.visit_token(t),
                    },
                    SyntaxElement::Node(n) => {
                        if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                            && n.children_with_tokens().count() == 0
                        {
                            continue;
                        }
                        formatter.visit_node(n);
                    }
                }
            }
        };

        if use_builder_style {
            self.builder.builder_group(format_remaining);
        } else {
            self.builder.chain_group(format_remaining);
        }
    }

    /// Visit a single method call (receiver.method()) with break opportunity before dot
    ///
    /// Only breaks before dot when receiver is long (>20 chars). For short receivers
    /// like `String.format(...)`, we keep `receiver.method(` together and let args break.
    pub(super) fn visit_single_method_call(&mut self, node: &SyntaxNode) {
        let children: Vec<_> = node.children_with_tokens().collect();

        // Calculate receiver length (everything before the dot)
        let mut receiver_len = 0;
        for child in &children {
            match child {
                SyntaxElement::Token(t) if t.kind() == SyntaxKind::DOT => break,
                SyntaxElement::Token(t) if t.kind() != SyntaxKind::WHITESPACE => {
                    receiver_len += t.text().len();
                }
                SyntaxElement::Node(n) => {
                    receiver_len += usize::from(n.text().len());
                }
                _ => {}
            }
        }

        // For short receivers, don't use chain breaking - let arg list handle breaks
        if receiver_len < 20 {
            for child in &children {
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
                        _ => self.visit_token(t),
                    },
                    SyntaxElement::Node(n) => {
                        if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                            && n.children_with_tokens().count() == 0
                        {
                            continue;
                        }
                        self.visit_node(n);
                    }
                }
            }
            return;
        }

        // For long receivers, add a break opportunity before dot, and force it.
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        self.builder.begin_builder(continuation);

        for child in &children {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE => {}
                    SyntaxKind::DOT => {
                        self.builder.chain_break();
                        self.builder.text(".");
                    }
                    SyntaxKind::LINE_COMMENT => {
                        self.builder.text(" ");
                        self.builder.line_comment(t.text().to_string());
                    }
                    SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                        self.builder.text(" ");
                        self.builder.literal(t.text().to_string());
                    }
                    _ => self.visit_token(t),
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                        && n.children_with_tokens().count() == 0
                    {
                        continue;
                    }
                    self.visit_node(n);
                }
            }
        }

        self.builder.end();
    }

    /// Check if a method call is part of a chain (has receiver that's also a method call)
    pub(super) fn is_method_chain(node: &SyntaxNode) -> bool {
        // Only treat it as a chain when the receiver is itself a method call.
        //
        // Treating FIELD_ACCESS_EXPR as a chain receiver causes undesired receiver splitting
        // for common patterns like `System.out.println(...)`, where `System.out` is just a
        // dotted receiver.
        node.children()
            .next()
            .is_some_and(|first_child| first_child.kind() == SyntaxKind::METHOD_CALL_EXPR)
    }

    /// Check if a method chain is a builder pattern
    /// Returns (is_builder, method_count)
    pub(super) fn analyze_chain(node: &SyntaxNode) -> (bool, usize) {
        // With nested expression trees, walk the chain from the outermost
        // METHOD_CALL_EXPR down through nested receivers to count method calls
        // and detect builder patterns
        let mut is_builder = false;
        let mut method_count = 0;

        // Walk the chain recursively (outermost call first)
        let mut current = Some(node.clone());
        while let Some(n) = current {
            if n.kind() == SyntaxKind::METHOD_CALL_EXPR {
                method_count += 1;
                // Check for builder/newBuilder IDENT token
                for elem in n.children_with_tokens() {
                    if let SyntaxElement::Token(t) = elem {
                        if t.kind() == SyntaxKind::IDENT {
                            let name = t.text();
                            if name == "builder" || name == "newBuilder" {
                                is_builder = true;
                            }
                        }
                    }
                }
            }
            // Descend into the receiver (first child node)
            current = n.children().next().filter(|c| {
                matches!(
                    c.kind(),
                    SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::FIELD_ACCESS_EXPR
                )
            });
        }

        (is_builder, method_count)
    }

    /// Visit a method chain with proper line breaking
    /// palantir style: break before dot when chain exceeds 80 chars
    /// builder patterns use continuation indent (8 spaces) and always break
    pub(super) fn visit_method_chain(&mut self, node: &SyntaxNode) {
        let (is_builder, method_count) = Self::analyze_chain(node);

        // Use builder formatting for builder patterns with 3+ method calls
        if is_builder && method_count >= 3 {
            self.builder.builder_group(|b| {
                let mut formatter = JavaFormatter::new(b);
                formatter.visit_method_chain_split(node);
            });
        } else if method_count >= 4 {
            // Break longer chains consistently, even if they technically fit.
            self.builder.builder_group(|b| {
                let mut formatter = JavaFormatter::new(b);
                formatter.visit_method_chain_split(node);
            });
        } else {
            self.builder.chain_group(|b| {
                let mut formatter = JavaFormatter::new(b);
                formatter.visit_method_chain_split(node);
            });
        }
    }

    /// Visit method chain with receiver.firstMethod() staying together
    /// This emits the receiver and first method call, then processes remaining chain
    ///
    /// Builder and ordinary chains share this path; the caller chooses the group.
    /// The 8-space builder continuation indent is pinned by `test_builder_pattern`
    /// in `tests/golden_tests.rs`.
    fn visit_method_chain_split(&mut self, node: &SyntaxNode) {
        let mut parts: Vec<_> = node.children_with_tokens().collect();

        // Process first element (could be receiver chain or identifier)
        if let Some(first) = parts.first() {
            match first {
                SyntaxElement::Node(n) if n.kind() == SyntaxKind::METHOD_CALL_EXPR => {
                    // Recursively process the nested chain
                    self.visit_method_chain_split(n);
                    parts.remove(0);
                }
                SyntaxElement::Node(n) => {
                    // This is the receiver (NAME_EXPR, etc.)
                    // Emit it and the first dot + method call WITHOUT chain_break
                    self.visit_node(n);
                    parts.remove(0);

                    // Emit first dot (no chain_break before it)
                    if let Some(SyntaxElement::Token(t)) = parts.first() {
                        if t.kind() == SyntaxKind::DOT {
                            self.builder.text(".");
                            parts.remove(0);
                        }
                    }

                    // Emit first method name
                    while let Some(child) = parts.first() {
                        match child {
                            SyntaxElement::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                                parts.remove(0);
                            }
                            SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => {
                                self.builder.text(t.text());
                                parts.remove(0);
                            }
                            SyntaxElement::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                                self.visit_node(n);
                                parts.remove(0);
                                break; // Done with first method call
                            }
                            _ => break,
                        }
                    }
                }
                _ => {}
            }
        }

        // Now process remaining dots and method calls WITH chain_break
        for child in parts {
            match child {
                SyntaxElement::Token(t) => {
                    match t.kind() {
                        SyntaxKind::DOT => {
                            self.builder.chain_break();
                            self.builder.text(".");
                        }
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        _ => self.visit_token(&t),
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a local variable declaration (e.g., String x = "test";)
    /// Handles method chains/builders in the value expression with proper formatting
    pub(super) fn visit_local_var_decl(&mut self, node: &SyntaxNode) {
        let (is_chain, is_builder, method_count, _dot_count) = Self::analyze_flat_chain(node);

        // Use chain/builder formatting for method chains
        if is_chain && is_builder && method_count >= 3 {
            self.visit_local_var_decl_chain(node, true);
        } else if is_chain {
            self.visit_local_var_decl_chain(node, false);
        } else {
            self.visit_local_var_decl_simple(node);
        }
    }

    /// Visit a simple local variable declaration (no chain)
    fn visit_local_var_decl_simple(&mut self, node: &SyntaxNode) {
        // Check if RHS contains structural breaks (lambdas with blocks, anonymous classes)
        let has_structural_breaks = Self::contains_structural_breaks(node);

        if !has_structural_breaks {
            if let Some(string_literals) = Self::extract_simple_string_concat_rhs(node) {
                if let Some(first) = string_literals.first() {
                    if Self::string_literal_content_len(first) >= 100 {
                        self.format_long_first_operand_string_concat_assignment(
                            node,
                            &string_literals,
                        );
                        return;
                    }
                }
            }

            if Self::constructor_assignment_overflows(node, self.builder.config().max_line_width) {
                self.format_constructor_assignment_with_broken_rhs(node);
                return;
            }
        }

        if has_structural_breaks {
            // Use structural sizing so lambdas/anonymous classes don't force breaking at `=`
            self.builder.begin_consistent_structural(0);
        }

        let mut prev_was_type = false;
        let mut prev_kind: Option<SyntaxKind> = None;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    if matches!(prev_kind, Some(SyntaxKind::FINAL_KW | SyntaxKind::VAR_KW)) {
                        self.builder.text(" ");
                    }

                    self.visit_node(&n);
                    prev_was_type = is_type_node(&n);
                    prev_kind = Some(n.kind());
                }
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        continue;
                    } else if matches!(
                        t.kind(),
                        SyntaxKind::LINE_COMMENT
                            | SyntaxKind::BLOCK_COMMENT
                            | SyntaxKind::DOC_COMMENT
                    ) {
                        self.builder.text(" ");
                        self.builder.literal(t.text());
                    } else if is_identifier_or_contextual_keyword(t.kind()) && prev_was_type {
                        self.builder.text(" ");
                        self.builder.text(t.text());
                        prev_was_type = false;
                    } else if t.kind() == SyntaxKind::EQ {
                        self.builder.text(" = ");
                    } else if t.kind() == SyntaxKind::FINAL_KW || t.kind() == SyntaxKind::VAR_KW {
                        {
                            self.builder.text(t.text());
                        }
                    } else {
                        self.visit_token(&t);
                    }
                    prev_kind = Some(t.kind());
                }
            }
        }

        if has_structural_breaks {
            self.builder.end();
        }
    }

    fn constructor_assignment_overflows(node: &SyntaxNode, max_width: usize) -> bool {
        if !node
            .children()
            .any(|child| child.kind() == SyntaxKind::NEW_EXPR)
        {
            return false;
        }

        let root = node.ancestors().last().unwrap_or_else(|| node.clone());
        let source = root.text().to_string();
        let start = usize::from(node.text_range().start());
        let column = source[..start]
            .rsplit_once('\n')
            .map_or(start, |(_, line)| line.chars().count());
        let flat_width = node
            .text()
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .count();

        column + flat_width > max_width
    }

    fn format_constructor_assignment_with_broken_rhs(&mut self, node: &SyntaxNode) {
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        let mut prev_was_type = false;
        let mut prev_kind: Option<SyntaxKind> = None;
        let mut rhs_group_open = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    if matches!(prev_kind, Some(SyntaxKind::FINAL_KW | SyntaxKind::VAR_KW)) {
                        self.builder.text(" ");
                    }
                    self.visit_node(&n);
                    prev_was_type = is_type_node(&n);
                    prev_kind = Some(n.kind());
                }
                SyntaxElement::Token(t) => {
                    match t.kind() {
                        SyntaxKind::WHITESPACE => {}
                        SyntaxKind::EQ => {
                            self.builder.text(" =");
                            self.builder.begin_consistent(continuation);
                            self.builder.hardline();
                            rhs_group_open = true;
                        }
                        SyntaxKind::LINE_COMMENT
                        | SyntaxKind::BLOCK_COMMENT
                        | SyntaxKind::DOC_COMMENT => {
                            self.builder.text(" ");
                            self.builder.literal(t.text());
                        }
                        kind if is_identifier_or_contextual_keyword(kind) && prev_was_type => {
                            self.builder.text(" ");
                            self.builder.text(t.text());
                            prev_was_type = false;
                        }
                        SyntaxKind::FINAL_KW | SyntaxKind::VAR_KW => self.builder.text(t.text()),
                        _ => self.visit_token(&t),
                    }
                    prev_kind = Some(t.kind());
                }
            }
        }

        if rhs_group_open {
            self.builder.end();
        }
    }

    fn extract_simple_string_concat_rhs(node: &SyntaxNode) -> Option<Vec<String>> {
        let mut seen_eq = false;
        let mut expect_literal = true;
        let mut saw_plus = false;
        let mut literals: Vec<String> = Vec::new();

        for elem in node.descendants_with_tokens() {
            let SyntaxElement::Token(t) = elem else {
                continue;
            };

            match t.kind() {
                SyntaxKind::EQ => {
                    seen_eq = true;
                    continue;
                }
                _ if !seen_eq => continue,
                SyntaxKind::WHITESPACE => continue,
                SyntaxKind::SEMICOLON => break,
                SyntaxKind::STRING_LITERAL if expect_literal => {
                    literals.push(t.text().to_string());
                    expect_literal = false;
                }
                SyntaxKind::PLUS if !expect_literal => {
                    saw_plus = true;
                    expect_literal = true;
                }
                _ => return None,
            }
        }

        if saw_plus && !expect_literal && !literals.is_empty() {
            Some(literals)
        } else {
            None
        }
    }

    fn string_literal_content_len(lit: &str) -> usize {
        lit.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .map_or(lit.len(), str::len)
    }

    fn format_long_first_operand_string_concat_assignment(
        &mut self,
        node: &SyntaxNode,
        string_literals: &[String],
    ) {
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        let mut prev_was_type = false;
        let mut prev_kind: Option<SyntaxKind> = None;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    if matches!(prev_kind, Some(SyntaxKind::FINAL_KW | SyntaxKind::VAR_KW)) {
                        self.builder.text(" ");
                    }
                    self.visit_node(&n);
                    prev_was_type = is_type_node(&n);
                    prev_kind = Some(n.kind());
                }
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        continue;
                    } else if matches!(
                        t.kind(),
                        SyntaxKind::LINE_COMMENT
                            | SyntaxKind::BLOCK_COMMENT
                            | SyntaxKind::DOC_COMMENT
                    ) {
                        self.builder.text(" ");
                        self.builder.literal(t.text());
                    } else if is_identifier_or_contextual_keyword(t.kind()) && prev_was_type {
                        self.builder.text(" ");
                        self.builder.text(t.text());
                        prev_was_type = false;
                    } else if t.kind() == SyntaxKind::EQ {
                        // Header line ends with `=`, then RHS starts on the next line.
                        self.builder.text(" =");
                        self.builder.begin_consistent(continuation);
                        self.builder.hardline();

                        // First operand line (continuation indent).
                        self.builder.text(string_literals[0].clone());

                        // Subsequent operands: one per line, double continuation indent.
                        self.builder.begin_consistent(continuation);
                        for lit in &string_literals[1..] {
                            self.builder.hardline();
                            self.builder.text("+ ");
                            self.builder.text(lit.clone());
                        }
                        self.builder.text(";");
                        self.builder.end();
                        self.builder.end();
                        return;
                    } else if t.kind() == SyntaxKind::FINAL_KW || t.kind() == SyntaxKind::VAR_KW {
                        {
                            self.builder.text(t.text());
                        }
                    } else {
                        self.visit_token(&t);
                    }

                    prev_kind = Some(t.kind());
                }
            }
        }
    }

    /// Visit a local variable declaration with a method chain/builder value
    fn visit_local_var_decl_chain(&mut self, node: &SyntaxNode, use_builder_style: bool) {
        // Format: "var x = SomeType.builder()" then break for subsequent calls
        let format_decl = |b: &mut DocumentBuilder| {
            let mut formatter = JavaFormatter::new(b);
            let mut prev_was_type = false;
            let mut prev_kind: Option<SyntaxKind> = None;
            let mut past_eq = false;
            let mut saw_chainable_expr = false; // Track if we've seen a chainable expression (NEW_EXPR or METHOD_CALL_EXPR)

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Node(n) => {
                        if matches!(prev_kind, Some(SyntaxKind::FINAL_KW | SyntaxKind::VAR_KW)) {
                            formatter.builder.text(" ");
                        }
                        if past_eq
                            && n.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                            && n.children_with_tokens().count() == 0
                        {
                            continue;
                        }
                        formatter.visit_node(&n);
                        prev_was_type = is_type_node(&n);
                        prev_kind = Some(n.kind());
                        // Track chainable expressions (NEW_EXPR or METHOD_CALL_EXPR) to know when to break
                        if past_eq
                            && matches!(
                                n.kind(),
                                SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::NEW_EXPR
                            )
                        {
                            saw_chainable_expr = true;
                        }
                    }
                    SyntaxElement::Token(t) => {
                        if t.kind() == SyntaxKind::WHITESPACE {
                            continue;
                        } else if matches!(
                            t.kind(),
                            SyntaxKind::LINE_COMMENT
                                | SyntaxKind::BLOCK_COMMENT
                                | SyntaxKind::DOC_COMMENT
                        ) {
                            formatter.builder.text(" ");
                            formatter.builder.literal(t.text().to_string());
                        } else if is_identifier_or_contextual_keyword(t.kind()) && prev_was_type {
                            formatter.builder.text(" ");
                            formatter.builder.text(t.text());
                            prev_was_type = false;
                        } else if t.kind() == SyntaxKind::EQ {
                            formatter.builder.text(" = ");
                            past_eq = true;
                        } else if t.kind() == SyntaxKind::DOT && past_eq {
                            // Break before dot after any chainable expression
                            // e.g., "new MetricConfig()" followed by ".samples()" should break
                            if saw_chainable_expr {
                                formatter.builder.chain_break();
                            }
                            formatter.builder.text(".");
                        } else if t.kind() == SyntaxKind::FINAL_KW || t.kind() == SyntaxKind::VAR_KW
                        {
                            {
                                formatter.builder.text(t.text());
                            }
                        } else {
                            formatter.visit_token(&t);
                        }
                        prev_kind = Some(t.kind());
                    }
                }
            }
        };

        if use_builder_style {
            self.builder.builder_group(format_decl);
        } else {
            self.builder.chain_group(format_decl);
        }
    }
}
