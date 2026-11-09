// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type extraction from Scala CST nodes.
//!
//! This module extracts type information from tree-sitter CST nodes,
//! supporting:
//! - Simple types: `String`, `Int`, `Boolean`
//! - Generic types: `List[String]`, `Map[Int, String]`
//! - Function types: `(Int, String) => Boolean`
//! - Tuple types: `(Int, String, Boolean)`
//! - Union types (Scala 3): `String | Int`

use tracing::debug;
use tree_sitter::Node;

use crate::parser::NodeExt;

/// Extract type from a node
///
/// This is the main entry point for type extraction. It handles:
/// - type_identifier: Simple types like `String`
/// - parameterized_type: Generic types like `List[String]`
/// - function_type: Function types like `(Int, String) => Boolean`
/// - tuple_type: Tuple types like `(Int, String)`
/// - infix_type: Union types like `String | Int`
pub fn extract_type(node: &Node, source: &str) -> Option<String> {
    match node.kind() {
        "type_identifier" => Some(node.text(source).to_string()),
        "parameterized_type" => extract_parameterized_type(node, source),
        "function_type" => extract_function_type(node, source),
        "tuple_type" => extract_tuple_type(node, source),
        "infix_type" => extract_infix_type(node, source),
        _ => {
            // For unknown type nodes, try to find a type child recursively
            debug!("Unknown type node kind: {}", node.kind());
            find_type_in_children(node, source)
        }
    }
}

/// Extract parameterized/generic type (e.g., `List[String]`, `Map[Int, String]`)
///
/// Structure in tree-sitter:
/// ```text
/// parameterized_type
///   type_identifier: "List"
///   type_arguments
///     type_identifier: "String"
/// ```
fn extract_parameterized_type(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    // First child should be the base type
    let base_type = cursor.node().text(source).to_string();

    // Look for type_arguments
    while cursor.goto_next_sibling() {
        let child = cursor.node();
        if child.kind() == "type_arguments" {
            if let Some(args) = extract_type_arguments(&child, source) {
                return Some(format!("{base_type}[{}]", args.join(", ")));
            }
        }
    }

    // No type arguments found - return simple type
    Some(base_type)
}

/// Extract type arguments from type_arguments node
///
/// Handles comma-separated type parameters: `[String, Int]`
fn extract_type_arguments(node: &Node, source: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    loop {
        let child = cursor.node();

        // Skip punctuation (brackets, commas)
        if child.kind() != "[" && child.kind() != "]" && child.kind() != "," {
            if let Some(arg_type) = extract_type(&child, source) {
                args.push(arg_type);
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

/// Extract function type (e.g., `(Int, String) => Boolean`)
///
/// Structure in tree-sitter:
/// ```text
/// function_type
///   tuple_type  // parameter types
///     type_identifier: "Int"
///     type_identifier: "String"
///   "=>"
///   type_identifier: "Boolean"  // return type
/// ```
fn extract_function_type(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    let mut param_types: Option<String> = None;
    let mut return_type: Option<String> = None;
    let mut found_arrow = false;

    loop {
        let child = cursor.node();

        match child.kind() {
            "=>" => {
                found_arrow = true;
            }
            "tuple_type" if !found_arrow => {
                // Parameter types (before =>)
                param_types = extract_tuple_type(&child, source);
            }
            _ if found_arrow => {
                // Return type (after =>)
                return_type = extract_type(&child, source);
            }
            _ if !found_arrow && param_types.is_none() => {
                // Single parameter type (not a tuple)
                param_types = extract_type(&child, source).map(|t| format!("({t})"));
            }
            _ => {}
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    match (param_types, return_type) {
        (Some(params), Some(ret)) => Some(format!("{params} => {ret}")),
        _ => None,
    }
}

/// Extract tuple type (e.g., `(Int, String, Boolean)`)
///
/// Structure in tree-sitter:
/// ```text
/// tuple_type
///   "("
///   type_identifier: "Int"
///   ","
///   type_identifier: "String"
///   ")"
/// ```
fn extract_tuple_type(node: &Node, source: &str) -> Option<String> {
    let mut types = Vec::new();
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    loop {
        let child = cursor.node();

        // Skip punctuation (parens, commas)
        if child.kind() != "(" && child.kind() != ")" && child.kind() != "," {
            if let Some(typ) = extract_type(&child, source) {
                types.push(typ);
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    if types.is_empty() {
        None
    } else {
        Some(format!("({})", types.join(", ")))
    }
}

/// Extract infix type (Scala 3 union types: `String | Int`)
///
/// Structure in tree-sitter:
/// ```text
/// infix_type
///   type_identifier: "String"
///   operator: "|"
///   type_identifier: "Int"
/// ```
fn extract_infix_type(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    let mut left_type: Option<String> = None;
    let mut operator: Option<String> = None;
    let mut right_type: Option<String> = None;

    loop {
        let child = cursor.node();

        if child.kind() == "operator" {
            operator = Some(child.text(source).to_string());
        } else if left_type.is_none() {
            left_type = extract_type(&child, source);
        } else if operator.is_some() {
            right_type = extract_type(&child, source);
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    match (left_type, operator, right_type) {
        (Some(left), Some(op), Some(right)) => Some(format!("{left} {op} {right}")),
        _ => None,
    }
}

/// Find type in children recursively (fallback for unknown node types)
fn find_type_in_children(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    loop {
        let child = cursor.node();
        if let Some(typ) = extract_type(&child, source) {
            return Some(typ);
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    None
}
