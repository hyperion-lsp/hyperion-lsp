// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::SyntaxNode;

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

fn parse_and_check(source: &str) -> (usize, Vec<String>) {
    let (_node, errors) = parse(source);

    let error_messages: Vec<String> = errors.iter().map(|e| format!("{e:?}")).collect();

    (errors.len(), error_messages)
}

#[test]
fn test_simple_jpa_annotation() {
    let source = r#"
        class Test {
            @Column(name = "id")
            private String id;
        }
    "#;
    let (error_count, errors) = parse_and_check(source);
    if error_count > 0 {
        eprintln!("Errors: {error_count}");
        for err in &errors {
            eprintln!("  - {err}");
        }
    }
    assert_eq!(error_count, 0, "Should parse simple JPA annotation");
}

#[test]
fn test_annotation_with_multiple_params() {
    let source = r#"
        class Test {
            @Column(name = "id", length = 32, nullable = false)
            private String id;
        }
    "#;
    let (error_count, errors) = parse_and_check(source);
    if error_count > 0 {
        eprintln!("Errors: {error_count}");
        for err in &errors {
            eprintln!("  - {err}");
        }
    }
    assert_eq!(
        error_count, 0,
        "Should parse annotation with multiple parameters"
    );
}

#[test]
fn test_nested_annotation() {
    let source = r#"
        class Test {
            @JoinTable(
                name = "owners",
                joinColumns = @JoinColumn(name = "id"))
            private List<String> ownerIds;
        }
    "#;
    let (error_count, errors) = parse_and_check(source);
    if error_count > 0 {
        eprintln!("Errors: {error_count}");
        for err in &errors {
            eprintln!("  - {err}");
        }
    }
    assert_eq!(error_count, 0, "Should parse nested annotations");
}
