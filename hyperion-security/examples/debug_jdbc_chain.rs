// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};

fn print_detailed(node: &hyperion_parser::syntax::SyntaxNode, indent: usize) {
    let indent_str = "  ".repeat(indent);
    let text = node.text().to_string();
    let short_text = if text.len() > 80 {
        format!("{}...", &text[..77])
    } else {
        text
    };
    println!(
        "{}KIND: {:?} | TEXT: {:?}",
        indent_str,
        node.kind(),
        short_text.trim()
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
import javax.servlet.http.HttpServletRequest;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    getConnection().createStatement().executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
    dataSource.getConnection().prepareStatement("SELECT foo FROM bar WHERE id = " + whoops);
    this.stmt.executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    let root = RawSyntaxNode::new_root(green_node);

    println!("Parse errors: {:?}", errors);

    for node in root.descendants() {
        if node.kind() == SyntaxKind::EXPR_STMT || node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            println!("\n=== {:?} ===", node.kind());
            print_detailed(&node, 0);
        }
    }
}
