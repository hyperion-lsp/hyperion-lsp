// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for annotations and modifiers on for-each loop variables
//!
//! Per JLS §14.14.2, enhanced for statements support annotations and modifiers:
//! ```java
//! for (@SuppressWarnings("unused") String item : items) { }
//! for (final String item : items) { }
//! for (@Nullable final String item : items) { }
//! ```
//!
//! Each case checks a different enhanced-loop variable declaration.

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

fn has_child_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children().any(|c| c.kind() == kind)
}

#[test]
fn test_foreach_with_annotation_simple() {
    let source = r#"
        class Test {
            void test() {
                String[] items = {"a", "b"};
                for (@SuppressWarnings("unused") String item : items) {
                    System.out.println(item);
                }
            }
        }
    "#;

    let root = parse_ok(source);

    // Should have a FOREACH_STMT node
    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(
        foreach_stmts.len(),
        1,
        "Expected 1 FOREACH_STMT, found {}",
        foreach_stmts.len()
    );

    // The FOREACH_STMT should have an ANNOTATION child
    let foreach = &foreach_stmts[0];
    assert!(
        has_child_kind(foreach, SyntaxKind::ANNOTATION),
        "FOREACH_STMT should have ANNOTATION child"
    );
}

#[test]
fn test_foreach_with_annotation_complex() {
    // An annotation argument precedes a named reference type.
    let source = r#"
        class ShelfAnnotationTest {
            void test() {
                java.util.List<LibraryBook> books = getBooks();
                for (@SuppressWarnings("UnusedLocalVariable") LibraryBook book : books) {
                    printBook(book);
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(foreach_stmts.len(), 1);

    assert!(
        has_child_kind(&foreach_stmts[0], SyntaxKind::ANNOTATION),
        "FOREACH_STMT should parse annotation correctly"
    );
}

#[test]
fn test_foreach_with_final_modifier() {
    let source = r#"
        class Test {
            void test(String[] items) {
                for (final String item : items) {
                    System.out.println(item);
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(foreach_stmts.len(), 1);

    // Check for FINAL_KW token in the foreach statement
    let foreach_text = foreach_stmts[0].text().to_string();
    assert!(
        foreach_text.contains("final"),
        "FOREACH_STMT should preserve 'final' keyword"
    );
}

#[test]
fn test_foreach_with_annotation_and_final() {
    let source = r#"
        class Test {
            void test(String[] items) {
                for (@Nullable final String item : items) {
                    if (item != null) {
                        System.out.println(item);
                    }
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(foreach_stmts.len(), 1);

    let foreach = &foreach_stmts[0];
    assert!(
        has_child_kind(foreach, SyntaxKind::ANNOTATION),
        "FOREACH_STMT should have ANNOTATION child"
    );

    let foreach_text = foreach.text().to_string();
    assert!(
        foreach_text.contains("final"),
        "FOREACH_STMT should preserve 'final' keyword"
    );
}

#[test]
fn test_foreach_with_multiple_annotations() {
    let source = r#"
        class Test {
            void test(String[] items) {
                for (@Nullable @Deprecated String item : items) {
                    process(item);
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(foreach_stmts.len(), 1);

    // Should have two ANNOTATION children
    let foreach = &foreach_stmts[0];
    let annotations: Vec<_> = foreach
        .children()
        .filter(|c| c.kind() == SyntaxKind::ANNOTATION)
        .collect();
    assert_eq!(
        annotations.len(),
        2,
        "Expected 2 annotations, found {}",
        annotations.len()
    );
}

#[test]
fn test_foreach_without_annotation_still_works() {
    // Ensure we didn't break regular for-each loops
    let source = r#"
        class Test {
            void test(String[] items) {
                for (String item : items) {
                    System.out.println(item);
                }
            }
        }
    "#;

    let root = parse_ok(source);

    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(foreach_stmts.len(), 1);
}

#[test]
fn test_traditional_for_not_affected() {
    // Make sure traditional for loops still work
    let source = r#"
        class Test {
            void test() {
                for (int i = 0; i < 10; i++) {
                    System.out.println(i);
                }
            }
        }
    "#;

    let root = parse_ok(source);

    // Should be FOR_STMT, not FOREACH_STMT
    let for_stmts = find_nodes(&root, SyntaxKind::FOR_STMT);
    assert_eq!(
        for_stmts.len(),
        1,
        "Traditional for should parse as FOR_STMT"
    );

    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(
        foreach_stmts.len(),
        0,
        "Traditional for should NOT parse as FOREACH_STMT"
    );
}

#[test]
fn test_nested_catalog_file_loops_with_split_throws_clause() {
    // Nested enhanced loops, generic parameters, a split throws clause, and a
    // constructor argument combining a literal, a method call, and a suffix.
    let source = r#"
package org.example.library;

import java.io.File;
import java.net.MalformedURLException;
import java.net.URL;
import java.util.ArrayList;
import java.util.List;

class CatalogLocations {
    static List<URL> collect(List<File> shelves, List<URL> bookmarks)
        throws MalformedURLException
    {
        List<URL> locations = new ArrayList<>();
        locations.addAll(bookmarks);
        for (File shelf : shelves)
        {
            for (File book : shelf.listFiles())
            {
                URL page = book.toURI().toURL();
                locations.add(new URL("https:" + page.toString() + "?view=cover"));
            }
        }
        return locations;
    }
}
"#;
    let root = parse_ok(source);
    let foreach_stmts = find_nodes(&root, SyntaxKind::FOREACH_STMT);
    assert_eq!(foreach_stmts.len(), 2, "Should have 2 enhanced for-loops");
}
