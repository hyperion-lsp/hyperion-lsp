// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method call pattern matching for sink detection
//!
//! This module extracts method call patterns from nested METHOD_CALL_EXPR nodes
//! where receiver, DOT, IDENT (method name), and ARG_LIST are direct children.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// A matched method call pattern: receiver.method(args)
#[derive(Debug)]
pub struct MethodCallPattern<'a> {
    pub receiver_name: Option<String>,
    pub method_name: String,
    pub call_node: &'a SyntaxNode,
    pub receiver_node: Option<SyntaxNode>,
}

/// Extract a method call pattern from a nested METHOD_CALL_EXPR node.
///
/// With nested expression trees, METHOD_CALL_EXPR contains:
/// ```text
/// METHOD_CALL_EXPR
///   receiver_expr  (NAME_EXPR | METHOD_CALL_EXPR | NEW_EXPR | FIELD_ACCESS_EXPR)
///   DOT
///   IDENT          (method name)
///   ARG_LIST
/// ```
///
/// For unqualified calls (no receiver), there's just:
/// ```text
/// METHOD_CALL_EXPR
///   NAME_EXPR("methodName")
///   ARG_LIST
/// ```
pub fn extract_from_nested(node: &SyntaxNode) -> Option<MethodCallPattern<'_>> {
    if node.kind() != SyntaxKind::METHOD_CALL_EXPR {
        return None;
    }

    let mut method_name: Option<String> = None;
    let mut found_dot = false;
    let mut has_arg_list = false;

    let child_nodes: Vec<_> = node.children().collect();

    for elem in node.children_with_tokens() {
        match elem {
            rowan::NodeOrToken::Node(ref child) => {
                if child.kind() == SyntaxKind::ARG_LIST {
                    has_arg_list = true;
                }
            }
            rowan::NodeOrToken::Token(ref token) => {
                if token.kind() == SyntaxKind::DOT {
                    found_dot = true;
                } else if found_dot && token.kind().is_identifier() {
                    method_name = Some(token.text().to_string());
                }
            }
        }
    }

    if !has_arg_list {
        return None;
    }

    // Find receiver node (first expression child)
    let receiver_ref = child_nodes.iter().find(|c| is_expr_kind(c.kind()));

    if found_dot {
        // Qualified call: receiver.method(args)
        let method = method_name?;
        let receiver_name = receiver_ref.and_then(extract_receiver_name);

        Some(MethodCallPattern {
            receiver_name,
            method_name: method,
            call_node: node,
            receiver_node: node.children().find(|c| is_expr_kind(c.kind())),
        })
    } else {
        // Unqualified call: method(args) — NAME_EXPR contains method name
        let name_expr = receiver_ref?;
        let name = extract_name_expr_ident(name_expr)?;

        Some(MethodCallPattern {
            receiver_name: None,
            method_name: name,
            call_node: node,
            receiver_node: None,
        })
    }
}

const fn is_expr_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NAME_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::CAST_EXPR
    )
}

/// Extract a human-readable receiver name from a receiver expression node.
fn extract_receiver_name(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::NAME_EXPR => extract_name_expr_ident(node),
        SyntaxKind::FIELD_ACCESS_EXPR => extract_field_access_ident(node),
        SyntaxKind::NEW_EXPR => extract_new_expr_class_name(node),
        SyntaxKind::METHOD_CALL_EXPR => {
            // Chained call: infer type from method name
            infer_type_from_chained_method(node)
        }
        SyntaxKind::THIS_EXPR => Some("this".to_string()),
        SyntaxKind::SUPER_EXPR => Some("super".to_string()),
        _ => None,
    }
}

/// Infer receiver type from a chained METHOD_CALL_EXPR (the inner call).
/// Maps common factory method names to their return types.
fn infer_type_from_chained_method(node: &SyntaxNode) -> Option<String> {
    // Extract the method name from this nested METHOD_CALL_EXPR
    let method_name = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind().is_identifier())
        .map(|t| t.text().to_string())?;

    match method_name.as_str() {
        "getConnection" => Some("Connection".to_string()),
        "createStatement" => Some("Statement".to_string()),
        "prepareStatement" | "prepareCall" => Some("PreparedStatement".to_string()),
        _ => {
            // Fall back to the chain root (deepest NAME_EXPR receiver)
            find_chain_root(node)
        }
    }
}

/// Walk down nested receivers to find the root NAME_EXPR identifier.
/// For `foo.bar().baz()`, returns "foo".
pub fn find_chain_root(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::NAME_EXPR => return extract_name_expr_ident(&child),
            SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::FIELD_ACCESS_EXPR => {
                return find_chain_root(&child);
            }
            _ => {}
        }
    }
    // If no nested receiver, try IDENT token (unqualified call as chain root)
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind().is_identifier())
        .map(|t| t.text().to_string())
}

fn extract_field_access_ident(node: &SyntaxNode) -> Option<String> {
    // With nested trees, FIELD_ACCESS_EXPR contains receiver + DOT + IDENT.
    //
    // For simple field receivers like `System.out`, the final IDENT (`out`) is the right
    // receiver to match sink patterns like `out.println(...)`.
    //
    // For chained call receivers like `success(this).feedback("...")`, the FIELD_ACCESS_EXPR
    // includes nested METHOD_CALL_EXPR nodes; in that case, prefer the chain root (`success`)
    // so that downstream calls like `.output(...)` can match sink patterns.
    let has_nested_calls = node
        .descendants()
        .any(|d| d.kind() == SyntaxKind::METHOD_CALL_EXPR);
    if has_nested_calls {
        return find_chain_root(node);
    }

    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|t| t.kind().is_identifier())
        .last()
        .map(|t| t.text().to_string())
}

fn extract_new_expr_class_name(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::CLASS_TYPE {
            return child
                .descendants_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|t| t.kind().is_identifier())
                .last()
                .map(|t| t.text().to_string());
        }
    }
    None
}

/// Extracts the identifier from a NAME_EXPR node
fn extract_name_expr_ident(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind().is_identifier())
        .map(|t| t.text().to_string())
}

/// Find all METHOD_CALL_EXPR nodes in a subtree (including the node itself).
/// With nested trees, chained calls appear as nested METHOD_CALL_EXPR nodes.
pub fn find_method_calls(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut result = Vec::new();
    collect_method_calls(node, &mut result);
    result
}

fn collect_method_calls(node: &SyntaxNode, result: &mut Vec<SyntaxNode>) {
    if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
        result.push(node.clone());
    }
    for child in node.children() {
        collect_method_calls(&child, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    #[test]
    fn contextual_keyword_receivers_keep_their_identity() {
        for name in ["record", "var", "yield", "sealed", "permits"] {
            let source =
                format!("class Test {{ void run(ConsumerRecord {name}) {{ {name}.key(); }} }}");
            let tokens = Lexer::new(&source).collect();
            let (green, errors) = Parser::parse(tokens);
            assert!(errors.is_empty(), "{name}: {errors:?}");
            let root = SyntaxNode::new_root(green);
            let calls = find_method_calls(&root);
            assert_eq!(calls.len(), 1, "{name}");
            let pattern = extract_from_nested(&calls[0]).expect("qualified call");
            assert_eq!(pattern.receiver_name.as_deref(), Some(name));
            assert_eq!(pattern.method_name, "key");
        }
    }
}
