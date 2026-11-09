// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! AST Builder - Convert tree-sitter CST to Groovy AST
//!
//! This module walks tree-sitter parse trees and produces typed AST nodes
//! that can be compiled to bytecode.

use crate::ast::{ClosureExpr, Expression, GStringPart, Parameter, Statement};
use tree_sitter::Node;

/// Builds AST from tree-sitter parse trees
pub struct AstBuilder<'a> {
    source: &'a str,
}

impl<'a> AstBuilder<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source }
    }

    /// Get source text for a node
    fn node_text(&self, node: Node) -> &str {
        &self.source[node.start_byte()..node.end_byte()]
    }

    /// Build statements from a program/script root
    pub fn build_statements(&self, node: Node) -> Vec<Statement> {
        let mut statements = Vec::new();
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        // tree-sitter-groovy cannot parse some Gradle/Groovy DSL patterns, notably "command"
        // calls with dotted arguments:
        //
        //   implementation libs.bundles.logging
        //
        // It splits this into:
        // - juxt_function_call: "implementation libs"
        // - ERROR: "."
        // - expression_statement: "bundles.logging"
        //
        // To keep Hyperion's Gradle bytecode execution working, we stitch this back into a
        // single MethodCall argument: libs.bundles.logging.
        let mut i = 0;
        while i < children.len() {
            let child = children[i];

            if i + 1 < children.len() && child.end_byte() == children[i + 1].start_byte() {
                if let Some(suffix) = self.detached_integer_suffix(children[i + 1]) {
                    if let Some(statement) = self.build_statement(child) {
                        if let Some(statement) =
                            Self::apply_detached_integer_suffix(statement, suffix)
                        {
                            statements.push(statement);
                            i += 2;
                            continue;
                        }
                    }
                }
            }

            // Support both shapes:
            // - program: juxt_function_call, ERROR(.), expression_statement
            // - block: expression_statement(juxt_function_call), ERROR(.), expression_statement
            let call_expr = if child.kind() == "juxt_function_call" {
                self.build_expression(child)
            } else if child.kind() == "expression_statement" {
                child
                    .child(0)
                    .filter(|inner| inner.kind() == "juxt_function_call")
                    .and_then(|inner| self.build_expression(inner))
            } else {
                None
            };

            if let Some(stmt) = self.command_with_parenthesized_argument(&children, i) {
                statements.push(stmt);
                i += 2;
                continue;
            }

            if call_expr.is_some()
                && i + 2 < children.len()
                && children[i + 1].kind() == "ERROR"
                && self.node_text(children[i + 1]).trim() == "."
                && children[i + 2].kind() == "expression_statement"
            {
                if let (Some(call_expr), Some(tail_node)) = (call_expr, children[i + 2].child(0)) {
                    if let Some(tail_expr) = self.build_expression(tail_node) {
                        if let Some(merged) =
                            Self::merge_juxt_call_with_dotted_tail(call_expr, tail_expr)
                        {
                            statements.push(Statement::Expression(merged));
                            i += 3;
                            continue;
                        }
                    }
                }
            }

            if let Some(stmt) = self.build_statement(child) {
                statements.push(stmt);
            }
            i += 1;
        }

        statements
    }

    /// `implementation project(':lib')`: tree-sitter parses the argument list of the inner
    /// call as the statement after the command. The two are joined when that statement
    /// starts on the same line. Returns the joined statement; the caller skips two nodes.
    fn command_with_parenthesized_argument(
        &self,
        children: &[Node],
        index: usize,
    ) -> Option<Statement> {
        let child = *children.get(index)?;
        let call_expr = if child.kind() == "juxt_function_call" {
            self.build_expression(child)
        } else if child.kind() == "expression_statement" {
            child
                .child(0)
                .filter(|inner| inner.kind() == "juxt_function_call")
                .and_then(|inner| self.build_expression(inner))
        } else {
            None
        }?;
        let arguments =
            self.parenthesized_statement_arguments(child, children.get(index + 1).copied())?;
        Self::merge_juxt_call_with_parenthesized_argument(call_expr, arguments)
            .map(Statement::Expression)
    }

    /// The single argument of a parenthesized argument list that starts a statement on the
    /// line `before` ends on.
    fn parenthesized_statement_arguments(
        &self,
        before: Node,
        next: Option<Node>,
    ) -> Option<Vec<Expression>> {
        let next = next?;
        if next.kind() != "expression_statement" {
            return None;
        }
        let paren = next
            .child(0)
            .filter(|node| node.kind() == "parenthesized_expression")?;
        let gap = self.source.get(before.end_byte()..paren.start_byte())?;
        if !gap.trim().is_empty() {
            return None;
        }
        let mut cursor = paren.walk();
        let inner: Vec<Node> = paren.named_children(&mut cursor).collect();
        match inner.as_slice() {
            [single] => self
                .build_expression(*single)
                .map(|argument| vec![argument]),
            _ => None,
        }
    }

    /// Joins `implementation project` with its parenthesized argument into
    /// `implementation(project(...))`. A bare-name argument is the only call it applies to.
    fn merge_juxt_call_with_parenthesized_argument(
        call_expr: Expression,
        arguments: Vec<Expression>,
    ) -> Option<Expression> {
        match call_expr {
            Expression::MethodCall {
                receiver: None,
                method_name,
                arguments: mut outer,
            } if outer.len() == 1 => {
                let name = match outer.remove(0) {
                    Expression::Identifier(name) | Expression::Variable(name) => name,
                    _ => return None,
                };
                Some(Expression::MethodCall {
                    receiver: None,
                    method_name,
                    arguments: vec![Expression::MethodCall {
                        receiver: None,
                        method_name: name,
                        arguments,
                    }],
                })
            }
            _ => None,
        }
    }

    fn detached_integer_suffix(&self, node: Node) -> Option<char> {
        if node.kind() != "expression_statement" {
            return None;
        }
        let expression = node.child(0)?;
        if expression.kind() != "identifier" {
            return None;
        }
        match self.node_text(expression) {
            "I" | "i" => Some('i'),
            "G" | "g" => Some('g'),
            _ => None,
        }
    }

    fn apply_detached_integer_suffix(statement: Statement, suffix: char) -> Option<Statement> {
        fn apply(expression: Expression, suffix: char) -> Option<Expression> {
            let value = match expression {
                Expression::IntLiteral(value) | Expression::LongLiteral(value) => value,
                _ => return None,
            };
            if suffix == 'g' {
                return Some(Expression::Unsupported(
                    "Groovy BigInteger literals are not supported".to_string(),
                ));
            }
            Some(match i32::try_from(value) {
                Ok(value) => Expression::IntLiteral(i64::from(value)),
                Err(_) => Expression::Unsupported(format!(
                    "Groovy integer literal {value}i exceeds the forced 32-bit integer range"
                )),
            })
        }

        match statement {
            Statement::Return(Some(expression)) => {
                apply(expression, suffix).map(|expression| Statement::Return(Some(expression)))
            }
            Statement::Expression(expression) => {
                apply(expression, suffix).map(Statement::Expression)
            }
            Statement::VariableDecl {
                name,
                type_annotation,
                initializer: Some(expression),
            } => apply(expression, suffix).map(|initializer| Statement::VariableDecl {
                name,
                type_annotation,
                initializer: Some(initializer),
            }),
            _ => None,
        }
    }

    fn merge_juxt_call_with_dotted_tail(
        call_expr: Expression,
        tail_expr: Expression,
    ) -> Option<Expression> {
        fn flatten_tail_segments(expr: &Expression) -> Option<Vec<String>> {
            match expr {
                Expression::Identifier(name) | Expression::Variable(name) => {
                    Some(vec![name.clone()])
                }
                Expression::PropertyAccess {
                    object, property, ..
                } => {
                    let mut segs = flatten_tail_segments(object)?;
                    segs.push(property.clone());
                    Some(segs)
                }
                _ => None,
            }
        }

        fn append_segments(base: Expression, segments: Vec<String>) -> Expression {
            let mut current = base;
            for segment in segments {
                current = Expression::PropertyAccess {
                    object: Box::new(current),
                    property: segment,
                    safe_navigation: false,
                };
            }
            current
        }

        match call_expr {
            Expression::MethodCall {
                receiver,
                method_name,
                mut arguments,
            } if receiver.is_none() && arguments.len() == 1 => {
                let base = arguments.remove(0);
                let segments = flatten_tail_segments(&tail_expr)?;
                let combined_arg = append_segments(base, segments);

                Some(Expression::MethodCall {
                    receiver: None,
                    method_name,
                    arguments: vec![combined_arg],
                })
            }
            _ => None,
        }
    }

    /// Build a single statement
    pub fn build_statement(&self, node: Node) -> Option<Statement> {
        match node.kind() {
            "expression_statement" => {
                let expr_node = node.child(0)?;
                if expr_node.kind() == "switch_expression" {
                    return self.build_statement(expr_node);
                }
                let expr = self.build_expression(expr_node)?;
                Some(Statement::Expression(expr))
            }
            "return_statement" => {
                let expression_children: Vec<_> = (0..node.child_count())
                    .filter_map(|index| node.child(index))
                    .filter(|child| {
                        let kind = child.kind();
                        !child.is_missing()
                            && !kind.is_empty()
                            && kind != "return"
                            && kind != "keyword"
                            && kind != ";"
                            && !kind.contains("comment")
                    })
                    .collect();

                if expression_children.len() > 1 {
                    let has_detached_big_number_suffix = expression_children.len() == 2
                        && matches!(
                            expression_children[0].kind(),
                            "integer_literal"
                                | "decimal_integer_literal"
                                | "hex_integer_literal"
                                | "octal_integer_literal"
                                | "binary_integer_literal"
                                | "floating_point_literal"
                                | "decimal_floating_point_literal"
                                | "ERROR"
                        )
                        && matches!(self.node_text(expression_children[1]), "G" | "g")
                        && expression_children[0].end_byte() == expression_children[1].start_byte();
                    let reason = if has_detached_big_number_suffix {
                        let numeric = self.node_text(expression_children[0]);
                        if numeric.contains('.') || numeric.contains('e') || numeric.contains('E') {
                            "Groovy BigDecimal literals are not supported"
                        } else {
                            "Groovy BigInteger literals are not supported"
                        }
                    } else {
                        "tree-sitter could not preserve the complete Groovy return expression"
                    };
                    return Some(Statement::Return(Some(Expression::Unsupported(
                        reason.to_string(),
                    ))));
                }

                // Try "value" field first, then check children
                let expr_node = node
                    .child_by_field_name("value")
                    .or_else(|| expression_children.first().copied());
                let expr = expr_node.and_then(|n| self.build_expression(n));
                Some(Statement::Return(expr))
            }
            "variable_declaration" | "local_variable_declaration" => self.build_variable_decl(node),
            "if_statement" => self.build_if_statement(node),
            "while_statement" => self.build_while_statement(node),
            "for_statement" => self.build_traditional_for_statement(node),
            "enhanced_for_statement" => self.build_enhanced_for_statement(node),
            "try_statement" => self.build_try_statement(node),
            "break_statement" => Some(Statement::Break),
            "continue_statement" => Some(Statement::Continue),
            "assert_statement" => Some(Statement::Unsupported(
                "Groovy assert statements are not supported".to_string(),
            )),
            "do_statement" => Some(Statement::Unsupported(
                "Groovy do-while statements are not supported".to_string(),
            )),
            "labeled_statement" => Some(Statement::Unsupported(
                "Groovy labeled statements are not supported".to_string(),
            )),
            "synchronized_statement" => Some(Statement::Unsupported(
                "Groovy synchronized statements are not supported".to_string(),
            )),
            "try_with_resources_statement" => Some(Statement::Unsupported(
                "Groovy try-with-resources statements are not supported".to_string(),
            )),
            "yield_statement" => Some(Statement::Unsupported(
                "Groovy yield statements are not supported".to_string(),
            )),
            // Lowering rejects switch until it has real branch generation. Keep
            // the construct visible instead of misclassifying it as an expression.
            "switch_expression" => Some(Statement::Switch {
                expression: Expression::NullLiteral,
                cases: Vec::new(),
                default_case: None,
            }),
            "throw_statement" => {
                // throw <expression>;
                // Find the expression child (skip "throw" keyword)
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != "throw" && child.kind() != ";" {
                        if let Some(expr) = self.build_expression(child) {
                            return Some(Statement::Throw(expr));
                        }
                    }
                }
                None
            }
            "function_definition" | "method_definition" => self.build_function_definition(node),
            "class_declaration" => Some(Statement::Unsupported(
                "Groovy class declarations".to_string(),
            )),
            "interface_declaration" => Some(Statement::Unsupported(
                "Groovy interface declarations".to_string(),
            )),
            "enum_declaration" => Some(Statement::Unsupported(
                "Groovy enum declarations".to_string(),
            )),
            "annotation_type_declaration" => Some(Statement::Unsupported(
                "Groovy annotation type declarations".to_string(),
            )),
            "record_declaration" => Some(Statement::Unsupported(
                "Groovy record declarations".to_string(),
            )),
            "module_declaration" => Some(Statement::Unsupported(
                "Groovy module declarations".to_string(),
            )),
            "package_declaration" => Some(Statement::Unsupported(
                "Groovy package declarations are not supported by script bytecode generation"
                    .to_string(),
            )),
            "import_declaration" => Some(Statement::Unsupported(
                "Groovy import declarations are not supported by script bytecode generation"
                    .to_string(),
            )),
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration"
            | "field_declaration"
            | "constant_declaration"
            | "annotation_type_element_declaration" => Some(Statement::Unsupported(format!(
                "unsupported Groovy declaration: {}",
                node.kind()
            ))),
            "block" => {
                let stmts = self.build_statements(node);
                Some(Statement::Block(stmts))
            }
            // Skip comments and whitespace
            "comment" | "line_comment" | "block_comment" => None,

            // ERROR nodes may contain statements that tree-sitter couldn't parse
            // E.g., "return obj?.property" gets parsed as ERROR because of ?.
            "ERROR" => {
                let text = self.node_text(node).trim();
                // The grammar currently wraps these valid loop-control statements
                // in ERROR nodes when they appear in some braced Groovy bodies.
                // Preserve their control-flow meaning instead of dropping them.
                if text == "break" {
                    return Some(Statement::Break);
                }
                if text == "continue" {
                    return Some(Statement::Continue);
                }

                if let Some(expression_text) = text.strip_prefix("return") {
                    if let Some(expr) = self.try_parse_elvis_expression(expression_text.trim()) {
                        return Some(Statement::Return(Some(expr)));
                    }
                }

                // Check if this looks like a return statement
                let mut cursor = node.walk();
                let children: Vec<_> = node.children(&mut cursor).collect();

                if !children.is_empty() && self.node_text(children[0]) == "return" {
                    // This is a return statement that failed to parse
                    // Try to build the expression from the remaining children
                    if let Some(expr) = self.try_parse_safe_navigation(node) {
                        return Some(Statement::Return(Some(expr)));
                    }
                }

                if let Some(expression) = self
                    .try_parse_elvis_expression(text)
                    .or_else(|| self.build_expression(node))
                {
                    return Some(Statement::Expression(expression));
                }

                Some(Statement::Unsupported(
                    "tree-sitter could not parse a Groovy statement".to_string(),
                ))
            }

            _ => {
                // Try to interpret as expression
                self.build_expression(node).map(Statement::Expression)
            }
        }
    }

    /// Build an expression from a node
    pub fn build_expression(&self, node: Node) -> Option<Expression> {
        if matches!(node.kind(), "binary_expression" | "ERROR")
            && Self::contains_unquoted_spread_navigation(self.node_text(node))
        {
            return Some(Expression::Unsupported(
                "Groovy spread navigation (*.member)".to_string(),
            ));
        }

        match node.kind() {
            // Literals
            "integer_literal"
            | "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal" => {
                let text = self.node_text(node);
                self.parse_int_literal(text)
            }
            "floating_point_literal" | "decimal_floating_point_literal" => {
                let text = self.node_text(node);
                self.parse_float_literal(text)
            }
            "string_literal" | "string" | "character_literal" => {
                let text = self.node_text(node);
                let unescaped = self.unescape_string(text);

                // WORKAROUND: tree-sitter-groovy doesn't reliably detect GStrings
                // Post-process string literals to detect ${} interpolation patterns
                if unescaped.contains("${") {
                    self.parse_gstring_from_literal(&unescaped)
                } else {
                    Some(Expression::StringLiteral(unescaped))
                }
            }
            "gstring" | "gstring_literal" => self.build_gstring(node),
            "boolean_literal" | "true" | "false" => {
                let text = self.node_text(node);
                Some(Expression::BooleanLiteral(text == "true"))
            }
            "null_literal" | "null" => Some(Expression::NullLiteral),

            // Identifiers and variables
            "identifier" | "simple_name" => {
                Some(Expression::Identifier(self.node_text(node).to_string()))
            }

            // Transparent grammar wrapper used when reparsing a standalone
            // fragment as an expression.
            "expression_statement" => self.build_expression(node.child(0)?),

            // Binary expressions
            "binary_expression"
            | "additive_expression"
            | "multiplicative_expression"
            | "relational_expression"
            | "equality_expression"
            | "logical_and_expression"
            | "logical_or_expression" => self.build_binary_expression(node),

            // Unary expressions
            "unary_expression" | "prefix_expression" => self.build_unary_expression(node),

            // Method calls
            "method_call_expression"
            | "call_expression"
            | "method_invocation"
            | "juxt_function_call" => self.build_method_call(node),

            // Property access (including safe navigation ?.)
            "member_access_expression"
            | "property_expression"
            | "field_access"
            | "safe_navigation_expression"
            | "safe_member_access"
            | "navigation_expression" => self.build_property_access(node),

            // Index access
            "index_expression" | "subscript_expression" | "array_access" => {
                self.build_index_access(node)
            }

            // Closures
            "closure" | "lambda_expression" | "closure_expression" => self.build_closure(node),

            // Parenthesized
            "parenthesized_expression" => {
                let inner = node.child(1)?; // Skip opening paren
                self.build_expression(inner)
                    .map(|e| Expression::Parenthesized(Box::new(e)))
            }

            "cast_expression" => Some(Expression::Unsupported(
                "Groovy cast expressions are not supported".to_string(),
            )),
            "instanceof_expression" => Some(Expression::Unsupported(
                "Groovy instanceof expressions are not supported".to_string(),
            )),
            "update_expression" => Some(Expression::Unsupported(
                "Groovy increment/decrement expressions are not supported".to_string(),
            )),
            "array_creation_expression" => Some(Expression::Unsupported(
                "Groovy array creation expressions are not supported".to_string(),
            )),
            "class_literal" => Some(Expression::Unsupported(
                "Groovy class literal expressions are not supported".to_string(),
            )),
            "method_reference" => Some(Expression::Unsupported(
                "Groovy method reference expressions are not supported".to_string(),
            )),
            "template_expression" => Some(Expression::Unsupported(
                "Groovy template expressions are not supported".to_string(),
            )),
            "this" => Some(Expression::Unsupported(
                "explicit Groovy `this` expressions are not supported".to_string(),
            )),
            "hex_floating_point_literal" => Some(Expression::Unsupported(
                "Groovy hexadecimal floating-point literals are not supported".to_string(),
            )),

            // Ternary conditional
            "conditional_expression" | "ternary_expression" => {
                self.build_conditional_expression(node)
            }

            // Object creation: new ClassName(args)
            "object_creation_expression" => {
                let mut class_name = String::new();
                let mut arguments = Vec::new();

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "type_identifier" | "scoped_type_identifier" | "identifier"
                            if class_name.is_empty() =>
                        {
                            class_name = self.node_text(child).to_string();
                        }
                        "argument_list" | "arguments" => {
                            let mut arg_cursor = child.walk();
                            for arg_child in child.children(&mut arg_cursor) {
                                if let Some(arg) = self.build_expression(arg_child) {
                                    arguments.push(arg);
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if !class_name.is_empty() {
                    Some(Expression::NewObject {
                        class_name,
                        arguments,
                    })
                } else {
                    None
                }
            }

            // List literal
            "list_literal" | "array_literal" => self.build_list_literal(node),

            // Map literal
            "map_literal" => self.build_map_literal(node),

            // Array initializer - can be a closure in disguise!
            // tree-sitter parses `{ a, b -> expr }` as array_initializer with lambda_expression child
            "array_initializer" => self.build_array_initializer_or_closure(node),

            // Range expressions
            "range_expression" => self.build_range_expression(node),

            // ERROR nodes - may be safe navigation (?.)
            "ERROR" => self.try_parse_safe_navigation(node),

            // Assignment expressions (x = value, x += value, etc.)
            "assignment_expression" | "assignment" => self.build_assignment_expression(node),

            _ => {
                if node.is_named() {
                    Some(Expression::Unsupported(format!(
                        "unsupported Groovy expression node: {}",
                        node.kind()
                    )))
                } else {
                    None
                }
            }
        }
    }

    fn contains_unquoted_spread_navigation(source: &str) -> bool {
        let bytes = source.as_bytes();
        let mut quote = None;
        let mut escaped = false;
        let mut index = 0usize;

        while index + 1 < bytes.len() {
            let byte = bytes[index];
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == delimiter {
                    quote = None;
                }
            } else if matches!(byte, b'\'' | b'"') {
                quote = Some(byte);
            } else if byte == b'*' && bytes[index + 1] == b'.' {
                return true;
            }
            index += 1;
        }

        false
    }

    fn parse_int_literal(&self, text: &str) -> Option<Expression> {
        let clean = text.replace('_', "");
        let suffix = clean
            .chars()
            .last()
            .filter(|suffix| matches!(suffix, 'L' | 'l' | 'I' | 'i' | 'G' | 'g'));
        let numeric = suffix.map_or(clean.as_str(), |_| &clean[..clean.len() - 1]);

        if matches!(suffix, Some('G' | 'g')) {
            return Some(Expression::Unsupported(
                "Groovy BigInteger literals are not supported".to_string(),
            ));
        }

        let (digits, radix, is_decimal) = if numeric.starts_with("0x") || numeric.starts_with("0X")
        {
            (&numeric[2..], 16, false)
        } else if numeric.starts_with("0b") || numeric.starts_with("0B") {
            (&numeric[2..], 2, false)
        } else if numeric.starts_with('0') && numeric.len() > 1 {
            (&numeric[1..], 8, false)
        } else {
            (numeric, 10, true)
        };

        if is_decimal {
            let Ok(value) = numeric.parse::<i64>() else {
                return Some(Expression::Unsupported(format!(
                    "Groovy integer literal {text} is outside the supported signed 64-bit range"
                )));
            };
            if matches!(suffix, Some('I' | 'i')) {
                return match i32::try_from(value) {
                    Ok(value) => Some(Expression::IntLiteral(i64::from(value))),
                    Err(_) => Some(Expression::Unsupported(format!(
                        "Groovy integer literal {text} exceeds the forced 32-bit integer range"
                    ))),
                };
            }
            if matches!(suffix, Some('L' | 'l')) || i32::try_from(value).is_err() {
                Some(Expression::LongLiteral(value))
            } else {
                Some(Expression::IntLiteral(value))
            }
        } else if matches!(suffix, Some('L' | 'l')) {
            match u64::from_str_radix(digits, radix) {
                Ok(bits) => Some(Expression::LongLiteral(i64::from_be_bytes(
                    bits.to_be_bytes(),
                ))),
                Err(_) => Some(Expression::Unsupported(format!(
                    "Groovy integer literal {text} exceeds the 64-bit non-decimal range"
                ))),
            }
        } else {
            match u32::from_str_radix(digits, radix) {
                Ok(bits) => Some(Expression::IntLiteral(i64::from(i32::from_be_bytes(
                    bits.to_be_bytes(),
                )))),
                Err(_) => Some(Expression::Unsupported(format!(
                    "Groovy integer literal {text} exceeds the 32-bit non-decimal range; add an L suffix for a long"
                ))),
            }
        }
    }

    fn parse_float_literal(&self, text: &str) -> Option<Expression> {
        let clean = text.replace('_', "");
        let suffix = clean
            .chars()
            .last()
            .filter(|suffix| matches!(suffix, 'F' | 'f' | 'D' | 'd' | 'G' | 'g'));
        let numeric = suffix.map_or(clean.as_str(), |_| &clean[..clean.len() - 1]);

        if matches!(suffix, Some('G' | 'g')) {
            return Some(Expression::Unsupported(
                "Groovy BigDecimal literals are not supported".to_string(),
            ));
        }

        let value = numeric.parse::<f64>().ok()?;
        let mantissa = numeric
            .trim_start_matches(['+', '-'])
            .split(['e', 'E'])
            .next()
            .unwrap_or(numeric);
        let nonzero_mantissa = mantissa.bytes().any(|byte| matches!(byte, b'1'..=b'9'));
        if !value.is_finite() || (value == 0.0 && nonzero_mantissa) {
            return Some(Expression::Unsupported(format!(
                "Groovy floating-point literal {text} is outside the finite nonzero JVM range"
            )));
        }

        if matches!(suffix, Some('F' | 'f')) {
            let narrowed = value as f32;
            if !narrowed.is_finite() || (value != 0.0 && narrowed == 0.0) {
                return Some(Expression::Unsupported(format!(
                    "Groovy float literal {text} is outside the finite nonzero JVM float range"
                )));
            }
            Some(Expression::FloatLiteral(f64::from(narrowed)))
        } else {
            Some(Expression::DoubleLiteral(value))
        }
    }

    fn unescape_string(&self, text: &str) -> String {
        // Remove quotes
        let inner = if text.starts_with("'''") {
            text.trim_start_matches("'''")
                .trim_end_matches("'''")
                .to_string()
        } else if text.starts_with('"') {
            text.trim_start_matches('"')
                .trim_end_matches('"')
                .to_string()
        } else if text.starts_with('\'') {
            text.trim_start_matches('\'')
                .trim_end_matches('\'')
                .to_string()
        } else {
            text.to_string()
        };

        // Unescape in a single left-to-right pass. Sequential independent
        // `replace` calls are wrong: on input `\\n` (escaped backslash then a
        // literal 'n') a leading `replace("\\n", "\n")` would consume the second
        // backslash + 'n' as a newline. A single scan consumes the escaped
        // backslash first, so `\\n` correctly decodes to backslash + 'n'.
        let mut result = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('t') => result.push('\t'),
                    Some('r') => result.push('\r'),
                    Some('\\') => result.push('\\'),
                    Some('"') => result.push('"'),
                    Some('\'') => result.push('\''),
                    // Unknown escape: preserve both characters verbatim.
                    Some(other) => {
                        result.push('\\');
                        result.push(other);
                    }
                    None => result.push('\\'),
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    fn build_binary_expression(&self, node: Node) -> Option<Expression> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() >= 3 {
            let left = self.build_expression(children[0])?;
            let operator = self.node_text(children[1]).to_string();
            let right = self.build_expression(children[2])?;

            Some(Expression::BinaryOp {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            })
        } else {
            None
        }
    }

    /// Build an assignment expression: left = right, left += right, etc.
    fn build_assignment_expression(&self, node: Node) -> Option<Expression> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() >= 3 {
            let left = self.build_expression(children[0])?;
            let operator = self.node_text(children[1]).to_string();
            let right = self.build_expression(children[2])?;

            // For compound assignments like +=, -=, *=, /=, we need to desugar them
            // x += y => x = x + y
            if operator.len() > 1 && operator.ends_with('=') {
                let arith_op = &operator[..operator.len() - 1]; // Strip the =
                Some(Expression::BinaryOp {
                    operator: "=".to_string(),
                    left: Box::new(left.clone()),
                    right: Box::new(Expression::BinaryOp {
                        operator: arith_op.to_string(),
                        left: Box::new(left),
                        right: Box::new(right),
                    }),
                })
            } else {
                // Simple assignment: x = y
                Some(Expression::BinaryOp {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
        } else {
            None
        }
    }

    fn build_range_expression(&self, node: Node) -> Option<Expression> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() >= 3 {
            let start = self.build_expression(children[0])?;
            let _operator = self.node_text(children[1]).to_string();

            // For exclusive range (1..<10), tree-sitter parses it incorrectly
            // as: 1, .., ERROR(<), 10
            // We need to check if there's an ERROR node with < and skip it
            let mut end_idx = 2;
            let mut exclusive = false;

            if children.len() >= 4 && children[2].kind() == "ERROR" {
                // Check if ERROR contains <
                if self.node_text(children[2]).contains('<') {
                    exclusive = true;
                    end_idx = 3;
                }
            }

            let end = self.build_expression(children[end_idx])?;

            Some(Expression::RangeLiteral {
                start: Box::new(start),
                end: Box::new(end),
                exclusive,
            })
        } else {
            None
        }
    }

    fn build_unary_expression(&self, node: Node) -> Option<Expression> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() >= 2 {
            let operator = self.node_text(children[0]).to_string();
            let operand = self.build_expression(children[1])?;

            Some(Expression::UnaryOp {
                operator,
                operand: Box::new(operand),
            })
        } else {
            None
        }
    }

    fn build_method_call(&self, node: Node) -> Option<Expression> {
        let mut receiver = None;
        let mut method_name = String::new();
        let mut arguments = Vec::new();
        let mut first_identifier: Option<String> = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "simple_name" => {
                    let text = self.node_text(child).to_string();
                    if first_identifier.is_none() {
                        // First identifier - could be receiver or method name
                        first_identifier = Some(text);
                    } else {
                        // Second identifier - this is the method name
                        // So the first identifier was the receiver
                        if let Some(recv_name) = first_identifier.take() {
                            receiver = Some(Box::new(Expression::Identifier(recv_name)));
                        }
                        method_name = text;
                    }
                }
                "argument_list" | "arguments" => {
                    if child.has_error() {
                        return Some(Expression::Unsupported(
                            "tree-sitter could not preserve Groovy method arguments".to_string(),
                        ));
                    }

                    // Groovy named arguments (e.g. `apply plugin: 'java'`) are parsed by
                    // tree-sitter-groovy as `map_item` nodes inside the argument list.
                    // In Groovy, named args are passed as an implicit first-argument map.
                    let mut named_entries: Vec<(String, Expression)> = Vec::new();
                    let mut positional_args: Vec<Expression> = Vec::new();

                    let parse_map_item = |builder: &AstBuilder<'_>,
                                          item: Node|
                     -> Option<(String, Expression)> {
                        let mut key: Option<String> = None;
                        let mut value: Option<Expression> = None;
                        let mut seen_colon = false;

                        let mut item_cursor = item.walk();
                        for item_child in item.children(&mut item_cursor) {
                            if !seen_colon {
                                match item_child.kind() {
                                    "identifier" | "simple_name" => {
                                        key = Some(builder.node_text(item_child).to_string());
                                    }
                                    "string_literal" | "string" | "character_literal" => {
                                        let raw = builder.node_text(item_child);
                                        key = Some(builder.unescape_string(raw));
                                    }
                                    ":" => {
                                        seen_colon = true;
                                    }
                                    _ => {}
                                }
                                if item_child.kind() == ":" || builder.node_text(item_child) == ":"
                                {
                                    seen_colon = true;
                                }
                            } else if value.is_none() {
                                if let Some(expr) = builder.build_expression(item_child) {
                                    value = Some(expr);
                                }
                            }
                        }

                        match (key, value) {
                            (Some(k), Some(v)) => Some((k, v)),
                            _ => None,
                        }
                    };

                    let mut arg_cursor = child.walk();
                    for arg in child.children(&mut arg_cursor) {
                        if arg.kind() == "map_item" {
                            if let Some((k, v)) = parse_map_item(self, arg) {
                                named_entries.push((k, v));
                                continue;
                            }
                        }

                        if let Some(expr) = self.build_expression(arg) {
                            positional_args.push(expr);
                        }
                    }

                    if !named_entries.is_empty() {
                        arguments.push(Expression::MapLiteral(named_entries));
                    }
                    arguments.extend(positional_args);
                }
                "closure" | "block" => {
                    if let Some(expr) = self.build_expression(child) {
                        arguments.push(expr);
                    }
                }
                "." | "?." => {
                    // Separator between receiver and method name, skip
                }
                _ => {
                    // Check if it's a receiver expression (for complex receivers)
                    if let Some(expr) = self.build_expression(child) {
                        if method_name.is_empty() && first_identifier.is_none() {
                            receiver = Some(Box::new(expr));
                        }
                    }
                }
            }
        }

        // If we only saw one identifier, it's the method name (no receiver)
        if method_name.is_empty() {
            if let Some(name) = first_identifier {
                method_name = name;
            }
        }

        if method_name.is_empty() {
            return None;
        }

        Some(Expression::MethodCall {
            receiver,
            method_name,
            arguments,
        })
    }

    fn build_property_access(&self, node: Node) -> Option<Expression> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() >= 3 {
            let object = self.build_expression(children[0])?;
            let operator = self.node_text(children[1]);
            let property = self.node_text(children[2]).to_string();

            Some(Expression::PropertyAccess {
                object: Box::new(object),
                property,
                safe_navigation: operator.contains("?."),
            })
        } else {
            None
        }
    }

    /// Reparse a source fragment as a standalone expression.
    ///
    /// Used when tree-sitter-groovy produces a broken subtree (e.g. `return`
    /// inside a lambda_expression) and we need to extract the expression from
    /// its text representation.
    fn reparse_expression(&self, source: &str) -> Option<Expression> {
        let source = source.trim();
        let bytes = source.as_bytes();
        if bytes.len() >= 2 && matches!(bytes[0], b'\'' | b'"') && bytes.last() == Some(&bytes[0]) {
            let unescaped = self.unescape_string(source);
            return if unescaped.contains("${") {
                self.parse_gstring_from_literal(&unescaped)
            } else {
                Some(Expression::StringLiteral(unescaped))
            };
        }

        let mut parser = crate::tree_sitter_wrapper::GroovyParser::new().ok()?;
        let parsed = parser.parse(source).ok()?;
        let builder = AstBuilder::new(source);
        // The reparsed tree's root is a program node; grab the first child
        // which should be an expression_statement or expression.
        let root = parsed.root();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if let Some(expr) = builder.build_expression(child) {
                return Some(expr);
            }
            // Also try as statement — if it's an expression_statement, unwrap it
            if let Some(Statement::Expression(expr)) = builder.build_statement(child) {
                return Some(expr);
            }
        }

        // Some Groovy expressions (notably list literals) are ambiguous at a
        // standalone script root but parse correctly in an expression-bearing
        // statement. Reparse in return context before giving up.
        let wrapped = format!("return {source}");
        let parsed = parser.parse(&wrapped).ok()?;
        let builder = AstBuilder::new(&wrapped);
        builder
            .build_statements(parsed.root())
            .into_iter()
            .find_map(|statement| match statement {
                Statement::Return(Some(expression)) => Some(expression),
                _ => None,
            })
    }

    /// Recover the Elvis operator, which the current tree-sitter grammar emits
    /// as a single ERROR node. Split only on a top-level `?:`, respecting quoted
    /// strings and nested delimiters, then parse both operands normally.
    fn try_parse_elvis_expression(&self, source: &str) -> Option<Expression> {
        let split = Self::find_top_level_elvis(source)?;
        let primary_source = source[..split].trim();
        let fallback_source = source[split + 2..].trim();
        if primary_source.is_empty() || fallback_source.is_empty() {
            return None;
        }

        let primary = self
            .try_parse_elvis_expression(primary_source)
            .or_else(|| self.reparse_expression(primary_source))?;
        let fallback = self
            .try_parse_elvis_expression(fallback_source)
            .or_else(|| self.reparse_expression(fallback_source))?;
        Some(Expression::ElvisOp {
            primary: Box::new(primary),
            fallback: Box::new(fallback),
        })
    }

    fn find_top_level_elvis(source: &str) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut quote = None;
        let mut escaped = false;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut index = 0usize;

        while index + 1 < bytes.len() {
            let byte = bytes[index];
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == delimiter {
                    quote = None;
                }
                index += 1;
                continue;
            }

            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'?' if bytes[index + 1] == b':'
                    && paren_depth == 0
                    && bracket_depth == 0
                    && brace_depth == 0 =>
                {
                    return Some(index);
                }
                _ => {}
            }
            index += 1;
        }
        None
    }

    fn try_parse_safe_navigation(&self, node: Node) -> Option<Expression> {
        // Tree-sitter-groovy doesn't support safe navigation (?.)
        // It parses "obj?.property" as ERROR with children:
        // - identifier "obj"
        // - ? "?"
        // - ERROR(".")
        // - identifier "property"
        //
        // OR in a return statement:
        // - return "return"
        // - identifier "obj"
        // - ? "?"
        // - ERROR(".")
        // - identifier "property"

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        // Skip "return" keyword if present
        let start_idx = if !children.is_empty() && self.node_text(children[0]) == "return" {
            1
        } else {
            0
        };

        // Look for pattern: identifier, ?, ERROR(.), identifier
        if children.len() >= start_idx + 4 {
            let mut i = start_idx;
            let mut current_obj = self.build_expression(children[i])?;

            // Build chain of safe navigation
            i += 1; // Move to first ?
            while i < children.len() && self.node_text(children[i]) == "?" {
                // Pattern: ?, ERROR(.) or ., identifier
                if i + 3 <= children.len() {
                    let dot_node = children[i + 1];
                    if (dot_node.kind() == "ERROR" || self.node_text(dot_node) == ".")
                        && children[i + 2].kind() == "identifier"
                    {
                        let property = self.node_text(children[i + 2]).to_string();

                        current_obj = Expression::PropertyAccess {
                            object: Box::new(current_obj),
                            property,
                            safe_navigation: true,
                        };

                        // Move to next potential ?
                        i += 3;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            return Some(current_obj);
        }

        // Fallback: try to parse children
        for child in children {
            if let Some(expr) = self.build_expression(child) {
                return Some(expr);
            }
        }

        None
    }

    fn build_index_access(&self, node: Node) -> Option<Expression> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        if children.len() >= 2 {
            let object = self.build_expression(children[0])?;
            // Find the index expression (skip brackets)
            for child in &children[1..] {
                if child.kind() != "[" && child.kind() != "]" {
                    if let Some(index) = self.build_expression(*child) {
                        return Some(Expression::IndexAccess {
                            object: Box::new(object),
                            index: Box::new(index),
                        });
                    }
                }
            }
        }
        None
    }

    /// Handle array_initializer which might be a closure in disguise
    ///
    /// tree-sitter parses `{ a, b -> expr }` as:
    /// ```text
    /// array_initializer
    ///   identifier("a")
    ///   lambda_expression("b -> expr")
    /// ```
    /// We need to detect this pattern and build a proper closure.
    fn build_array_initializer_or_closure(&self, node: Node) -> Option<Expression> {
        // Check if this contains a lambda_expression or arrow (->)
        let mut has_lambda = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "lambda_expression" || child.kind() == "->" {
                has_lambda = true;
                break;
            }
        }

        if has_lambda {
            // This is actually a closure: { params -> body }
            // Collect all identifiers before the lambda as parameters
            let mut parameters = Vec::new();
            let mut body = Vec::new();

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "{" | "}" | "," => {
                        // Skip braces and commas
                    }
                    "identifier" => {
                        // Parameter before the arrow
                        parameters.push(self.node_text(child).to_string());
                    }
                    "lambda_expression" => {
                        // The lambda_expression contains: identifier, ->, expression
                        // The identifier is the last parameter, the expression is the body
                        let mut lambda_cursor = child.walk();
                        for lambda_child in child.children(&mut lambda_cursor) {
                            match lambda_child.kind() {
                                "identifier" => {
                                    parameters.push(self.node_text(lambda_child).to_string());
                                }
                                "->" => {
                                    // Skip arrow
                                }
                                _ => {
                                    // Body may be an expression or a statement (e.g. return)
                                    if let Some(stmt) = self.build_statement(lambda_child) {
                                        body.push(stmt);
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        // Any other expression goes into body
                        if let Some(expr) = self.build_expression(child) {
                            body.push(Statement::Expression(expr));
                        }
                    }
                }
            }

            Some(Expression::Closure(Box::new(ClosureExpr {
                parameters,
                body,
                implicit_it: false,
            })))
        } else {
            // Regular array initializer - treat as list literal
            self.build_list_literal(node)
        }
    }

    fn build_closure(&self, node: Node) -> Option<Expression> {
        let mut parameters = Vec::new();
        let mut body = Vec::new();
        let mut implicit_it = true;

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        // tree-sitter-groovy MISPARSES a multi-parameter closure
        // `{ a, b -> … }`: the leading params land in a `local_variable_declaration`
        // (with a `MISSING ";"`) and the following `lambda_expression` has NO
        // parameters. Recover the real parameters from that declaration's
        // declarators. Bare leading identifiers (other groovy shapes) are also
        // params. Both only when an arrow is present (`{ x }` is implicit-`it`).
        let has_arrow = children
            .iter()
            .any(|c| matches!(c.kind(), "lambda_expression" | "->"));

        let mut i = 0;
        while i < children.len() {
            let child = children[i];
            match child.kind() {
                "local_variable_declaration" if has_arrow => {
                    // Misparsed leading parameters `a, b` before the arrow.
                    implicit_it = false;
                    let mut dc = child.walk();
                    for d in child.children(&mut dc) {
                        if d.kind() == "variable_declarator" {
                            let mut nc = d.walk();
                            for n in d.children(&mut nc) {
                                if n.kind() == "identifier" {
                                    parameters.push(self.node_text(n).to_string());
                                }
                            }
                        }
                    }
                }
                "identifier" if has_arrow => {
                    // Leading parameter before the arrow.
                    implicit_it = false;
                    parameters.push(self.node_text(child).to_string());
                }
                "," => {}
                "closure_parameters" | "parameter_list" | "parameters" => {
                    implicit_it = false;
                    let mut param_cursor = child.walk();
                    for param in child.children(&mut param_cursor) {
                        if param.kind() == "identifier" || param.kind() == "parameter" {
                            parameters.push(self.node_text(param).to_string());
                        }
                    }
                }
                "lambda_expression" => {
                    // Handle { x -> expr } syntax where tree-sitter creates a lambda_expression
                    // Structure: lambda_expression { identifier, ->, expression }
                    implicit_it = false;
                    let mut lambda_cursor = child.walk();
                    let mut past_arrow = false;
                    for lambda_child in child.children(&mut lambda_cursor) {
                        match lambda_child.kind() {
                            "identifier" if !past_arrow => {
                                // Identifiers before the arrow are parameters. An
                                // identifier after it may be the entire body (`{ x -> x }`).
                                parameters.push(self.node_text(lambda_child).to_string());
                            }
                            "->" => {
                                past_arrow = true;
                            }
                            _ => {
                                // Body may be an expression or a statement (e.g. return).
                                // tree-sitter-groovy may parse `return expr` inside a
                                // lambda_expression as a single binary_expression whose
                                // text starts with "return".  Detect that and extract
                                // the return value.
                                let text = self.node_text(lambda_child);
                                if let Some(stripped) = text.strip_prefix("return") {
                                    // tree-sitter-groovy doesn't recognize `return` inside
                                    // a lambda_expression — it parses `return x + 1` as a
                                    // broken binary_expression with children:
                                    //   [identifier "return", ERROR "x", "+" "+", literal "1"]
                                    // The subtree is unusable, so we reparse the remainder
                                    // text as a standalone expression.
                                    let remainder = stripped.trim();
                                    let expr = if remainder.is_empty() {
                                        None
                                    } else {
                                        self.reparse_expression(remainder)
                                    };
                                    body.push(Statement::Return(expr));
                                } else if let Some(stmt) = self.build_statement(lambda_child) {
                                    body.push(stmt);
                                }
                            }
                        }
                    }
                }
                "block" | "closure_body" => {
                    body = self.build_statements(child);
                }
                "{" | "}" => {
                    // Skip braces
                }
                _ => {
                    // Same tree-sitter-groovy workaround as build_statements(), but for closures
                    // that do not wrap their body in a "block"/"closure_body" node.
                    if child.kind() == "juxt_function_call"
                        && i + 2 < children.len()
                        && children[i + 1].kind() == "ERROR"
                        && self.node_text(children[i + 1]).trim() == "."
                        && children[i + 2].kind() == "expression_statement"
                    {
                        if let (Some(call_expr), Some(tail_node)) =
                            (self.build_expression(child), children[i + 2].child(0))
                        {
                            if let Some(tail_expr) = self.build_expression(tail_node) {
                                if let Some(merged) =
                                    Self::merge_juxt_call_with_dotted_tail(call_expr, tail_expr)
                                {
                                    body.push(Statement::Expression(merged));
                                    i += 3;
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(stmt) = self.command_with_parenthesized_argument(&children, i) {
                        body.push(stmt);
                        i += 2;
                        continue;
                    }

                    if let Some(stmt) = self.build_statement(child) {
                        body.push(stmt);
                    }
                }
            }

            i += 1;
        }

        Some(Expression::Closure(Box::new(ClosureExpr {
            parameters,
            body,
            implicit_it,
        })))
    }

    fn build_gstring(&self, node: Node) -> Option<Expression> {
        let mut parts = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "string_content" | "gstring_content" | "gstring_path" => {
                    let text = self.node_text(child).to_string();
                    if !text.is_empty() {
                        parts.push(GStringPart::Text(text));
                    }
                }
                "interpolation" | "gstring_value" => {
                    let mut interp_cursor = child.walk();
                    for interp_child in child.children(&mut interp_cursor) {
                        if let Some(expr) = self.build_expression(interp_child) {
                            parts.push(GStringPart::Interpolation(Box::new(expr)));
                            break;
                        }
                    }
                }
                "identifier" => {
                    // Simple $var interpolation
                    let var_name = self.node_text(child).to_string();
                    parts.push(GStringPart::Interpolation(Box::new(
                        Expression::Identifier(var_name),
                    )));
                }
                _ => {}
            }
        }

        Some(Expression::GString(parts))
    }

    /// Parse a string literal containing ${} interpolations into a GString
    /// WORKAROUND for tree-sitter-groovy not detecting GStrings
    fn parse_gstring_from_literal(&self, text: &str) -> Option<Expression> {
        let mut parts = Vec::new();
        let mut current_text = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' && chars.peek() == Some(&'{') {
                // Found interpolation start
                chars.next(); // consume '{'

                // Add accumulated text as a part
                if !current_text.is_empty() {
                    parts.push(GStringPart::Text(current_text.clone()));
                    current_text.clear();
                }

                // Extract interpolation expression until '}'
                let mut expr_text = String::new();
                let mut brace_depth = 1;
                let mut quote = None;
                let mut escaped = false;
                for ch in chars.by_ref() {
                    if let Some(active_quote) = quote {
                        expr_text.push(ch);
                        if escaped {
                            escaped = false;
                        } else if ch == '\\' {
                            escaped = true;
                        } else if ch == active_quote {
                            quote = None;
                        }
                    } else if matches!(ch, '\'' | '"') {
                        quote = Some(ch);
                        expr_text.push(ch);
                    } else if ch == '{' {
                        brace_depth += 1;
                        expr_text.push(ch);
                    } else if ch == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            break;
                        }
                        expr_text.push(ch);
                    } else {
                        expr_text.push(ch);
                    }
                }

                let expr_text = expr_text.trim();
                let expr = if brace_depth != 0 {
                    Expression::Unsupported("unterminated Groovy GString interpolation".to_string())
                } else if expr_text.is_empty() {
                    Expression::Unsupported("empty Groovy GString interpolation".to_string())
                } else {
                    self.reparse_expression(expr_text).unwrap_or_else(|| {
                        Expression::Unsupported(
                            "unsupported Groovy GString interpolation".to_string(),
                        )
                    })
                };

                parts.push(GStringPart::Interpolation(Box::new(expr)));
            } else {
                current_text.push(ch);
            }
        }

        // Add remaining text
        if !current_text.is_empty() {
            parts.push(GStringPart::Text(current_text));
        }

        Some(Expression::GString(parts))
    }

    fn build_conditional_expression(&self, node: Node) -> Option<Expression> {
        if let Some(elvis) = self.try_parse_elvis_expression(self.node_text(node)) {
            return Some(elvis);
        }

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        // condition ? then : else
        if children.len() >= 5 {
            let condition = self.build_expression(children[0])?;
            let then_expr = self.build_expression(children[2])?;
            let else_expr = self.build_expression(children[4])?;

            Some(Expression::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            None
        }
    }

    fn build_list_literal(&self, node: Node) -> Option<Expression> {
        // tree-sitter-groovy reports the empty-map colon as an error even
        // though this exact source is unambiguous.
        if self.node_text(node).trim() == "[:]" {
            return Some(Expression::MapLiteral(Vec::new()));
        }
        if node.has_error() {
            return Some(Expression::Unsupported(
                "tree-sitter could not preserve a Groovy list or map literal".to_string(),
            ));
        }

        let mut elements = Vec::new();
        let mut cursor = node.walk();
        let mut has_colon = false;

        for child in node.children(&mut cursor) {
            // Check if this is an empty map literal [:]
            if child.kind() == ":" {
                has_colon = true;
            }

            // Also check if ERROR node contains : (tree-sitter parses [:] with ERROR containing :)
            if child.kind() == "ERROR" {
                let mut err_cursor = child.walk();
                for err_child in child.children(&mut err_cursor) {
                    if err_child.kind() == ":" {
                        has_colon = true;
                        break;
                    }
                }
            }

            if child.kind() != "["
                && child.kind() != "]"
                && child.kind() != ","
                && child.kind() != ":"
                && child.kind() != "ERROR"
            {
                if let Some(expr) = self.build_expression(child) {
                    elements.push(expr);
                }
            }
        }

        // If we found a colon and no elements, this is an empty map [:]
        if has_colon && elements.is_empty() {
            return Some(Expression::MapLiteral(vec![]));
        }

        Some(Expression::ListLiteral(elements))
    }

    fn build_map_literal(&self, node: Node) -> Option<Expression> {
        if node.has_error() {
            return Some(Expression::Unsupported(
                "tree-sitter could not preserve a Groovy map literal".to_string(),
            ));
        }

        let mut entries = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "map_entry" || child.kind() == "map_item" {
                let mut entry_cursor = child.walk();
                let entry_children: Vec<_> = child.children(&mut entry_cursor).collect();

                if entry_children.len() >= 3 {
                    let key = self.node_text(entry_children[0]).to_string();
                    if let Some(value) = self.build_expression(entry_children[2]) {
                        entries.push((key, value));
                    }
                }
            }
        }

        Some(Expression::MapLiteral(entries))
    }

    fn build_variable_decl(&self, node: Node) -> Option<Statement> {
        let mut count_cursor = node.walk();
        let declarator_count = node
            .children(&mut count_cursor)
            .filter(|child| child.kind() == "variable_declarator")
            .count();
        if declarator_count > 1 {
            return Some(Statement::Unsupported(
                "Groovy declarations with multiple variables are not supported".to_string(),
            ));
        }

        let mut name = String::new();
        let mut type_annotation = None;
        let mut initializer = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type"
                | "type_identifier"
                | "integral_type"
                | "floating_point_type"
                | "boolean_type"
                | "void_type"
                | "generic_type"
                | "scoped_type_identifier"
                | "array_type" => {
                    type_annotation = Some(self.node_text(child).to_string());
                }
                "identifier" => {
                    // Direct identifier (simple case)
                    name = self.node_text(child).to_string();
                }
                "variable_declarator" => {
                    // Variable declarator contains: identifier, =, initializer
                    // Walk into it to extract parts separately
                    let mut decl_cursor = child.walk();
                    for decl_child in child.children(&mut decl_cursor) {
                        match decl_child.kind() {
                            "identifier" => {
                                name = self.node_text(decl_child).to_string();
                            }
                            "=" => {} // Skip assignment operator
                            _ => {
                                // Anything else is the initializer expression
                                if initializer.is_none() {
                                    initializer = self.build_expression(decl_child);
                                }
                            }
                        }
                    }
                }
                "=" => {}
                _ => {
                    if initializer.is_none() {
                        initializer = self.build_expression(child);
                    }
                }
            }
        }

        if name.is_empty() {
            return None;
        }

        Some(Statement::VariableDecl {
            name,
            type_annotation,
            initializer,
        })
    }

    fn build_control_body(&self, node: Node) -> Option<Vec<Statement>> {
        match node.kind() {
            "block" | "closure_body" => Some(self.build_statements(node)),
            // tree-sitter-groovy commonly represents a braced statement body as
            // a closure, either directly or wrapped in an expression statement.
            // It is syntax here, not a closure value: retain its statements.
            "closure" => match self.build_closure(node)? {
                Expression::Closure(closure) => Some(closure.body),
                _ => None,
            },
            "expression_statement" if node.child(0)?.kind() == "closure" => {
                self.build_control_body(node.child(0)?)
            }
            _ => self.build_statement(node).map(|statement| vec![statement]),
        }
    }

    fn build_if_statement(&self, node: Node) -> Option<Statement> {
        let mut condition = None;
        let mut then_body = Vec::new();
        let mut else_body = None;
        let mut parsing_else = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "parenthesized_expression" | "condition" => {
                    let inner = child.child(1)?; // Skip opening paren
                    condition = self.build_expression(inner);
                }
                "if" => {}
                "else" => parsing_else = true,
                "else_clause" => {
                    let mut else_cursor = child.walk();
                    for else_child in child.children(&mut else_cursor) {
                        if let Some(body) = self.build_control_body(else_child) {
                            else_body = Some(body);
                            break;
                        }
                    }
                }
                _ if condition.is_some() => {
                    let Some(body) = self.build_control_body(child) else {
                        continue;
                    };
                    if parsing_else {
                        else_body = Some(body);
                        parsing_else = false;
                    } else if then_body.is_empty() {
                        then_body = body;
                    }
                }
                _ => {}
            }
        }

        Some(Statement::If {
            condition: condition?,
            then_body,
            else_body,
        })
    }

    fn build_while_statement(&self, node: Node) -> Option<Statement> {
        let mut condition = None;
        let mut body = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "parenthesized_expression" | "condition" => {
                    let inner = child.child(1)?;
                    condition = self.build_expression(inner);
                }
                "while" => {}
                _ if condition.is_some() => {
                    if let Some(parsed_body) = self.build_control_body(child) {
                        body = parsed_body;
                    }
                }
                _ => {}
            }
        }

        Some(Statement::While {
            condition: condition?,
            body,
        })
    }

    fn build_enhanced_for_statement(&self, node: Node) -> Option<Statement> {
        let variable = node
            .child_by_field_name("name")
            .map(|name| self.node_text(name).to_string())?;
        let iterable = self.build_expression(node.child_by_field_name("value")?)?;
        let body = self.build_control_body(node.child_by_field_name("body")?)?;

        Some(Statement::For {
            variable,
            iterable,
            body,
        })
    }

    fn build_traditional_for_statement(&self, node: Node) -> Option<Statement> {
        let mut init_cursor = node.walk();
        let init_nodes: Vec<_> = node
            .children_by_field_name("init", &mut init_cursor)
            .collect();
        let mut update_cursor = node.walk();
        let update_nodes: Vec<_> = node
            .children_by_field_name("update", &mut update_cursor)
            .collect();

        // The current AST intentionally models one initializer and one update.
        // Preserve larger Java-style lists as unsupported instead of dropping
        // all but the first expression.
        if init_nodes.len() > 1 || update_nodes.len() > 1 {
            return Some(Statement::Unsupported(
                "Groovy for loops with multiple initializers or updates are not supported"
                    .to_string(),
            ));
        }

        let init = init_nodes.first().and_then(|init_node| {
            self.build_statement(*init_node)
                .or_else(|| self.build_expression(*init_node).map(Statement::Expression))
        });
        let condition = node
            .child_by_field_name("condition")
            .and_then(|condition| self.build_expression(condition));
        let increment = update_nodes
            .first()
            .and_then(|update| self.build_expression(*update));
        let body = self.build_control_body(node.child_by_field_name("body")?)?;

        Some(Statement::TraditionalFor {
            init: init.map(Box::new),
            condition,
            increment,
            body,
        })
    }

    fn build_try_statement(&self, node: Node) -> Option<Statement> {
        let mut try_body = Vec::new();
        let mut catch_clauses = Vec::new();
        let mut finally_body = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "block" if try_body.is_empty() => {
                    try_body = self.build_statements(child);
                }
                "catch_clause" => {
                    // Extract exception type and variable
                    let mut exc_type = "Exception".to_string();
                    let mut exc_var = "e".to_string();
                    let mut catch_body = Vec::new();

                    let mut catch_cursor = child.walk();
                    for catch_child in child.children(&mut catch_cursor) {
                        match catch_child.kind() {
                            "type"
                            | "type_identifier"
                            | "scoped_type_identifier"
                            | "catch_type" => {
                                exc_type = self.node_text(catch_child).to_string();
                            }
                            "identifier" => {
                                exc_var = self.node_text(catch_child).to_string();
                            }
                            "catch_formal_parameter" | "formal_parameter" => {
                                let mut parameter_cursor = catch_child.walk();
                                for parameter_child in catch_child.children(&mut parameter_cursor) {
                                    match parameter_child.kind() {
                                        "type"
                                        | "type_identifier"
                                        | "scoped_type_identifier"
                                        | "catch_type" => {
                                            exc_type = self.node_text(parameter_child).to_string();
                                        }
                                        "identifier" => {
                                            exc_var = self.node_text(parameter_child).to_string();
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            "block" => {
                                catch_body = self.build_statements(catch_child);
                            }
                            _ => {}
                        }
                    }

                    catch_clauses.push(crate::ast::CatchBlock {
                        exception_type: exc_type,
                        variable_name: exc_var,
                        body: catch_body,
                    });
                }
                "finally_clause" => {
                    let mut finally_cursor = child.walk();
                    for finally_child in child.children(&mut finally_cursor) {
                        if finally_child.kind() == "block" {
                            finally_body = Some(self.build_statements(finally_child));
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        Some(Statement::Try {
            body: try_body,
            catch_blocks: catch_clauses,
            finally_block: finally_body,
        })
    }

    /// Build a function/method definition
    ///
    /// tree-sitter parses `def add(a, b) { body }` as:
    /// ```text
    /// function_definition
    ///   def
    ///   identifier("add")
    ///   formal_parameters("(a, b)")
    ///   closure("{ body }")  -- or block
    /// ```
    fn build_function_definition(&self, node: Node) -> Option<Statement> {
        let mut name = String::new();
        let mut parameters = Vec::new();
        let mut body = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "def" => {
                    // Skip keyword
                }
                "identifier" if name.is_empty() => {
                    name = self.node_text(child).to_string();
                }
                "formal_parameters" | "parameters" | "parameter_list" => {
                    // Parse parameters
                    // tree-sitter sometimes parses "a, b" as a single formal_parameter
                    // with type_identifier, ERROR, identifier - need to collect all names
                    let mut param_cursor = child.walk();
                    for param_child in child.children(&mut param_cursor) {
                        match param_child.kind() {
                            "formal_parameter" | "parameter" => {
                                // formal_parameter can contain type and name, but tree-sitter
                                // sometimes puts multiple params in one formal_parameter
                                let mut inner_cursor = param_child.walk();
                                for inner in param_child.children(&mut inner_cursor) {
                                    match inner.kind() {
                                        "identifier" => {
                                            // Each identifier is a parameter
                                            parameters.push(Parameter {
                                                name: self.node_text(inner).to_string(),
                                                type_annotation: None,
                                                default_value: None,
                                            });
                                        }
                                        "type_identifier" => {
                                            // In Groovy `def add(a, b)`, tree-sitter may parse
                                            // "a" as type_identifier - treat as parameter name
                                            parameters.push(Parameter {
                                                name: self.node_text(inner).to_string(),
                                                type_annotation: None,
                                                default_value: None,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            "identifier" => {
                                // Simple identifier parameter
                                parameters.push(Parameter {
                                    name: self.node_text(param_child).to_string(),
                                    type_annotation: None,
                                    default_value: None,
                                });
                            }
                            "(" | ")" | "," => {
                                // Skip delimiters
                            }
                            _ => {}
                        }
                    }
                }
                "closure" | "block" => {
                    // The function body
                    body = self.build_statements(child);
                }
                _ => {}
            }
        }

        if name.is_empty() {
            return None;
        }

        Some(Statement::MethodDef {
            name,
            parameters,
            return_type: None,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter_wrapper::GroovyParser;

    fn parse_and_build(source: &str) -> Vec<Statement> {
        let mut parser = GroovyParser::new().expect("Parser init failed");
        let parsed = parser.parse(source).expect("Parse failed");
        let builder = AstBuilder::new(source);
        builder.build_statements(parsed.root())
    }

    #[test]
    fn test_int_literal() {
        let stmts = parse_and_build("42");
        assert!(!stmts.is_empty());
    }

    #[test]
    fn catch_parameter_type_and_name_are_preserved() {
        let statements = parse_and_build(
            "try { return 1 } catch (java.lang.RuntimeException problem) { return 2 }",
        );
        assert!(
            matches!(
                statements.as_slice(),
                [Statement::Try { catch_blocks, .. }]
                    if matches!(
                        catch_blocks.as_slice(),
                        [crate::ast::CatchBlock { exception_type, variable_name, .. }]
                            if exception_type == "java.lang.RuntimeException"
                                && variable_name == "problem"
                    )
            ),
            "statements: {statements:#?}"
        );
    }

    #[test]
    fn qualified_constructor_name_is_preserved() {
        let statements = parse_and_build("return new java.lang.RuntimeException()");
        assert!(
            matches!(
                statements.as_slice(),
                [Statement::Return(Some(Expression::NewObject { class_name, arguments }))]
                    if class_name == "java.lang.RuntimeException" && arguments.is_empty()
            ),
            "statements: {statements:#?}"
        );
    }

    #[test]
    fn non_decimal_integer_literals_preserve_twos_complement_width() {
        for (source, expected) in [
            ("return 0xFFFFFFFF", Expression::IntLiteral(-1)),
            (
                "return 0b11111111111111111111111111111111",
                Expression::IntLiteral(-1),
            ),
            ("return 037777777777", Expression::IntLiteral(-1)),
            ("return 0xFFFFFFFFL", Expression::LongLiteral(4_294_967_295)),
            ("return 0xFFFFFFFFFFFFFFFFL", Expression::LongLiteral(-1)),
        ] {
            let statements = parse_and_build(source);
            assert_eq!(
                statements,
                vec![Statement::Return(Some(expected))],
                "source: {source}"
            );
        }

        let statements = parse_and_build("def value = 42i\nreturn value");
        assert!(
            matches!(
                statements.as_slice(),
                [
                    Statement::VariableDecl {
                        initializer: Some(Expression::IntLiteral(42)),
                        ..
                    },
                    Statement::Return(Some(Expression::Identifier(name)))
                ] if name == "value"
            ),
            "statements: {statements:#?}"
        );
    }

    #[test]
    fn unsupported_integer_widths_and_domains_are_explicit() {
        for (source, expected) in [
            ("return 2147483648i", "forced 32-bit"),
            ("return 0x1FFFFFFFF", "add an L suffix"),
            ("return 1g", "BigInteger literals"),
        ] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.as_slice(),
                    [Statement::Return(Some(Expression::Unsupported(reason)))]
                        if reason.contains(expected)
                ),
                "source: {source}; statements: {statements:#?}"
            );
        }
    }

    #[test]
    fn floating_point_suffixes_preserve_their_runtime_domain() {
        for (source, expected) in [
            ("return 1.5f", Expression::FloatLiteral(1.5)),
            ("return 1.5d", Expression::DoubleLiteral(1.5)),
        ] {
            let statements = parse_and_build(source);
            assert_eq!(
                statements,
                vec![Statement::Return(Some(expected))],
                "source: {source}"
            );
        }
    }

    #[test]
    fn unsupported_or_unrepresentable_floating_literals_are_explicit() {
        for source in ["return 1.5g", "return 1e999d", "return 1e-999f"] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.as_slice(),
                    [Statement::Return(Some(Expression::Unsupported(reason)))]
                        if reason.contains("literal")
                ),
                "source: {source}; statements: {statements:#?}"
            );
        }
    }

    #[test]
    fn test_binary_expression() {
        let stmts = parse_and_build("2 + 3");
        assert!(!stmts.is_empty());
    }

    #[test]
    fn statement_conditions_do_not_consume_following_statements() {
        let statements = parse_and_build(
            "def result = 2\nif (\"\") { result = 1 } else { result = 3 }\nreturn result",
        );
        assert_eq!(statements.len(), 3, "statements: {statements:#?}");
        assert!(matches!(
            &statements[1],
            Statement::If {
                then_body,
                else_body: Some(else_body),
                ..
            } if then_body.len() == 1 && else_body.len() == 1
        ));
        assert!(matches!(statements[2], Statement::Return(_)));
    }

    #[test]
    fn error_wrapped_loop_control_is_not_erased() {
        for (source, expected) in [
            ("while (true) { break }", Statement::Break),
            ("while (true) { continue }", Statement::Continue),
        ] {
            let statements = parse_and_build(source);
            let Statement::While { body, .. } = &statements[0] else {
                panic!("expected while statement for {source}: {statements:#?}");
            };
            assert_eq!(body, &[expected], "source: {source}");
        }
    }

    #[test]
    fn for_loop_fields_are_preserved() {
        let statements = parse_and_build(
            "for (def i = 0; i < 2; i = i + 1) { value = i }\nfor (item in [1]) { value = item }",
        );
        assert!(matches!(
            &statements[0],
            Statement::TraditionalFor {
                init: Some(_),
                condition: Some(_),
                increment: Some(_),
                body,
            } if body.len() == 1
        ));
        assert!(matches!(
            &statements[1],
            Statement::For {
                variable,
                body,
                ..
            } if variable == "item" && body.len() == 1
        ));
    }

    #[test]
    fn multiple_variable_declarations_fail_closed() {
        let statements = parse_and_build("def first = 1, second = 2");
        assert!(matches!(
            statements.as_slice(),
            [Statement::Unsupported(feature)]
                if feature.contains("multiple variables")
        ));
    }

    #[test]
    fn error_bearing_aggregates_fail_as_a_whole() {
        for (source, expected) in [
            (
                "def foo = null\nreturn [1, foo?.bar]",
                "list or map literal",
            ),
            (
                "def foo = null\nreturn consume(foo?.bar)",
                "method arguments",
            ),
            (
                "def foo = null\nreturn [key: foo?.bar]",
                "list or map literal",
            ),
        ] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.last(),
                    Some(Statement::Return(Some(Expression::Unsupported(feature))))
                        if feature.contains(expected)
                ),
                "source: {source}; statements: {statements:#?}"
            );
        }

        assert!(matches!(
            parse_and_build("return [:]").as_slice(),
            [Statement::Return(Some(Expression::MapLiteral(entries)))] if entries.is_empty()
        ));
    }

    #[test]
    fn gstring_interpolation_uses_the_expression_parser() {
        let statements = parse_and_build("return \"value=${1 + 2}\"");
        assert!(matches!(
            statements.as_slice(),
            [Statement::Return(Some(Expression::GString(parts)))]
                if matches!(
                    parts.as_slice(),
                    [GStringPart::Text(text), GStringPart::Interpolation(expression)]
                        if text == "value="
                            && matches!(expression.as_ref(), Expression::BinaryOp { operator, .. } if operator == "+")
                )
        ));

        let unsupported = parse_and_build("return \"${counter++}\"");
        assert!(matches!(
            unsupported.as_slice(),
            [Statement::Return(Some(Expression::GString(parts)))]
                if matches!(
                    parts.as_slice(),
                    [GStringPart::Interpolation(expression)]
                        if matches!(expression.as_ref(), Expression::Unsupported(feature) if feature.contains("increment/decrement"))
                )
        ));
    }

    #[test]
    fn unrecoverable_error_node_is_preserved_as_unsupported() {
        let statements = parse_and_build("if ('') { return 1 }");
        assert!(matches!(
            statements.as_slice(),
            [Statement::Unsupported(feature)]
                if feature.contains("tree-sitter could not parse")
        ));
    }

    #[test]
    fn cast_and_instanceof_syntax_remain_visible_when_unsupported() {
        for (source, expected) in [
            ("return (String) value", "cast expressions"),
            ("return value instanceof String", "instanceof expressions"),
        ] {
            let statements = parse_and_build(source);
            assert!(matches!(
                statements.as_slice(),
                [Statement::Return(Some(Expression::Unsupported(feature)))]
                    if feature.contains(expected)
            ));
        }
    }

    #[test]
    fn operator_bearing_primary_syntax_is_not_reduced_to_a_child() {
        for (source, expected) in [
            ("return counter++", "increment/decrement"),
            ("return new int[3]", "array creation"),
            ("return String.class", "class literal"),
            ("return this", "explicit Groovy `this`"),
            ("return String::valueOf", "method reference"),
            ("return 0x1.0p1", "hexadecimal floating-point"),
        ] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.as_slice(),
                    [Statement::Return(Some(Expression::Unsupported(feature)))]
                        if feature.contains(expected)
                ),
                "source: {source}; statements: {statements:?}"
            );
        }
    }

    #[test]
    fn unsupported_statements_are_not_reduced_to_their_expressions_or_bodies() {
        for (source, expected) in [
            ("assert false", "assert statements"),
            ("do { value = 1 } while (false)", "do-while statements"),
            ("label: return 1", "labeled statements"),
            ("synchronized(this) { return 1 }", "synchronized statements"),
            (
                "try (resource = open()) { return 1 }",
                "try-with-resources statements",
            ),
            ("yield 1", "yield statements"),
        ] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.as_slice(),
                    [Statement::Unsupported(feature)] if feature.contains(expected)
                ),
                "source: {source}; statements: {statements:?}"
            );
        }
    }

    #[test]
    fn unsupported_declarations_are_not_reduced_to_identifiers() {
        for (source, expected) in [
            ("package example\nreturn 1", "package declarations"),
            ("import java.util.List\nreturn 1", "import declarations"),
            ("interface Widget {}", "interface declarations"),
            ("enum Mode { ON }", "enum declarations"),
            ("@interface Marker {}", "annotation type declarations"),
            ("record Pair(int value) {}", "record declarations"),
            ("module example {}", "module declarations"),
        ] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.first(),
                    Some(Statement::Unsupported(feature)) if feature.contains(expected)
                ),
                "source: {source}; statements: {statements:?}"
            );
        }
    }

    #[test]
    fn test_elvis_expression_recovered_from_error_node() {
        let statements = parse_and_build("return '' ?: 'fallback'");
        assert!(
            matches!(
                statements.as_slice(),
                [Statement::Return(Some(Expression::ElvisOp { primary, fallback }))]
                    if matches!(primary.as_ref(), Expression::StringLiteral(value) if value.is_empty())
                        && matches!(fallback.as_ref(), Expression::StringLiteral(value) if value == "fallback")
            ),
            "unexpected AST: {statements:#?}"
        );

        let list_elvis = AstBuilder::new("").try_parse_elvis_expression("[1] ?: 'empty'");
        assert!(
            matches!(
                list_elvis,
                Some(Expression::ElvisOp { ref primary, .. })
                    if matches!(primary.as_ref(), Expression::ListLiteral(items) if items.len() == 1)
            ),
            "unexpected list Elvis AST: {list_elvis:#?}"
        );

        let list_statements = parse_and_build("return [1] ?: 'empty'");
        assert!(
            matches!(
                list_statements.as_slice(),
                [Statement::Return(Some(Expression::ElvisOp { primary, .. }))]
                    if matches!(primary.as_ref(), Expression::ListLiteral(items) if items.len() == 1)
            ),
            "unexpected list Elvis statements: {list_statements:#?}"
        );
    }

    #[test]
    fn test_string_literal() {
        let stmts = parse_and_build("\"hello\"");
        assert!(!stmts.is_empty());
    }

    #[test]
    fn test_unescape_escaped_backslash_before_letter() {
        let builder = AstBuilder::new("");
        // Source characters: backslash, backslash, 'n' -> decoded backslash + 'n'.
        assert_eq!(builder.unescape_string(r#""\\n""#), "\\n");
        assert_eq!(builder.unescape_string(r#""\\t""#), "\\t");
        assert_eq!(builder.unescape_string(r#""\\r""#), "\\r");
    }

    #[test]
    fn test_unescape_simple_escapes() {
        let builder = AstBuilder::new("");
        assert_eq!(builder.unescape_string(r#""a\nb""#), "a\nb");
        assert_eq!(builder.unescape_string(r#""a\tb""#), "a\tb");
        assert_eq!(builder.unescape_string(r#""quote\"end""#), "quote\"end");
        assert_eq!(builder.unescape_string(r#""back\\end""#), "back\\end");
    }

    #[test]
    fn test_unescape_double_backslash_only() {
        let builder = AstBuilder::new("");
        // `\\` -> single backslash.
        assert_eq!(builder.unescape_string(r#""\\""#), "\\");
    }

    #[test]
    fn test_method_call() {
        let stmts = parse_and_build("println('hello')");
        assert!(!stmts.is_empty());
    }

    #[test]
    fn test_return_statement() {
        let stmts = parse_and_build("return 2 + 3");
        println!("Statements: {:?}", stmts);
        assert!(!stmts.is_empty(), "Expected statements from 'return 2 + 3'");
        // Check it's a return statement
        match &stmts[0] {
            Statement::Return(Some(expr)) => {
                println!("Return expr: {:?}", expr);
                match expr {
                    Expression::BinaryOp {
                        operator,
                        left,
                        right,
                    } => {
                        assert_eq!(operator, "+");
                        println!("Left: {:?}, Right: {:?}", left, right);
                    }
                    _ => panic!("Expected BinaryOp, got {:?}", expr),
                }
            }
            _ => panic!("Expected Return statement, got {:?}", stmts[0]),
        }
    }

    #[test]
    fn test_debug_method_parse_tree() {
        let source = r#"def add(a, b) { return a + b }"#;
        let mut parser = GroovyParser::new().expect("Parser init failed");
        let parsed = parser.parse(source).expect("Parse failed");

        fn print_node(source: &str, node: tree_sitter::Node, indent: usize) {
            let text = &source[node.start_byte()..node.end_byte()];
            let short_text: String = text.chars().take(80).collect();
            eprintln!(
                "{:indent$}{} [{:?}]",
                "",
                node.kind(),
                short_text,
                indent = indent
            );

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                print_node(source, child, indent + 2);
            }
        }

        print_node(source, parsed.root(), 0);

        // Now try building the AST
        let builder = AstBuilder::new(source);
        let stmts = builder.build_statements(parsed.root());
        eprintln!("\n=== Built AST ===");
        for stmt in &stmts {
            eprintln!("{:?}", stmt);
        }
    }

    #[test]
    fn test_debug_closure_parse_tree() {
        let source = r#"def add = { a, b -> a + b }"#;
        let mut parser = GroovyParser::new().expect("Parser init failed");
        let parsed = parser.parse(source).expect("Parse failed");

        fn print_node(source: &str, node: tree_sitter::Node, indent: usize) {
            let text = &source[node.start_byte()..node.end_byte()];
            let short_text: String = text.chars().take(80).collect();
            eprintln!(
                "{:indent$}{} [{:?}]",
                "",
                node.kind(),
                short_text,
                indent = indent
            );

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                print_node(source, child, indent + 2);
            }
        }

        print_node(source, parsed.root(), 0);

        // Now try building the AST
        let builder = AstBuilder::new(source);
        let stmts = builder.build_statements(parsed.root());
        eprintln!("\n=== Built AST ===");
        for stmt in &stmts {
            eprintln!("{:?}", stmt);
        }

        // Check the closure is actually built
        match &stmts[0] {
            Statement::VariableDecl {
                name, initializer, ..
            } => {
                assert_eq!(name, "add");
                match initializer {
                    Some(Expression::Closure(closure)) => {
                        assert_eq!(closure.parameters, vec!["a", "b"]);
                    }
                    other => panic!("Expected Closure, got {:?}", other),
                }
            }
            other => panic!("Expected VariableDecl, got {:?}", other),
        }
    }

    #[test]
    fn single_parameter_identity_closure_preserves_parameter_and_body() {
        let statements = parse_and_build("def identity = { value -> value }");
        let Statement::VariableDecl {
            initializer: Some(Expression::Closure(closure)),
            ..
        } = &statements[0]
        else {
            panic!("Expected closure declaration, got {:?}", statements[0]);
        };

        assert_eq!(closure.parameters, ["value"]);
        assert!(!closure.implicit_it);
        assert!(matches!(
            closure.body.as_slice(),
            [Statement::Expression(Expression::Identifier(name))] if name == "value"
        ));
    }

    #[test]
    fn test_command_call_with_parenthesized_call_argument() {
        // `implementation project(':lib')` means implementation(project(':lib')).
        let stmts = parse_and_build("implementation project(':lib')");
        assert_eq!(stmts.len(), 1, "Expected a single statement, got {stmts:?}");

        let Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name,
            arguments,
        }) = &stmts[0]
        else {
            panic!("Expected a method call, got {:?}", stmts[0]);
        };
        assert_eq!(method_name, "implementation");
        assert_eq!(arguments.len(), 1);
        let Expression::MethodCall {
            method_name: inner,
            arguments: inner_arguments,
            ..
        } = &arguments[0]
        else {
            panic!("Expected a nested call, got {:?}", arguments[0]);
        };
        assert_eq!(inner, "project");
        assert_eq!(inner_arguments.len(), 1);
    }

    #[test]
    fn test_command_call_with_property_chain_argument() {
        // Gradle Groovy DSL supports "command" style calls:
        //   implementation libs.bundles.logging
        //
        // We rely on this parsing shape in hyperion-bytecode when executing Gradle build files.
        let source = "implementation libs.bundles.logging";
        let stmts = parse_and_build(source);
        assert_eq!(stmts.len(), 1, "Expected a single statement");

        let expr = match &stmts[0] {
            Statement::Expression(expr) => expr,
            other => panic!("Expected expression statement, got {:?}", other),
        };

        match expr {
            Expression::MethodCall {
                receiver: None,
                method_name,
                arguments,
            } => {
                assert_eq!(method_name, "implementation");
                assert_eq!(arguments.len(), 1, "Expected one argument");

                // Expect: libs.bundles.logging as a nested property access chain.
                let arg0 = &arguments[0];
                match arg0 {
                    Expression::PropertyAccess {
                        object, property, ..
                    } => {
                        assert_eq!(property, "logging");
                        match object.as_ref() {
                            Expression::PropertyAccess {
                                object, property, ..
                            } => {
                                assert_eq!(property, "bundles");
                                assert!(
                                    matches!(object.as_ref(), Expression::Identifier(id) if id == "libs")
                                );
                            }
                            other => {
                                panic!("Expected libs.bundles as PropertyAccess, got {:?}", other)
                            }
                        }
                    }
                    other => panic!("Expected PropertyAccess argument, got {:?}", other),
                }
            }
            other => panic!("Expected MethodCall, got {:?}", other),
        }
    }

    #[test]
    fn test_dependencies_block_with_version_catalog_bundle_command_call() {
        let source = r#"
dependencies {
  implementation libs.bundles.logging
}
"#;
        let stmts = parse_and_build(source);

        // Find dependencies { ... } call
        let deps_args = stmts.iter().find_map(|stmt| match stmt {
            Statement::Expression(Expression::MethodCall {
                receiver: None,
                method_name,
                arguments,
            }) if method_name == "dependencies" => Some(arguments),
            _ => None,
        });
        let deps_args = deps_args.expect("Expected a dependencies { ... } call");

        let closure = deps_args
            .iter()
            .find_map(|arg| match arg {
                Expression::Closure(c) => Some(c.as_ref()),
                _ => None,
            })
            .expect("Expected dependencies closure argument");

        assert_eq!(
            closure.body.len(),
            1,
            "Expected a single statement in dependencies block"
        );

        let inner_expr = match &closure.body[0] {
            Statement::Expression(expr) => expr,
            other => panic!(
                "Expected expression inside dependencies block, got {:?}",
                other
            ),
        };

        match inner_expr {
            Expression::MethodCall {
                receiver: None,
                method_name,
                arguments,
            } => {
                assert_eq!(method_name, "implementation");
                assert_eq!(arguments.len(), 1);
                let arg0 = &arguments[0];
                // libs.bundles.logging
                match arg0 {
                    Expression::PropertyAccess {
                        object, property, ..
                    } => {
                        assert_eq!(property, "logging");
                        match object.as_ref() {
                            Expression::PropertyAccess {
                                object, property, ..
                            } => {
                                assert_eq!(property, "bundles");
                                assert!(
                                    matches!(object.as_ref(), Expression::Identifier(id) if id == "libs")
                                );
                            }
                            other => {
                                panic!("Expected libs.bundles as PropertyAccess, got {:?}", other)
                            }
                        }
                    }
                    other => panic!("Expected PropertyAccess argument, got {:?}", other),
                }
            }
            other => panic!("Expected MethodCall in dependencies block, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_named_argument_parsed_as_map_literal() {
        let source = "apply plugin: 'java'";
        let stmts = parse_and_build(source);
        assert_eq!(stmts.len(), 1, "Expected a single statement");

        let expr = match &stmts[0] {
            Statement::Expression(expr) => expr,
            other => panic!("Expected expression statement, got {:?}", other),
        };

        match expr {
            Expression::MethodCall {
                receiver: None,
                method_name,
                arguments,
            } => {
                assert_eq!(method_name, "apply");
                assert_eq!(
                    arguments.len(),
                    1,
                    "Expected apply(...) to have one argument"
                );
                match &arguments[0] {
                    Expression::MapLiteral(entries) => {
                        assert_eq!(entries.len(), 1, "Expected one named argument");
                        assert_eq!(entries[0].0, "plugin");
                        assert!(matches!(
                            &entries[0].1,
                            Expression::StringLiteral(s) if s == "java"
                        ));
                    }
                    other => panic!("Expected map literal argument, got {:?}", other),
                }
            }
            other => panic!("Expected MethodCall, got {:?}", other),
        }
    }

    #[test]
    fn preserves_control_flow_that_the_bytecode_layer_must_validate() {
        let statements =
            parse_and_build("break; continue; switch (1) { case 1: return 2; default: return 3 }");
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[0], Statement::Break));
        assert!(matches!(statements[1], Statement::Continue));
        assert!(matches!(statements[2], Statement::Switch { .. }));
    }

    #[test]
    fn preserves_class_declarations_as_unsupported_statements() {
        let statements = parse_and_build("class Widget { def value() { 42 } }");
        assert!(
            matches!(
                statements.as_slice(),
                [Statement::Unsupported(feature)] if feature == "Groovy class declarations"
            ),
            "AST: {statements:#?}"
        );
    }

    #[test]
    fn preserves_non_variable_assignment_targets() {
        let statements = parse_and_build("obj.value = 1; values[0] = 2");
        assert_eq!(statements.len(), 2, "AST: {statements:#?}");
        assert!(matches!(
            &statements[0],
            Statement::Expression(Expression::BinaryOp { operator, left, .. })
                if operator == "=" && matches!(left.as_ref(), Expression::PropertyAccess { .. })
        ));
        assert!(matches!(
            &statements[1],
            Statement::Expression(Expression::BinaryOp { operator, left, .. })
                if operator == "=" && matches!(left.as_ref(), Expression::IndexAccess { .. })
        ));
    }

    #[test]
    fn preserves_operators_that_bytecode_does_not_yet_lower() {
        let statements = parse_and_build("return 1 in [1]; return ~1");
        assert_eq!(statements.len(), 2, "AST: {statements:#?}");
        assert!(matches!(
            &statements[0],
            Statement::Return(Some(Expression::BinaryOp { operator, .. })) if operator == "in"
        ));
        assert!(matches!(
            &statements[1],
            Statement::Return(Some(Expression::UnaryOp { operator, .. })) if operator == "~"
        ));
    }

    #[test]
    fn preserves_spread_navigation_as_an_unsupported_expression() {
        for source in ["return [1, 2]*.toString()", "return [1, 2]*.class"] {
            let statements = parse_and_build(source);
            assert!(
                matches!(
                    statements.as_slice(),
                    [Statement::Return(Some(Expression::Unsupported(feature)))]
                        if feature == "Groovy spread navigation (*.member)"
                ),
                "source: {source}; AST: {statements:#?}"
            );
        }

        let literal = parse_and_build("return '*.' + \"literal\"");
        assert!(
            matches!(
                literal.as_slice(),
                [Statement::Return(Some(Expression::BinaryOp { operator, .. }))]
                    if operator == "+"
            ),
            "quoted spread token is data, not syntax: {literal:#?}"
        );
    }
}
