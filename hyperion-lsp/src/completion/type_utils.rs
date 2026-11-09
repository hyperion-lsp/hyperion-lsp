// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type utility functions for completion
//!
//! Pure utility functions for type inference and formatting that don't require
//! access to workspace state.

use hyperion_semantics::symbol::{PrimitiveType, Type};
use tracing::debug;

/// Infer expected type from an assignment pattern
///
/// Parses the text before cursor to extract variable type from declarations like:
/// - "String name = " -> String
/// - "List<User> users = " -> List
/// - "int count = " -> int
pub fn infer_type_from_assignment(before_cursor: &str) -> Option<String> {
    let trimmed = before_cursor.trim_end();

    // Must end with '=' (but not '==' or other operators)
    if !trimmed.ends_with('=') {
        return None;
    }

    // Check it's not '==', '!=', '<=', '>=', '+=', etc.
    // Get the character before '=' by iterating from the end
    let mut chars = trimmed.chars();
    chars.next_back(); // Skip the '=' at the end
    if let Some(second_last) = chars.next_back() {
        if matches!(
            second_last,
            '=' | '!' | '<' | '>' | '+' | '-' | '*' | '/' | '&' | '|' | '^'
        ) {
            return None;
        }
    }

    let before_eq = &trimmed[..trimmed.len() - 1].trim_end();

    // Find statement start (after ; { } or start of string)
    let stmt_start = before_eq.rfind([';', '{', '}']).map_or(0, |i| i + 1);

    let statement = before_eq[stmt_start..].trim();

    // Skip if it's a comparison or other non-declaration
    if statement.is_empty() || statement.contains("==") {
        return None;
    }

    // Parse: [modifiers] Type variableName
    // Examples: "String name", "final int count", "List<User> users", "private String value"
    let parts: Vec<&str> = statement.split_whitespace().collect();

    if parts.len() < 2 {
        return None;
    }

    // Find the type (skip modifiers like public, private, static, final)
    let modifiers = [
        "public",
        "private",
        "protected",
        "static",
        "final",
        "volatile",
        "transient",
    ];
    let mut type_idx = 0;
    for (i, part) in parts.iter().enumerate() {
        if !modifiers.contains(part) {
            type_idx = i;
            break;
        }
    }

    if type_idx >= parts.len() - 1 {
        return None; // No type found before variable name
    }

    // Extract type (might have generics)
    let type_str = parts[type_idx];

    // Handle generics: "List<User>" -> "List"
    // Also handle array types: "String[]" -> "String"
    let simple_type = type_str
        .split('<')
        .next()
        .unwrap_or(type_str)
        .trim_end_matches("[]")
        .to_string();

    if simple_type.is_empty() {
        None
    } else {
        Some(simple_type)
    }
}

/// Infer the expected type from a variable declaration before `new`
///
/// Parses patterns like:
/// - `ArrayList<String> list = new ` → "ArrayList"
/// - `HashMap<K, V> map = new ` → "HashMap"
/// - `User user = new ` → "User"
pub fn infer_expected_constructor_type(before_cursor: &str) -> Option<String> {
    // Look for pattern: TypeName<...> varName = new
    // Work backwards from cursor, but only look at the current statement

    let trimmed = before_cursor.trim_end();
    if !trimmed.ends_with("new") {
        return None;
    }

    // Remove "new" and get the part before it
    let before_new = trimmed[..trimmed.len() - 3].trim_end();

    // Should end with "="
    if !before_new.ends_with('=') {
        return None;
    }

    let before_eq = before_new[..before_new.len() - 1].trim_end();

    // Find the start of the current statement (after ; or { or start of block)
    // This prevents scanning back into previous statements
    let stmt_start = before_eq.rfind([';', '{', '}']).map_or(0, |i| i + 1);

    let statement = before_eq[stmt_start..].trim();

    // Now we should have "TypeName<...> varName" or "TypeName varName"
    // Handle generic types: find matching < >
    let mut angle_depth = 0;
    let mut type_end = statement.len();

    // Scan backwards to find where the variable name starts
    for (i, c) in statement.chars().rev().enumerate() {
        let pos = statement.len() - 1 - i;
        match c {
            '>' => angle_depth += 1,
            '<' => angle_depth -= 1,
            ' ' | '\t' | '\n' if angle_depth == 0 => {
                type_end = pos;
                break;
            }
            _ => {}
        }
    }

    // Extract the part before the variable name (the type declaration)
    let type_part = statement[..type_end].trim();

    // Extract simple type name (before any generic parameters)
    let type_name = if let Some(angle_pos) = type_part.rfind('<') {
        // Has generic params, extract base type
        let base = type_part[..angle_pos].trim();
        // Handle qualified types like "java.util.ArrayList" - get last part
        base.rsplit('.').next().unwrap_or(base)
    } else {
        // No generic params, get the last word (type name)
        type_part.split_whitespace().last().unwrap_or(type_part)
    };

    // Handle qualified names
    let simple_name = type_name.rsplit('.').next().unwrap_or(type_name);

    if simple_name.is_empty() || !simple_name.chars().next().is_some_and(char::is_uppercase) {
        return None;
    }

    debug!("Inferred expected constructor type: {}", simple_name);
    Some(simple_name.to_string())
}

/// Format a semantic Type for display in completion details
pub fn format_type(typ: &Type) -> String {
    match typ {
        Type::Primitive(p) => match p {
            PrimitiveType::Boolean => "boolean",
            PrimitiveType::Byte => "byte",
            PrimitiveType::Char => "char",
            PrimitiveType::Short => "short",
            PrimitiveType::Int => "int",
            PrimitiveType::Long => "long",
            PrimitiveType::Float => "float",
            PrimitiveType::Double => "double",
        }
        .to_string(),
        Type::Class(c) => {
            if c.type_arguments.is_empty() {
                c.name.to_string()
            } else {
                format!(
                    "{}<{}>",
                    c.name,
                    c.type_arguments
                        .iter()
                        .map(format_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Type::Array {
            element_type,
            dimensions,
        } => {
            let mut result = format_type(element_type);
            for _ in 0..*dimensions {
                result.push_str("[]");
            }
            result
        }
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::TypeVariable { name, .. } => name.clone(),
        Type::Wildcard { .. } => "?".to_string(),
        Type::Unknown => "?".to_string(),
        Type::Intersection { types } => types
            .iter()
            .map(format_type)
            .collect::<Vec<_>>()
            .join(" & "),
        Type::Union { types } => types
            .iter()
            .map(format_type)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

/// Find the opening parenthesis position in source text
pub fn find_opening_paren(node_text: &str, node_start: usize) -> Option<usize> {
    node_text.find('(').map(|local_pos| node_start + local_pos)
}

/// Count commas before position to determine parameter index
///
/// Only counts commas at the top nesting level (depth == 1 after first paren).
pub fn count_commas_before_position(source_text: &str, start: usize, end: usize) -> u32 {
    let text = &source_text[start..end.min(source_text.len())];
    let mut count = 0;
    let mut depth = 0;

    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 1 => count += 1, // Only count commas at parameter level
            _ => {}
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_type_from_assignment_simple() {
        assert_eq!(
            infer_type_from_assignment("String name = "),
            Some("String".to_string())
        );
        assert_eq!(
            infer_type_from_assignment("int count = "),
            Some("int".to_string())
        );
    }

    #[test]
    fn test_infer_type_from_assignment_with_modifiers() {
        assert_eq!(
            infer_type_from_assignment("final String name = "),
            Some("String".to_string())
        );
        assert_eq!(
            infer_type_from_assignment("private static int count = "),
            Some("int".to_string())
        );
    }

    #[test]
    fn test_infer_type_from_assignment_generic() {
        assert_eq!(
            infer_type_from_assignment("List<String> items = "),
            Some("List".to_string())
        );
        assert_eq!(
            infer_type_from_assignment("Map<String, Integer> map = "),
            Some("Map".to_string())
        );
    }

    #[test]
    fn test_infer_type_from_assignment_not_assignment() {
        assert_eq!(infer_type_from_assignment("x == y"), None);
        assert_eq!(infer_type_from_assignment("x != y"), None);
        assert_eq!(infer_type_from_assignment("x += 1"), None);
    }

    #[test]
    fn test_infer_expected_constructor_type() {
        assert_eq!(
            infer_expected_constructor_type("ArrayList<String> list = new "),
            Some("ArrayList".to_string())
        );
        assert_eq!(
            infer_expected_constructor_type("User user = new "),
            Some("User".to_string())
        );
    }

    #[test]
    fn test_count_commas() {
        assert_eq!(count_commas_before_position("(a, b, c)", 0, 9), 2);
        assert_eq!(count_commas_before_position("(a, (b, c), d)", 0, 14), 2);
    }
}
