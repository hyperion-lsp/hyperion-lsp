// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java syntax tree visitor for formatting
//!
//! This module traverses the Java CST from hyperion-parser and builds
//! a document IR for pretty-printing.

mod chains;
mod declarations;
mod expressions;
mod statements;
mod types;

use hyperion_parser::syntax::{AstNode, ImportDecl, SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::SyntaxElement;

use crate::ir::DocumentBuilder;

/// Java formatter that visits syntax nodes and builds document IR
pub struct JavaFormatter<'a> {
    pub(super) builder: &'a mut DocumentBuilder,
}

/// Whether a Javadoc token is followed by the package declaration. Such a doc comment
/// belongs above the package, so it is written before it.
fn leads_into_package(token: &SyntaxToken) -> bool {
    let mut next = token.next_sibling_or_token();
    while let Some(element) = next {
        if element.kind() == SyntaxKind::PACKAGE_DECL {
            return true;
        }
        if element.kind() != SyntaxKind::WHITESPACE {
            return false;
        }
        next = element.next_sibling_or_token();
    }
    false
}

impl<'a> JavaFormatter<'a> {
    /// Create a new Java formatter
    pub const fn new(builder: &'a mut DocumentBuilder) -> Self {
        Self { builder }
    }

    /// Whether the source had a blank line immediately before this node.
    ///
    /// Walks the token stream backwards from the node's first token through
    /// contiguous whitespace, so it sees blanks regardless of which subtree the
    /// whitespace trivia got attached to (e.g. after a braceless `if` body).
    pub(super) fn source_blank_line_before(node: &SyntaxNode) -> bool {
        let Some(first) = node.first_token() else {
            return false;
        };
        let mut newlines = 0usize;
        let mut tok = first.prev_token();
        while let Some(t) = tok {
            if t.kind() != SyntaxKind::WHITESPACE {
                break;
            }
            newlines += t.text().chars().filter(|&c| c == '\n').count();
            tok = t.prev_token();
        }
        newlines >= 2
    }

    /// Truncate text for logging preview
    fn truncate_text(text: &str, max_len: usize) -> String {
        let text = text.trim();
        if text.len() <= max_len {
            text.replace('\n', "\\n")
        } else {
            // Cut at a character boundary: a byte index inside a multibyte character panics.
            let mut cut = max_len.min(text.len());
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            format!("{}...", text[..cut].replace('\n', "\\n"))
        }
    }

    /// Format a complete syntax tree (usually a SourceFile)
    pub fn format(mut self, root: &SyntaxNode) {
        self.visit_node(root);
    }

    /// Visit a syntax node and its children
    pub(super) fn visit_node(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::SOURCE_FILE => self.visit_source_file(node),
            SyntaxKind::PACKAGE_DECL => self.visit_package_decl(node),
            SyntaxKind::IMPORT_DECL => self.visit_import_decl(node),
            SyntaxKind::CLASS_DECL => self.visit_class_decl(node),
            SyntaxKind::INTERFACE_DECL => self.visit_interface_decl(node),
            SyntaxKind::ENUM_DECL => self.visit_enum_decl(node),
            SyntaxKind::RECORD_DECL => self.visit_record_decl(node),
            SyntaxKind::ANNOTATION_DECL => self.visit_annotation_decl(node),
            SyntaxKind::METHOD_DECL => self.visit_method_decl(node),
            SyntaxKind::CONSTRUCTOR_DECL => self.visit_constructor_decl(node),
            SyntaxKind::FIELD_DECL => self.visit_field_decl(node),
            SyntaxKind::LOCAL_VAR_DECL => self.visit_local_var_decl(node),
            SyntaxKind::BLOCK_STMT => self.visit_block(node),
            SyntaxKind::IF_STMT => self.visit_if_stmt(node),
            SyntaxKind::WHILE_STMT => self.visit_while_stmt(node),
            SyntaxKind::DO_WHILE_STMT => self.visit_do_while_stmt(node),
            SyntaxKind::FOR_STMT => self.visit_for_stmt(node),
            SyntaxKind::FOREACH_STMT => self.visit_foreach_stmt(node),
            SyntaxKind::RETURN_STMT => self.visit_return_stmt(node),
            SyntaxKind::THROW_STMT => self.visit_throw_stmt(node),
            SyntaxKind::TRY_STMT => self.visit_try_stmt(node),
            SyntaxKind::CATCH_CLAUSE => self.visit_catch_clause(node),
            SyntaxKind::TRY_WITH_RESOURCES_STMT => self.visit_try_with_resources_stmt(node),
            SyntaxKind::SWITCH_STMT => self.visit_switch_stmt(node),
            SyntaxKind::SWITCH_EXPR => self.visit_switch_expr(node),
            SyntaxKind::CASE_CLAUSE => self.visit_case_clause(node),
            SyntaxKind::SYNCHRONIZED_STMT => self.visit_synchronized_stmt(node),
            SyntaxKind::BREAK_STMT => self.visit_break_stmt(node),
            SyntaxKind::CONTINUE_STMT => self.visit_continue_stmt(node),
            SyntaxKind::ASSERT_STMT => self.visit_assert_stmt(node),
            SyntaxKind::LABELED_STMT => self.visit_labeled_stmt(node),
            SyntaxKind::YIELD_STMT => self.visit_yield_stmt(node),
            SyntaxKind::EMPTY_STMT => self.visit_empty_stmt(node),
            SyntaxKind::EXPR_STMT => self.visit_expr_stmt(node),
            SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                self.visit_block_comment(node);
            }
            // Expression visitors
            SyntaxKind::METHOD_CALL_EXPR => self.visit_method_call_expr(node),
            SyntaxKind::BINARY_EXPR => self.visit_binary_expr(node),
            SyntaxKind::TERNARY_EXPR => self.visit_ternary_expr(node),
            SyntaxKind::LAMBDA_EXPR => self.visit_lambda_expr(node),
            SyntaxKind::FIELD_ACCESS_EXPR => self.visit_field_access_expr(node),
            SyntaxKind::NEW_EXPR => self.visit_new_expr(node),
            SyntaxKind::ARRAY_INIT_EXPR => self.visit_array_init_expr(node),
            SyntaxKind::PAREN_EXPR => self.visit_paren_expr(node),
            SyntaxKind::UNARY_EXPR => self.visit_unary_expr(node),
            SyntaxKind::ASSIGNMENT_EXPR => self.visit_assignment_expr(node),
            SyntaxKind::CAST_EXPR => self.visit_cast_expr(node),
            SyntaxKind::INSTANCEOF_EXPR | SyntaxKind::PATTERN_INSTANCEOF_EXPR => {
                self.visit_instanceof_expr(node);
            }
            SyntaxKind::ARRAY_ACCESS_EXPR => self.visit_array_access_expr(node),
            SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR => {
                self.visit_method_ref_expr(node);
            }
            SyntaxKind::THIS_EXPR => self.visit_this_expr(node),
            SyntaxKind::SUPER_EXPR => self.visit_super_expr(node),
            SyntaxKind::CLASS_LITERAL_EXPR => self.visit_class_literal_expr(node),
            SyntaxKind::NAME_EXPR | SyntaxKind::LITERAL_EXPR => self.visit_simple_expr(node),
            // Type visitors
            SyntaxKind::WILDCARD_TYPE => self.visit_wildcard_type(node),
            SyntaxKind::INTERSECTION_TYPE => self.visit_intersection_type(node),
            SyntaxKind::UNION_TYPE => self.visit_union_type(node),
            SyntaxKind::ARRAY_TYPE => self.visit_array_type(node),
            SyntaxKind::PRIMITIVE_TYPE => self.visit_primitive_type(node),
            SyntaxKind::CLASS_TYPE => self.visit_class_type(node),
            SyntaxKind::TYPE_ARGS => self.visit_type_args(node),
            SyntaxKind::TYPE_PARAM_LIST => self.visit_type_param_list(node),
            SyntaxKind::TYPE_PARAM => self.visit_type_param(node),
            SyntaxKind::TYPE_BOUND => self.visit_type_bound(node),
            SyntaxKind::MODIFIER_LIST => self.visit_modifier_list(node),
            SyntaxKind::ANNOTATION => self.visit_annotation(node),
            SyntaxKind::ANNOTATION_ARG_LIST => self.visit_annotation_arg_list(node),
            SyntaxKind::ANNOTATION_ARG => self.visit_annotation_arg(node),
            SyntaxKind::ANNOTATION_ARRAY_VALUE => self.visit_annotation_array_value(node),
            SyntaxKind::MODIFIER => self.visit_modifier(node),
            SyntaxKind::ENUM_CONSTANT => self.visit_enum_constant(node),
            SyntaxKind::QUALIFIED_NAME => self.visit_qualified_name(node),
            SyntaxKind::PARAM_LIST => self.visit_param_list(node),
            SyntaxKind::PARAM => self.visit_param(node),
            SyntaxKind::ARG_LIST => self.visit_arg_list(node),
            SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION => {
                self.visit_explicit_constructor_invocation(node);
            }
            _ => {
                // Log unhandled node kinds at debug level for development diagnostics
                // This helps identify missing visitors that may need explicit formatting
                tracing::debug!(
                    kind = ?node.kind(),
                    text_preview = %Self::truncate_text(&node.text().to_string(), 50),
                    "Unhandled node kind, using generic traversal"
                );
                self.visit_children(node);
            }
        }
    }

    /// Visit all children of a node
    pub(super) fn visit_children(&mut self, node: &SyntaxNode) {
        let children: Vec<_> = node.children_with_tokens().collect();
        for (i, child) in children.iter().enumerate() {
            match child {
                SyntaxElement::Node(n) => self.visit_node(n),
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Horizontal-only whitespace at the node's edges is the
                        // parent's responsibility: emitting it here doubles up
                        // with the spacing the parent adds around this node
                        // (e.g. `this.x  = v` from trailing space inside the LHS).
                        let horizontal_only = !t.text().contains('\n');
                        if horizontal_only && (i == 0 || i + 1 == children.len()) {
                            continue;
                        }
                        // Skip whitespace before BINARY_EXPR - the binary expression
                        // will add its own spacing around the operator
                        if let Some(SyntaxElement::Node(next)) = children.get(i + 1) {
                            if next.kind() == SyntaxKind::BINARY_EXPR {
                                continue;
                            }
                        }
                    }
                    self.visit_token(t);
                }
            }
        }
    }

    /// Whether a comment starts on a new source line rather than trailing the
    /// preceding token. Trivia is losslessly retained in the CST, so looking
    /// at the previous token works even when the whitespace is attached to a
    /// different statement subtree.
    pub(super) fn comment_has_source_line_before(token: &SyntaxToken) -> bool {
        token.prev_token().is_some_and(|previous| {
            previous.kind() == SyntaxKind::WHITESPACE && previous.text().contains('\n')
        })
    }

    /// Visit a token
    pub(super) fn visit_token(&mut self, token: &SyntaxToken) {
        match token.kind() {
            SyntaxKind::WHITESPACE => {
                // Handle whitespace - convert to appropriate breaks
                let text = token.text();
                let newline_count = text.chars().filter(|&c| c == '\n').count();
                if newline_count >= 2 {
                    // Preserve blank lines (but limit to 1)
                    // First newline: empty (blank line itself)
                    // Second newline: indented (starts next statement)
                    self.builder.blank_line();
                    self.builder.hardline();
                } else if newline_count == 1 {
                    self.builder.hardline();
                } else if !text.is_empty() {
                    self.builder.space();
                }
            }
            SyntaxKind::LINE_COMMENT => {
                // Line comments don't include the newline, so we need to add it
                self.builder.literal(token.text().to_string());
                self.builder.hardline();
            }
            SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                self.builder.literal(token.text().to_string());
            }
            _ => {
                self.builder.text(token.text());
            }
        }
    }

    /// Visit a source file (root node)
    #[allow(clippy::too_many_lines)]
    fn visit_source_file(&mut self, node: &SyntaxNode) {
        let mut had_package = false;
        let mut had_imports = false;
        let mut had_leading_comment = false;
        // Whether the last leading comment was a line comment, which already ends its line.
        let mut leading_line_comment_ended = false;

        // Collect imports in source order, remembering whether the source had a
        // blank line before each one (to preserve the author's grouping).
        let mut imports: Vec<(SyntaxNode, bool)> = Vec::new();

        // First pass: output leading comments, collect imports, and output package
        for child in node.children_with_tokens() {
            match &child {
                SyntaxElement::Node(n) => match n.kind() {
                    SyntaxKind::PACKAGE_DECL => {
                        if had_leading_comment {
                            if !leading_line_comment_ended {
                                self.builder.hardline();
                            }
                            // Preserve the blank line between a license header and
                            // the package declaration when the source had one
                            let blank_before = n
                                .prev_sibling_or_token()
                                .and_then(|e| e.into_token())
                                .is_some_and(|t| {
                                    t.kind() == SyntaxKind::WHITESPACE
                                        && t.text().chars().filter(|&c| c == '\n').count() >= 2
                                });
                            if blank_before {
                                self.builder.blank_line();
                            }
                        }
                        self.visit_package_decl(n);
                        had_package = true;
                    }
                    SyntaxKind::IMPORT_DECL => {
                        let blank_before = n
                            .prev_sibling_or_token()
                            .and_then(|e| e.into_token())
                            .is_some_and(|t| {
                                t.kind() == SyntaxKind::WHITESPACE
                                    && t.text().chars().filter(|&c| c == '\n').count() >= 2
                            });
                        imports.push((n.clone(), blank_before));
                    }
                    _ => {}
                },
                SyntaxElement::Token(t) => {
                    // Output leading comments (before package declaration)
                    if !had_package
                        && (t.kind() == SyntaxKind::BLOCK_COMMENT
                            || t.kind() == SyntaxKind::LINE_COMMENT
                            || (t.kind() == SyntaxKind::DOC_COMMENT && leads_into_package(t)))
                    {
                        // A line comment has no newline of its own, so the next token
                        // would be commented out with it. Each leading comment ends its line.
                        self.builder.literal(t.text().to_string());
                        leading_line_comment_ended = t.kind() == SyntaxKind::LINE_COMMENT;
                        if leading_line_comment_ended {
                            self.builder.hardline();
                        }
                        had_leading_comment = true;
                    }
                }
            }
        }

        if self.builder.config().sort_imports {
            // Sorted mode: static imports first, then regular, each group sorted
            let sort_key = |n: &SyntaxNode| -> String { n.text().to_string().trim().to_string() };
            let mut static_imports: Vec<SyntaxNode> = Vec::new();
            let mut regular_imports: Vec<SyntaxNode> = Vec::new();
            for (n, _) in &imports {
                if let Some(import) = ImportDecl::cast(n.clone()) {
                    if import.is_static() {
                        static_imports.push(n.clone());
                    } else {
                        regular_imports.push(n.clone());
                    }
                }
            }
            static_imports.sort_by_key(sort_key);
            regular_imports.sort_by_key(sort_key);

            if !static_imports.is_empty() || !regular_imports.is_empty() {
                had_imports = true;
                if had_package {
                    self.builder.hardline();
                }
                for import_node in &static_imports {
                    self.visit_import_decl(import_node);
                }
                // Blank line between static and regular imports
                if !static_imports.is_empty() && !regular_imports.is_empty() {
                    self.builder.hardline();
                }
                for import_node in &regular_imports {
                    self.visit_import_decl(import_node);
                }
            }
        } else if !imports.is_empty() {
            // Preserve mode (palantir behavior): source order, with the source's
            // blank-line grouping collapsed to at most one blank line each.
            had_imports = true;
            if had_package {
                self.builder.hardline();
            }
            for (i, (import_node, blank_before)) in imports.iter().enumerate() {
                if i > 0 && *blank_before {
                    self.builder.blank_line();
                }
                self.visit_import_decl(import_node);
            }
        }

        // Second pass: output type declarations (with their modifiers)
        let mut outputted_blank_after_imports = false;
        let mut seen_package = false;
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => match n.kind() {
                    SyntaxKind::PACKAGE_DECL => {
                        // Already handled in first pass
                        seen_package = true;
                    }
                    SyntaxKind::IMPORT_DECL => {
                        // Already handled
                    }
                    SyntaxKind::MODIFIER_LIST => {
                        // Modifiers/annotations before a type declaration.
                        // Palantir forces a blank line before the first type
                        // declaration after the package/import section.
                        if (had_package || had_imports) && !outputted_blank_after_imports {
                            self.builder.hardline();
                        }
                        self.visit_modifier_list(&n);
                        // Hardline after annotation-only modifier list, space otherwise
                        if Self::has_trailing_annotation(&n) {
                            self.builder.hardline();
                        } else {
                            self.builder.text(" ");
                        }
                        // Mark as done so following type declaration doesn't add blank line
                        outputted_blank_after_imports = true;
                    }
                    SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::RECORD_DECL
                    | SyntaxKind::ANNOTATION_DECL => {
                        if (had_package || had_imports) && !outputted_blank_after_imports {
                            // Blank line after imports - import_decl already adds hardline,
                            // so just one more for blank line
                            self.builder.hardline();
                            outputted_blank_after_imports = true;
                        }
                        self.visit_node(&n);
                    }
                    _ => self.visit_node(&n),
                },
                SyntaxElement::Token(t) => {
                    // Skip leading comments (already output in first pass)
                    if !seen_package
                        && (t.kind() == SyntaxKind::BLOCK_COMMENT
                            || t.kind() == SyntaxKind::LINE_COMMENT
                            || (t.kind() == SyntaxKind::DOC_COMMENT && leads_into_package(&t)))
                    {
                        continue;
                    }

                    if matches!(
                        t.kind(),
                        SyntaxKind::DOC_COMMENT
                            | SyntaxKind::BLOCK_COMMENT
                            | SyntaxKind::LINE_COMMENT
                    ) {
                        // Comment before type declaration - Palantir forces a
                        // blank line here after the package/import section
                        if (had_package || had_imports) && !outputted_blank_after_imports {
                            self.builder.hardline();
                        }
                        self.builder.literal(t.text().to_string());
                        self.builder.hardline();
                        // Mark as handled so CLASS_DECL doesn't add another blank line
                        outputted_blank_after_imports = true;
                    } else if t.kind() != SyntaxKind::WHITESPACE {
                        self.visit_token(&t);
                    }
                }
            }
        }
    }

    /// Visit a modifier list (annotations + modifiers like public, static, final)
    pub(super) fn visit_modifier_list(&mut self, node: &SyntaxNode) {
        let mut first = true;
        let mut last_was_doc_comment = false;
        let mut had_annotation = false;
        let mut first_modifier = true;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::DOC_COMMENT {
                        // Doc comments get their own line
                        if !first {
                            self.builder.hardline();
                        }
                        self.visit_block_comment(&n);
                        last_was_doc_comment = true;
                    } else if n.kind() == SyntaxKind::ANNOTATION {
                        if last_was_doc_comment || !first {
                            self.builder.hardline();
                        }
                        self.visit_annotation(&n);
                        had_annotation = true;
                        last_was_doc_comment = false;
                    } else if n.kind() == SyntaxKind::MODIFIER {
                        // Modifiers like public, static, final
                        if first_modifier && had_annotation {
                            // Newline between annotations and modifiers
                            self.builder.hardline();
                        } else if !first_modifier {
                            // Space between multiple modifiers (non-breaking)
                            self.builder.text(" ");
                        } else if last_was_doc_comment {
                            self.builder.hardline();
                        }
                        self.visit_modifier(&n);
                        first_modifier = false;
                        last_was_doc_comment = false;
                    } else {
                        self.visit_node(&n);
                        last_was_doc_comment = false;
                    }
                    first = false;
                }
                SyntaxElement::Token(t) => {
                    // Skip whitespace in modifier lists - we control spacing
                    if t.kind() != SyntaxKind::WHITESPACE {
                        self.visit_token(&t);
                    }
                }
            }
        }
        // Note: caller is responsible for adding space/newline after modifier list
    }

    /// Visit an annotation like @Override or @SuppressWarnings("unchecked")
    fn visit_annotation(&mut self, node: &SyntaxNode) {
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

    /// Visit an annotation argument list like (name = "value", count = 5)
    fn visit_annotation_arg_list(&mut self, node: &SyntaxNode) {
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        let mut in_group = false;
        let mut first_arg = true;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    match t.kind() {
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        SyntaxKind::L_PAREN => {
                            self.builder.text("(");
                            self.builder.begin_consistent(continuation);
                            self.builder.softline();
                            in_group = true;
                        }
                        SyntaxKind::R_PAREN => {
                            if in_group {
                                self.builder.end();
                                in_group = false;
                            }
                            self.builder.text(")");
                        }
                        SyntaxKind::COMMA => {
                            self.builder.text(",");
                            self.builder.space();
                        }
                        SyntaxKind::LINE_COMMENT => {
                            self.builder.line_comment(t.text().to_string());
                        }
                        SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                            self.builder.literal(t.text().to_string());
                        }
                        _ => {
                            self.builder.text(t.text());
                        }
                    }
                }
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::ANNOTATION_ARG {
                        if !first_arg {
                            // Comma already handled above
                        }
                        self.visit_annotation_arg(&n);
                        first_arg = false;
                    } else {
                        // Single value annotation like @SuppressWarnings("unchecked")
                        self.visit_node(&n);
                    }
                }
            }
        }
    }

    /// Visit an annotation argument like name = "value"
    fn visit_annotation_arg(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    match t.kind() {
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        SyntaxKind::EQ => {
                            self.builder.text(" = ");
                        }
                        SyntaxKind::LINE_COMMENT => {
                            self.builder.line_comment(t.text().to_string());
                        }
                        SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                            self.builder.literal(t.text().to_string());
                        }
                        _ => {
                            self.builder.text(t.text());
                        }
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit an annotation array value like {Type.KRAFT, Type.ZOOKEEPER}
    fn visit_annotation_array_value(&mut self, node: &SyntaxNode) {
        // Use indent_size for array elements, not continuation_indent
        // This matches Palantir style: 4-space indent for array elements
        let indent = i32::try_from(self.builder.config().indent_size).unwrap_or(4);
        let mut in_group = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    match t.kind() {
                        SyntaxKind::WHITESPACE => {
                            // Skip whitespace
                        }
                        SyntaxKind::L_BRACE => {
                            self.builder.text("{");
                            self.builder.begin_inconsistent(indent);
                            self.builder.softline();
                            in_group = true;
                        }
                        SyntaxKind::R_BRACE => {
                            if in_group {
                                self.builder.end();
                                in_group = false;
                            }
                            self.builder.text("}");
                        }
                        SyntaxKind::COMMA => {
                            self.builder.text(",");
                            self.builder.space();
                        }
                        SyntaxKind::LINE_COMMENT => {
                            self.builder.line_comment(t.text().to_string());
                        }
                        SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT => {
                            self.builder.literal(t.text().to_string());
                        }
                        _ => {
                            self.builder.text(t.text());
                        }
                    }
                }
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a modifier (public, private, static, final, etc.)
    fn visit_modifier(&mut self, node: &SyntaxNode) {
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

    /// Visit a qualified name like com.example.Foo
    fn visit_qualified_name(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    // Skip trailing whitespace in qualified names
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

    /// Visit a parameter list (method/constructor parameters)
    fn visit_param_list(&mut self, node: &SyntaxNode) {
        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        let mut in_param_group = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace
                    } else if t.kind() == SyntaxKind::L_PAREN {
                        // Start parameter wrapping group
                        self.builder.text("(");
                        self.builder.begin_consistent(continuation);
                        self.builder.softline();
                        in_param_group = true;
                    } else if t.kind() == SyntaxKind::R_PAREN {
                        // End parameter wrapping group
                        if in_param_group {
                            self.builder.end();
                            in_param_group = false;
                        }
                        self.builder.text(")");
                    } else if t.kind() == SyntaxKind::COMMA {
                        // Comma followed by space (becomes newline when broken)
                        self.builder.text(",");
                        self.builder.space();
                    } else {
                        self.visit_token(&t);
                    }
                }
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::PARAM {
                        self.visit_param(&n);
                    } else {
                        self.visit_node(&n);
                    }
                }
            }
        }
    }

    /// Visit an argument list (method call arguments)
    /// Uses continuation indent when arguments don't fit on one line
    pub(super) fn visit_arg_list(&mut self, node: &SyntaxNode) {
        // Palantir behavior:
        // - Single-argument calls keep the first argument on the same line even if the
        //   argument wraps internally (e.g., a long binary concat).
        // - Multi-argument calls may break after `(` and align arguments using continuation indent.
        let has_comma = node
            .children_with_tokens()
            .any(|c| matches!(c, SyntaxElement::Token(t) if t.kind() == SyntaxKind::COMMA));

        if !has_comma {
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
                    SyntaxElement::Node(n) => self.visit_node(&n),
                }
            }
            return;
        }

        let continuation = i32::try_from(self.builder.config().continuation_indent).unwrap_or(8);
        let mut in_arg_group = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace
                    } else if t.kind() == SyntaxKind::L_PAREN {
                        self.builder.text("(");
                        self.builder.begin_consistent(continuation);
                        self.builder.softline();
                        self.builder.begin_inconsistent(0);
                        in_arg_group = true;
                    } else if t.kind() == SyntaxKind::R_PAREN {
                        if in_arg_group {
                            // The closing parenthesis belongs to the argument
                            // layout level. A following statement semicolon is
                            // emitted by the parent, but must still count when
                            // this level decides whether it fits.
                            self.builder.end();
                            self.builder.text(")");
                            let mut next = t.next_token();
                            while next
                                .as_ref()
                                .is_some_and(|token| token.kind() == SyntaxKind::WHITESPACE)
                            {
                                next = next.and_then(|token| token.next_token());
                            }
                            if next.is_some_and(|token| token.kind() == SyntaxKind::SEMICOLON) {
                                self.builder.reserve_width(1);
                            }
                            self.builder.end();
                            in_arg_group = false;
                        } else {
                            self.builder.text(")");
                        }
                    } else if t.kind() == SyntaxKind::COMMA {
                        self.builder.text(",");
                        self.builder.space();
                    } else {
                        self.visit_token(&t);
                    }
                }
                SyntaxElement::Node(n) => self.visit_node(&n),
            }
        }
    }

    /// Visit an explicit constructor invocation (this() or super() call)
    fn visit_explicit_constructor_invocation(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);
            formatter.visit_children(node);
        });
    }

    /// Visit a single parameter (type + name)
    fn visit_param(&mut self, node: &SyntaxNode) {
        let mut prev_was_type = false;
        let mut prev_was_ellipsis = false;

        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        // Skip whitespace
                    } else if is_identifier_or_contextual_keyword(t.kind())
                        && (prev_was_type || prev_was_ellipsis)
                    {
                        // Space between type/ellipsis and parameter name (including contextual keywords)
                        self.builder.text(" ");
                        self.builder.text(t.text());
                        prev_was_type = false;
                        prev_was_ellipsis = false;
                    } else if t.kind() == SyntaxKind::ELLIPSIS {
                        // Varargs: String... (no trailing space, handled when we see IDENT)
                        self.builder.text("...");
                        prev_was_ellipsis = true;
                        prev_was_type = false;
                    } else {
                        self.visit_token(&t);
                    }
                }
                SyntaxElement::Node(n) => {
                    if is_type_node(&n) {
                        self.visit_node(&n);
                        prev_was_type = true;
                        prev_was_ellipsis = false;
                    } else if n.kind() == SyntaxKind::MODIFIER_LIST {
                        // Handle annotations on parameters (e.g., @NonNull)
                        self.visit_modifier_list(&n);
                        self.builder.text(" ");
                    } else {
                        self.visit_node(&n);
                    }
                }
            }
        }
    }

    /// Visit a package declaration
    fn visit_package_decl(&mut self, node: &SyntaxNode) {
        self.visit_children(node);
        self.builder.hardline();
    }

    /// Visit an import declaration
    fn visit_import_decl(&mut self, node: &SyntaxNode) {
        self.visit_children(node);
        self.builder.hardline();
    }
}

/// Check if a node is a type (used for spacing in declarations)
pub(super) fn is_type_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::CLASS_TYPE
            | SyntaxKind::PRIMITIVE_TYPE
            | SyntaxKind::ARRAY_TYPE
            | SyntaxKind::WILDCARD_TYPE
            | SyntaxKind::TYPE_ARGS
            | SyntaxKind::VAR_TYPE
    )
}

/// Check if a node is a statement (used for spacing before loop/if bodies)
pub(super) fn is_statement_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::BLOCK_STMT
            | SyntaxKind::EXPR_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::DO_WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOREACH_STMT
            | SyntaxKind::SWITCH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::TRY_WITH_RESOURCES_STMT
            | SyntaxKind::SYNCHRONIZED_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::THROW_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::ASSERT_STMT
            | SyntaxKind::EMPTY_STMT
            | SyntaxKind::LABELED_STMT
            | SyntaxKind::YIELD_STMT
    )
}

/// Check if a token kind is an identifier or contextual keyword that can be used as a name
pub(super) const fn is_identifier_or_contextual_keyword(kind: SyntaxKind) -> bool {
    // Use the parser's is_identifier() which includes all contextual keywords
    kind.is_identifier()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    #[test]
    fn truncate_text_does_not_split_a_multibyte_character() {
        // 'é' is two bytes, so a cut at byte 2 would fall inside it.
        let text = "aé".repeat(4);
        let cut = JavaFormatter::truncate_text(&text, 2);
        assert_eq!(cut, "a...");
        assert_eq!(JavaFormatter::truncate_text("short", 10), "short");
    }

    fn parse_source(source: &str) -> SyntaxNode {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green_node)
    }

    // Find the outermost METHOD_CALL_EXPR
    fn find_method_call(node: &SyntaxNode) -> Option<SyntaxNode> {
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            return Some(node.clone());
        }
        for child in node.children() {
            if let Some(found) = find_method_call(&child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn test_analyze_builder_chain() {
        // Need to wrap in class/method context for proper parsing
        let source = r"
class Test {
    void test() {
        var x = SomeType.builder().name(name).value(42).enabled(true).build();
    }
}
";
        let root = parse_source(source);

        eprintln!("AST: {:#?}", root);

        let method_call = find_method_call(&root).expect("should find method call");
        let (is_builder, method_count) = JavaFormatter::analyze_chain(&method_call);

        eprintln!("is_builder: {is_builder}, method_count: {method_count}");

        assert!(is_builder, "should detect builder pattern");
        assert!(
            method_count >= 3,
            "should count at least 3 method calls, got {method_count}"
        );
    }

    #[test]
    fn test_is_method_chain() {
        let source = "class T { void t() { a.b().c(); } }";
        let root = parse_source(source);

        eprintln!("is_chain test AST: {:#?}", root);

        let method_call = find_method_call(&root).expect("should find method call");
        let is_chain = JavaFormatter::is_method_chain(&method_call);

        eprintln!("is_chain: {is_chain}");

        // Note: with current parser, chains are flattened so is_method_chain returns false
        // The chain detection needs to happen at the parent level
    }

    #[test]
    fn test_stream_chain_ast() {
        let source = r"
class T {
    void t() {
        list.stream().filter(x -> x > 0).map(x -> x * 2).collect(Collectors.toList());
    }
}
";
        let root = parse_source(source);
        eprintln!("stream chain AST: {:#?}", root);
    }

    #[test]
    fn test_for_loop_ast() {
        let source = r"class T { void t() { for (int i = 0; i < 10; i++) {} } }";
        let root = parse_source(source);
        eprintln!("For loop AST:\n{:#?}", root);
    }

    #[test]
    fn test_this_call_ast() {
        let source = r"class T { T() { this(a, b, c); } }";
        let root = parse_source(source);
        eprintln!("this() call AST:\n{:#?}", root);
    }
}
