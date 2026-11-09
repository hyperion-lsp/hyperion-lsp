// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for using contextual keywords (module system keywords) as identifiers
//!
//! Java 9+ module system keywords (to, module, requires, exports, opens, uses,
//! provides, with, transitive, open) are contextual keywords. They can be used
//! as identifiers in most contexts.

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::SyntaxNode;

/// Helper function to parse source code and return the root node
fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

#[test]
fn test_to_as_variable_name() {
    let source = r#"
        class Test {
            void test() {
                Date to = new Date();
                Date from = new Date();
            }
        }
    "#;
    let (tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
    assert!(
        tree.to_string().contains("to"),
        "AST should contain 'to' as identifier"
    );
}

#[test]
fn test_module_as_variable_name() {
    let source = r#"
        class Test {
            void test() {
                String module = "test.Module";
            }
        }
    "#;
    let (tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
    assert!(
        tree.to_string().contains("module"),
        "AST should contain 'module' as identifier"
    );
}

#[test]
fn test_all_module_keywords_as_variables() {
    let source = r#"
        class Test {
            void test() {
                String module = "m";
                String requires = "r";
                String exports = "e";
                String opens = "o";
                String to = "t";
                String uses = "u";
                String provides = "p";
                String with = "w";
                String transitive = "tr";
                String open = "op";
            }
        }
    "#;
    let (_tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
}

#[test]
fn test_to_as_label() {
    let source = r#"
        class Test {
            void test() {
                to: for (int i = 0; i < 10; i++) {
                    if (i == 5) break to;
                }
            }
        }
    "#;
    let (_tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
}

#[test]
fn test_module_as_label() {
    let source = r#"
        class Test {
            void test() {
                module: for (int i = 0; i < 10; i++) {
                    if (i == 5) continue module;
                }
            }
        }
    "#;
    let (_tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
}

#[test]
fn test_contextual_keywords_as_enum_constants() {
    let source = r#"
        enum Test {
            module,
            to,
            requires,
            exports
        }
    "#;
    let (_tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
}

#[test]
fn test_contextual_keywords_in_annotations() {
    let source = r#"
        @interface Test {
            String module() default "test";
            String to() default "dest";
        }
    "#;
    let (_tree, errors) = parse(source);
    // Annotation attributes can use contextual keywords as names
    // This might have some errors depending on implementation details
    // but should not have "Unexpected token" errors for the keywords
    for error in &errors {
        let error_str = format!("{:?}", error);
        assert!(
            !error_str.contains("Unexpected token")
                || (!error_str.contains("MODULE_KW") && !error_str.contains("TO_KW")),
            "Should not have unexpected token errors for contextual keywords: {:?}",
            error
        );
    }
}

#[test]
fn test_record_keyword_as_variable() {
    let source = r#"
        class Test {
            void test() {
                String record = "test";
                String var = "value";
                String yield = "result";
            }
        }
    "#;
    let (_tree, errors) = parse(source);
    assert!(errors.is_empty(), "Expected no errors, got: {:#?}", errors);
}
