// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_lsp::navigation::parse_method_chain;
use hyperion_parser::{lexer::Lexer, parser::Parser, SyntaxNode};
use rowan::TextSize;

#[test]
fn parse_chain_for_builder_line_item() {
    let source = r#"
class OrderService {
    void test() {
        Order.builder()
            .lineI<|>tem("Coffee", cents(450))
            .taxRate(10)
            .build();
    }
}
"#;

    let test_file = crate::lsp_test_framework::TestFile::new(source);
    let marker = test_file
        .find_marker("<|>")
        .expect("marker missing in test source");

    let tokens = Lexer::new(&test_file.clean_content).collect::<Vec<_>>();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    let token_at = root.token_at_offset(TextSize::from(marker.offset as u32));
    let token = token_at
        .clone()
        .right_biased()
        .or_else(|| token_at.left_biased())
        .expect("expected token at marker");

    let (chain, target_idx) =
        parse_method_chain(&token).expect("expected method chain for builder call");

    assert_eq!(chain[target_idx].name, "lineItem");
    assert!(chain.iter().any(|seg| seg.name == "Order"));
    assert!(chain.iter().any(|seg| seg.name == "builder"));
}
