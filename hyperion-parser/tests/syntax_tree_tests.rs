// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for syntax tree construction and navigation
//!
//! This test suite covers:
//! - CST (Concrete Syntax Tree) structure and properties
//! - Tree navigation (parent, children, siblings)
//! - Text ranges and positions
//! - Trivia preservation (whitespace, comments)
//! - Typed AST node casting and queries
//! - Tree traversal patterns

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{AstNode, ClassDecl, MethodDecl, SourceFile, SyntaxKind, SyntaxNode};

/// Helper function to parse source and return root node
fn parse(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green_node, _) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

// ============================================================================
// Basic Tree Structure Tests
// ============================================================================

#[test]
fn test_root_node_is_source_file() {
    let root = parse("class Foo {}");
    assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
}

#[test]
fn test_tree_has_children() {
    let root = parse("class Foo {}");
    assert!(
        root.children().next().is_some(),
        "Root should have children"
    );
}

#[test]
fn test_children_with_tokens() {
    let root = parse("class Foo {}");
    assert!(root.children_with_tokens().next().is_some());
}

#[test]
fn test_node_text() {
    let root = parse("class Foo {}");
    let text = root.text().to_string();
    assert_eq!(text, "class Foo {}");
}

#[test]
fn test_node_text_range() {
    let root = parse("class Foo {}");
    let range = root.text_range();

    assert_eq!(u32::from(range.start()), 0u32);
    assert_eq!(u32::from(range.end()), 12u32);
}

// ============================================================================
// Tree Navigation Tests
// ============================================================================

#[test]
fn test_parent_navigation() {
    let root = parse("class Foo { int x; }");

    // Find a child node
    if let Some(child) = root.children().next() {
        // Child should have root as parent
        assert_eq!(child.parent().as_ref(), Some(&root));
    }
}

#[test]
fn test_siblings_navigation() {
    let root = parse("class Foo {} class Bar {}");

    let children: Vec<_> = root.children().collect();
    if children.len() >= 2 {
        let first = &children[0];
        let second = &children[1];

        // Check next sibling
        assert!(first.next_sibling().is_some());

        // Check previous sibling
        assert!(second.prev_sibling().is_some());
    }
}

#[test]
fn test_ancestors() {
    let root = parse("class Foo { void bar() {} }");

    // Find a deeply nested node
    let mut deep_node: Option<SyntaxNode> = None;
    for descendant in root.descendants() {
        if descendant.kind() == SyntaxKind::BLOCK_STMT {
            deep_node = Some(descendant);
            break;
        }
    }

    if let Some(node) = deep_node {
        // Check ancestors
        let ancestors: Vec<_> = node.ancestors().collect();
        assert!(ancestors.len() > 1, "Should have multiple ancestors");

        // Root should be in ancestors
        assert!(ancestors
            .iter()
            .any(|n| n.kind() == SyntaxKind::SOURCE_FILE));
    }
}

#[test]
fn test_first_and_last_child() {
    let root = parse("class Foo { int x; int y; }");

    if let Some(class_node) = root.children().next() {
        assert!(class_node.first_child().is_some());
        assert!(class_node.last_child().is_some());
    }
}

// ============================================================================
// Descendant Iteration Tests
// ============================================================================

#[test]
fn test_descendants() {
    let root = parse("class Foo { void bar() {} }");

    assert!(
        root.descendants().count() > 3,
        "Should have multiple descendants"
    );
}

#[test]
fn test_descendants_with_tokens() {
    let root = parse("class Foo {}");

    assert!(
        root.descendants_with_tokens().count() > 4,
        "Should include both nodes and tokens"
    );
}

#[test]
fn test_preorder_traversal() {
    let root = parse("class Foo {}");

    let mut count = 0;
    for _ in root.preorder() {
        count += 1;
    }

    assert!(count > 0, "Preorder should visit nodes");
}

#[test]
fn test_preorder_with_tokens() {
    let root = parse("class Foo {}");

    assert!(root.preorder_with_tokens().next().is_some());
}

// ============================================================================
// Token Tests
// ============================================================================

#[test]
fn test_find_tokens() {
    let root = parse("class Foo {}");

    let has_token = root
        .descendants_with_tokens()
        .find_map(|elem| elem.into_token())
        .is_some();

    assert!(has_token, "Should have tokens");
}

#[test]
fn test_keyword_tokens() {
    let root = parse("public class Foo {}");

    let class_keyword = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::CLASS_KW);

    assert!(class_keyword.is_some());
    assert_eq!(class_keyword.unwrap().text(), "class");
}

#[test]
fn test_identifier_tokens() {
    let root = parse("class Foo {}");

    let ident = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    assert!(ident.is_some());
    assert_eq!(ident.unwrap().text(), "Foo");
}

// ============================================================================
// Trivia Preservation Tests
// ============================================================================

#[test]
fn test_whitespace_is_preserved() {
    let source = "class   Foo   {   }";
    let root = parse(source);

    let text = root.text().to_string();
    assert_eq!(text, source, "Whitespace should be preserved exactly");
}

#[test]
fn test_line_comments_preserved() {
    let source = "// comment\nclass Foo {}";
    let root = parse(source);

    let has_comment = root
        .descendants_with_tokens()
        .any(|e| matches!(e.kind(), SyntaxKind::LINE_COMMENT));

    assert!(has_comment, "Line comments should be in the tree");
}

#[test]
fn test_block_comments_preserved() {
    let source = "/* comment */ class Foo {}";
    let root = parse(source);

    let has_comment = root
        .descendants_with_tokens()
        .any(|e| matches!(e.kind(), SyntaxKind::BLOCK_COMMENT));

    assert!(has_comment, "Block comments should be in the tree");
}

#[test]
fn test_javadoc_comments_preserved() {
    let source = "/** javadoc */ class Foo {}";
    let root = parse(source);

    let has_doc = root
        .descendants_with_tokens()
        .any(|e| matches!(e.kind(), SyntaxKind::DOC_COMMENT));

    assert!(has_doc, "Javadoc comments should be in the tree");
}

#[test]
fn test_newlines_preserved() {
    let source = "class Foo {\n\n}";
    let root = parse(source);

    let text = root.text().to_string();
    assert_eq!(text, source);
}

// ============================================================================
// Typed AST Node Tests
// ============================================================================

#[test]
fn test_source_file_cast() {
    let root = parse("class Foo {}");
    let source_file = SourceFile::cast(root.clone());

    assert!(source_file.is_some());
}

#[test]
fn test_class_decl_cast() {
    let root = parse("class Foo {}");

    let class_node = root.children().find(|n| n.kind() == SyntaxKind::CLASS_DECL);

    assert!(class_node.is_some());

    let class_decl = ClassDecl::cast(class_node.unwrap());
    assert!(class_decl.is_some());
}

#[test]
fn test_class_name_extraction() {
    let root = parse("class HelloWorld {}");

    let source_file = SourceFile::cast(root).unwrap();
    let types: Vec<_> = source_file.types().collect();
    let class = types.first().unwrap();

    let name = class.name().unwrap();
    assert_eq!(name.text(), "HelloWorld");
}

#[test]
fn test_package_decl_extraction() {
    let root = parse("package com.example; class Foo {}");

    let source_file = SourceFile::cast(root).unwrap();
    let package = source_file.package().unwrap();

    assert_eq!(package.syntax().kind(), SyntaxKind::PACKAGE_DECL);
}

#[test]
fn test_import_iteration() {
    let source = r#"
        import java.util.List;
        import java.io.*;
        class Foo {}
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    assert_eq!(source_file.imports().count(), 2);
}

#[test]
fn test_static_import_detection() {
    let root = parse("import static java.lang.Math.PI; class Foo {}");

    let source_file = SourceFile::cast(root).unwrap();
    let import = source_file.imports().next().unwrap();

    assert!(import.is_static());
}

#[test]
fn test_wildcard_import_detection() {
    let root = parse("import java.util.*; class Foo {}");

    let source_file = SourceFile::cast(root).unwrap();
    let import = source_file.imports().next().unwrap();

    assert!(import.is_wildcard());
}

// ============================================================================
// Class Member Access Tests
// ============================================================================

#[test]
fn test_field_iteration() {
    let source = r#"
        class Foo {
            int x;
            String s;
            boolean flag;
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = source_file.types().next() {
        assert_eq!(class_decl.fields().count(), 3);
    };
}

#[test]
fn test_method_iteration() {
    let source = r#"
        class Foo {
            void method1() {}
            int method2() { return 0; }
            String method3() { return ""; }
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = source_file.types().next() {
        assert_eq!(class_decl.methods().count(), 3);
    };
}

#[test]
fn test_nested_class_iteration() {
    let source = r#"
        class Outer {
            class Inner1 {}
            class Inner2 {}
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = source_file.types().next() {
        assert_eq!(class_decl.nested_types().count(), 2);
    };
}

// ============================================================================
// Modifier Tests
// ============================================================================

#[test]
fn test_public_modifier_detection() {
    let root = parse("public class Foo {}");

    let source_file = SourceFile::cast(root).unwrap();
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = source_file.types().next() {
        if let Some(modifiers) = class_decl.modifiers() {
            assert!(modifiers.is_public());
        }
    };
}

#[test]
fn test_static_modifier_detection() {
    let source = r#"
        class Outer {
            static class Nested {}
        }
    "#;
    let root = parse(source);

    // Find the nested class
    let nested_class = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CLASS_DECL)
        .nth(1); // Second class declaration

    if let Some(node) = nested_class {
        let class_decl = ClassDecl::cast(node).unwrap();
        if let Some(modifiers) = class_decl.modifiers() {
            assert!(modifiers.is_static());
        }
    }
}

#[test]
fn test_final_modifier_detection() {
    let root = parse("public final class Foo {}");

    let source_file = SourceFile::cast(root).unwrap();
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = source_file.types().next() {
        if let Some(modifiers) = class_decl.modifiers() {
            assert!(modifiers.is_final());
        }
    };
}

#[test]
fn test_abstract_modifier_detection() {
    let root = parse("abstract class Foo {}");

    let source_file = SourceFile::cast(root).unwrap();
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = source_file.types().next() {
        if let Some(modifiers) = class_decl.modifiers() {
            assert!(modifiers.is_abstract());
        }
    };
}

// ============================================================================
// Text Range and Position Tests
// ============================================================================

#[test]
fn test_node_positions_are_accurate() {
    let source = "class Foo {}";
    let root = parse(source);

    // Find the class keyword token
    let class_kw = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::CLASS_KW)
        .unwrap();

    let range = class_kw.text_range();
    assert_eq!(u32::from(range.start()), 0u32);
    assert_eq!(u32::from(range.end()), 5u32);
}

#[test]
fn test_identifier_position() {
    let source = "class Foo {}";
    let root = parse(source);

    let ident = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .unwrap();

    let range = ident.text_range();
    let start: u32 = range.start().into();
    let end: u32 = range.end().into();

    assert_eq!(&source[start as usize..end as usize], "Foo");
}

#[test]
fn test_nested_node_ranges() {
    let source = "class Foo { int x; }";
    let root = parse(source);

    // Find class body
    let class_body = root.descendants().find(|n| n.kind() == SyntaxKind::L_BRACE);

    if let Some(node) = class_body {
        let range = node.text_range();
        assert!(range.start() < range.end());
    }
}

// ============================================================================
// Tree Equality and Cloning Tests
// ============================================================================

#[test]
fn test_node_clone() {
    let root = parse("class Foo {}");
    let cloned = root.clone();

    assert_eq!(root.kind(), cloned.kind());
    assert_eq!(root.text().to_string(), cloned.text().to_string());
}

#[test]
fn test_node_equality() {
    let root = parse("class Foo {}");
    let cloned = root.clone();

    assert_eq!(root, cloned);
}

#[test]
fn test_different_nodes_not_equal() {
    let root1 = parse("class Foo {}");
    let root2 = parse("class Bar {}");

    assert_ne!(root1, root2);
}

// ============================================================================
// Query and Search Tests
// ============================================================================

#[test]
fn test_find_all_identifiers() {
    let source = "class Foo { int bar; void baz() {} }";
    let root = parse(source);

    let ident_count = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .count();

    assert!(ident_count >= 3); // Foo, bar, baz
}

#[test]
fn test_find_all_keywords() {
    let source = "public class Foo { public static void main() {} }";
    let root = parse(source);

    let keyword_count = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind().is_keyword())
        .count();

    assert!(keyword_count >= 4); // public, class, public, static, void
}

#[test]
fn test_find_methods_by_name() {
    let source = r#"
        class Foo {
            void test() {}
            void test2() {}
            void production() {}
        }
    "#;
    let root = parse(source);

    let test_method_count = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::METHOD_DECL)
        .filter_map(MethodDecl::cast)
        .filter(|m| m.name().is_some_and(|name| name.text().contains("test")))
        .count();

    assert_eq!(test_method_count, 2);
}

// ============================================================================
// Complex Tree Structure Tests
// ============================================================================

#[test]
fn test_deeply_nested_structure() {
    let source = r#"
        class Outer {
            class Middle {
                class Inner {
                    void method() {
                        if (true) {
                            while (condition) {
                                doSomething();
                            }
                        }
                    }
                }
            }
        }
    "#;
    let root = parse(source);

    // Check we can navigate through deep nesting
    let depth = root
        .descendants()
        .map(|n| n.ancestors().count())
        .max()
        .unwrap_or(0);

    assert!(depth > 5, "Should have deep nesting");
}

#[test]
fn test_multiple_top_level_classes() {
    let source = r#"
        class Foo {}
        class Bar {}
        class Baz {}
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    assert_eq!(source_file.types().count(), 3);
}

#[test]
fn test_generic_type_structure() {
    let source = "class Foo<T extends List<String>> {}";
    let root = parse(source);

    // Check that type parameters are in the tree
    let has_type_params = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TYPE_PARAM_LIST);

    assert!(has_type_params);
}

// ============================================================================
// Error Node Tests
// ============================================================================

#[test]
fn test_error_nodes_for_invalid_syntax() {
    let source = "class Foo { @@@ }";
    let root = parse(source);

    let has_errors = root
        .descendants_with_tokens()
        .any(|e| e.kind() == SyntaxKind::ERROR);

    assert!(has_errors, "Should have error nodes for invalid syntax");
}

// ============================================================================
// Real-World Code Structure Tests
// ============================================================================

#[test]
fn test_hello_world_structure() {
    let source = r#"
        public class HelloWorld {
            public static void main(String[] args) {
                System.out.println("Hello, World!");
            }
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();

    // Should have exactly one class
    let types: Vec<_> = source_file.types().collect();
    assert_eq!(types.len(), 1);

    // Class should have one method
    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = types.first() {
        let methods: Vec<_> = class_decl.methods().collect();
        assert_eq!(methods.len(), 1);

        // Method should be named "main"
        assert_eq!(methods[0].name().unwrap().text(), "main");
    }
}

#[test]
fn test_interface_structure() {
    let source = r#"
        interface Runnable {
            void run();
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    assert_eq!(source_file.types().count(), 1);
}

#[test]
fn test_enum_structure() {
    let source = r#"
        enum Color {
            RED,
            GREEN,
            BLUE
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    assert_eq!(source_file.types().count(), 1);
}

#[test]
fn test_record_structure() {
    let source = "record Point(int x, int y) {}";
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();
    assert_eq!(source_file.types().count(), 1);
}

#[test]
fn test_comprehensive_class_structure() {
    let source = r#"
        package com.example;

        import java.util.*;

        @Entity
        public final class User {
            private static final long serialVersionUID = 1L;

            @Id
            private Long id;

            private String name;

            static {
                System.out.println("Static init");
            }

            public User(Long id, String name) {
                this.id = id;
                this.name = name;
            }

            public Long getId() {
                return id;
            }

            public String getName() {
                return name;
            }

            @Override
            public String toString() {
                return "User(" + id + ", " + name + ")";
            }

            private class Inner {
                void method() {}
            }

            public static class Builder {
                public User build() { return null; }
            }
        }
    "#;
    let root = parse(source);

    let source_file = SourceFile::cast(root).unwrap();

    // Should have package
    assert!(source_file.package().is_some());

    // Should have imports
    assert!(source_file.imports().next().is_some());

    // Should have one class
    let types: Vec<_> = source_file.types().collect();
    assert_eq!(types.len(), 1);

    if let Some(hyperion_parser::syntax::TypeDecl::Class(class_decl)) = types.first() {
        // Should have fields
        assert!(class_decl.fields().next().is_some());

        // Should have methods
        assert!(class_decl.methods().next().is_some());

        // Should have nested classes
        assert!(class_decl.nested_types().next().is_some());
    }
}

#[test]
fn test_lexer_byte_positions_for_identical_lines() {
    use hyperion_parser::lexer::Lexer;

    let source = "class A {\n    void m() {}\n    void m() {}\n}";

    println!("\nSource: {:?}", source);
    println!("Source length: {} bytes", source.len());

    let lexer = Lexer::new(source);
    let mut method_count = 0;

    for (kind, text, range) in lexer {
        if text == "m" && kind == SyntaxKind::IDENT {
            method_count += 1;
            let start: u32 = range.start().into();
            let end: u32 = range.end().into();

            // Verify by extracting from source
            let extracted = &source[start as usize..end as usize];

            println!(
                "  Method {} 'm' at bytes {}..{}, extracted='{}'",
                method_count, start, end, extracted
            );

            assert_eq!(
                text, extracted,
                "Lexer byte position mismatch for method {}! Range {}..{} gives '{}' instead of '{}'",
                method_count, start, end, extracted, text
            );
        }
    }

    assert_eq!(method_count, 2, "Should find exactly 2 methods");
}

#[test]
fn test_parser_syntax_tree_positions() {
    use hyperion_parser::lexer::Lexer;
    use hyperion_parser::parser::Parser;

    let source = "class A {\n    void m() {}\n    void m() {}\n}";

    println!("\nSource: {:?}", source);

    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    println!("\nFinding METHOD_DECL nodes in syntax tree:");

    let mut method_count = 0;
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            method_count += 1;

            // Find the method name identifier
            for child in node.descendants_with_tokens() {
                if let Some(token) = child.as_token() {
                    if token.kind() == SyntaxKind::IDENT && token.text() == "m" {
                        let range = token.text_range();
                        let start: u32 = range.start().into();
                        let end: u32 = range.end().into();

                        let extracted = &source[start as usize..end as usize];

                        println!(
                            "  Method {} 'm' at bytes {}..{}, extracted='{}', matches={}",
                            method_count,
                            start,
                            end,
                            extracted,
                            token.text() == extracted
                        );

                        assert_eq!(
                            token.text(),
                            extracted,
                            "Syntax tree position mismatch for method {}!",
                            method_count
                        );
                    }
                }
            }
        }
    }

    assert_eq!(method_count, 2, "Should find exactly 2 methods");
}

#[test]
fn test_parser_complex_source_positions() {
    use hyperion_parser::lexer::Lexer;
    use hyperion_parser::parser::Parser;

    // Complex methods
    let source = r#"class Launcher {
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
}"#;

    println!("\n=== PARSER COMPLEX SOURCE TEST ===");
    println!("Source len: {} bytes", source.len());

    // Show expected positions
    println!("Expected: byte 53 for first startup, byte 120 for second startup");
    println!("Bytes at positions:");
    println!("  [53]: {:?}", source.as_bytes().get(53));
    println!("  [120]: {:?}", source.as_bytes().get(120));
    println!("  [121]: {:?}", source.as_bytes().get(121));

    let tokens: Vec<_> = Lexer::new(source).collect();

    // First, verify lexer byte positions
    println!("\nLexer token positions for 'startup':");
    for (kind, text, range) in &tokens {
        if *text == "startup" {
            let start: u32 = range.start().into();
            let end: u32 = range.end().into();
            let extracted = &source[start as usize..end as usize];
            println!(
                "  {:?} '{}' at bytes {}..{}, extracted='{}', matches={}",
                kind,
                text,
                start,
                end,
                extracted,
                *text == extracted
            );
        }
    }
    let (green_node, _errors) = Parser::parse(tokens.clone());
    let root = SyntaxNode::new_root(green_node);

    // Walk ALL tokens to find the drift
    println!("\nAll syntax tree tokens (near methods):");
    let mut cumulative_pos = 0u32;
    for (i, (kind, text, range)) in tokens.iter().enumerate() {
        let lexer_start: u32 = range.start().into();
        if cumulative_pos != lexer_start {
            println!(
                "  ** DRIFT at token {}: cumulative={} vs lexer={} (diff={})",
                i,
                cumulative_pos,
                lexer_start,
                (cumulative_pos as i32) - (lexer_start as i32)
            );
        }
        if *text == "startup"
            || *text == "throws"
            || *text == "Exception"
            || *text == "{"
            || *text == "}"
        {
            println!(
                "  [{:3}] {:?} '{}' len={} cumulative={} lexer={}",
                i,
                kind,
                text,
                text.len(),
                cumulative_pos,
                lexer_start
            );
        }
        cumulative_pos += text.len() as u32;
    }

    // Walk syntax tree tokens to find where extra byte is
    println!("\nSyntax tree tokens (ALL from byte 50 to 100):");
    let mut prev_end = 0u32;
    let mut lexer_idx = 0usize;
    for token in root
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
    {
        let range = token.text_range();
        let start: u32 = range.start().into();
        let end: u32 = range.end().into();
        let text = token.text();

        // Token ranges in the lossless tree should be contiguous and cover the full source.
        // If this fails, it indicates drift between lexer accounting and CST offsets.
        assert_eq!(
            start,
            prev_end,
            "Non-contiguous token range for {:?} {:?}: expected start {} but got {}",
            token.kind(),
            text,
            prev_end,
            start
        );

        // Find matching lexer token
        while lexer_idx < tokens.len() {
            let (_, ltext, _) = &tokens[lexer_idx];
            if *ltext == text {
                break;
            }
            lexer_idx += 1;
        }
        let lexer_pos = if lexer_idx < tokens.len() {
            let r = &tokens[lexer_idx].2;
            let s: u32 = r.start().into();
            lexer_idx += 1;
            s
        } else {
            999
        };

        // Show tokens around the method
        if (50..=100).contains(&start) {
            let drift = start as i32 - lexer_pos as i32;
            let drift_marker = if drift != 0 {
                format!(" *** DRIFT={}", drift)
            } else {
                String::new()
            };
            println!(
                "  {:?} {:?} at {}..{} (lexer={}){}",
                token.kind(),
                text,
                start,
                end,
                lexer_pos,
                drift_marker
            );
        }
        prev_end = end;
    }

    assert_eq!(
        prev_end as usize,
        source.len(),
        "Syntax tree tokens should cover full source: expected end {} but got {}",
        source.len(),
        prev_end
    );

    println!("\nFinding METHOD_DECL nodes:");

    let mut method_count = 0;
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            method_count += 1;

            // Find the method name identifier (identifier followed by L_PAREN)
            let node_tokens: Vec<_> = node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .collect();

            for (i, token) in node_tokens.iter().enumerate() {
                if token.kind() == SyntaxKind::IDENT && token.text() == "startup" {
                    if let Some(next) = node_tokens.get(i + 1) {
                        if next.kind() == SyntaxKind::L_PAREN {
                            let range = token.text_range();
                            let start: u32 = range.start().into();
                            let end: u32 = range.end().into();

                            let extracted = &source[start as usize..end as usize];

                            println!(
                                "  Method {} 'startup' at bytes {}..{}, extracted='{}', matches={}",
                                method_count,
                                start,
                                end,
                                extracted,
                                token.text() == extracted
                            );

                            assert_eq!(
                                token.text(),
                                extracted,
                                "Parser position mismatch for method {}!",
                                method_count
                            );
                        }
                    }
                }
            }
        }
    }

    assert_eq!(method_count, 2, "Should find exactly 2 methods");
}

// ============================================================================
// Annotation .class Suffix Bug Tests
// ============================================================================

#[test]
fn test_annotation_enum_no_drift() {
    // Test: Annotation with enum constant (no .class suffix)
    // Should NOT have position drift from parser backtracking on the DOT
    let source = r#"@MyAnnotation(value = MyEnum.VALUE, other = "test")
class Test {
    void method1() {}
    void method2() {}
}"#;

    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green_node, _) = Parser::parse(tokens.clone());
    let root = SyntaxNode::new_root(green_node);

    let mut method_count = 0;
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            method_count += 1;

            for token in node.descendants() {
                if token.kind() == SyntaxKind::IDENT && token.text() == "method" {
                    let range = token.text_range();
                    let start = usize::from(range.start());
                    let end = usize::from(range.end());

                    let extracted = &source[start..end];
                    assert_eq!(
                        extracted, "method",
                        "Method {} at byte {} has drift (extracted='{}')",
                        method_count, start, extracted
                    );
                }
            }
        }
    }

    assert_eq!(method_count, 2, "Should find exactly 2 methods");
}

#[test]
fn test_annotation_binary_expr_with_enum_no_drift() {
    // Test: Annotation with binary expression containing enum
    // This exercises the .class suffix lookahead in binary expression handler
    let source = r#"@MyAnnotation(value = "prefix" + MyEnum.VALUE + "suffix")
class Test {
    void methodA() {}
    void methodB() {}
}"#;

    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green_node, _) = Parser::parse(tokens.clone());
    let root = SyntaxNode::new_root(green_node);

    let mut method_count = 0;
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            method_count += 1;

            for token in node.descendants() {
                if token.kind() == SyntaxKind::IDENT {
                    let text_str = token.text().to_string();
                    if text_str.starts_with("method") {
                        let range = token.text_range();
                        let start = usize::from(range.start());
                        let end = usize::from(range.end());

                        let extracted = &source[start..end];
                        assert_eq!(
                            extracted,
                            if method_count == 1 {
                                "methodA"
                            } else {
                                "methodB"
                            },
                            "Method {} at byte {} has drift (extracted='{}')",
                            method_count,
                            start,
                            extracted
                        );
                    }
                }
            }
        }
    }

    assert_eq!(method_count, 2, "Should find exactly 2 methods");
}

#[test]
fn test_annotation_with_class_literal_parsing() {
    // Test: Annotation with .class suffix DOES work
    let source = r#"@MyAnnotation(value = String.class)
class Test {
    void method() {}
}"#;

    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green_node, _) = Parser::parse(tokens.clone());
    let root = SyntaxNode::new_root(green_node);

    // Should parse without errors
    let mut found_method = false;
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            found_method = true;
        }
    }
    assert!(
        found_method,
        "Should successfully parse annotation with .class literal"
    );
}
