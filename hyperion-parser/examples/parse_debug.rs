// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{lexer::Lexer, parser::Parser, ParseError, SyntaxNode};

fn parse_java(content: &str) -> (SyntaxNode, Vec<ParseError>) {
    let tokens = Lexer::new(content).collect::<Vec<_>>();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

fn main() {
    let path = std::env::args().nth(1).expect("Usage: parse_debug <file>");
    let content = std::fs::read_to_string(&path).unwrap();
    let (_tree, errors) = parse_java(&content);

    if errors.is_empty() {
        println!("No parse errors");
    } else {
        println!("{} parse errors:", errors.len());
        for err in errors.iter().take(15) {
            println!("  {:?}: {}", err.range, err.message);
        }
    }
}
