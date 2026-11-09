// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{Lexer, Parser, SyntaxKind};

#[test]
fn test_norwegian_characters() {
    let source = r#"
        public class Test {
            public long getTotalPriceInØre() {
                return 100;
            }

            public String naïve() {
                return "test";
            }
        }
    "#;

    // Test lexing
    let tokens: Vec<_> = Lexer::new(source).collect();

    let unicode_idents: Vec<_> = tokens
        .iter()
        .filter(|(kind, text, _)| *kind == SyntaxKind::IDENT && !text.is_ascii())
        .collect();

    println!("\n=== Unicode Identifiers Found ===");
    for (kind, text, range) in &unicode_idents {
        println!("  {kind:?}: '{text}' at {range:?}");
    }

    assert!(
        unicode_idents.len() >= 2,
        "Should find at least 2 Unicode identifiers"
    );

    let has_norwegian = unicode_idents.iter().any(|(_, text, _)| text.contains("Ø"));
    let has_french = unicode_idents.iter().any(|(_, text, _)| text.contains("ï"));

    assert!(has_norwegian, "Should lex Norwegian Ø");
    assert!(has_french, "Should lex French ï");

    // Test parsing
    let (green_node, errors) = Parser::parse(tokens);
    let root = hyperion_parser::SyntaxNode::new_root(green_node);

    println!("\n=== Parse Errors: {} ===", errors.len());
    for error in &errors {
        println!("  {error:?}");
    }

    // Should have no errors now
    assert_eq!(errors.len(), 0, "Should parse without errors");

    // Find method declarations
    let methods: Vec<_> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::METHOD_DECL)
        .collect();

    println!("\n=== Methods Found: {} ===", methods.len());
    for method in &methods {
        let text = method.text().to_string();
        if text.contains("Ø") || text.contains("ï") {
            println!("  {}", text.lines().next().unwrap_or(""));
        }
    }

    assert_eq!(methods.len(), 2, "Should find both methods");
}

#[test]
fn test_various_unicode_scripts() {
    let source = r#"
        public class Unicode {
            String αβγ = "Greek";
            int 数字 = 123;
            boolean isValid = true;
        }
    "#;

    let tokens: Vec<_> = Lexer::new(source).collect();

    let greek_ident = tokens
        .iter()
        .find(|(kind, text, _)| *kind == SyntaxKind::IDENT && *text == "αβγ");

    let chinese_ident = tokens
        .iter()
        .find(|(kind, text, _)| *kind == SyntaxKind::IDENT && *text == "数字");

    assert!(greek_ident.is_some(), "Should lex Greek identifier αβγ");
    assert!(
        chinese_ident.is_some(),
        "Should lex Chinese identifier 数字"
    );

    println!("\n✅ Successfully lexed:");
    println!("  - Greek: αβγ");
    println!("  - Chinese: 数字");
}
