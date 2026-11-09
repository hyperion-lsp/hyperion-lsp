// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for field annotation extraction
//!
//! Extracts annotations from field declarations to support:
//! - Dependency injection: @Autowired, @Inject
//! - Lombok: @NonNull on fields
//! - Custom qualifiers: @Named, @Qualifier

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{lexer::Lexer, parser::Parser};
use hyperion_semantics::nullability::extraction::collect_field_annotations;

/// Parse Java code and extract the class node
fn parse_class(code: &str) -> SyntaxNode {
    let tokens: Vec<_> = Lexer::new(code).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);
    root.descendants()
        .find(|n| n.kind() == SyntaxKind::CLASS_DECL)
        .expect("No class declaration found in test code")
}

#[test]
fn test_extract_autowired_annotation() {
    let code = r#"
        class Service {
            @Autowired
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["Autowired"]);
}

#[test]
fn test_extract_inject_annotation() {
    let code = r#"
        class Service {
            @Inject
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["Inject"]);
}

#[test]
fn test_extract_multiple_annotations() {
    let code = r#"
        class Service {
            @Inject
            @Named("userRepo")
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["Inject", "Named"]);
}

#[test]
fn test_extract_lombok_nonnull() {
    let code = r#"
        class User {
            @NonNull
            private String name;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("name"));
    assert_eq!(field_annotations["name"], vec!["NonNull"]);
}

#[test]
fn test_extract_qualifier_annotation() {
    let code = r#"
        class Service {
            @Autowired
            @Qualifier("primary")
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["Autowired", "Qualifier"]);
}

#[test]
fn test_multiple_fields_with_annotations() {
    let code = r#"
        class Service {
            @Autowired
            private Repository userRepo;

            @Inject
            @Named("primary")
            private Repository productRepo;

            private String name;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // userRepo should have @Autowired
    assert!(field_annotations.contains_key("userRepo"));
    assert_eq!(field_annotations["userRepo"], vec!["Autowired"]);

    // productRepo should have @Inject and @Named
    assert!(field_annotations.contains_key("productRepo"));
    assert_eq!(field_annotations["productRepo"], vec!["Inject", "Named"]);

    // name should have no annotations
    assert!(field_annotations.contains_key("name"));
    assert!(field_annotations["name"].is_empty());
}

#[test]
fn test_field_with_no_annotations() {
    let code = r#"
        class Service {
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    assert!(field_annotations["repo"].is_empty());
}

#[test]
fn test_fully_qualified_annotation() {
    let code = r#"
        class Service {
            @org.springframework.beans.factory.annotation.Autowired
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    // Should extract full qualified name
    assert_eq!(
        field_annotations["repo"],
        vec!["org.springframework.beans.factory.annotation.Autowired"]
    );
}

#[test]
fn test_annotation_with_parameters() {
    let code = r#"
        class Service {
            @Named(value = "userRepo")
            private Repository repo;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    // Should extract just the simple name without parameters
    assert_eq!(field_annotations["repo"], vec!["Named"]);
}

#[test]
fn test_private_static_final_field() {
    let code = r#"
        class Service {
            @Autowired
            private static final Repository REPO = null;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("REPO"));
    assert_eq!(field_annotations["REPO"], vec!["Autowired"]);
}

#[test]
fn test_public_field() {
    let code = r#"
        class Config {
            @NonNull
            public String apiKey;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("apiKey"));
    assert_eq!(field_annotations["apiKey"], vec!["NonNull"]);
}

#[test]
fn test_array_field() {
    let code = r#"
        class Service {
            @Autowired
            private Repository[] repos;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repos"));
    assert_eq!(field_annotations["repos"], vec!["Autowired"]);
}

#[test]
fn test_generic_field() {
    let code = r#"
        class Service {
            @Autowired
            private List<Repository> repos;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repos"));
    assert_eq!(field_annotations["repos"], vec!["Autowired"]);
}

#[test]
fn test_nested_class_fields() {
    let code = r#"
        class Outer {
            @Autowired
            private Repository outerRepo;

            class Inner {
                @Inject
                private Repository innerRepo;
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Each class owns an independent annotation map; nested fields must not
    // make same-named outer fields appear injected/non-null.
    assert!(field_annotations.contains_key("outerRepo"));
    assert_eq!(field_annotations["outerRepo"], vec!["Autowired"]);
    assert!(!field_annotations.contains_key("innerRepo"));

    let inner = class_node
        .descendants()
        .find(|node| node.kind() == SyntaxKind::CLASS_DECL && *node != class_node)
        .expect("inner class");
    let inner_annotations = collect_field_annotations(&inner);
    assert_eq!(inner_annotations["innerRepo"], vec!["Inject"]);
}

#[test]
fn test_field_with_initializer() {
    let code = r#"
        class Service {
            @Autowired
            private Repository repo = new Repository();
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["Autowired"]);
}

#[test]
fn test_primitive_field() {
    let code = r#"
        class Config {
            @NonNull
            private int port;
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.contains_key("port"));
    assert_eq!(field_annotations["port"], vec!["NonNull"]);
}

#[test]
fn test_empty_class() {
    let code = r#"
        class Empty {
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.is_empty());
}

#[test]
fn test_class_with_only_methods() {
    let code = r#"
        class Service {
            public void doSomething() {
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    assert!(field_annotations.is_empty());
}
