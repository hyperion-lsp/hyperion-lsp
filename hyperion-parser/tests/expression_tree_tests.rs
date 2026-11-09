// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for binary expression tree structure
//!
//! Verifies that binary expressions produce properly nested/structured trees.
//! The parser uses checkpoint-based precedence climbing:
//! - Different precedence levels produce nested BINARY_EXPR (e.g., a + b * c)
//! - Same precedence levels produce sibling BINARY_EXPR nodes where the
//!   continuation node contains only the operator and right operand
//!   (left operand is the preceding sibling BINARY_EXPR)

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

fn parse(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green_node, _) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

/// Find the first node of a given kind in the tree
fn find_first(root: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    root.descendants().find(|n| n.kind() == kind)
}

/// Extract child element kinds (excluding whitespace tokens)
fn child_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
    node.children_with_tokens()
        .filter(|c| c.kind() != SyntaxKind::WHITESPACE)
        .map(|c| c.kind())
        .collect()
}

// ============================================================================
// Simple binary expression: a + b
// ============================================================================

#[test]
fn binary_expr_has_two_operands() {
    let root = parse("class T { int x = a + b; }");
    let binary = find_first(&root, SyntaxKind::BINARY_EXPR).expect("should have BINARY_EXPR");

    let kinds = child_kinds(&binary);
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::NAME_EXPR,
            SyntaxKind::PLUS,
            SyntaxKind::NAME_EXPR
        ]
    );
}

#[test]
fn binary_expr_operand_text() {
    let root = parse("class T { int x = a + b; }");
    let binary = find_first(&root, SyntaxKind::BINARY_EXPR).expect("should have BINARY_EXPR");

    let children: Vec<_> = binary.children().collect();
    assert_eq!(children.len(), 2, "BINARY_EXPR should have 2 child nodes");
    assert_eq!(children[0].text().to_string().trim(), "a");
    assert_eq!(children[1].text().to_string().trim(), "b");
}

// ============================================================================
// Same-precedence chaining: a + b + c
// ============================================================================

#[test]
fn same_precedence_produces_sibling_binary_exprs() {
    // a + b + c produces two sibling BINARY_EXPR nodes:
    //   BINARY_EXPR [NAME_EXPR(a), PLUS, NAME_EXPR(b)]
    //   BINARY_EXPR [PLUS, NAME_EXPR(c)]  (left operand is preceding sibling)
    let root = parse("class T { int x = a + b + c; }");

    let binary_exprs: Vec<_> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::BINARY_EXPR)
        .collect();

    assert_eq!(
        binary_exprs.len(),
        2,
        "a + b + c should produce 2 BINARY_EXPR nodes"
    );

    // First: a + b (has both operands)
    let first_kinds = child_kinds(&binary_exprs[0]);
    assert_eq!(
        first_kinds,
        vec![
            SyntaxKind::NAME_EXPR,
            SyntaxKind::PLUS,
            SyntaxKind::NAME_EXPR
        ]
    );

    // Second: + c (continuation — operator and right operand only)
    let second_kinds = child_kinds(&binary_exprs[1]);
    assert_eq!(second_kinds, vec![SyntaxKind::PLUS, SyntaxKind::NAME_EXPR]);

    // Both should be siblings under the same parent
    assert_eq!(
        binary_exprs[0].parent().map(|p| p.kind()),
        binary_exprs[1].parent().map(|p| p.kind()),
        "both BINARY_EXPR nodes should share the same parent"
    );
}

// ============================================================================
// Different precedence: a + b * c (nested tree)
// ============================================================================

#[test]
fn precedence_mul_over_add() {
    // a + b * c should produce:
    // BINARY_EXPR[NAME_EXPR(a), PLUS, BINARY_EXPR[NAME_EXPR(b), STAR, NAME_EXPR(c)]]
    let root = parse("class T { int x = a + b * c; }");

    let outer = find_first(&root, SyntaxKind::BINARY_EXPR).expect("should have BINARY_EXPR");

    let outer_kinds = child_kinds(&outer);
    assert_eq!(
        outer_kinds[0],
        SyntaxKind::NAME_EXPR,
        "left should be NAME_EXPR(a)"
    );
    assert_eq!(outer_kinds[1], SyntaxKind::PLUS);
    assert_eq!(
        outer_kinds[2],
        SyntaxKind::BINARY_EXPR,
        "right should be BINARY_EXPR(b*c)"
    );

    // Inner uses STAR (higher precedence, genuinely nested)
    let inner = outer
        .children()
        .find(|c| c.kind() == SyntaxKind::BINARY_EXPR)
        .expect("should have inner BINARY_EXPR");
    let inner_kinds = child_kinds(&inner);
    assert_eq!(inner_kinds[1], SyntaxKind::STAR);
}

// ============================================================================
// Comparison operators
// ============================================================================

#[test]
fn comparison_expr_structure() {
    let root = parse("class T { boolean x = a == b; }");
    let binary = find_first(&root, SyntaxKind::BINARY_EXPR).expect("should have BINARY_EXPR");

    let kinds = child_kinds(&binary);
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::NAME_EXPR,
            SyntaxKind::EQ_EQ,
            SyntaxKind::NAME_EXPR
        ]
    );
}

// ============================================================================
// Logical operators: && binds tighter than ||
// ============================================================================

#[test]
fn logical_and_or_precedence() {
    // a || b && c: && has higher precedence so should be nested inside ||
    let root = parse("class T { boolean x = a || b && c; }");

    let outer = find_first(&root, SyntaxKind::BINARY_EXPR).expect("should have BINARY_EXPR");

    let outer_kinds = child_kinds(&outer);
    assert_eq!(outer_kinds[0], SyntaxKind::NAME_EXPR);
    assert_eq!(outer_kinds[1], SyntaxKind::PIPE_PIPE);
    assert_eq!(
        outer_kinds[2],
        SyntaxKind::BINARY_EXPR,
        "&& should bind tighter than ||"
    );
}

// ============================================================================
// Method call chain: nested METHOD_CALL_EXPR
// ============================================================================

#[test]
fn method_call_chain_nested_tree() {
    // getUser().getName() should have nested METHOD_CALL_EXPR
    let root = parse("class T { void m() { getUser().getName(); } }");

    // Find the METHOD_CALL_EXPR that has a DOT child (the chained call)
    let outer = root
        .descendants()
        .find(|n| {
            n.kind() == SyntaxKind::METHOD_CALL_EXPR
                && n.children_with_tokens()
                    .any(|c| c.kind() == SyntaxKind::DOT)
        })
        .expect("should have outer METHOD_CALL_EXPR with DOT");

    // It should contain an inner METHOD_CALL_EXPR (getUser())
    let has_inner_call = outer
        .children()
        .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR);
    assert!(
        has_inner_call,
        "outer call should contain inner METHOD_CALL_EXPR"
    );

    // The outer should have IDENT "getName"
    let has_get_name = outer.children_with_tokens().any(|c| {
        c.as_token()
            .is_some_and(|t| t.kind() == SyntaxKind::IDENT && t.text() == "getName")
    });
    assert!(has_get_name, "outer call should have IDENT 'getName'");
}
