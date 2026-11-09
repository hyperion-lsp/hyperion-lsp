// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constant value classification for taint analysis.
//!
//! Distinguishes compile-time constant strings from unknown/tainted values.
//! This is critical for accurate taint analysis - string literals and constants
//! should not be flagged as tainted.

use hyperion_parser::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

/// Classification of a method's return value for taint analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnValueKind {
    /// Method always returns a constant string.
    ConstString,
    /// Method always returns a constant list of strings.
    ConstStringList,
    /// Return value is unknown or depends on runtime data.
    Unknown,
    /// Return value propagates taint from parameter at given index.
    PropagatesParam(usize),
    /// Return value propagates taint from multiple parameters.
    PropagatesParams(Vec<usize>),
}

/// Represents a compile-time constant value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstantValue {
    /// A constant string value.
    String(String),
    /// A constant list of strings.
    StringList(Vec<String>),
    /// A constant integer value.
    Integer(i64),
    /// A constant boolean value.
    Boolean(bool),
    /// The null literal.
    Null,
}

impl ConstantValue {
    /// Check if this is a string constant.
    #[must_use]
    pub const fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Check if this is a string list constant.
    #[must_use]
    pub const fn is_string_list(&self) -> bool {
        matches!(self, Self::StringList(_))
    }

    /// Get the string value if this is a string constant.
    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get the string list if this is a string list constant.
    #[must_use]
    pub fn as_string_list(&self) -> Option<&[String]> {
        match self {
            Self::StringList(list) => Some(list),
            _ => None,
        }
    }
}

/// Evaluates whether expressions are compile-time constants.
pub struct ConstantEvaluator {
    /// Known constant fields (qualified name -> value).
    constant_fields: HashMap<String, ConstantValue>,
}

impl Default for ConstantEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstantEvaluator {
    /// Create a new constant evaluator with no pre-defined constants.
    #[must_use]
    pub fn new() -> Self {
        Self {
            constant_fields: HashMap::new(),
        }
    }

    /// Create a constant evaluator with pre-defined constant fields.
    #[must_use]
    pub const fn with_constants(constants: HashMap<String, ConstantValue>) -> Self {
        Self {
            constant_fields: constants,
        }
    }

    /// Add a constant field to the evaluator.
    pub fn add_constant(&mut self, name: String, value: ConstantValue) {
        self.constant_fields.insert(name, value);
    }

    /// Evaluate if an expression is a compile-time constant.
    ///
    /// Returns `Some(ConstantValue)` if the expression is constant,
    /// `None` if it's not constant or unknown.
    #[must_use]
    pub fn evaluate(&self, expr: &SyntaxNode) -> Option<ConstantValue> {
        match expr.kind() {
            // String literal
            SyntaxKind::STRING_LITERAL => {
                let text = expr.text().to_string();
                Some(ConstantValue::String(unescape_string_literal(&text)))
            }

            // Text block literal (Java 13+)
            SyntaxKind::TEXT_BLOCK_LITERAL => {
                let text = expr.text().to_string();
                Some(ConstantValue::String(unescape_text_block(&text)))
            }

            // Literal expression wrapper
            SyntaxKind::LITERAL_EXPR => {
                // Get the actual literal token inside
                for child in expr.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        match token.kind() {
                            SyntaxKind::STRING_LITERAL => {
                                return Some(ConstantValue::String(unescape_string_literal(
                                    token.text(),
                                )));
                            }
                            SyntaxKind::TEXT_BLOCK_LITERAL => {
                                return Some(ConstantValue::String(unescape_text_block(
                                    token.text(),
                                )));
                            }
                            SyntaxKind::INT_LITERAL | SyntaxKind::LONG_LITERAL => {
                                return parse_integer_literal(token.text())
                                    .map(ConstantValue::Integer);
                            }
                            SyntaxKind::BOOLEAN_LITERAL => {
                                let text = token.text();
                                return Some(ConstantValue::Boolean(text == "true"));
                            }
                            SyntaxKind::NULL_LITERAL => {
                                return Some(ConstantValue::Null);
                            }
                            _ => {}
                        }
                    }
                }
                None
            }

            // Integer literals
            SyntaxKind::INT_LITERAL | SyntaxKind::LONG_LITERAL => {
                parse_integer_literal(&expr.text().to_string()).map(ConstantValue::Integer)
            }

            // Boolean literals
            SyntaxKind::BOOLEAN_LITERAL => {
                let text = expr.text().to_string();
                Some(ConstantValue::Boolean(text == "true"))
            }

            // Null literal
            SyntaxKind::NULL_LITERAL => Some(ConstantValue::Null),

            // Binary expression - check for string concatenation
            SyntaxKind::BINARY_EXPR => self.evaluate_binary_expr(expr),

            // Parenthesized expression
            SyntaxKind::PAREN_EXPR => {
                // Evaluate the inner expression
                for child in expr.children() {
                    if child.kind() != SyntaxKind::WHITESPACE {
                        return self.evaluate(&child);
                    }
                }
                None
            }

            // Name expression - check if it's a known constant
            SyntaxKind::NAME_EXPR => {
                let name = extract_identifier_text(expr)?;
                self.constant_fields.get(&name).cloned()
            }

            // Field access - check if it's a known constant
            SyntaxKind::FIELD_ACCESS_EXPR => {
                let qualified_name = extract_qualified_name(expr)?;
                self.constant_fields.get(&qualified_name).cloned()
            }

            // Method call - check for List.of() and Arrays.asList()
            SyntaxKind::METHOD_CALL_EXPR => self.evaluate_method_call(expr),

            _ => None,
        }
    }

    /// Evaluate a binary expression for constant values.
    fn evaluate_binary_expr(&self, expr: &SyntaxNode) -> Option<ConstantValue> {
        let mut operator = None;
        let mut operands = Vec::new();

        for child in expr.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::PLUS {
                        operator = Some(SyntaxKind::PLUS);
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    if is_expression_kind(node.kind()) {
                        operands.push(node);
                    }
                }
            }
        }

        // Only handle string concatenation for now
        if operator == Some(SyntaxKind::PLUS) && operands.len() == 2 {
            let left = self.evaluate(&operands[0])?;
            let right = self.evaluate(&operands[1])?;
            return Self::concat_values(&left, &right);
        }

        None
    }

    /// Concatenate two constant values for string concatenation.
    fn concat_values(left: &ConstantValue, right: &ConstantValue) -> Option<ConstantValue> {
        match (left, right) {
            (ConstantValue::String(l), ConstantValue::String(r)) => {
                Some(ConstantValue::String(format!("{l}{r}")))
            }
            (ConstantValue::String(l), ConstantValue::Integer(r)) => {
                Some(ConstantValue::String(format!("{l}{r}")))
            }
            (ConstantValue::Integer(l), ConstantValue::String(r)) => {
                Some(ConstantValue::String(format!("{l}{r}")))
            }
            _ => None,
        }
    }

    /// Evaluate a method call for constant values.
    fn evaluate_method_call(&self, expr: &SyntaxNode) -> Option<ConstantValue> {
        // Look for List.of() or Arrays.asList() patterns
        let method_name = extract_method_call_name(expr)?;

        match method_name.as_str() {
            "of" | "asList" => {
                // Check if receiver is List or Arrays
                let receiver = extract_method_call_receiver(expr);
                match receiver.as_deref() {
                    Some("List" | "Arrays") => {
                        // Evaluate all arguments as constants
                        let args = self.evaluate_method_call_args(expr)?;
                        let mut strings = Vec::new();
                        for arg in args {
                            match arg {
                                ConstantValue::String(s) => strings.push(s),
                                _ => return None, // Non-string argument
                            }
                        }
                        Some(ConstantValue::StringList(strings))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Evaluate method call arguments.
    fn evaluate_method_call_args(&self, expr: &SyntaxNode) -> Option<Vec<ConstantValue>> {
        let mut args = Vec::new();

        // Find ARG_LIST child
        for child in expr.children() {
            if child.kind() == SyntaxKind::ARG_LIST {
                for arg_child in child.children() {
                    // Skip whitespace and commas
                    if !matches!(arg_child.kind(), SyntaxKind::WHITESPACE) {
                        {
                            let value = self.evaluate(&arg_child)?;
                            args.push(value);
                        }
                    }
                }
                return Some(args);
            }
        }

        Some(args) // Empty args if no ARG_LIST found
    }

    /// Check if a field declaration is a constant (static final with constant initializer).
    #[must_use]
    pub fn is_constant_field(&self, field: &SyntaxNode) -> bool {
        if field.kind() != SyntaxKind::FIELD_DECL {
            return false;
        }

        let mut is_static = false;
        let mut is_final = false;

        // Check modifiers — may be a child of FIELD_DECL or a preceding sibling.
        // The parser places MODIFIER_LIST as a sibling in the class body, not as a
        // child of FIELD_DECL.
        let modifier_list = field
            .children()
            .find(|c| c.kind() == SyntaxKind::MODIFIER_LIST)
            .or_else(|| {
                let prev = field.prev_sibling()?;
                if prev.kind() == SyntaxKind::MODIFIER_LIST {
                    Some(prev)
                } else {
                    None
                }
            });

        if let Some(modifiers) = modifier_list {
            for modifier in modifiers.descendants_with_tokens() {
                if let Some(token) = modifier.as_token() {
                    match token.kind() {
                        SyntaxKind::STATIC_KW => is_static = true,
                        SyntaxKind::FINAL_KW => is_final = true,
                        _ => {}
                    }
                }
            }
        }

        if !is_static || !is_final {
            return false;
        }

        // Check if initializer is constant — expression may be a child or descendant
        for child in field.children() {
            if is_expression_kind(child.kind()) {
                return self.evaluate(&child).is_some();
            }
        }

        false
    }
}

/// Extract all constant fields from a class.
#[must_use]
pub fn extract_constant_fields(class: &SyntaxNode) -> HashMap<String, ConstantValue> {
    let mut constants = HashMap::new();
    let evaluator = ConstantEvaluator::new();

    let class_name = extract_class_name(class);

    for child in class.descendants() {
        if child.kind() == SyntaxKind::FIELD_DECL && evaluator.is_constant_field(&child) {
            if let Some((name, value)) = extract_field_constant(&child, &evaluator) {
                let qualified_name = if let Some(ref cn) = class_name {
                    format!("{cn}.{name}")
                } else {
                    name
                };
                constants.insert(qualified_name, value);
            }
        }
    }

    constants
}

// Helper functions

fn unescape_string_literal(s: &str) -> String {
    // Remove surrounding quotes
    let content = s.trim_matches('"');

    // Basic unescaping (could be expanded)
    content
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn unescape_text_block(s: &str) -> String {
    // Remove """ delimiters
    let content = s.trim_start_matches("\"\"\"").trim_end_matches("\"\"\"");
    // Strip leading newline and common indentation (simplified)
    content.trim_start_matches('\n').to_string()
}

fn parse_integer_literal(s: &str) -> Option<i64> {
    let s = s.trim_end_matches(['L', 'l']);
    let s = s.replace('_', "");

    if s.starts_with("0x") || s.starts_with("0X") {
        i64::from_str_radix(&s[2..], 16).ok()
    } else if s.starts_with("0b") || s.starts_with("0B") {
        i64::from_str_radix(&s[2..], 2).ok()
    } else if s.starts_with('0') && s.len() > 1 && s.chars().all(|c| c.is_ascii_digit()) {
        i64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse().ok()
    }
}

fn extract_identifier_text(expr: &SyntaxNode) -> Option<String> {
    for child in expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_qualified_name(expr: &SyntaxNode) -> Option<String> {
    let mut parts = Vec::new();

    fn collect_parts(node: &SyntaxNode, parts: &mut Vec<String>) {
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::IDENT {
                        parts.push(token.text().to_string());
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    collect_parts(&node, parts);
                }
            }
        }
    }

    collect_parts(expr, &mut parts);

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn extract_method_call_name(expr: &SyntaxNode) -> Option<String> {
    // In METHOD_CALL_EXPR, the method name is typically the IDENT token before ARG_LIST
    let mut last_ident = None;

    for child in expr.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT {
                    last_ident = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::ARG_LIST {
                    return last_ident;
                }
                // Check for IDENT inside NAME_EXPR
                if node.kind() == SyntaxKind::NAME_EXPR {
                    for inner in node.children_with_tokens() {
                        if let Some(token) = inner.as_token() {
                            if token.kind() == SyntaxKind::IDENT {
                                last_ident = Some(token.text().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    last_ident
}

fn extract_method_call_receiver(expr: &SyntaxNode) -> Option<String> {
    // Look for the receiver expression (before DOT)
    for child in expr.children() {
        match child.kind() {
            SyntaxKind::NAME_EXPR => {
                return extract_identifier_text(&child);
            }
            SyntaxKind::FIELD_ACCESS_EXPR => {
                // Return the rightmost identifier before the method name
                let mut last_ident = None;
                for c in child.children_with_tokens() {
                    if let Some(token) = c.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            last_ident = Some(token.text().to_string());
                        }
                    }
                }
                return last_ident;
            }
            _ => {}
        }
    }
    None
}

fn extract_class_name(class: &SyntaxNode) -> Option<String> {
    for child in class.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_field_constant(
    field: &SyntaxNode,
    evaluator: &ConstantEvaluator,
) -> Option<(String, ConstantValue)> {
    let mut field_name = None;
    let mut field_value = None;

    // In FIELD_DECL, IDENT and expression are direct children (no VAR_DECLARATOR wrapper)
    for child in field.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT && field_name.is_none() {
                    field_name = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if is_expression_kind(node.kind()) {
                    field_value = evaluator.evaluate(&node);
                }
            }
        }
    }

    match (field_name, field_value) {
        (Some(name), Some(value)) => Some((name, value)),
        _ => None,
    }
}

/// Find the previous sibling node that is an expression.
/// This handles the parser quirk where binary expression operands may be siblings.
const fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn parse_expr(code: &str) -> SyntaxNode {
        // Wrap expression in a minimal class
        let wrapped = format!("class Test {{ Object x = {}; }}", code);
        let tokens = Lexer::new(&wrapped).collect();
        let (green, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green);

        // Navigate to the expression - in FIELD_DECL, expression is a direct child
        // Note: Due to parser structure, binary expr may be split weird, so we
        // look for the LAST expression child to get the full expression
        for node in root.descendants() {
            if node.kind() == SyntaxKind::FIELD_DECL {
                let mut last_expr = None;
                for child in node.children() {
                    if is_expression_kind(child.kind()) {
                        last_expr = Some(child);
                    }
                }
                if let Some(expr) = last_expr {
                    return expr;
                }
            }
        }

        root
    }

    #[test]
    fn test_string_literal_is_constant() {
        let evaluator = ConstantEvaluator::new();
        let expr = parse_expr(r#""hello""#);
        let result = evaluator.evaluate(&expr);
        assert_eq!(result, Some(ConstantValue::String("hello".to_string())));
    }

    #[test]
    fn test_constant_concat() {
        let evaluator = ConstantEvaluator::new();
        let expr = parse_expr(r#""hello" + " world""#);
        let result = evaluator.evaluate(&expr);
        assert_eq!(
            result,
            Some(ConstantValue::String("hello world".to_string()))
        );
    }

    #[test]
    fn test_integer_literal() {
        let evaluator = ConstantEvaluator::new();
        let expr = parse_expr("42");
        let result = evaluator.evaluate(&expr);
        assert_eq!(result, Some(ConstantValue::Integer(42)));
    }

    #[test]
    fn test_hex_integer_literal() {
        let evaluator = ConstantEvaluator::new();
        let expr = parse_expr("0xFF");
        let result = evaluator.evaluate(&expr);
        assert_eq!(result, Some(ConstantValue::Integer(255)));
    }

    #[test]
    fn test_boolean_literal() {
        let evaluator = ConstantEvaluator::new();

        let expr_true = parse_expr("true");
        assert_eq!(
            evaluator.evaluate(&expr_true),
            Some(ConstantValue::Boolean(true))
        );

        let expr_false = parse_expr("false");
        assert_eq!(
            evaluator.evaluate(&expr_false),
            Some(ConstantValue::Boolean(false))
        );
    }

    #[test]
    fn test_null_literal() {
        let evaluator = ConstantEvaluator::new();
        let expr = parse_expr("null");
        let result = evaluator.evaluate(&expr);
        assert_eq!(result, Some(ConstantValue::Null));
    }

    #[test]
    fn test_known_constant_field() {
        let mut constants = HashMap::new();
        constants.insert("FOO".to_string(), ConstantValue::String("bar".to_string()));
        let evaluator = ConstantEvaluator::with_constants(constants);

        let expr = parse_expr("FOO");
        let result = evaluator.evaluate(&expr);
        assert_eq!(result, Some(ConstantValue::String("bar".to_string())));
    }

    #[test]
    fn test_string_integer_concat() {
        let evaluator = ConstantEvaluator::new();
        let expr = parse_expr(r#""count: " + 42"#);
        let result = evaluator.evaluate(&expr);
        assert_eq!(result, Some(ConstantValue::String("count: 42".to_string())));
    }
}
