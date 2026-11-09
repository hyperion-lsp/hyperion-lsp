// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let src = std::fs::read_to_string(path).unwrap();
    let t = hyperion_kotlin::parse(&src);
    let root = hyperion_kotlin::SyntaxNode::new_root(t.green_node.clone());
    println!("{:#?}", root);
    println!("ERRORS {:?}", t.errors);
}
