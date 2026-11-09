// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scala Parser using tree-sitter
//!
//! This module provides a tree-sitter-based parser for Scala source code.
//! The parser produces a concrete syntax tree (CST) that can be traversed
//! to extract symbols, navigate definitions, and perform semantic analysis.
//!
//! The tree-sitter grammar provides:
//! - Robust parsing of Scala 2 and Scala 3 syntax
//! - Accurate symbol extraction with proper scoping
//! - Support for complex Scala constructs (nested classes, companion objects, etc.)
//! - Precise text ranges for navigation

use std::mem;
use tree_sitter::{Language, Node, Parser, Tree, TreeCursor};

const PARSE_ERROR_PREVIEW_CHARS: usize = 160;

/// Parse result containing the syntax tree and any errors
#[derive(Debug)]
pub struct ParseResult {
    pub tree: Tree,
    pub errors: Vec<ParseError>,
}

/// Parse error with location information
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

/// Parse Scala source code into a concrete syntax tree
///
/// # Arguments
///
/// * `source` - The Scala source code to parse
///
/// # Returns
///
/// A `ParseResult` containing the syntax tree and any parse errors
///
/// # Errors
///
/// Returns an error if the tree-sitter parser fails to initialize
pub fn parse(source: &str) -> anyhow::Result<ParseResult> {
    let mut parser = Parser::new();

    // tree_sitter_scala::LANGUAGE is a LanguageFn (wrapper around unsafe C function)
    // Call it to get the language pointer, then wrap in Language
    let language_fn = tree_sitter_scala::LANGUAGE;
    let language_ptr = unsafe { language_fn.into_raw()() };

    // Language is repr(transparent) over *const ffi::TSLanguage
    // We have *const () from the C function, which is equivalent
    let language = unsafe { mem::transmute::<*const (), Language>(language_ptr) };

    let result = parser.set_language(&language);
    mem::forget(language); // Don't call Drop since we don't own the pointer

    result.map_err(|e| anyhow::anyhow!("Failed to set Scala language: {e}"))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse Scala source"))?;

    // Collect parse errors by traversing the tree looking for ERROR nodes
    let errors = collect_errors(&tree, source);

    Ok(ParseResult { tree, errors })
}

/// Collect all ERROR nodes from the parse tree
fn collect_errors(tree: &Tree, source: &str) -> Vec<ParseError> {
    let mut errors = Vec::new();
    let root = tree.root_node();

    let mut cursor = root.walk();
    collect_errors_recursive(&mut cursor, source, &mut errors);

    errors
}

/// Recursively collect ERROR nodes
fn collect_errors_recursive(cursor: &mut TreeCursor, source: &str, errors: &mut Vec<ParseError>) {
    let node = cursor.node();

    if node.is_error() || node.kind() == "ERROR" {
        let start = node.start_position();
        let text = node
            .utf8_text(source.as_bytes())
            .unwrap_or("<invalid utf8>");
        let preview = parse_error_preview(text);

        errors.push(ParseError {
            message: format!("Syntax error: unexpected '{preview}'"),
            byte_offset: node.start_byte(),
            line: start.row,
            column: start.column,
        });
    }

    // Recurse into children
    if cursor.goto_first_child() {
        loop {
            collect_errors_recursive(cursor, source, errors);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn parse_error_preview(text: &str) -> String {
    let Some((end, _)) = text.char_indices().nth(PARSE_ERROR_PREVIEW_CHARS) else {
        return text.to_owned();
    };
    format!("{}… ({} bytes total)", &text[..end], text.len())
}

/// Helper trait for convenient node access
pub trait NodeExt {
    /// Get child node by field name
    fn child_by_field(&self, field_name: &str) -> Option<Node<'_>>;

    /// Get text content of this node
    fn text<'a>(&self, source: &'a str) -> &'a str;

    /// Check if node kind matches
    fn is_kind(&self, kind: &str) -> bool;

    /// Get all children of a specific kind
    fn children_by_kind(&self, kind: &str) -> Vec<Node<'_>>;
}

impl NodeExt for Node<'_> {
    fn child_by_field(&self, field_name: &str) -> Option<Node<'_>> {
        self.child_by_field_name(field_name)
    }

    fn text<'a>(&self, source: &'a str) -> &'a str {
        self.utf8_text(source.as_bytes()).unwrap_or("")
    }

    fn is_kind(&self, kind: &str) -> bool {
        self.kind() == kind
    }

    fn children_by_kind(&self, kind: &str) -> Vec<Node<'_>> {
        let mut cursor = self.walk();
        let mut result = Vec::new();

        if cursor.goto_first_child() {
            loop {
                let node = cursor.node();
                if node.kind() == kind {
                    result.push(node);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_class() {
        let source = "class Foo";
        let result = parse(source).expect("Parse failed");
        assert_eq!(result.errors.len(), 0, "Should have no parse errors");

        let root = result.tree.root_node();
        assert_eq!(root.kind(), "compilation_unit");
    }

    #[test]
    fn parse_error_preview_is_utf8_safe_and_bounded() {
        let text = "😀".repeat(PARSE_ERROR_PREVIEW_CHARS + 40);
        let preview = parse_error_preview(&text);

        assert!(preview.starts_with(&"😀".repeat(PARSE_ERROR_PREVIEW_CHARS)));
        assert!(preview.ends_with(&format!("({} bytes total)", text.len())));
        assert!(preview.len() < text.len());
    }

    #[test]
    fn test_parse_class_with_method() {
        let source = r#"
            class User {
              def getName(): String = "test"
            }
        "#;
        let result = parse(source).expect("Parse failed");
        assert_eq!(result.errors.len(), 0, "Should have no parse errors");
    }

    #[test]
    fn test_parse_object() {
        let source = "object Utils";
        let result = parse(source).expect("Parse failed");
        assert_eq!(result.errors.len(), 0, "Should have no parse errors");
    }

    #[test]
    fn test_parse_trait() {
        let source = "trait Configurable";
        let result = parse(source).expect("Parse failed");
        assert_eq!(result.errors.len(), 0, "Should have no parse errors");
    }

    #[test]
    fn test_parse_package() {
        let source = "package com.example\n\nclass Foo";
        let result = parse(source).expect("Parse failed");
        assert_eq!(result.errors.len(), 0, "Should have no parse errors");
    }

    #[test]
    fn test_parse_with_syntax_error() {
        let source = "class { invalid }";
        let result = parse(source).expect("Parse should not fail on syntax errors");
        assert!(!result.errors.is_empty(), "Should detect syntax errors");
    }

    #[test]
    fn test_node_ext_text() {
        let source = "class Foo";
        let result = parse(source).expect("Parse failed");
        let root = result.tree.root_node();

        let text = root.text(source);
        assert_eq!(text, source);
    }
}
