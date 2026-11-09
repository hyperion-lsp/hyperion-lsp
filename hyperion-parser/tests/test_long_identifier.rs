// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{Lexer, Parser, SyntaxKind};

#[test]
fn test_long_method_name() {
    let source = r"
        public class CatalogConfiguration {
            public Integer getRefreshPeriodMillis() {
                return null;
            }
        }
    ";

    // Test lexing
    let tokens: Vec<_> = Lexer::new(source).collect();

    println!("\n=== TOKENS ===");
    for (kind, text, range) in &tokens {
        if *kind == SyntaxKind::IDENT {
            println!("IDENT: '{}' (len: {}) at {:?}", text, text.len(), range);
        }
    }

    // Count how many times we see the method name
    let method_name_count = tokens
        .iter()
        .filter(|(kind, text, _)| *kind == SyntaxKind::IDENT && *text == "getRefreshPeriodMillis")
        .count();

    println!("\nMethod name 'getRefreshPeriodMillis' found {method_name_count} times");
    assert!(method_name_count > 0, "Method name should be lexed");

    // Test parsing
    let (green_node, errors) = Parser::parse(tokens);
    let root = hyperion_parser::SyntaxNode::new_root(green_node);

    println!("\n=== PARSE ERRORS: {} ===", errors.len());
    for error in &errors {
        println!("  - {error:?}");
    }

    // Find METHOD_DECL node
    let mut found_method = false;
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            println!("\n=== FOUND METHOD_DECL ===");
            println!(
                "Text: {}",
                node.text()
                    .to_string()
                    .chars()
                    .take(100)
                    .collect::<String>()
            );

            for (i, child) in node.children_with_tokens().enumerate() {
                match child {
                    rowan::NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::IDENT {
                            println!("  {}: IDENT = '{}'", i, t.text());
                            if t.text() == "getRefreshPeriodMillis" {
                                found_method = true;
                            }
                        }
                    }
                    rowan::NodeOrToken::Node(n) => {
                        println!("  {}: {:?}", i, n.kind());
                    }
                }
            }
        }
    }

    assert!(found_method, "Should find method in METHOD_DECL node");
}
