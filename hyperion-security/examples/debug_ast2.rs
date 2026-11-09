// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};

fn print_detailed(node: &hyperion_parser::syntax::SyntaxNode, indent: usize) {
    let indent_str = "  ".repeat(indent);
    let text = node.text().to_string();
    let short_text = if text.len() > 50 {
        format!("{}...", &text[..47])
    } else {
        text
    };
    println!(
        "{}KIND: {:?} | TEXT: {:?}",
        indent_str,
        node.kind(),
        short_text
    );

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => print_detailed(&n, indent + 1),
            rowan::NodeOrToken::Token(t) => {
                if !t.kind().is_trivia() {
                    println!("{}  TOKEN {:?}: {:?}", indent_str, t.kind(), t.text());
                }
            }
        }
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

    println!("=== LOCAL_VAR_DECL details ===");
    for node in root.descendants() {
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            print_detailed(&node, 0);
        }
    }
}
