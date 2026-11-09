// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{Lexer, Parser, SyntaxNode};

fn print_tree(node: &SyntaxNode, indent: usize) {
    let indent_str = "  ".repeat(indent);
    println!("{}{}({:?})", indent_str, node.kind(), node.text_range());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => print_tree(&n, indent + 1),
            rowan::NodeOrToken::Token(t) => {
                let text = t.text();
                if text.trim().is_empty() {
                    println!("{}  {:?} [ws]", indent_str, t.kind());
                } else {
                    println!("{}  {:?} {:?}", indent_str, t.kind(), text);
                }
            }
        }
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/test_enum.java".to_string());
    let source = std::fs::read_to_string(&path).unwrap();
    let tokens = Lexer::new(&source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    if !errors.is_empty() {
        println!("Parse errors: {:?}", errors);
    }

    print_tree(&root, 0);
}
