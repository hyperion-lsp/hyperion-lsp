// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scala-specific code completion
//!
//! This module provides completion items for Scala language constructs:
//! - Keywords (class, object, trait, def, val, var, etc.)
//! - Type keywords (String, Int, Boolean, etc.)
//! - Snippets (main, match, case class, etc.)

use std::sync::OnceLock;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Cached Scala keyword completions
static SCALA_KEYWORD_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Cached Scala type-related keyword completions
static SCALA_TYPE_KEYWORD_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Cached Scala snippet completions
static SCALA_SNIPPET_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Get Scala keyword completions (cached)
pub fn keyword_completions() -> Vec<CompletionItem> {
    SCALA_KEYWORD_COMPLETIONS
        .get_or_init(|| {
            let keywords = vec![
                // Type declarations
                ("class", "Declare a class"),
                ("trait", "Declare a trait"),
                ("object", "Declare an object (singleton)"),
                ("case class", "Declare a case class"),
                ("sealed trait", "Declare a sealed trait"),
                ("sealed class", "Declare a sealed class"),
                // Modifiers
                ("abstract", "Abstract modifier"),
                ("final", "Final modifier"),
                ("private", "Private visibility modifier"),
                ("protected", "Protected visibility modifier"),
                ("override", "Override modifier"),
                ("implicit", "Implicit modifier"),
                ("lazy", "Lazy evaluation modifier"),
                // Property declarations
                ("val", "Immutable value"),
                ("var", "Mutable variable"),
                // Function declaration
                ("def", "Function or method declaration"),
                // Control flow
                ("if", "If expression"),
                ("else", "Else clause"),
                ("match", "Pattern matching expression"),
                ("case", "Case clause"),
                ("while", "While loop"),
                ("do", "Do-while loop"),
                ("for", "For comprehension"),
                ("yield", "Yield expression in for comprehension"),
                ("return", "Return statement"),
                // Exception handling
                ("try", "Try block"),
                ("catch", "Catch block"),
                ("finally", "Finally block"),
                ("throw", "Throw expression"),
                // Other keywords
                ("this", "This reference"),
                ("super", "Super reference"),
                ("null", "Null literal"),
                ("true", "Boolean true"),
                ("false", "Boolean false"),
                ("package", "Package declaration"),
                ("import", "Import declaration"),
                ("type", "Type alias declaration"),
                ("extends", "Inheritance/mixin"),
                ("with", "Mixin trait"),
                ("new", "Object instantiation"),
            ];

            keywords
                .into_iter()
                .map(|(keyword, detail)| CompletionItem {
                    label: keyword.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(detail.to_string()),
                    insert_text: Some(keyword.to_string()),
                    ..Default::default()
                })
                .collect()
        })
        .clone()
}

/// Get Scala type keyword completions (cached)
pub fn type_keyword_completions() -> Vec<CompletionItem> {
    SCALA_TYPE_KEYWORD_COMPLETIONS
        .get_or_init(|| {
            let type_keywords = vec![
                ("String", "String type"),
                ("Int", "32-bit signed integer"),
                ("Long", "64-bit signed integer"),
                ("Short", "16-bit signed integer"),
                ("Byte", "8-bit signed integer"),
                ("Boolean", "Boolean type (true/false)"),
                ("Char", "16-bit Unicode character"),
                ("Float", "32-bit floating point"),
                ("Double", "64-bit floating point"),
                ("Unit", "Unit type (like void)"),
                ("Any", "Top type (supertype of all types)"),
                ("AnyRef", "Top type for reference types"),
                ("AnyVal", "Top type for value types"),
                ("Nothing", "Bottom type (subtype of all types)"),
                ("Null", "Null type"),
                ("Option", "Optional value"),
                ("Some", "Some value"),
                ("None", "No value"),
                ("List", "Immutable list"),
                ("Seq", "Sequence"),
                ("Set", "Set"),
                ("Map", "Map"),
                ("Array", "Mutable array"),
                ("Vector", "Immutable vector"),
                ("Either", "Either type"),
                ("Left", "Left value"),
                ("Right", "Right value"),
                ("Try", "Try type for exception handling"),
                ("Success", "Success value"),
                ("Failure", "Failure value"),
            ];

            type_keywords
                .into_iter()
                .map(|(keyword, detail)| CompletionItem {
                    label: keyword.to_string(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(detail.to_string()),
                    insert_text: Some(keyword.to_string()),
                    ..Default::default()
                })
                .collect()
        })
        .clone()
}

/// Get Scala snippet completions (cached)
pub fn snippet_completions() -> Vec<CompletionItem> {
    SCALA_SNIPPET_COMPLETIONS
        .get_or_init(|| {
            vec![
                CompletionItem {
                    label: "main".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Main method".to_string()),
                    insert_text: Some(
                        "def main(args: Array[String]): Unit = {\n\t$0\n}".to_string(),
                    ),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "println".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Print line".to_string()),
                    insert_text: Some("println($0)".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "match".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Match expression".to_string()),
                    insert_text: Some("match {\n\tcase $1 => $0\n}".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "for".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("For comprehension".to_string()),
                    insert_text: Some("for {\n\t$1 <- $2\n} yield $0".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "class".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Class declaration".to_string()),
                    insert_text: Some("class $1 {\n\t$0\n}".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "case class".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Case class declaration".to_string()),
                    insert_text: Some("case class $1($2)".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "object".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Object declaration (singleton)".to_string()),
                    insert_text: Some("object $1 {\n\t$0\n}".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "trait".to_string(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    detail: Some("Trait declaration".to_string()),
                    insert_text: Some("trait $1 {\n\t$0\n}".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
            ]
        })
        .clone()
}

/// Check if a character is valid in a Scala identifier
pub fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}
