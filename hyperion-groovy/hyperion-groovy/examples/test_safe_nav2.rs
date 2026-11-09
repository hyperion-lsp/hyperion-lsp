// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_groovy::{tree_sitter_wrapper::GroovyParser, AstBuilder};

fn main() {
    let source = r#"
        def obj = getObject()
        return obj?.property
    "#;

    println!("Source:\n{}", source);

    // Parse with tree-sitter
    let mut parser = GroovyParser::new().unwrap();
    let parsed = parser.parse(source).unwrap();

    println!("\n=== Tree-sitter Parse Tree ===");
    fn print_node(node: &tree_sitter::Node, source: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        let text = node.utf8_text(source.as_bytes()).unwrap_or("<error>");
        println!("{}{}: {:?}", indent, node.kind(), text);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            print_node(&child, source, depth + 1);
        }
    }
    print_node(&parsed.root(), source, 0);

    // Build AST
    let builder = AstBuilder::new(source);
    let statements = builder.build_statements(parsed.root());

    println!("\n=== AST Statements ===");
    for (i, stmt) in statements.iter().enumerate() {
        println!("Statement {}: {:?}", i, stmt);
    }
}
