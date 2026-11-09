// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test for ParseMode::DeclarationsOnly - verifies method bodies are skipped

use hyperion_parser::{
    lexer::Lexer,
    parser::{ParseMode, Parser},
    syntax::{SyntaxKind, SyntaxNode},
};

#[test]
fn test_declarations_only_skips_method_body() {
    let source = r#"
class Foo {
    public void doSomething(String name, int count) {
        // This body should be skipped in DeclarationsOnly mode
        for (int i = 0; i < count; i++) {
            System.out.println(name + " " + i);
        }
        if (name != null) {
            throw new RuntimeException("test");
        }
    }
}
"#;

    // Parse in Full mode
    let tokens_full = Lexer::new(source).collect();
    let (green_full, _) = Parser::parse(tokens_full);
    let root_full = SyntaxNode::new_root(green_full);

    // Parse in DeclarationsOnly mode
    let tokens_decl = Lexer::new(source).collect();
    let (green_decl, _) = Parser::parse_with_mode(tokens_decl, ParseMode::DeclarationsOnly);
    let root_decl = SyntaxNode::new_root(green_decl);

    // DeclarationsOnly should produce a smaller tree (no FOR_STMT, IF_STMT inside method body)
    let full_node_count = count_nodes(&root_full);
    let decl_node_count = count_nodes(&root_decl);

    println!("Full mode nodes: {}", full_node_count);
    println!("DeclarationsOnly nodes: {}", decl_node_count);

    assert!(
        decl_node_count < full_node_count,
        "DeclarationsOnly should produce fewer nodes ({} >= {})",
        decl_node_count,
        full_node_count
    );

    // Verify method declaration is still present
    let method_found = find_node(&root_decl, SyntaxKind::METHOD_DECL).is_some();
    assert!(
        method_found,
        "METHOD_DECL should be present in DeclarationsOnly mode"
    );

    // Verify the internal statements are NOT present
    let for_stmt_found = find_node(&root_decl, SyntaxKind::FOR_STMT).is_some();
    assert!(
        !for_stmt_found,
        "FOR_STMT should NOT be present in DeclarationsOnly mode"
    );
}

#[test]
fn test_declarations_only_skips_constructor_body() {
    let source = r#"
class Bar {
    private String name;

    public Bar(String name) {
        this.name = name;
        validateName(name);
    }

    private void validateName(String name) {
        if (name == null) {
            throw new IllegalArgumentException("name");
        }
    }
}
"#;

    // Parse in DeclarationsOnly mode
    let tokens = Lexer::new(source).collect();
    let (green, _) = Parser::parse_with_mode(tokens, ParseMode::DeclarationsOnly);
    let root = SyntaxNode::new_root(green);

    // Verify constructor is present
    let ctor_found = find_node(&root, SyntaxKind::CONSTRUCTOR_DECL).is_some();
    assert!(ctor_found, "CONSTRUCTOR_DECL should be present");

    // Verify IF_STMT from validateName is NOT present
    let if_found = find_node(&root, SyntaxKind::IF_STMT).is_some();
    assert!(
        !if_found,
        "IF_STMT should NOT be present in DeclarationsOnly mode"
    );
}

#[test]
fn test_declarations_only_preserves_class_structure() {
    let source = r#"
package com.example;

public class MyService implements Runnable, Comparable<MyService> {
    private static final String NAME = "test";
    private int count;

    public void run() {
        while (true) {
            doWork();
        }
    }

    private void doWork() {
        count++;
    }

    public int compareTo(MyService other) {
        return Integer.compare(this.count, other.count);
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse_with_mode(tokens, ParseMode::DeclarationsOnly);
    let root = SyntaxNode::new_root(green);

    // No parse errors
    assert!(
        errors.is_empty(),
        "Should parse without errors: {:?}",
        errors
    );

    // Verify structure
    assert!(find_node(&root, SyntaxKind::PACKAGE_DECL).is_some());
    assert!(find_node(&root, SyntaxKind::CLASS_DECL).is_some());
    assert!(find_node(&root, SyntaxKind::FIELD_DECL).is_some());

    // Count methods (should be 3: run, doWork, compareTo)
    let method_count = count_nodes_of_kind(&root, SyntaxKind::METHOD_DECL);
    assert_eq!(method_count, 3, "Should have 3 methods");

    // No WHILE_STMT from run() method body
    assert!(find_node(&root, SyntaxKind::WHILE_STMT).is_none());
}

fn count_nodes(node: &SyntaxNode) -> usize {
    1 + node.children().map(|c| count_nodes(&c)).sum::<usize>()
}

fn count_nodes_of_kind(node: &SyntaxNode, kind: SyntaxKind) -> usize {
    let self_count = usize::from(node.kind() == kind);
    self_count
        + node
            .children()
            .map(|c| count_nodes_of_kind(&c, kind))
            .sum::<usize>()
}

fn find_node(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    if node.kind() == kind {
        return Some(node.clone());
    }
    for child in node.children() {
        if let Some(found) = find_node(&child, kind) {
            return Some(found);
        }
    }
    None
}
