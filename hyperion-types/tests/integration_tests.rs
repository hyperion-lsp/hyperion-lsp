// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests with real parser output
//!
//! These tests verify that type extraction and resolution work correctly
//! with actual Java code parsed by hyperion-parser, not just unit tests
//! with hardcoded TypeStructure instances.

use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use hyperion_types::{extract_and_resolve_type_without_lookup, ResolutionContext};
use std::collections::HashMap;

/// Helper to parse Java code and get the root node
fn parse_java(code: &str) -> SyntaxNode {
    let tokens = Lexer::new(code).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

/// Helper to find first node of a given kind
fn find_node(root: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    if root.kind() == kind {
        return Some(root.clone());
    }
    root.descendants().find(|child| child.kind() == kind)
}

#[test]
fn test_extract_primitive_int() {
    let code = r"
        class Test {
            int count;
        }
    ";

    let root = parse_java(code);
    let primitive_type =
        find_node(&root, SyntaxKind::PRIMITIVE_TYPE).expect("Should find PRIMITIVE_TYPE node");

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let result = extract_and_resolve_type_without_lookup(&primitive_type, &context)
        .expect("Should extract primitive type");

    assert_eq!(result.base, "int");
    assert!(result.is_primitive);
    assert_eq!(result.dimensions, 0);
    assert!(result.type_args.is_empty());
}

#[test]
fn test_extract_class_type_simple() {
    let code = r"
        class Test {
            String name;
        }
    ";

    let root = parse_java(code);
    let class_type = find_node(&root, SyntaxKind::CLASS_TYPE).expect("Should find CLASS_TYPE node");

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract class type");

    // String is a java.lang type, should be resolved
    assert_eq!(result.base, "java.lang.String");
    assert!(!result.is_primitive);
    assert_eq!(result.dimensions, 0);
    assert!(result.type_args.is_empty());
}

#[test]
fn test_extract_array_type() {
    let code = r"
        class Test {
            String[] names;
        }
    ";

    let root = parse_java(code);

    // Parser structure: FIELD_DECL contains CLASS_TYPE, ARRAY_TYPE (as siblings).
    // Each ARRAY_TYPE sibling node (holding its own L_BRACKET/R_BRACKET tokens)
    // represents one `[]` dimension pair.
    // The workspace pattern: pass CLASS_TYPE and handle dimensions manually
    // This matches the actual working usage in hyperion-workspace
    let field_decl = find_node(&root, SyntaxKind::FIELD_DECL).expect("Should find FIELD_DECL node");

    // Find CLASS_TYPE (element type) and count dimensions
    let class_type = field_decl
        .children()
        .find(|n| n.kind() == SyntaxKind::CLASS_TYPE)
        .expect("Should find CLASS_TYPE");

    let dimensions = field_decl
        .children()
        .filter(|n| n.kind() == SyntaxKind::ARRAY_TYPE)
        .count();

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let mut result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract element type");

    // Add array dimensions (matching workspace pattern)
    result.dimensions = dimensions;

    assert_eq!(result.base, "java.lang.String");
    assert!(!result.is_primitive);
    assert_eq!(result.dimensions, 1);
    assert!(result.type_args.is_empty());
}

#[test]
fn test_extract_generic_type() {
    let code = r"
        import java.util.List;
        class Test {
            List<String> items;
        }
    ";

    let root = parse_java(code);

    // Find List<String> CLASS_TYPE node (should have TYPE_ARGS child)
    let class_type = root
        .descendants()
        .find(|n| {
            n.kind() == SyntaxKind::CLASS_TYPE
                && n.children().any(|c| c.kind() == SyntaxKind::TYPE_ARGS)
        })
        .expect("Should find generic CLASS_TYPE node");

    let mut imports = HashMap::new();
    imports.insert("List".to_string(), "java.util.List".to_string());
    let context = ResolutionContext::new(imports, None, None);

    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract generic type");

    assert_eq!(result.base, "java.util.List");
    assert!(!result.is_primitive);
    assert_eq!(result.dimensions, 0);
    assert_eq!(result.type_args.len(), 1);

    let type_arg = &result.type_args[0];
    assert_eq!(type_arg.base, "java.lang.String");
    assert!(!type_arg.is_primitive);
}

#[test]
fn test_extract_nested_generics() {
    let code = r"
        import java.util.Map;
        import com.example.User;
        class Test {
            Map<String, User> userMap;
        }
    ";

    let root = parse_java(code);

    // Find Map<String, User> CLASS_TYPE node
    let class_type = root
        .descendants()
        .find(|n| {
            n.kind() == SyntaxKind::CLASS_TYPE
                && n.children().any(|c| c.kind() == SyntaxKind::TYPE_ARGS)
        })
        .expect("Should find generic CLASS_TYPE node");

    let mut imports = HashMap::new();
    imports.insert("Map".to_string(), "java.util.Map".to_string());
    imports.insert("User".to_string(), "com.example.User".to_string());
    let context = ResolutionContext::new(imports, None, None);

    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract nested generic type");

    assert_eq!(result.base, "java.util.Map");
    assert_eq!(result.type_args.len(), 2);

    assert_eq!(result.type_args[0].base, "java.lang.String");
    assert_eq!(result.type_args[1].base, "com.example.User");
}

#[test]
fn test_extract_generic_array() {
    let code = r"
        import java.util.List;
        class Test {
            List<String>[] lists;
        }
    ";

    let root = parse_java(code);

    let field_decl = find_node(&root, SyntaxKind::FIELD_DECL).expect("Should find FIELD_DECL node");

    let class_type = field_decl
        .children()
        .find(|n| n.kind() == SyntaxKind::CLASS_TYPE)
        .expect("Should find CLASS_TYPE");

    let dimensions = field_decl
        .children()
        .filter(|n| n.kind() == SyntaxKind::ARRAY_TYPE)
        .count();

    let mut imports = HashMap::new();
    imports.insert("List".to_string(), "java.util.List".to_string());
    let context = ResolutionContext::new(imports, None, None);

    let mut result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract generic array type");

    result.dimensions = dimensions;

    assert_eq!(result.base, "java.util.List");
    assert_eq!(result.dimensions, 1);
    assert_eq!(result.type_args.len(), 1);
    assert_eq!(result.type_args[0].base, "java.lang.String");
}

#[test]
fn test_extract_multidimensional_array() {
    let code = r"
        class Test {
            int[][] matrix;
        }
    ";

    let root = parse_java(code);

    let field_decl = find_node(&root, SyntaxKind::FIELD_DECL).expect("Should find FIELD_DECL node");

    let primitive_type = field_decl
        .children()
        .find(|n| n.kind() == SyntaxKind::PRIMITIVE_TYPE)
        .expect("Should find PRIMITIVE_TYPE");

    let dimensions = field_decl
        .children()
        .filter(|n| n.kind() == SyntaxKind::ARRAY_TYPE)
        .count();

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let mut result = extract_and_resolve_type_without_lookup(&primitive_type, &context)
        .expect("Should extract multidimensional array");

    result.dimensions = dimensions;

    assert_eq!(result.base, "int");
    assert!(result.is_primitive);
    assert_eq!(result.dimensions, 2);
}

#[test]
fn test_extract_with_package_context() {
    let code = r"
        package com.example;
        class Test {
            User user;
        }
    ";

    let root = parse_java(code);

    let class_type = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CLASS_TYPE)
        .find(|n| {
            // Find the User type (not the Test class)
            n.descendants_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .any(|t| t.kind() == SyntaxKind::IDENT && t.text() == "User")
        })
        .expect("Should find User CLASS_TYPE node");

    // With package context, User should resolve to com.example.User
    let context = ResolutionContext::new(HashMap::new(), Some("com.example".into()), None);

    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract type with package context");

    assert_eq!(result.base, "com.example.User");
}

#[test]
fn test_extract_qualified_type() {
    let code = r"
        class Test {
            java.util.ArrayList<String> list;
        }
    ";

    let root = parse_java(code);

    let class_type = root
        .descendants()
        .find(|n| {
            n.kind() == SyntaxKind::CLASS_TYPE
                && n.children().any(|c| c.kind() == SyntaxKind::TYPE_ARGS)
        })
        .expect("Should find ArrayList CLASS_TYPE node");

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract qualified type");

    // Should preserve the fully qualified name
    assert_eq!(result.base, "java.util.ArrayList");
    assert_eq!(result.type_args.len(), 1);
    assert_eq!(result.type_args[0].base, "java.lang.String");
}

#[test]
fn test_format_fqn_string() {
    let code = r"
        import java.util.List;
        import com.example.User;
        class Test {
            List<User>[] items;
        }
    ";

    let root = parse_java(code);

    let field_decl = find_node(&root, SyntaxKind::FIELD_DECL).expect("Should find FIELD_DECL node");

    let class_type = field_decl
        .children()
        .find(|n| n.kind() == SyntaxKind::CLASS_TYPE)
        .expect("Should find CLASS_TYPE");

    let dimensions = field_decl
        .children()
        .filter(|n| n.kind() == SyntaxKind::ARRAY_TYPE)
        .count();

    let mut imports = HashMap::new();
    imports.insert("List".to_string(), "java.util.List".to_string());
    imports.insert("User".to_string(), "com.example.User".to_string());
    let context = ResolutionContext::new(imports, None, None);

    let mut result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract type");

    result.dimensions = dimensions;

    // Test FQN string formatting
    let fqn = result.to_fqn_string();
    assert_eq!(fqn, "java.util.List<com.example.User>[]");
}

#[test]
fn test_extract_diamond_operator() {
    // new ArrayList<>() — TYPE_ARGS node exists but is empty
    let code = r"
        class Test {
            void test() {
                java.util.List<String> list = new java.util.ArrayList<>();
            }
        }
    ";

    let root = parse_java(code);

    // Find the NEW_EXPR, then get the CLASS_TYPE inside it
    let new_expr = find_node(&root, SyntaxKind::NEW_EXPR).expect("Should find NEW_EXPR");
    let class_type = find_node(&new_expr, SyntaxKind::CLASS_TYPE)
        .expect("Should find CLASS_TYPE inside NEW_EXPR");

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract diamond type");

    assert_eq!(result.base, "java.util.ArrayList");
    assert!(
        result.type_args.is_empty(),
        "Diamond should have no type args"
    );
    assert!(result.is_diamond, "Should detect diamond operator");
}

#[test]
fn test_extract_non_diamond_generics() {
    // new ArrayList<String>() — explicit type args, NOT diamond
    let code = r"
        class Test {
            void test() {
                java.util.List<String> list = new java.util.ArrayList<String>();
            }
        }
    ";

    let root = parse_java(code);
    let new_expr = find_node(&root, SyntaxKind::NEW_EXPR).expect("Should find NEW_EXPR");
    let class_type = find_node(&new_expr, SyntaxKind::CLASS_TYPE)
        .expect("Should find CLASS_TYPE inside NEW_EXPR");

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract generic type");

    assert_eq!(result.base, "java.util.ArrayList");
    assert_eq!(result.type_args.len(), 1);
    assert!(
        !result.is_diamond,
        "Explicit type args should not be diamond"
    );
}

#[test]
fn test_extract_no_generics_not_diamond() {
    // new ArrayList() — raw type, no TYPE_ARGS node at all
    let code = r"
        class Test {
            void test() {
                java.util.List list = new java.util.ArrayList();
            }
        }
    ";

    let root = parse_java(code);
    let new_expr = find_node(&root, SyntaxKind::NEW_EXPR).expect("Should find NEW_EXPR");
    let class_type = find_node(&new_expr, SyntaxKind::CLASS_TYPE)
        .expect("Should find CLASS_TYPE inside NEW_EXPR");

    let context = ResolutionContext::new(HashMap::new(), None, None);
    let result = extract_and_resolve_type_without_lookup(&class_type, &context)
        .expect("Should extract raw type");

    assert_eq!(result.base, "java.util.ArrayList");
    assert!(result.type_args.is_empty());
    assert!(
        !result.is_diamond,
        "Raw type without <> should not be diamond"
    );
}
