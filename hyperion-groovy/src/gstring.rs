// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! GString (Groovy String) interpolation support
//!
//! Implements GString parsing and evaluation. GStrings support two forms of interpolation:
//! - Simple: `$variable` - evaluates to the variable's value
//! - Expression: `${expression}` - evaluates arbitrary Groovy expressions
//!
//! GStrings also support escaping with backslash:
//! - `\$` becomes literal `$`
//! - `\` before other characters is preserved

use crate::ast::{Expression, GStringPart};
use std::collections::HashMap;

/// Represents a parsed GString with static and dynamic parts
#[derive(Clone, Debug)]
pub struct ParsedGString {
    /// Parts of the GString (text segments and interpolations)
    pub parts: Vec<GStringPart>,
}

impl ParsedGString {
    /// Create a new parsed GString
    pub fn new(parts: Vec<GStringPart>) -> Self {
        ParsedGString { parts }
    }

    /// Get all static text in the GString
    pub fn static_text(&self) -> String {
        let mut result = String::new();
        for part in &self.parts {
            if let GStringPart::Text(text) = part {
                result.push_str(text);
            }
        }
        result
    }

    /// Check if this GString contains any interpolations
    pub fn has_interpolations(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, GStringPart::Interpolation(_)))
    }

    /// Get all interpolated expressions
    pub fn interpolations(&self) -> Vec<&Expression> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                GStringPart::Interpolation(expr) => Some(expr.as_ref()),
                _ => None,
            })
            .collect()
    }
}

/// GString evaluator for runtime interpolation
pub struct GStringEvaluator {
    /// Variable bindings for interpolation
    variables: HashMap<String, String>,
}

impl GStringEvaluator {
    /// Create a new GString evaluator
    pub fn new() -> Self {
        GStringEvaluator {
            variables: HashMap::new(),
        }
    }

    /// Set a variable value for interpolation
    pub fn set_variable(&mut self, name: String, value: String) {
        self.variables.insert(name, value);
    }

    /// Get a variable value
    pub fn get_variable(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(|s| s.as_str())
    }

    /// Evaluate a GString with the current bindings
    pub fn evaluate(&self, gstring: &ParsedGString) -> String {
        let mut result = String::new();

        for part in &gstring.parts {
            match part {
                GStringPart::Text(text) => {
                    result.push_str(text);
                }
                GStringPart::Interpolation(expr) => {
                    // For now, we extract what we can from the expression
                    let value = self.evaluate_expression(expr);
                    result.push_str(&value);
                }
            }
        }

        result
    }

    /// Evaluate an expression in a GString context
    fn evaluate_expression(&self, expr: &Expression) -> String {
        match expr {
            Expression::StringLiteral(s) => s.clone(),
            Expression::IntLiteral(i) => i.to_string(),
            Expression::LongLiteral(i) => i.to_string(),
            Expression::FloatLiteral(f) => f.to_string(),
            Expression::DoubleLiteral(d) => d.to_string(),
            Expression::BooleanLiteral(b) => b.to_string(),
            Expression::NullLiteral => "null".to_string(),
            Expression::Identifier(name) => {
                // Try to parse as literal first
                if let Ok(i) = name.parse::<i64>() {
                    return i.to_string();
                }
                if let Ok(f) = name.parse::<f64>() {
                    return f.to_string();
                }
                if name == "true" {
                    return "true".to_string();
                }
                if name == "false" {
                    return "false".to_string();
                }
                if name == "null" {
                    return "null".to_string();
                }

                // Otherwise look up as variable
                self.get_variable(name).unwrap_or("null").to_string()
            }
            Expression::Variable(name) => self.get_variable(name).unwrap_or("null").to_string(),
            Expression::PropertyAccess {
                object,
                property,
                safe_navigation,
            } => {
                // Simple property access: evaluate object, then append property name
                let obj_value = self.evaluate_expression(object);
                if obj_value == "null" && *safe_navigation {
                    "null".to_string()
                } else {
                    // For now, just format as object.property
                    format!("{}.{}", obj_value, property)
                }
            }
            Expression::MethodCall {
                receiver,
                method_name,
                arguments,
            } => {
                // Format method call as receiver.method(args)
                let receiver_str = if let Some(recv) = receiver {
                    self.evaluate_expression(recv)
                } else {
                    method_name.clone()
                };

                let args_str = arguments
                    .iter()
                    .map(|arg| self.evaluate_expression(arg))
                    .collect::<Vec<_>>()
                    .join(", ");

                format!("{}({})", receiver_str, args_str)
            }
            Expression::BinaryOp {
                operator,
                left,
                right,
            } => {
                let left_val = self.evaluate_expression(left);
                let right_val = self.evaluate_expression(right);
                format!("{} {} {}", left_val, operator, right_val)
            }
            Expression::UnaryOp { operator, operand } => {
                let operand_val = self.evaluate_expression(operand);
                format!("{}{}", operator, operand_val)
            }
            Expression::ListLiteral(items) => {
                let items_str = items
                    .iter()
                    .map(|item| self.evaluate_expression(item))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", items_str)
            }
            Expression::MapLiteral(pairs) => {
                let pairs_str = pairs
                    .iter()
                    .map(|(key, val)| format!("{}:{}", key, self.evaluate_expression(val)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", pairs_str)
            }
            Expression::Parenthesized(expr) => {
                format!("({})", self.evaluate_expression(expr))
            }
            _ => "...".to_string(), // Other complex expressions
        }
    }
}

impl Default for GStringEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a GString literal string into parts
pub fn parse_gstring(input: &str) -> ParsedGString {
    let mut parts = Vec::new();
    let mut current_text = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                // Handle escaping
                if let Some(next) = chars.peek() {
                    if matches!(next, '$' | '\\') {
                        current_text.push(*next);
                        chars.next();
                    } else {
                        current_text.push(ch);
                    }
                } else {
                    current_text.push(ch);
                }
            }
            '$' => {
                // Start of interpolation
                if chars.peek() == Some(&'{') {
                    // Expression interpolation: ${...}
                    if !current_text.is_empty() {
                        parts.push(GStringPart::Text(current_text.clone()));
                        current_text.clear();
                    }

                    chars.next(); // consume '{'
                    let expr_str = extract_until_closing_brace(&mut chars);
                    // For now, store the expression string as a placeholder
                    // In a full implementation, this would be parsed into an Expression
                    let expr = Expression::Identifier(expr_str);
                    parts.push(GStringPart::Interpolation(Box::new(expr)));
                } else {
                    // Simple interpolation: $variable
                    if !current_text.is_empty() {
                        parts.push(GStringPart::Text(current_text.clone()));
                        current_text.clear();
                    }

                    let var_name = extract_identifier(&mut chars);
                    let expr = Expression::Identifier(var_name);
                    parts.push(GStringPart::Interpolation(Box::new(expr)));
                }
            }
            _ => {
                current_text.push(ch);
            }
        }
    }

    // Add any remaining text
    if !current_text.is_empty() {
        parts.push(GStringPart::Text(current_text));
    }

    ParsedGString::new(parts)
}

/// Extract the content inside ${...}
fn extract_until_closing_brace(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut result = String::new();
    let mut brace_depth = 1;
    let mut in_string = false;
    let mut prev_char = '\0';

    while brace_depth > 0 {
        if let Some(ch) = chars.next() {
            match ch {
                '"' if prev_char != '\\' => {
                    // Toggle string state (unescaped quote)
                    in_string = !in_string;
                    result.push(ch);
                }
                '{' if !in_string => {
                    brace_depth += 1;
                    result.push(ch);
                }
                '}' if !in_string => {
                    brace_depth -= 1;
                    if brace_depth > 0 {
                        result.push(ch);
                    }
                }
                _ => {
                    result.push(ch);
                }
            }
            prev_char = ch;
        } else {
            break;
        }
    }

    result
}

/// Extract a simple identifier starting from current position
/// Groovy identifiers must start with a letter or underscore, not a digit
fn extract_identifier(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut result = String::new();

    // First character must be a letter or underscore (not a digit)
    if let Some(&ch) = chars.peek() {
        if ch.is_alphabetic() || ch == '_' {
            result.push(ch);
            chars.next();
        } else {
            // Not a valid identifier start
            return result;
        }
    }

    // Subsequent characters can be alphanumeric or underscore
    while let Some(&ch) = chars.peek() {
        if ch.is_alphanumeric() || ch == '_' {
            result.push(ch);
            chars.next();
        } else {
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gstring_plain_text() {
        let gstring = parse_gstring("hello world");
        assert!(!gstring.has_interpolations());
        assert_eq!(gstring.static_text(), "hello world");
    }

    #[test]
    fn test_parse_gstring_simple_interpolation() {
        let gstring = parse_gstring("hello $name");
        assert!(gstring.has_interpolations());
        assert_eq!(gstring.interpolations().len(), 1);
    }

    #[test]
    fn test_parse_gstring_expression_interpolation() {
        let gstring = parse_gstring("result: ${x + y}");
        assert!(gstring.has_interpolations());
        assert_eq!(gstring.interpolations().len(), 1);
    }

    #[test]
    fn test_parse_gstring_multiple_interpolations() {
        let gstring = parse_gstring("$greeting $name, you are ${age} years old");
        assert!(gstring.has_interpolations());
        assert_eq!(gstring.interpolations().len(), 3);
    }

    #[test]
    fn test_parse_gstring_escaped_dollar() {
        let gstring = parse_gstring("price: \\$100");
        assert!(!gstring.has_interpolations());
        assert_eq!(gstring.static_text(), "price: $100");
    }

    #[test]
    fn test_parse_gstring_escaped_backslash() {
        let gstring = parse_gstring("path: C:\\\\Users");
        assert!(!gstring.has_interpolations());
    }

    #[test]
    fn test_evaluator_simple_variable() {
        let mut evaluator = GStringEvaluator::new();
        evaluator.set_variable("name".to_string(), "Alice".to_string());

        let gstring = parse_gstring("Hello $name");
        let result = evaluator.evaluate(&gstring);
        assert_eq!(result, "Hello Alice");
    }

    #[test]
    fn test_evaluator_multiple_variables() {
        let mut evaluator = GStringEvaluator::new();
        evaluator.set_variable("first".to_string(), "Alice".to_string());
        evaluator.set_variable("last".to_string(), "Smith".to_string());

        let gstring = parse_gstring("$first $last");
        let result = evaluator.evaluate(&gstring);
        assert_eq!(result, "Alice Smith");
    }

    #[test]
    fn test_evaluator_expression_interpolation() {
        let evaluator = GStringEvaluator::new();
        let gstring = parse_gstring("value: ${42}");
        let result = evaluator.evaluate(&gstring);
        assert_eq!(result, "value: 42");
    }

    #[test]
    fn test_evaluator_literal_values() {
        let evaluator = GStringEvaluator::new();

        let int_gstring = parse_gstring("${123}");
        assert_eq!(evaluator.evaluate(&int_gstring), "123");

        let bool_gstring = parse_gstring("${true}");
        assert_eq!(evaluator.evaluate(&bool_gstring), "true");

        let null_gstring = parse_gstring("${null}");
        assert_eq!(evaluator.evaluate(&null_gstring), "null");
    }

    #[test]
    fn test_evaluator_undefined_variable() {
        let evaluator = GStringEvaluator::new();
        let gstring = parse_gstring("hello $undefined");
        let result = evaluator.evaluate(&gstring);
        assert_eq!(result, "hello null");
    }

    #[test]
    fn test_gstring_coercion_to_string() {
        // GString with only static text becomes plain String
        let gstring = parse_gstring("static text");
        assert_eq!(gstring.static_text(), "static text");
    }

    #[test]
    fn test_complex_gstring() {
        let mut evaluator = GStringEvaluator::new();
        evaluator.set_variable("user".to_string(), "Bob".to_string());
        evaluator.set_variable("count".to_string(), "5".to_string());

        let gstring = parse_gstring("User $user has $count items");
        let result = evaluator.evaluate(&gstring);
        assert_eq!(result, "User Bob has 5 items");
    }
}
