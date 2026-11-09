// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Property-based tests for the parser using proptest
//!
//! These tests ensure fundamental parser invariants hold across a wide range of inputs:
//! - Parser never panics on any input
//! - Parsed AST has consistent structure
//! - Parser handles all valid Java syntax correctly

use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use proptest::prelude::*;

// ============================================================================
// Test Strategies - Generate Valid Java Syntax
// ============================================================================

/// Java keywords that cannot be used as identifiers
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
];

/// Generate valid Java identifiers (excluding keywords)
fn identifier() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z_][a-zA-Z0-9_]{0,30}")
        .expect("valid identifier regex")
        .prop_filter("not a keyword", |s| !JAVA_KEYWORDS.contains(&s.as_str()))
}

/// Generate valid primitive types
fn primitive_type() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("byte"),
        Just("short"),
        Just("int"),
        Just("long"),
        Just("float"),
        Just("double"),
        Just("boolean"),
        Just("char"),
    ]
}

/// Generate valid modifiers
fn modifier() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("public"),
        Just("private"),
        Just("protected"),
        Just("static"),
        Just("final"),
        Just("abstract"),
    ]
}

/// Generate a simple class declaration
fn simple_class() -> impl Strategy<Value = String> {
    (prop::option::of(modifier()), identifier()).prop_map(|(mod_opt, name)| {
        let modifier = mod_opt.map_or(String::new(), |m| format!("{m} "));
        format!("{modifier}class {name} {{}}")
    })
}

/// Generate a class with a field
fn class_with_field() -> impl Strategy<Value = String> {
    (identifier(), primitive_type(), identifier()).prop_map(
        |(class_name, field_type, field_name)| {
            format!("class {class_name} {{ {field_type} {field_name}; }}")
        },
    )
}

/// Generate a class with a method
fn class_with_method() -> impl Strategy<Value = String> {
    (identifier(), primitive_type(), identifier()).prop_map(
        |(class_name, return_type, method_name)| {
            format!("class {class_name} {{ {return_type} {method_name}() {{ return null; }} }}")
        },
    )
}

/// Generate a package declaration
fn package_declaration() -> impl Strategy<Value = String> {
    prop::collection::vec(identifier(), 1..4)
        .prop_map(|parts| format!("package {};", parts.join(".")))
}

/// Generate an import statement
fn import_statement() -> impl Strategy<Value = String> {
    (prop::collection::vec(identifier(), 1..4), identifier()).prop_map(
        |(package_parts, class_name)| format!("import {}.{class_name};", package_parts.join(".")),
    )
}

/// Generate a complete compilation unit
fn compilation_unit() -> impl Strategy<Value = String> {
    (
        prop::option::of(package_declaration()),
        prop::collection::vec(import_statement(), 0..3),
        simple_class(),
    )
        .prop_map(|(pkg, imports, class)| {
            let mut result = String::new();

            if let Some(pkg) = pkg {
                result.push_str(&pkg);
                result.push('\n');
            }

            for import in imports {
                result.push_str(&import);
                result.push('\n');
            }

            result.push_str(&class);
            result
        })
}

// ============================================================================
// Property Tests - Parser Invariants
// ============================================================================

proptest! {
    /// Property: Parser should never panic on any input
    ///
    /// This is the most fundamental invariant - the parser must be robust
    /// and handle all inputs gracefully, even malformed ones.
    #[test]
    fn parser_never_panics(source in "\\PC{0,100}") {
        let tokens = Lexer::new(&source).collect();
        let _result = Parser::parse(tokens);
        // If we get here without panicking, the test passes
    }

    /// Property: Parser should never panic on valid Java syntax
    #[test]
    fn parser_never_panics_on_valid_syntax(source in compilation_unit()) {
        let tokens = Lexer::new(&source).collect();
        let _result = Parser::parse(tokens);
        // If we get here without panicking, the test passes
    }

    /// Property: Parsed AST root is always SOURCE_FILE
    #[test]
    fn root_is_always_source_file(source in "\\PC{0,100}") {
        let tokens = Lexer::new(&source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        prop_assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
    }

    /// Property: Valid simple classes should parse without errors
    #[test]
    fn simple_classes_parse_without_errors(source in simple_class()) {
        let tokens = Lexer::new(&source).collect();
        let (_green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors for valid class, got: {errors:?}"
        );
    }

    /// Property: Classes with fields should parse without errors
    #[test]
    fn classes_with_fields_parse_correctly(source in class_with_field()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors, got: {errors:?}"
        );

        let root = SyntaxNode::new_root(green_node);

        // Should contain a field declaration
        let has_field = root.descendants().any(|node| {
            node.kind() == SyntaxKind::FIELD_DECL
        });

        prop_assert!(has_field, "Expected to find FIELD_DECL in AST");
    }

    /// Property: Classes with methods should parse without errors
    #[test]
    fn classes_with_methods_parse_correctly(source in class_with_method()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors, got: {errors:?}"
        );

        let root = SyntaxNode::new_root(green_node);

        // Should contain a method declaration
        let has_method = root.descendants().any(|node| {
            node.kind() == SyntaxKind::METHOD_DECL
        });

        prop_assert!(has_method, "Expected to find METHOD_DECL in AST");
    }

    /// Property: Package declarations should parse correctly
    #[test]
    fn package_declarations_parse_correctly(source in package_declaration()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors, got: {errors:?}"
        );

        let root = SyntaxNode::new_root(green_node);

        // Should contain a package declaration
        let has_package = root.descendants().any(|node| {
            node.kind() == SyntaxKind::PACKAGE_DECL
        });

        prop_assert!(has_package, "Expected to find PACKAGE_DECL in AST");
    }

    /// Property: Import statements should parse correctly
    #[test]
    fn import_statements_parse_correctly(source in import_statement()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors, got: {errors:?}"
        );

        let root = SyntaxNode::new_root(green_node);

        // Should contain an import declaration
        let has_import = root.descendants().any(|node| {
            node.kind() == SyntaxKind::IMPORT_DECL
        });

        prop_assert!(has_import, "Expected to find IMPORT_DECL in AST");
    }

    /// Property: Complete compilation units should parse correctly
    #[test]
    fn compilation_units_parse_correctly(source in compilation_unit()) {
        let tokens = Lexer::new(&source).collect();
        let (_green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors for valid compilation unit, got: {errors:?}"
        );
    }
}

// ============================================================================
// Structural Invariants
// ============================================================================

proptest! {
    /// Property: Every node in the AST should have a valid parent chain to root
    #[test]
    fn every_node_has_valid_parent_chain(source in simple_class()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        for node in root.descendants() {
            let mut current = Some(node.clone());
            let mut depth = 0;

            while let Some(node) = current {
                depth += 1;
                prop_assert!(
                    depth < 1000,
                    "Parent chain too deep - possible cycle"
                );
                current = node.parent();
            }
        }
    }

    /// Property: Every token in the AST should be a child of some node
    #[test]
    fn every_token_has_parent(source in simple_class()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        for token in root.descendants_with_tokens() {
            if let Some(token) = token.as_token() {
                prop_assert!(
                    token.parent().is_some(),
                    "Found orphaned token: {:?}",
                    token.kind()
                );
            }
        }
    }

    /// Property: Text ranges should be monotonic
    ///
    /// Each child's text range should be contained within its parent's range.
    #[test]
    fn text_ranges_are_monotonic(source in simple_class()) {
        let tokens = Lexer::new(&source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        for node in root.descendants() {
            if let Some(parent) = node.parent() {
                let node_range = node.text_range();
                let parent_range = parent.text_range();

                prop_assert!(
                    node_range.start() >= parent_range.start(),
                    "Child starts before parent"
                );
                prop_assert!(
                    node_range.end() <= parent_range.end(),
                    "Child ends after parent"
                );
            }
        }
    }
}

// ============================================================================
// Parser Determinism
// ============================================================================

proptest! {
    /// Property: Parsing the same input twice should give identical results
    #[test]
    fn parser_is_deterministic(source in compilation_unit()) {
        let tokens1 = Lexer::new(&source).collect();
        let (green1, errors1) = Parser::parse(tokens1);

        let tokens2 = Lexer::new(&source).collect();
        let (green2, errors2) = Parser::parse(tokens2);

        // Errors should be identical
        prop_assert_eq!(errors1.len(), errors2.len());

        // Parse trees should be structurally identical
        let root1 = SyntaxNode::new_root(green1);
        let root2 = SyntaxNode::new_root(green2);

        prop_assert_eq!(root1.kind(), root2.kind());
        prop_assert_eq!(
            root1.descendants().count(),
            root2.descendants().count()
        );
    }
}

// ============================================================================
// Lexer Invariants
// ============================================================================

proptest! {
    /// Property: Lexer should never panic on any input
    #[test]
    fn lexer_never_panics(source in "\\PC{0,1000}") {
        let _tokens: Vec<_> = Lexer::new(&source).collect();
        // If we get here without panicking, the test passes
    }

    /// Property: Lexer output coverage - all tokens should cover the input
    #[test]
    fn lexer_covers_all_input(source in "[a-zA-Z0-9 ]{0,100}") {
        let tokens: Vec<_> = Lexer::new(&source).collect();
        let total_len: usize = tokens.iter().map(|(_, _, range)| {
            let len: u32 = range.len().into();
            len as usize
        }).sum();

        prop_assert_eq!(
            total_len,
            source.len(),
            "Lexer tokens don't cover entire input"
        );
    }

    /// Property: Lexer tokens should be contiguous (no gaps)
    #[test]
    fn lexer_tokens_are_contiguous(source in "[a-zA-Z0-9 ]{1,100}") {
        let tokens: Vec<_> = Lexer::new(&source).collect();

        if tokens.is_empty() {
            return Ok(());
        }

        let mut expected_offset = 0u32;

        for (_kind, _text, range) in &tokens {
            let start: u32 = range.start().into();
            prop_assert_eq!(
                start,
                expected_offset,
                "Gap in lexer output at offset {}", expected_offset
            );
            let len: u32 = range.len().into();
            expected_offset += len;
        }

        prop_assert_eq!(
            expected_offset as usize,
            source.len(),
            "Lexer didn't consume entire input"
        );
    }
}

// ============================================================================
// Stress Tests - Large Inputs
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        max_shrink_iters: 1000,
        .. ProptestConfig::default()
    })]

    /// Property: Parser handles deeply nested structures
    #[test]
    fn parser_handles_nested_blocks(depth in 1usize..50) {
        let mut source = String::from("class Test { void test() {");

        for _ in 0..depth {
            source.push_str(" {");
        }

        for _ in 0..depth {
            source.push_str(" }");
        }

        source.push_str(" } }");

        let tokens = Lexer::new(&source).collect();
        let (_green_node, _errors) = Parser::parse(tokens);
        // Should not panic or overflow stack
    }

    /// Property: Parser handles many siblings
    #[test]
    fn parser_handles_many_fields(count in 1usize..100) {
        let mut source = String::from("class Test {");

        for i in 0..count {
            source.push_str(&format!(" int field{i};"));
        }

        source.push_str(" }");

        let tokens = Lexer::new(&source).collect();
        let (green_node, errors) = Parser::parse(tokens);

        prop_assert!(
            errors.is_empty(),
            "Expected no parse errors, got: {errors:?}"
        );

        let root = SyntaxNode::new_root(green_node);
        let field_count = root.descendants()
            .filter(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .count();

        prop_assert_eq!(field_count, count);
    }
}
