// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Pattern-match exhaustiveness checking for Scala.
//!
//! This module checks whether pattern matches on sealed traits/classes
//! are exhaustive (cover all possible cases).
//!
//! # Example
//!
//! ```scala
//! sealed trait Result
//! case class Success(value: Int) extends Result
//! case class Error(msg: String) extends Result
//!
//! def handle(r: Result): String = r match {
//!   case Success(v) => s"Got $v"
//!   case Error(e) => s"Error: $e"
//!   // Exhaustive - no warning
//! }
//! ```
//!
//! If a case is missing, this module reports which cases are not covered.

use crate::parser::NodeExt;
use std::collections::HashSet;
use tracing::debug;
use tree_sitter::Node;

/// Result of checking a match expression for exhaustiveness
#[derive(Debug, Clone)]
pub struct ExhaustivenessCheckResult {
    /// The byte range of the match expression
    pub byte_range: (usize, usize),

    /// Whether the match is exhaustive
    pub is_exhaustive: bool,

    /// The sealed type being matched (if known)
    pub sealed_type: Option<String>,

    /// Missing case classes (empty if exhaustive)
    pub missing_cases: Vec<String>,

    /// Whether a wildcard pattern (_) or case _ is present
    pub has_wildcard: bool,
}

/// Information extracted from a match expression
#[derive(Debug)]
pub struct MatchExpressionInfo {
    /// Byte range of the match expression
    pub byte_range: (usize, usize),

    /// Type checks covered by patterns (case Success(...), case Error(...))
    pub covered_types: HashSet<String>,

    /// Whether there's a wildcard pattern (case _ => ...)
    pub has_wildcard: bool,
}

/// Extract information from a match expression node
///
/// This identifies:
/// 1. What patterns are matched (case Success, case Error, etc.)
/// 2. Whether there's a wildcard/default case
pub fn extract_match_info(match_node: &Node, source: &str) -> Option<MatchExpressionInfo> {
    if match_node.kind() != "match_expression" {
        debug!("Node is not a match_expression: {}", match_node.kind());
        return None;
    }

    let byte_range = (match_node.start_byte(), match_node.end_byte());
    let mut covered_types = HashSet::new();
    let mut has_wildcard = false;

    // Traverse the match expression looking for case clauses
    traverse_for_cases(match_node, source, &mut covered_types, &mut has_wildcard);

    Some(MatchExpressionInfo {
        byte_range,
        covered_types,
        has_wildcard,
    })
}

/// Recursively traverse nodes looking for case clauses
fn traverse_for_cases(
    node: &Node,
    source: &str,
    covered_types: &mut HashSet<String>,
    has_wildcard: &mut bool,
) {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return;
    }

    loop {
        let child = cursor.node();

        match child.kind() {
            "case_clause" => {
                extract_pattern_from_case(&child, source, covered_types, has_wildcard);
            }
            _ => {
                // Recurse into children
                traverse_for_cases(&child, source, covered_types, has_wildcard);
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extract the pattern from a case clause
///
/// Handles:
/// - case Success(x) => ...
/// - case Error(msg) => ...
/// - case _ => ... (wildcard)
fn extract_pattern_from_case(
    case_node: &Node,
    source: &str,
    covered_types: &mut HashSet<String>,
    has_wildcard: &mut bool,
) {
    let mut cursor = case_node.walk();

    if !cursor.goto_first_child() {
        return;
    }

    // Look for pattern nodes
    loop {
        let child = cursor.node();

        match child.kind() {
            // Constructor pattern: Success(x). tree-sitter-scala emits this as
            // `case_class_pattern`; `constructor_pattern` is kept for forward/
            // backward grammar compatibility. Typed pattern: `e: Error`.
            "case_class_pattern" | "constructor_pattern" | "typed_pattern" => {
                if let Some(type_name) = extract_type_from_pattern(&child, source) {
                    debug!("Found pattern type: {}", type_name);
                    covered_types.insert(type_name);
                }
            }

            // Identifier pattern: Success or Error (without constructor)
            "stable_identifier" | "identifier" => {
                let text = child.text(source);
                if text == "_" {
                    *has_wildcard = true;
                } else if text.chars().next().is_some_and(char::is_uppercase) {
                    // Capitalized identifier = likely a case object or type
                    debug!("Found identifier pattern: {}", text);
                    covered_types.insert(text.to_string());
                }
            }

            // Wildcard pattern
            "wildcard" => {
                *has_wildcard = true;
            }

            _ => {}
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extract the type name from a pattern
///
/// For constructor_pattern, extract the type being constructed
fn extract_type_from_pattern(pattern_node: &Node, source: &str) -> Option<String> {
    // Prefer the explicit `type` field. For a typed pattern `e: Error` the first
    // child is the lowercase binder `e`, not the matched type, so scanning for the
    // first identifier would wrongly return the binder.
    if let Some(type_node) = pattern_node.child_by_field_name("type") {
        let text = type_node.text(source);
        let simple_name = text.rsplit('.').next().unwrap_or(text);
        return Some(simple_name.to_string());
    }

    let mut cursor = pattern_node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    // Look for a type identifier. A bare `identifier` is never the matched type:
    // it is a binder (typed_pattern) or a constructor argument (case_class_pattern).
    loop {
        let child = cursor.node();

        match child.kind() {
            "type_identifier" | "stable_type_identifier" | "stable_identifier" => {
                // Extract simple name (last component if qualified, e.g. Result.Success).
                let text = child.text(source);
                let simple_name = text.rsplit('.').next().unwrap_or(text);
                return Some(simple_name.to_string());
            }
            _ => {}
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    None
}

/// Check exhaustiveness against a list of sealed subclasses
///
/// # Arguments
/// * `match_info` - Information extracted from the match expression
/// * `sealed_subclasses` - FQNs of all case classes extending the sealed trait
/// * `sealed_type_simple_name` - Simple name of the sealed trait (for matching)
///
/// # Returns
/// Result indicating whether the match is exhaustive and which cases are missing
pub fn check_exhaustiveness(
    match_info: &MatchExpressionInfo,
    sealed_subclasses: &[String],
    sealed_type_simple_name: &str,
) -> ExhaustivenessCheckResult {
    // If there's a wildcard, it's always exhaustive
    if match_info.has_wildcard {
        return ExhaustivenessCheckResult {
            byte_range: match_info.byte_range,
            is_exhaustive: true,
            sealed_type: Some(sealed_type_simple_name.to_string()),
            missing_cases: vec![],
            has_wildcard: true,
        };
    }

    // Check which subclasses are covered
    let mut missing = Vec::new();

    for subclass_fqn in sealed_subclasses {
        // Extract simple name from FQN
        // e.g., "com.example.Result.Success" -> "Success"
        let subclass_simple = subclass_fqn.rsplit('.').next().unwrap_or(subclass_fqn);

        // Check if this case is covered
        // We check:
        // 1. Simple name: "Success"
        // 2. Full FQN: "com.example.Result.Success"
        // 3. Qualified with parent: "Result.Success"
        let is_covered = match_info.covered_types.contains(subclass_simple)
            || match_info.covered_types.contains(subclass_fqn)
            || match_info.covered_types.iter().any(|t| {
                t.ends_with(subclass_simple)
                    && (t == &format!("{sealed_type_simple_name}.{subclass_simple}")
                        || t.ends_with(&format!(".{subclass_simple}")))
            });

        if !is_covered {
            missing.push(subclass_simple.to_string());
        }
    }

    ExhaustivenessCheckResult {
        byte_range: match_info.byte_range,
        is_exhaustive: missing.is_empty(),
        sealed_type: Some(sealed_type_simple_name.to_string()),
        missing_cases: missing,
        has_wildcard: false,
    }
}

/// Find all match expressions in a syntax tree and return their byte ranges
///
/// Returns the byte ranges of match expressions rather than Node references
/// to avoid lifetime issues with tree-sitter nodes.
pub fn find_match_expression_ranges(root: &Node, _source: &str) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    find_match_ranges_recursive(root, &mut result);
    result
}

fn find_match_ranges_recursive(node: &Node, result: &mut Vec<(usize, usize)>) {
    if node.kind() == "match_expression" {
        result.push((node.start_byte(), node.end_byte()));
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            find_match_ranges_recursive(&child, result);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    struct ParsedSource {
        match_count: usize,
        first_match_info: Option<MatchExpressionInfo>,
    }

    fn parse_and_extract(source: &str) -> ParsedSource {
        let parse_result = parse(source).expect("Parse failed");
        let root = parse_result.tree.root_node();

        // Find match expressions using helper
        let match_ranges = find_match_expression_ranges(&root, source);
        let match_count = match_ranges.len();

        // We need to re-traverse to find the first match node. The range was
        // produced from this tree, so it is safe to reuse within this scope.
        let first_match_info = match_ranges
            .first()
            .and_then(|&range| extract_first_match_info(&root, source, range));

        ParsedSource {
            match_count,
            first_match_info,
        }
    }

    // Helper to extract info from the first match expression
    fn extract_first_match_info(
        root: &Node,
        source: &str,
        range: (usize, usize),
    ) -> Option<MatchExpressionInfo> {
        // Find the match node and extract info in one pass
        fn search_and_extract(
            node: &Node,
            source: &str,
            target_range: (usize, usize),
        ) -> Option<MatchExpressionInfo> {
            if node.kind() == "match_expression"
                && node.start_byte() == target_range.0
                && node.end_byte() == target_range.1
            {
                return extract_match_info(node, source);
            }

            let mut cursor = node.walk();
            if cursor.goto_first_child() {
                loop {
                    if let Some(info) = search_and_extract(&cursor.node(), source, target_range) {
                        return Some(info);
                    }
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }

            None
        }

        search_and_extract(root, source, range)
    }

    #[test]
    fn test_find_match_expressions() {
        let source = r#"
def handle(r: Result): String = r match {
    case Success(v) => "ok"
    case Error(e) => "fail"
}
"#;
        let parsed = parse_and_extract(source);
        assert_eq!(parsed.match_count, 1);
    }

    #[test]
    fn test_extract_match_info_simple() {
        let source = r#"
def test(result: Result): Unit = result match {
    case Success(x) => println("ok")
    case Error(msg) => println("fail")
}
"#;
        let parsed = parse_and_extract(source);
        assert_eq!(parsed.match_count, 1);

        let info = parsed.first_match_info.expect("Should extract info");
        assert!(
            info.covered_types.contains("Success"),
            "Missing Success in covered_types: {:?}",
            info.covered_types
        );
        assert!(
            info.covered_types.contains("Error"),
            "Missing Error in covered_types: {:?}",
            info.covered_types
        );
        assert!(!info.has_wildcard);
    }

    #[test]
    fn test_extract_match_info_typed_patterns() {
        // `case s: Success` binds `s` and matches type `Success`; the covered type
        // must be the type name, not the binder identifier.
        let source = r#"
def test(result: Result): Unit = result match {
    case s: Success => println("ok")
    case e: Error => println("fail")
}
"#;
        let parsed = parse_and_extract(source);
        let info = parsed.first_match_info.expect("Should extract info");
        assert!(
            info.covered_types.contains("Success"),
            "Missing Success in covered_types: {:?}",
            info.covered_types
        );
        assert!(
            info.covered_types.contains("Error"),
            "Missing Error in covered_types: {:?}",
            info.covered_types
        );
        assert!(
            !info.covered_types.contains("s") && !info.covered_types.contains("e"),
            "binder identifiers must not be recorded as covered types: {:?}",
            info.covered_types
        );
    }

    #[test]
    fn test_extract_match_info_with_wildcard() {
        let source = r#"
def test(x: Any): Unit = x match {
    case Success(v) => println("ok")
    case _ => println("default")
}
"#;
        let parsed = parse_and_extract(source);
        assert_eq!(parsed.match_count, 1);

        let info = parsed.first_match_info.expect("Should extract info");
        assert!(info.has_wildcard);
    }

    #[test]
    fn test_exhaustiveness_check_complete() {
        let match_info = MatchExpressionInfo {
            byte_range: (0, 100),
            covered_types: ["Success", "Error"].iter().map(|s| s.to_string()).collect(),
            has_wildcard: false,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&match_info, &sealed_subclasses, "Result");
        assert!(result.is_exhaustive);
        assert!(result.missing_cases.is_empty());
    }

    #[test]
    fn test_exhaustiveness_check_incomplete() {
        let match_info = MatchExpressionInfo {
            byte_range: (0, 100),
            covered_types: std::iter::once("Success".to_string()).collect(),
            has_wildcard: false,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&match_info, &sealed_subclasses, "Result");
        assert!(!result.is_exhaustive);
        assert_eq!(result.missing_cases, vec!["Error"]);
    }

    #[test]
    fn test_exhaustiveness_with_wildcard_is_always_exhaustive() {
        let match_info = MatchExpressionInfo {
            byte_range: (0, 100),
            covered_types: HashSet::new(), // No specific cases
            has_wildcard: true,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&match_info, &sealed_subclasses, "Result");
        assert!(result.is_exhaustive);
        assert!(result.has_wildcard);
    }

    #[test]
    fn test_qualified_type_matching() {
        let match_info = MatchExpressionInfo {
            byte_range: (0, 100),
            covered_types: ["Result.Success", "Result.Error"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            has_wildcard: false,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&match_info, &sealed_subclasses, "Result");
        assert!(result.is_exhaustive);
    }
}
