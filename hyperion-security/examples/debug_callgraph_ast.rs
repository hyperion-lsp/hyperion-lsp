// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};

fn print_tree(node: &hyperion_parser::syntax::SyntaxNode, indent: usize) {
    let indent_str = "  ".repeat(indent);
    let text = node.text().to_string();
    let display_text = if text.len() > 40 {
        format!("{}...", &text[..40])
    } else {
        text
    };
    println!(
        "{}KIND: {:?}, TEXT: {:?}",
        indent_str,
        node.kind(),
        display_text.replace('\n', "\\n")
    );

    for child in node.children() {
        print_tree(&child, indent + 1);
    }
}

fn main() {
    let source = r#"
        class TestClass {
            public void recursive(int n) {
                if (n > 0) {
                    recursive(n - 1);
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = RawSyntaxNode::new_root(green_node);

    println!("=== Full AST ===");
    print_tree(&root, 0);

    println!("\n=== Looking for METHOD_DECL nodes ===");
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            println!("\nFound METHOD_DECL:");
            println!("  Text: {}", node.text());
            println!("  Children (nodes):");
            for child in node.children() {
                println!(
                    "    {:?}: {}",
                    child.kind(),
                    child
                        .text()
                        .to_string()
                        .chars()
                        .take(30)
                        .collect::<String>()
                );
            }
            println!("  Children (including tokens):");
            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Node(n) => {
                        println!(
                            "    NODE {:?}: {}",
                            n.kind(),
                            n.text().to_string().chars().take(30).collect::<String>()
                        );
                    }
                    rowan::NodeOrToken::Token(t) => {
                        println!("    TOKEN {:?}: {}", t.kind(), t.text());
                    }
                }
            }
        }
    }

    println!("\n=== Looking for EXPR_STMT with method calls ===");
    for node in root.descendants() {
        if node.kind() == SyntaxKind::EXPR_STMT {
            // Check if it contains a METHOD_CALL_EXPR
            let has_call = node
                .descendants()
                .any(|n| n.kind() == SyntaxKind::METHOD_CALL_EXPR);
            if has_call {
                println!("\nFound EXPR_STMT with method call:");
                println!("  Text: {}", node.text());
                println!("  Children:");
                for child in node.children() {
                    println!(
                        "    {:?}: {}",
                        child.kind(),
                        child
                            .text()
                            .to_string()
                            .chars()
                            .take(30)
                            .collect::<String>()
                    );
                }
            }
        }
    }

    println!("\n=== Looking for METHOD_CALL_EXPR nodes ===");
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            println!("\nFound METHOD_CALL_EXPR:");
            println!("  Text: {}", node.text());
            println!("  Parent: {:?}", node.parent().map(|p| p.kind()));
            if let Some(parent) = node.parent() {
                println!("  Parent's children:");
                for child in parent.children() {
                    println!(
                        "    {:?}: {}",
                        child.kind(),
                        child
                            .text()
                            .to_string()
                            .chars()
                            .take(30)
                            .collect::<String>()
                    );
                }
            }
        }
    }
}
