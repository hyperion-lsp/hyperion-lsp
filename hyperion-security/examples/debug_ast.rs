// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::syntax::{AstNode, SourceFile, SyntaxKind};
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};

fn print_tree(node: &hyperion_parser::syntax::SyntaxNode, indent: usize) {
    let indent_str = "  ".repeat(indent);
    println!("{}KIND: {:?}", indent_str, node.kind());
    println!("{}TEXT: {:?}", indent_str, node.text().to_string().trim());

    for child in node.children() {
        print_tree(&child, indent + 1);
    }
}

fn main() {
    let source = r#"
public class CryptoTest {
    public void encrypt() {
        Cipher cipher = Cipher.getInstance("AES/ECB/PKCS5Padding");
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = RawSyntaxNode::new_root(green_node);

    let Some(_source_file) = SourceFile::cast(root.clone()) else {
        println!("Failed to cast to SourceFile");
        return;
    };

    println!("=== Full AST ===");
    print_tree(&root, 0);

    println!("\n=== Looking for METHOD_CALL_EXPR nodes ===");
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            println!("\nFound METHOD_CALL_EXPR:");
            println!("  Text: {}", node.text());
        }
    }

    println!("\n=== Looking for NEW_EXPR nodes ===");
    for node in root.descendants() {
        if node.kind() == SyntaxKind::NEW_EXPR {
            println!("\nFound NEW_EXPR:");
            println!("  Text: {}", node.text());
        }
    }
}
