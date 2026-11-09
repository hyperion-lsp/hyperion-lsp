// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! A member's modifier keywords are found through the `MODIFIER` wrappers the parser
//! puts around them, and the modifier list sits beside the member.

use hyperion_parser::lexer::Lexer;
use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{AstNode, ModifierList, SyntaxKind, SyntaxNode};

fn parse(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

#[test]
fn a_private_method_is_private_and_its_neighbour_is_not() {
    let root = parse("class A { public void open() { } private void close() { } }");
    let methods: Vec<SyntaxNode> = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
        .collect();
    assert_eq!(methods.len(), 2);
    assert!(!declaration_modifiers(&methods[0]).is_private());
    assert!(declaration_modifiers(&methods[1]).is_private());
}

#[test]
fn modifier_list_accessor_sees_a_keyword_inside_its_wrapper() {
    let root = parse("class A { private int count; }");
    let list = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::MODIFIER_LIST)
        .find_map(ModifierList::cast)
        .expect("a modifier list");
    assert!(list.is_private());
    assert!(!list.is_static());
}
