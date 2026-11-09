// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Exhaustiveness checking for Kotlin when expressions on sealed classes
//!
//! # Overview
//!
//! When a `when` expression is used on a sealed class/interface, Kotlin requires
//! that all subclasses are covered by branches. This module implements checking
//! for that requirement.
//!
//! # Example
//!
//! ```kotlin
//! sealed class Result {
//!     data class Success(val data: String) : Result()
//!     data class Error(val msg: String) : Result()
//! }
//!
//! fun handle(r: Result) = when (r) {
//!     is Result.Success -> "ok"
//!     is Result.Error -> "fail"
//!     // No else needed - exhaustive!
//! }
//! ```
//!
//! If a branch is missing, this module will report the missing subclasses.

use crate::{SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, TextRange};
use std::collections::HashSet;

/// Result of checking a when expression for exhaustiveness
#[derive(Debug, Clone)]
pub struct ExhaustivenessCheckResult {
    /// The range of the when expression
    pub when_range: TextRange,
    /// Whether the when expression is exhaustive
    pub is_exhaustive: bool,
    /// The sealed type being matched (if known)
    pub sealed_type: Option<String>,
    /// Missing subclasses (empty if exhaustive)
    pub missing_branches: Vec<String>,
    /// Whether an else branch is present
    pub has_else_branch: bool,
}

/// Extract information about a when expression for exhaustiveness checking
///
/// This extracts:
/// 1. The subject expression (what's being matched)
/// 2. The covered branches (is Type checks)
/// 3. Whether there's an else branch
pub struct WhenExpressionInfo {
    /// The range of the when expression
    pub range: TextRange,
    /// The subject variable name (if simple identifier)
    pub subject_name: Option<String>,
    /// Type checks covered by branches (from "is Type" patterns)
    pub covered_types: HashSet<String>,
    /// Whether there's an else branch (makes it automatically exhaustive)
    pub has_else_branch: bool,
}

/// Extract when expression info from a WHEN_EXPR node
pub fn extract_when_info(when_node: &SyntaxNode) -> Option<WhenExpressionInfo> {
    if when_node.kind() != SyntaxKind::WHEN_EXPR {
        return None;
    }

    let range = when_node.text_range();
    let mut subject_name = None;
    let mut covered_types = HashSet::new();
    let mut has_else_branch = false;
    let mut in_subject = false;

    // Find subject
    for child in when_node.children_with_tokens() {
        if let NodeOrToken::Token(token) = child {
            match token.kind() {
                SyntaxKind::L_PAREN => in_subject = true,
                SyntaxKind::R_PAREN => in_subject = false,
                SyntaxKind::IDENT if in_subject && subject_name.is_none() => {
                    subject_name = Some(token.text().to_string());
                }
                _ => {}
            }
        }
    }

    // Due to parser quirks with WHEN_ENTRY boundaries, scan all tokens directly
    // for "is Type" patterns and "else" keyword
    let mut found_is = false;
    let mut type_parts = Vec::new();
    let mut in_body = false; // After L_BRACE

    for token_or_node in when_node.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = token_or_node {
            // Track when we're inside the when body
            if token.kind() == SyntaxKind::L_BRACE {
                in_body = true;
            }

            // Only process tokens inside the when body
            if !in_body {
                continue;
            }

            match token.kind() {
                SyntaxKind::ELSE_KW => {
                    // Save any pending type
                    if found_is && !type_parts.is_empty() {
                        covered_types.insert(type_parts.join("."));
                        type_parts.clear();
                    }
                    found_is = false;
                    has_else_branch = true;
                }
                SyntaxKind::IS_KW => {
                    // Save previous type if any
                    if found_is && !type_parts.is_empty() {
                        covered_types.insert(type_parts.join("."));
                        type_parts.clear();
                    }
                    found_is = true;
                }
                SyntaxKind::IDENT if found_is => {
                    type_parts.push(token.text().to_string());
                }
                SyntaxKind::DOT if found_is => {
                    // Keep collecting for qualified names
                }
                SyntaxKind::ARROW => {
                    // End of condition, save type if we have one
                    if found_is && !type_parts.is_empty() {
                        covered_types.insert(type_parts.join("."));
                        type_parts.clear();
                    }
                    found_is = false;
                }
                SyntaxKind::COMMA if found_is => {
                    // Multiple conditions: is A, is B -> ...
                    if !type_parts.is_empty() {
                        covered_types.insert(type_parts.join("."));
                        type_parts.clear();
                    }
                    found_is = false;
                }
                _ => {}
            }
        }
    }

    // Don't forget last type
    if found_is && !type_parts.is_empty() {
        covered_types.insert(type_parts.join("."));
    }

    Some(WhenExpressionInfo {
        range,
        subject_name,
        covered_types,
        has_else_branch,
    })
}

/// Check exhaustiveness of a when expression against sealed class subclasses
///
/// # Arguments
/// * `when_info` - Information extracted from the when expression
/// * `sealed_subclasses` - All subclasses of the sealed type
/// * `sealed_type_simple_name` - Simple name of the sealed type (for qualified name matching)
///
/// # Returns
/// The check result indicating whether the expression is exhaustive
pub fn check_exhaustiveness(
    when_info: &WhenExpressionInfo,
    sealed_subclasses: &[String],
    sealed_type_simple_name: &str,
) -> ExhaustivenessCheckResult {
    // If there's an else branch, it's always exhaustive
    if when_info.has_else_branch {
        return ExhaustivenessCheckResult {
            when_range: when_info.range,
            is_exhaustive: true,
            sealed_type: Some(sealed_type_simple_name.to_string()),
            missing_branches: vec![],
            has_else_branch: true,
        };
    }

    // Check which subclasses are covered
    let mut missing = Vec::new();

    for subclass_fqn in sealed_subclasses {
        // Extract simple name from FQN
        let subclass_simple = subclass_fqn.split('.').next_back().unwrap_or(subclass_fqn);

        // Check various ways the branch might be specified:
        // 1. Just the simple name: "Success"
        // 2. Qualified with parent: "Result.Success"
        // 3. Full FQN: "com.example.Result.Success"
        let is_covered = when_info.covered_types.contains(subclass_simple)
            || when_info.covered_types.contains(subclass_fqn)
            || when_info.covered_types.iter().any(|t| {
                // Check for Parent.Child pattern
                t.ends_with(subclass_simple)
                    && (t == &format!("{sealed_type_simple_name}.{subclass_simple}")
                        || t.ends_with(&format!(".{subclass_simple}")))
            });

        if !is_covered {
            missing.push(subclass_simple.to_string());
        }
    }

    ExhaustivenessCheckResult {
        when_range: when_info.range,
        is_exhaustive: missing.is_empty(),
        sealed_type: Some(sealed_type_simple_name.to_string()),
        missing_branches: missing,
        has_else_branch: false,
    }
}

/// Find all WHEN_EXPR nodes in a syntax tree
pub fn find_when_expressions(root: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut result = Vec::new();
    find_when_expressions_recursive(root, &mut result);
    result
}

fn find_when_expressions_recursive(node: &SyntaxNode, result: &mut Vec<SyntaxNode>) {
    if node.kind() == SyntaxKind::WHEN_EXPR {
        result.push(node.clone());
    }

    for child in node.children() {
        find_when_expressions_recursive(&child, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lexer, Parser};

    fn parse_and_find_when(source: &str) -> Vec<SyntaxNode> {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);
        find_when_expressions(&root)
    }

    #[test]
    fn test_find_when_expressions() {
        let source = r#"
fun handle(r: Result) = when (r) {
    is Success -> "ok"
    is Error -> "fail"
}
"#;
        let when_exprs = parse_and_find_when(source);
        assert_eq!(when_exprs.len(), 1);
    }

    #[test]
    fn test_extract_when_info_with_subject() {
        let source = r#"
fun test(result: Result) {
    when (result) {
        is Success -> println("ok")
        is Error -> println("fail")
    }
}
"#;
        let when_exprs = parse_and_find_when(source);
        assert_eq!(when_exprs.len(), 1);

        let info = extract_when_info(&when_exprs[0]).expect("Should extract info");
        assert_eq!(info.subject_name, Some("result".to_string()));
        assert!(info.covered_types.contains("Success"), "Missing Success");
        assert!(info.covered_types.contains("Error"), "Missing Error");
        assert!(!info.has_else_branch);
    }

    #[test]
    fn test_extract_when_info_with_else() {
        let source = r#"
fun test(x: Any) {
    when (x) {
        is Success -> println("ok")
        else -> println("default")
    }
}
"#;
        let when_exprs = parse_and_find_when(source);
        assert_eq!(when_exprs.len(), 1);

        let info = extract_when_info(&when_exprs[0]).expect("Should extract info");
        assert!(info.has_else_branch);
    }

    #[test]
    fn test_exhaustiveness_check_complete() {
        let when_info = WhenExpressionInfo {
            range: TextRange::new(0.into(), 100.into()),
            subject_name: Some("result".to_string()),
            covered_types: ["Success", "Error"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            has_else_branch: false,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&when_info, &sealed_subclasses, "Result");
        assert!(result.is_exhaustive);
        assert!(result.missing_branches.is_empty());
    }

    #[test]
    fn test_exhaustiveness_check_incomplete() {
        let when_info = WhenExpressionInfo {
            range: TextRange::new(0.into(), 100.into()),
            subject_name: Some("result".to_string()),
            covered_types: std::iter::once("Success".to_string()).collect(),
            has_else_branch: false,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&when_info, &sealed_subclasses, "Result");
        assert!(!result.is_exhaustive);
        assert_eq!(result.missing_branches, vec!["Error"]);
    }

    #[test]
    fn test_exhaustiveness_with_else_is_always_exhaustive() {
        let when_info = WhenExpressionInfo {
            range: TextRange::new(0.into(), 100.into()),
            subject_name: Some("result".to_string()),
            covered_types: HashSet::new(), // No type checks at all
            has_else_branch: true,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&when_info, &sealed_subclasses, "Result");
        assert!(result.is_exhaustive);
        assert!(result.has_else_branch);
    }

    #[test]
    fn test_qualified_type_matching() {
        let when_info = WhenExpressionInfo {
            range: TextRange::new(0.into(), 100.into()),
            subject_name: Some("result".to_string()),
            covered_types: ["Result.Success", "Result.Error"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            has_else_branch: false,
        };

        let sealed_subclasses = vec![
            "com.example.Result.Success".to_string(),
            "com.example.Result.Error".to_string(),
        ];

        let result = check_exhaustiveness(&when_info, &sealed_subclasses, "Result");
        assert!(result.is_exhaustive);
    }
}
