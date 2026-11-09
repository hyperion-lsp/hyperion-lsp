// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for local type declarations inside methods
//!
//! Per JLS §14.3, local classes, interfaces, enums, and records can be declared
//! inside methods and blocks:
//! ```java
//! void method() {
//!     class LocalClass { }
//!     interface LocalInterface { }
//!     enum LocalEnum { A, B }
//!     record LocalRecord(int x) { }
//! }
//! ```

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

fn parse_ok(source: &str) -> SyntaxNode {
    let (node, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");
    node
}

fn find_nodes(root: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    root.descendants().filter(|n| n.kind() == kind).collect()
}

#[test]
fn test_local_class_simple() {
    let source = r#"
        class Test {
            void someMethod() {
                class LocalClass {
                    void localMethod() { }
                }
            }
        }
    "#;

    let root = parse_ok(source);

    // Should have two CLASS_DECL nodes: outer Test and inner LocalClass
    let classes = find_nodes(&root, SyntaxKind::CLASS_DECL);
    assert_eq!(
        classes.len(),
        2,
        "Expected 2 CLASS_DECL (Test and LocalClass), found {}",
        classes.len()
    );
}

#[test]
fn test_local_class_with_modifiers() {
    let source = r#"
        class Test {
            void method() {
                final class FinalLocalClass { }
                abstract class AbstractLocalClass { }
            }
        }
    "#;

    let root = parse_ok(source);

    // Should have 3 CLASS_DECL: Test, FinalLocalClass, AbstractLocalClass
    let classes = find_nodes(&root, SyntaxKind::CLASS_DECL);
    assert_eq!(classes.len(), 3);
}

#[test]
fn test_local_interface() {
    let source = r#"
        class Test {
            void method() {
                interface LocalInterface {
                    void interfaceMethod();
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let interfaces = find_nodes(&root, SyntaxKind::INTERFACE_DECL);
    assert_eq!(
        interfaces.len(),
        1,
        "Expected 1 INTERFACE_DECL, found {}",
        interfaces.len()
    );
}

#[test]
fn test_local_enum() {
    let source = r#"
        class Test {
            void method() {
                enum LocalEnum {
                    VALUE_A, VALUE_B, VALUE_C
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let enums = find_nodes(&root, SyntaxKind::ENUM_DECL);
    assert_eq!(
        enums.len(),
        1,
        "Expected 1 ENUM_DECL, found {}",
        enums.len()
    );
}

#[test]
fn test_local_record() {
    let source = r#"
        class Test {
            void method() {
                record Point(int x, int y) { }
            }
        }
    "#;

    let root = parse_ok(source);

    let records = find_nodes(&root, SyntaxKind::RECORD_DECL);
    assert_eq!(
        records.len(),
        1,
        "Expected 1 RECORD_DECL, found {}",
        records.len()
    );
}

#[test]
fn test_local_class_instantiation() {
    let source = r#"
        class Test {
            void method() {
                class Helper {
                    int getValue() { return 42; }
                }

                Helper helper = new Helper();
                int value = helper.getValue();
            }
        }
    "#;

    let root = parse_ok(source);

    let classes = find_nodes(&root, SyntaxKind::CLASS_DECL);
    assert_eq!(classes.len(), 2, "Should have Test and Helper classes");

    // Should be able to instantiate local class
    let new_exprs = find_nodes(&root, SyntaxKind::NEW_EXPR);
    assert!(
        !new_exprs.is_empty(),
        "Should have NEW_EXPR for local class instantiation"
    );
}

#[test]
fn test_nested_local_classes() {
    let source = r#"
        class Test {
            void method() {
                class Outer {
                    void outerMethod() {
                        class Inner {
                            void innerMethod() { }
                        }
                    }
                }
            }
        }
    "#;

    let root = parse_ok(source);

    // Should have 3 CLASS_DECL: Test, Outer, Inner
    let classes = find_nodes(&root, SyntaxKind::CLASS_DECL);
    assert_eq!(
        classes.len(),
        3,
        "Expected 3 nested classes, found {}",
        classes.len()
    );
}

#[test]
fn test_local_class_in_block() {
    let source = r#"
        class Test {
            void method() {
                if (true) {
                    class BlockLocalClass {
                        void blockMethod() { }
                    }
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let classes = find_nodes(&root, SyntaxKind::CLASS_DECL);
    assert_eq!(classes.len(), 2, "Local class in block should be parsed");
}

#[test]
fn test_multiple_local_classes() {
    let source = r#"
        class Test {
            void method() {
                class First { }
                class Second { }
                class Third { }
            }
        }
    "#;

    let root = parse_ok(source);

    let classes = find_nodes(&root, SyntaxKind::CLASS_DECL);
    assert_eq!(
        classes.len(),
        4,
        "Expected 4 classes (Test + 3 local), found {}",
        classes.len()
    );
}
