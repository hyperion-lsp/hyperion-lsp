// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for annotations on local variable declarations
//!
//! Java allows annotations on local variables (JSR 308, Java 8+):
//! ```java
//! @SuppressWarnings("unchecked") List<String> list = new ArrayList<>();
//! @Nullable String name = null;
//! ```
//!
//! The parser must recognize these as LOCAL_VAR_DECL nodes with ANNOTATION children,
//! NOT as separate EXPR_STMT + LOCAL_VAR_DECL nodes.

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

fn parse_ok(source: &str) -> SyntaxNode {
    let (node, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");
    node
}

/// Helper to find all nodes of a specific kind
fn find_nodes(root: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    root.descendants().filter(|n| n.kind() == kind).collect()
}

/// Helper to check if a node has a child of a specific kind
fn has_child_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children().any(|c| c.kind() == kind)
}

#[test]
fn test_annotated_local_variable_simple() {
    let source = r#"
        class Test {
            void test() {
                @SuppressWarnings("unchecked")
                Object obj = new Object();
            }
        }
    "#;

    let root = parse_ok(source);

    // Should have exactly one LOCAL_VAR_DECL
    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(
        local_vars.len(),
        1,
        "Expected 1 LOCAL_VAR_DECL, found {}",
        local_vars.len()
    );

    // The LOCAL_VAR_DECL should have an ANNOTATION child
    let local_var = &local_vars[0];
    assert!(
        has_child_kind(local_var, SyntaxKind::ANNOTATION),
        "LOCAL_VAR_DECL should have ANNOTATION child"
    );

    // Should NOT have EXPR_STMT with the annotation parsed as expression
    let expr_stmts = find_nodes(&root, SyntaxKind::EXPR_STMT);
    for stmt in &expr_stmts {
        let text = stmt.text().to_string();
        assert!(
            !text.contains("SuppressWarnings"),
            "Annotation should not be parsed as EXPR_STMT: {text}"
        );
    }
}

#[test]
fn test_annotated_local_variable_nullable() {
    let source = r#"
        class Test {
            void test() {
                @Nullable String name = null;
            }
        }
    "#;

    let root = parse_ok(source);

    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(local_vars.len(), 1);

    let local_var = &local_vars[0];
    assert!(
        has_child_kind(local_var, SyntaxKind::ANNOTATION),
        "LOCAL_VAR_DECL should have @Nullable ANNOTATION child"
    );
}

#[test]
fn test_annotated_local_variable_multiple_annotations() {
    let source = r#"
        class Test {
            void test() {
                @Nullable @Deprecated
                String name = "test";
            }
        }
    "#;

    let root = parse_ok(source);

    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(local_vars.len(), 1);

    // Should have two ANNOTATION children
    let local_var = &local_vars[0];
    let annotations: Vec<_> = local_var
        .children()
        .filter(|c| c.kind() == SyntaxKind::ANNOTATION)
        .collect();
    assert_eq!(
        annotations.len(),
        2,
        "Expected 2 annotations, found {}",
        annotations.len()
    );
}

#[test]
fn test_annotated_local_variable_with_array_annotation() {
    let source = r#"
        class Test {
            void test() {
                @SuppressWarnings({"unchecked", "rawtypes"})
                List<String> list = new ArrayList<>();
            }
        }
    "#;

    let root = parse_ok(source);

    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(local_vars.len(), 1);

    assert!(
        has_child_kind(&local_vars[0], SyntaxKind::ANNOTATION),
        "LOCAL_VAR_DECL should have ANNOTATION child with array parameter"
    );
}

#[test]
fn test_final_annotated_local_variable() {
    let source = r#"
        class Test {
            void test() {
                @Nullable final String name = "test";
            }
        }
    "#;

    let root = parse_ok(source);

    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(local_vars.len(), 1);

    let local_var = &local_vars[0];
    assert!(has_child_kind(local_var, SyntaxKind::ANNOTATION));
}

#[test]
fn test_annotated_var_declaration() {
    let source = r#"
        class Test {
            void test() {
                @SuppressWarnings("unchecked")
                var list = new ArrayList<String>();
            }
        }
    "#;

    let root = parse_ok(source);

    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(local_vars.len(), 1);

    assert!(
        has_child_kind(&local_vars[0], SyntaxKind::ANNOTATION),
        "var declaration should support annotations"
    );
}

#[test]
fn test_non_annotated_local_variable_unchanged() {
    // Ensure we didn't break normal local variable parsing
    let source = r#"
        class Test {
            void test() {
                String name = "test";
                int x = 42;
                List<String> list = new ArrayList<>();
            }
        }
    "#;

    let root = parse_ok(source);

    let local_vars = find_nodes(&root, SyntaxKind::LOCAL_VAR_DECL);
    assert_eq!(
        local_vars.len(),
        3,
        "Should have 3 local variable declarations"
    );
}
