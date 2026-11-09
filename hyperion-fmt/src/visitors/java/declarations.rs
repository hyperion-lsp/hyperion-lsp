// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Declaration visitors: class, interface, enum, record, annotation, method, constructor, field

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::SyntaxElement;

use super::{is_identifier_or_contextual_keyword, is_type_node, JavaFormatter};

impl JavaFormatter<'_> {
    /// Visit a modifier list (annotations + modifiers like public, static, final)
    /// Check if a modifier list contains annotations (but no regular modifiers after them)
    /// This is used to determine if we need a hardline before the declaration
    pub(super) fn has_trailing_annotation(node: &SyntaxNode) -> bool {
        let mut had_annotation = false;
        let mut had_modifier_after_annotation = false;

        for child in node.children() {
            if child.kind() == SyntaxKind::ANNOTATION {
                had_annotation = true;
                had_modifier_after_annotation = false;
            } else if child.kind() == SyntaxKind::MODIFIER && had_annotation {
                had_modifier_after_annotation = true;
            }
        }

        // Return true if there's an annotation and no modifier after it
        had_annotation && !had_modifier_after_annotation
    }

    /// Decide whether two adjacent class-header elements need a space between them.
    ///
    /// Headers look like `public class Name<T> extends X implements A, B` or
    /// `record Name(int x)` — spacing is structural, never taken from trivia.
    const fn header_needs_space(prev: SyntaxKind, next: SyntaxKind) -> bool {
        !matches!(
            (prev, next),
            // `Name<T>`, `record Name(...)`: no space before the list
            (
                _,
                SyntaxKind::TYPE_PARAM_LIST | SyntaxKind::PARAM_LIST | SyntaxKind::COMMA
            ) | (SyntaxKind::AT, _)
        )
    }

    /// Visit a class declaration
    pub(super) fn visit_class_decl(&mut self, node: &SyntaxNode) {
        // Collect children to separate header from body
        let children: Vec<_> = node.children_with_tokens().collect();

        // Find the L_BRACE position
        let brace_pos = children.iter().position(|c| match c {
            SyntaxElement::Token(t) => t.kind() == SyntaxKind::L_BRACE,
            SyntaxElement::Node(_) => false,
        });

        // Format header (before L_BRACE) with structural spacing: source
        // whitespace is ignored entirely so spacing can never depend on trivia
        // hidden inside child nodes (e.g. `extends BaseThread implements ...`).
        let mut needs_hardline_after_modifiers = false;
        let mut prev: Option<SyntaxKind> = None;
        for (i, child) in children.iter().enumerate() {
            if brace_pos.is_some_and(|bp| i >= bp) {
                break;
            }
            let kind = match child {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        continue;
                    }
                    t.kind()
                }
                SyntaxElement::Node(n) => n.kind(),
            };

            if let Some(p) = prev {
                if needs_hardline_after_modifiers {
                    self.builder.hardline();
                    needs_hardline_after_modifiers = false;
                } else if Self::header_needs_space(p, kind) {
                    self.builder.text(" ");
                }
            }

            match child {
                SyntaxElement::Token(t) => self.builder.text(t.text()),
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::MODIFIER_LIST && Self::has_trailing_annotation(n) {
                        needs_hardline_after_modifiers = true;
                    }
                    self.visit_node(n);
                }
            }
            prev = Some(kind);
        }

        // Format body (from L_BRACE onwards)
        if let Some(brace_idx) = brace_pos {
            // Comment tokens count as content: a comment-only body must not be
            // collapsed (that would delete the comments)
            let has_body_content = children[brace_idx + 1..].iter().any(|c| match c {
                SyntaxElement::Node(_) => true,
                SyntaxElement::Token(t) => matches!(
                    t.kind(),
                    SyntaxKind::DOC_COMMENT | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
                ),
            });

            if !has_body_content {
                // Palantir collapses an empty body onto the header: `class A {}`
                self.builder.text(" {}");
                return;
            }
            self.builder.text(" ");
            self.builder.text("{");

            {
                // Track pending modifier list to pair with following declaration
                let mut pending_modifiers: Option<&SyntaxNode> = None;
                // Blank-line bookkeeping (palantir rules): a member "gets a blank
                // line before" iff it is a non-field or has javadoc. A blank is
                // FORCED between two members when either side qualifies; the
                // first member and comment gaps PRESERVE the source blank; the
                // gap before `}` suppresses it.
                let mut blank_pending = false;
                let mut separator_done = false;
                let mut last_comment_was_doc = false;
                let mut prev_member: Option<bool> = None; // Some(gets_blank_before)

                for child in &children[brace_idx + 1..] {
                    match child {
                        SyntaxElement::Token(t) => {
                            if t.kind() == SyntaxKind::R_BRACE {
                                // Suppress any pending blank before the closing brace
                                blank_pending = false;
                                self.builder.hardline();
                                self.builder.text("}");
                            } else if t.kind() == SyntaxKind::WHITESPACE {
                                // Record source blank lines; emit lazily at the
                                // next member/comment so forced and preserved
                                // blanks go through one place
                                let newline_count = t.text().chars().filter(|&c| c == '\n').count();
                                if newline_count >= 2 {
                                    blank_pending = true;
                                }
                            } else if t.kind() == SyntaxKind::DOC_COMMENT
                                || t.kind() == SyntaxKind::LINE_COMMENT
                                || t.kind() == SyntaxKind::BLOCK_COMMENT
                            {
                                // Comment attaches to the following member: the
                                // separating blank goes before the comment.
                                // Javadoc forces a blank; other comments preserve.
                                let force =
                                    t.kind() == SyntaxKind::DOC_COMMENT && prev_member.is_some();
                                if (force || blank_pending) && !separator_done {
                                    self.builder.hardline();
                                }
                                blank_pending = false;
                                separator_done = true;
                                last_comment_was_doc = t.kind() == SyntaxKind::DOC_COMMENT;
                                self.builder.indent(|b| {
                                    b.hardline();
                                    b.literal(t.text().to_string());
                                });
                            }
                        }
                        SyntaxElement::Node(n) => {
                            if n.kind() == SyntaxKind::MODIFIER_LIST {
                                // Store modifier list to pair with next declaration
                                pending_modifiers = Some(n);
                            } else {
                                // Palantir: only actual javadoc (not line/block
                                // comments) makes a field "get a blank line before"
                                let gets_blank_before = n.kind() != SyntaxKind::FIELD_DECL
                                    || (separator_done && last_comment_was_doc);
                                if !separator_done {
                                    let force = match prev_member {
                                        Some(prev_gets) => gets_blank_before || prev_gets,
                                        // First member: preserve only
                                        None => false,
                                    };
                                    if force || blank_pending {
                                        self.builder.hardline();
                                    }
                                }
                                blank_pending = false;
                                separator_done = false;
                                last_comment_was_doc = false;
                                prev_member = Some(gets_blank_before);

                                // Output member with its modifiers
                                self.builder.indent(|b| {
                                    b.hardline();
                                    let mut formatter = JavaFormatter::new(b);
                                    if let Some(mods) = pending_modifiers.take() {
                                        // Check if annotation ends the modifier list
                                        // (no regular modifiers after it)
                                        let needs_hardline = Self::has_trailing_annotation(mods);
                                        formatter.visit_modifier_list(mods);
                                        if needs_hardline {
                                            // Annotation on its own line
                                            formatter.builder.hardline();
                                        } else {
                                            // Regular modifier - space before declaration
                                            formatter.builder.text(" ");
                                        }
                                    }
                                    formatter.visit_node(n);
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    /// Visit an interface declaration
    pub(super) fn visit_interface_decl(&mut self, node: &SyntaxNode) {
        // Reuse class declaration logic (same structure)
        self.visit_class_decl(node);
    }

    /// Visit an enum declaration
    #[allow(clippy::too_many_lines)]
    pub(super) fn visit_enum_decl(&mut self, node: &SyntaxNode) {
        let children: Vec<_> = node.children_with_tokens().collect();

        // Find the L_BRACE position
        let brace_pos = children.iter().position(|c| match c {
            SyntaxElement::Token(t) => t.kind() == SyntaxKind::L_BRACE,
            SyntaxElement::Node(_) => false,
        });

        // Format header (enum Name)
        for (i, child) in children.iter().enumerate() {
            if brace_pos.is_some_and(|bp| i >= bp) {
                break;
            }
            match child {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Space between tokens - use non-breaking space
                        if brace_pos.is_some_and(|bp| i + 1 != bp) {
                            self.builder.text(" ");
                        }
                    } else {
                        self.visit_token(t);
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(n);
                }
            }
        }

        // Format body
        if let Some(brace_idx) = brace_pos {
            self.builder.text(" {");

            // Track state while iterating through body
            let mut pending_doc_comment: Option<String> = None;
            let mut had_enum_constants = false;
            let mut enum_constant_count = 0;
            let mut current_modifiers: Option<&SyntaxNode> = None;
            let mut after_enum_constants = false;

            // Count enum constants first to know when we're at the last one
            let total_enum_constants = children[brace_idx + 1..]
                .iter()
                .filter(|c| matches!(c, SyntaxElement::Node(n) if n.kind() == SyntaxKind::ENUM_CONSTANT))
                .count();

            // Check if there are members after constants
            let has_members_after_constants = children[brace_idx + 1..].iter().any(|c| {
                matches!(c, SyntaxElement::Node(n) if matches!(n.kind(),
                    SyntaxKind::FIELD_DECL | SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL))
            });

            // Source blank line waiting to be emitted before the next element
            let mut blank_pending = false;
            // Whether any member (non-constant) has been emitted yet
            let mut had_member = false;

            for child in &children[brace_idx + 1..] {
                match child {
                    SyntaxElement::Token(t) => {
                        if t.kind() == SyntaxKind::DOC_COMMENT {
                            // Store doc comment text to output before next element
                            pending_doc_comment = Some(t.text().to_string());
                        } else if t.kind() == SyntaxKind::WHITESPACE {
                            if t.text().chars().filter(|&c| c == '\n').count() >= 2 {
                                blank_pending = true;
                            }
                        } else if t.kind() == SyntaxKind::SEMICOLON && had_enum_constants {
                            // Semicolon after last enum constant. A blank recorded
                            // before it belongs after the semicolon, not before.
                            if has_members_after_constants || total_enum_constants > 0 {
                                self.builder.text(";");
                            }
                        }
                    }
                    SyntaxElement::Node(n) => match n.kind() {
                        SyntaxKind::ENUM_CONSTANT => {
                            // Blank before a constant: preserved from source, or
                            // forced when the constant has javadoc (after the first)
                            if (blank_pending || pending_doc_comment.is_some())
                                && had_enum_constants
                            {
                                self.builder.hardline(); // Blank line (no indent)
                            }
                            blank_pending = false;

                            self.builder.indent(|b| {
                                // Output pending doc comment if any
                                if let Some(doc) = pending_doc_comment.take() {
                                    b.hardline();
                                    b.literal(doc);
                                }
                                b.hardline();
                                let mut formatter = JavaFormatter::new(b);
                                formatter.visit_enum_constant(n);
                            });

                            enum_constant_count += 1;
                            had_enum_constants = true;

                            // Add comma after constant (except last one)
                            if enum_constant_count < total_enum_constants {
                                self.builder.text(",");
                            }
                        }
                        SyntaxKind::MODIFIER_LIST => {
                            current_modifiers = Some(n);
                        }
                        SyntaxKind::FIELD_DECL
                        | SyntaxKind::METHOD_DECL
                        | SyntaxKind::CONSTRUCTOR_DECL => {
                            // Palantir member rule: force a blank before non-field
                            // members (and after the constant section); fields
                            // preserve the source blank
                            let force = n.kind() != SyntaxKind::FIELD_DECL
                                && (had_enum_constants || had_member);
                            let after_constants_gap = had_enum_constants && !after_enum_constants;
                            if force || blank_pending || after_constants_gap {
                                self.builder.hardline();
                            } else if pending_doc_comment.is_some() && had_member {
                                self.builder.hardline();
                            }
                            after_enum_constants |= had_enum_constants;
                            blank_pending = false;
                            had_member = true;

                            self.builder.indent(|b| {
                                // Output pending doc comment if any
                                if let Some(doc) = pending_doc_comment.take() {
                                    b.hardline();
                                    b.literal(doc);
                                }
                                b.hardline();
                                let mut formatter = JavaFormatter::new(b);
                                if let Some(mods) = current_modifiers.take() {
                                    formatter.visit_modifier_list(mods);
                                    formatter.builder.text(" ");
                                }
                                formatter.visit_node(n);
                            });
                        }
                        _ => {}
                    },
                }
            }

            self.builder.hardline();
            self.builder.text("}");
        }
    }

    /// Visit an enum constant like RED("red") or BLUE
    pub(super) fn visit_enum_constant(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if t.kind() != SyntaxKind::WHITESPACE {
                        self.builder.text(t.text());
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a record declaration
    pub(super) fn visit_record_decl(&mut self, node: &SyntaxNode) {
        // Reuse class declaration logic (same structure)
        self.visit_class_decl(node);
    }

    /// Visit an annotation declaration
    pub(super) fn visit_annotation_decl(&mut self, node: &SyntaxNode) {
        // Reuse class declaration logic (same structure)
        self.visit_class_decl(node);
    }

    /// Visit a method declaration
    pub(super) fn visit_method_decl(&mut self, node: &SyntaxNode) {
        // Wrap the method signature (everything except body) in a group so that
        // breakable spaces (like before throws) only break when signature doesn't fit
        let mut prev_was_type = false;
        let mut prev_was_type_params = false;
        let mut in_signature_group = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::TYPE_PARAM_LIST {
                        // Start signature group at type params
                        if !in_signature_group {
                            self.builder.begin_consistent(0);
                            in_signature_group = true;
                        }
                        self.visit_node(&n);
                        prev_was_type_params = true;
                    } else if is_type_node(&n) {
                        // Start signature group at return type if not started
                        if !in_signature_group {
                            self.builder.begin_consistent(0);
                            in_signature_group = true;
                        }
                        if prev_was_type_params {
                            // Space between type params and return type
                            self.builder.text(" ");
                            prev_was_type_params = false;
                        }
                        self.visit_node(&n);
                        prev_was_type = true;
                    } else if n.kind() == SyntaxKind::BLOCK_STMT {
                        // End signature group before body
                        if in_signature_group {
                            self.builder.end();
                            in_signature_group = false;
                        }
                        // Method body gets special handling
                        self.builder.text(" ");
                        self.visit_block(&n);
                    } else if n.kind() == SyntaxKind::THROWS_CLAUSE {
                        // Allow break before throws clause with continuation indent
                        let continuation =
                            i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
                        self.builder.begin_consistent(continuation);
                        self.builder.break_with(" ");
                        self.visit_throws_clause(&n);
                        self.builder.end();
                    } else if n.kind() == SyntaxKind::PARAM_LIST {
                        // Visit param list (it handles wrapping with continuation indent)
                        self.visit_param_list(&n);
                    } else {
                        self.visit_node(&n);
                    }
                }
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace, we control spacing
                    } else if is_identifier_or_contextual_keyword(t.kind()) && prev_was_type {
                        // Space between return type and method name (including contextual keywords)
                        self.builder.text(" ");
                        self.builder.text(t.text());
                        prev_was_type = false;
                    } else if t.kind() == SyntaxKind::THROWS_KW {
                        self.builder.text(" throws ");
                    } else if t.kind() == SyntaxKind::SEMICOLON {
                        // End signature group before semicolon (abstract methods)
                        if in_signature_group {
                            self.builder.end();
                            in_signature_group = false;
                        }
                        self.builder.text(";");
                    } else {
                        self.visit_token(&t);
                    }
                }
            }
        }

        // Close signature group if still open (shouldn't normally happen)
        if in_signature_group {
            self.builder.end();
        }
    }

    /// Visit a throws clause (e.g., throws IOException, SQLException)
    fn visit_throws_clause(&mut self, node: &SyntaxNode) {
        let mut first_exception = true;
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    if !first_exception {
                        self.builder.text(", ");
                    }
                    self.visit_node(&n);
                    first_exception = false;
                }
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace
                    } else if t.kind() == SyntaxKind::THROWS_KW {
                        self.builder.text("throws ");
                    } else if t.kind() == SyntaxKind::COMMA {
                        // Handled by first_exception logic
                    } else {
                        self.visit_token(&t);
                    }
                }
            }
        }
    }

    /// Visit a constructor declaration
    pub(super) fn visit_constructor_decl(&mut self, node: &SyntaxNode) {
        self.visit_method_decl(node); // Same structure
    }

    /// Visit a field declaration
    pub(super) fn visit_field_decl(&mut self, node: &SyntaxNode) {
        // Check if RHS contains structural breaks (lambdas with blocks, anonymous classes)
        let has_structural_breaks = Self::contains_structural_breaks(node);

        if has_structural_breaks {
            // Use structural sizing so lambdas/anonymous classes don't force breaking at `=`
            self.builder.begin_consistent_structural(0);
        }

        let mut prev_was_type = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                    prev_was_type = is_type_node(&n);
                }
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace, we control spacing
                    } else if is_identifier_or_contextual_keyword(t.kind()) && prev_was_type {
                        // Space between type and field name (including contextual keywords)
                        self.builder.text(" ");
                        self.builder.text(t.text());
                        prev_was_type = false;
                    } else if t.kind() == SyntaxKind::EQ {
                        self.builder.text(" = ");
                    } else {
                        self.visit_token(&t);
                    }
                }
            }
        }

        if has_structural_breaks {
            self.builder.end();
        }
    }
}
