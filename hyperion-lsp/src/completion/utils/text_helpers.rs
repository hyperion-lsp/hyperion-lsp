// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Text manipulation utilities for completion
//!
//! Helper functions for extracting text, parsing identifiers, and manipulating source text.

use crate::position_utils::position_to_offset;
use tower_lsp::lsp_types::Position;

/// Extract the identifier prefix before the cursor position
/// Returns the partial word being typed (e.g., "sou" when typing "sout")
pub fn extract_identifier_prefix(source_text: &str, position: Position) -> String {
    let offset = match position_to_offset(source_text, position) {
        Ok(o) => o,
        Err(_) => return String::new(),
    };

    let before_cursor = &source_text[..offset];

    // Walk backwards to find the start of the identifier
    let start = before_cursor
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
        .map_or(0, |(i, _)| i + 1);

    before_cursor[start..].to_string()
}

/// Simplify a fully qualified type name to its simple name
/// - Handles generics: List<String> -> List
/// - Handles arrays: String[] -> String
/// - Gets simple name: java.lang.String -> String
pub fn simplify_type_name(full_type: &str) -> String {
    // Handle generics: List<String> -> List
    let base = full_type.split('<').next().unwrap_or(full_type);

    // Handle arrays: String[] -> String
    let base = base.trim_end_matches("[]");

    // Get simple name: java.lang.String -> String
    let simple = base.rsplit_once('.').map_or(base, |(_, simple)| simple);

    simple.to_string()
}

/// Find the opening parenthesis of a method call or constructor
pub fn find_opening_paren(node: &hyperion_parser::syntax::SyntaxNode) -> Option<usize> {
    let text = node.text().to_string();
    let node_start: usize = node.text_range().start().into();

    // Find '(' in the node's text
    text.find('(').map(|local_pos| node_start + local_pos)
}

/// Count commas before position to determine parameter index
pub fn count_commas_before_position(source_text: &str, start: usize, end: usize) -> u32 {
    if start >= source_text.len() {
        return 0;
    }
    let end = end.min(source_text.len());
    if start >= end {
        return 0;
    }
    let text = &source_text[start..end];
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
    fn test_simplify_type_name() {
        assert_eq!(simplify_type_name("java.lang.String"), "String");
        assert_eq!(simplify_type_name("List<String>"), "List");
        assert_eq!(simplify_type_name("String[]"), "String");
        assert_eq!(
            simplify_type_name("java.util.List<java.lang.String>"),
            "List"
        );
    }

    #[test]
    fn test_count_commas_before_position() {
        // Matches the test pattern in type_utils.rs - start must include the opening '('
        let text = "(a, b, c)";
        assert_eq!(count_commas_before_position(text, 0, 9), 2); // Full text - 2 commas
        assert_eq!(count_commas_before_position(text, 0, 3), 1); // "(a," - 1 comma seen
        assert_eq!(count_commas_before_position(text, 0, 6), 2); // "(a, b," - 2 commas seen
    }

    #[test]
    fn test_count_commas_nested() {
        let text = "foo(a, bar(x, y), b)";
        // The nested commas in bar() shouldn't count
        assert_eq!(count_commas_before_position(text, 4, 18), 1);
    }

    #[test]
    fn test_count_commas_out_of_order_range() {
        let text = "foo(a, b)";
        assert_eq!(count_commas_before_position(text, 10, 2), 0);
    }
}
