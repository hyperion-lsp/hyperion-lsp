// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Production parsing paths must handle failure explicitly. Tests intentionally
// use unwrap when reaching the asserted value is itself the test contract.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

//! Hyperion Parser - High-performance parsing for JVM languages
//!
//! This crate provides lexical analysis and parsing for Java and other JVM languages.
//! It builds lossless, immutable syntax trees using the rowan library, supporting:
//!
//! - Error-resilient parsing that handles malformed/incomplete code
//! - Incremental reparsing for efficient text edits
//! - Full fidelity syntax trees (preserves whitespace, comments, etc.)
//! - Support for Java 8-24 language features
//!
//! # Architecture
//!
//! The parser is structured in layers:
//! - **Lexer**: Tokenizes source code using logos
//! - **Parser**: Builds concrete syntax trees (CST) using rowan
//! - **AST**: Provides typed views over the CST
//!
//! # Performance Target
//!
//! Parse 1000 lines of code in <5ms

/// Syntax tree node types (SyntaxKind enum - source of truth)
pub mod syntax;

/// Token classification utilities
pub mod token;

/// Lexical analysis module
pub mod lexer;

/// Parsing module
pub mod parser;

/// Incremental parsing for efficient text edits
pub mod incremental;

/// Utility functions for working with tokens and syntax
pub mod utils;

/// Modifier and annotation lookup for declarations
pub mod modifiers;

// Re-export commonly used types
pub use incremental::{incremental_parse, TextEdit};
pub use lexer::Lexer;
pub use parser::{ParseError, ParseMode, Parser};
pub use syntax::{JavaLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
#[allow(deprecated)]
pub use utils::{is_identifier_kind, is_identifier_token};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_end_to_end_parse() {
        // End-to-end test: source -> lexer -> parser -> syntax tree
        let source = "class HelloWorld { }";

        // Lex
        let tokens = Lexer::new(source).collect();

        // Parse
        let (green_node, errors) = Parser::parse(tokens);

        // Should parse without errors
        assert!(errors.is_empty(), "Expected no parse errors");

        // Convert to syntax node (red tree)
        let root = SyntaxNode::new_root(green_node);

        // Root should be SOURCE_FILE
        assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
    }
}
