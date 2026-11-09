// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Utility functions for code actions
//!
//! This module contains shared utility functions used by various code action modules.

use tower_lsp::lsp_types::{Position, Range};

/// Convert LSP Position to byte offset
///
/// BUG FIX: Previously this function incorrectly counted UTF-8 chars instead of
/// UTF-16 code units for the character position. Now delegates to the correct
/// implementation in position_utils which properly handles UTF-16.
pub fn position_to_offset(
    source: &str,
    position: Position,
) -> Result<u32, crate::position_utils::PositionError> {
    crate::position_utils::position_to_offset_u32(source, position)
}

/// Get text within a range
///
/// Uses byte slicing directly since position_to_offset returns byte offsets.
pub fn get_text_in_range(source_text: &str, range: &Range) -> Option<String> {
    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;
    let end_offset = position_to_offset(source_text, range.end).ok()? as usize;

    // Use byte slicing - position_to_offset returns byte offsets, not char counts
    source_text
        .get(start_offset..end_offset)
        .map(|s| s.to_string())
}

/// Generate a variable name from an expression
pub fn generate_variable_name(expression: &str) -> String {
    let expr = expression.trim();

    // For "new ClassName(...)" - use lowercase class name
    if let Some(after_new) = expr.strip_prefix("new ") {
        if let Some(class_end) = after_new.find(|c: char| c == '(' || c == '<' || c.is_whitespace())
        {
            let class_name = &after_new[..class_end];
            // Handle qualified names like com.example.ClassName
            let simple_name = class_name.rsplit('.').next().unwrap_or(class_name);
            if !simple_name.is_empty() {
                let mut name = simple_name.to_string();
                if let Some(first) = name.get_mut(0..1) {
                    first.make_ascii_lowercase();
                }
                return name;
            }
        }
    }

    // For method calls - use result of method name
    if expr.ends_with(')') {
        if let Some(method_start) = expr.rfind('.') {
            let method_part = &expr[method_start + 1..];
            if let Some(paren_idx) = method_part.find('(') {
                let method_name = &method_part[..paren_idx];
                // Common patterns: getX -> x, isX -> x, findX -> x
                if let Some(stripped) = method_name.strip_prefix("get") {
                    if !stripped.is_empty() {
                        let mut name = stripped.to_string();
                        if let Some(first) = name.get_mut(0..1) {
                            first.make_ascii_lowercase();
                        }
                        return name;
                    }
                }
                if let Some(stripped) = method_name.strip_prefix("is") {
                    if !stripped.is_empty() {
                        let mut name = stripped.to_string();
                        if let Some(first) = name.get_mut(0..1) {
                            first.make_ascii_lowercase();
                        }
                        return name;
                    }
                }
                if let Some(stripped) = method_name.strip_prefix("find") {
                    if !stripped.is_empty() {
                        let mut name = stripped.to_string();
                        if let Some(first) = name.get_mut(0..1) {
                            first.make_ascii_lowercase();
                        }
                        return name;
                    }
                }
                // Use method name as-is
                return method_name.to_string();
            }
        }
    }

    // For literals
    if expr.starts_with('"') || expr.starts_with('\'') {
        return "str".to_string();
    }

    if expr.parse::<i64>().is_ok() {
        return "num".to_string();
    }

    if expr.parse::<f64>().is_ok() {
        return "value".to_string();
    }

    // For simple identifiers with dots (field access)
    if expr.contains('.') && !expr.contains('(') {
        if let Some(last_part) = expr.rsplit('.').next() {
            return last_part.to_string();
        }
    }

    // Default
    "result".to_string()
}

/// Infer the type of an expression (basic heuristics)
pub fn infer_expression_type(expression: &str) -> String {
    let expr = expression.trim();

    // String literals
    if expr.starts_with('"') || expr.starts_with("\"\"\"") {
        return "String".to_string();
    }

    // Character literals
    if expr.starts_with('\'') && expr.ends_with('\'') && expr.len() <= 4 {
        return "char".to_string();
    }

    // Boolean literals
    if expr == "true" || expr == "false" {
        return "boolean".to_string();
    }

    // Null literal
    if expr == "null" {
        return "Object".to_string();
    }

    // Integer literals (including hex, binary, octal)
    if expr.parse::<i64>().is_ok()
        || expr.starts_with("0x")
        || expr.starts_with("0X")
        || expr.starts_with("0b")
        || expr.starts_with("0B")
    {
        if expr.ends_with('L') || expr.ends_with('l') {
            return "long".to_string();
        }
        return "int".to_string();
    }

    // Float literals
    if expr.parse::<f64>().is_ok() || expr.ends_with('f') || expr.ends_with('F') {
        if expr.ends_with('f') || expr.ends_with('F') {
            return "float".to_string();
        }
        return "double".to_string();
    }

    // Constructor calls
    if let Some(after_new) = expr.strip_prefix("new ") {
        if let Some(class_end) = after_new.find(['(', '<', '[']) {
            let class_name = after_new[..class_end].trim();
            // Check for array
            if expr.contains('[') {
                return format!("{class_name}[]");
            }
            return class_name.to_string();
        }
    }

    // Method calls with common return type patterns
    if expr.ends_with(')') {
        if let Some(method_start) = expr.rfind('.') {
            let method_part = &expr[method_start + 1..];
            if let Some(paren_idx) = method_part.find('(') {
                let method_name = &method_part[..paren_idx];
                // Common patterns
                if method_name.starts_with("is")
                    || method_name.starts_with("has")
                    || method_name.starts_with("can")
                {
                    return "boolean".to_string();
                }
                if method_name == "toString" {
                    return "String".to_string();
                }
                if method_name == "hashCode" {
                    return "int".to_string();
                }
                if method_name == "size" || method_name == "length" || method_name == "indexOf" {
                    return "int".to_string();
                }
            }
        }
    }

    // Default: use var for type inference
    "var".to_string()
}

/// Find variables referenced in a code snippet
pub fn find_variables_in_code(code: &str) -> Vec<String> {
    let mut variables = Vec::new();
    let mut current_word = String::new();

    for ch in code.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current_word.push(ch);
        } else {
            if !current_word.is_empty() && !is_keyword(&current_word) && !is_literal(&current_word)
            {
                // Check if it starts with lowercase (likely a variable)
                if current_word.chars().next().is_some_and(char::is_lowercase)
                    && !variables.contains(&current_word)
                {
                    variables.push(current_word.clone());
                }
            }
            current_word.clear();
        }
    }

    // Check last word
    if !current_word.is_empty()
        && !is_keyword(&current_word)
        && !is_literal(&current_word)
        && current_word.chars().next().is_some_and(char::is_lowercase)
        && !variables.contains(&current_word)
    {
        variables.push(current_word);
    }

    variables
}

/// Check if a word is a Java keyword
pub fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "abstract"
            | "assert"
            | "boolean"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "int"
            | "interface"
            | "long"
            | "native"
            | "new"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "short"
            | "static"
            | "strictfp"
            | "super"
            | "switch"
            | "synchronized"
            | "this"
            | "throw"
            | "throws"
            | "transient"
            | "try"
            | "void"
            | "volatile"
            | "while"
            | "var"
            | "yield"
            | "record"
            | "sealed"
            | "permits"
            | "non-sealed"
    )
}

/// Check if a word is a literal
pub fn is_literal(word: &str) -> bool {
    word == "true" || word == "false" || word == "null" || word.parse::<i64>().is_ok()
}

/// Find the closing brace of a class/method starting from a position inside the block.
/// Returns the position of the first unmatched closing brace.
pub fn find_class_closing_brace(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in source[start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            continue;
        }

        if in_string {
            continue;
        }

        match ch {
            '{' => depth += 1,
            '}' => {
                // Return immediately when we find an unmatched closing brace
                if depth == 0 {
                    return Some(start + i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    None
}

/// Find the start of the class body (after opening brace)
pub fn find_class_body_start(source: &str, from: usize) -> Option<usize> {
    let before = &source[..from];
    let class_pos = before.rfind("class ")?;
    let after_class = &source[class_pos..];
    let brace_offset = after_class.find('{')?;
    Some(class_pos + brace_offset + 1)
}

/// Generate a constant name from an expression
pub fn generate_constant_name(expression: &str) -> String {
    let expr = expression.trim();

    // For string literals, try to extract meaningful name
    if expr.starts_with('"') && expr.ends_with('"') && expr.len() > 2 {
        let content = &expr[1..expr.len() - 1];
        // Convert to SCREAMING_SNAKE_CASE
        let name: String = content
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '_')
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_")
            .to_uppercase();

        if !name.is_empty() && name.len() <= 30 {
            return name;
        }
    }

    // For numeric literals
    if expr.parse::<i64>().is_ok() || expr.parse::<f64>().is_ok() {
        return "CONSTANT".to_string();
    }

    // For method calls, use the method name
    if expr.contains('.') && expr.ends_with(')') {
        if let Some(method_start) = expr.rfind('.') {
            let method = &expr[method_start + 1..];
            if let Some(paren_idx) = method.find('(') {
                let name = &method[..paren_idx];
                return name.to_uppercase();
            }
        }
    }

    "CONSTANT".to_string()
}

/// Generate a field name from an expression
pub fn generate_field_name(expr: &str) -> String {
    // Try to extract a meaningful name
    if let Some(rest) = expr.strip_prefix("new ") {
        // Extract class name from constructor
        if let Some(paren_pos) = rest.find('(') {
            let class_name = &rest[..paren_pos].trim();
            if let Some(last_dot) = class_name.rfind('.') {
                return to_camel_case(&class_name[last_dot + 1..]);
            }
            return to_camel_case(class_name);
        }
    }

    "field".to_string()
}

/// Convert string to camelCase
pub fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut first = true;
    for c in s.chars() {
        if first {
            result.push(c.to_ascii_lowercase());
            first = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Generate a parameter name from an expression
pub fn generate_param_name(expr: &str) -> String {
    if let Some(pos) = expr.rfind('.') {
        let after_dot = &expr[pos + 1..];
        if let Some(paren) = after_dot.find('(') {
            return format!("{}Result", &after_dot[..paren]);
        }
    }
    "param".to_string()
}

/// Get singular form of a collection name (simple heuristic)
pub fn singular_form(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if name.ends_with("es")
        && (name.ends_with("ses") || name.ends_with("xes") || name.ends_with("zes"))
    {
        if let Some(stem) = name.strip_suffix("es") {
            return stem.to_string();
        }
    }
    if let Some(stem) = name.strip_suffix('s') {
        if !name.ends_with("ss") {
            return stem.to_string();
        }
    }
    format!("{name}Item")
}

/// Find the start of a line
pub fn find_line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |p| p + 1)
}

/// Find the start of the next line
pub fn find_next_line_start(source: &str, offset: usize) -> usize {
    source[offset..]
        .find('\n')
        .map_or(source.len(), |p| offset + p + 1)
}
