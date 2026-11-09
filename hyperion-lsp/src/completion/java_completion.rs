// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java-specific code completion
//!
//! This module provides completion items for Java language constructs:
//! - Keywords (class, interface, public, static, etc.)
//! - Type keywords (int, boolean, String, void, etc.)
//! - Snippets (main, sysout, for, try, etc.)
//! - Wildcard type completions (?, ? extends, ? super)

use std::sync::OnceLock;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind,
};

/// Cached Java keyword completions
static KEYWORD_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Cached Java type-related keyword completions
static TYPE_KEYWORD_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Cached Java snippet completions
static SNIPPET_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Get Java keyword completions (cached)
pub fn keyword_completions() -> Vec<CompletionItem> {
    KEYWORD_COMPLETIONS
        .get_or_init(|| {
            let keywords = vec![
                // Type declarations
                ("class", "Declare a class"),
                ("interface", "Declare an interface"),
                ("enum", "Declare an enum"),
                ("record", "Declare a record (Java 14+)"),
                // Modifiers
                ("public", "Public access modifier"),
                ("private", "Private access modifier"),
                ("protected", "Protected access modifier"),
                ("static", "Static modifier"),
                ("final", "Final modifier"),
                ("abstract", "Abstract modifier"),
                ("synchronized", "Synchronized modifier"),
                ("volatile", "Volatile modifier"),
                ("transient", "Transient modifier"),
                ("native", "Native modifier"),
                // Control flow
                ("if", "If statement"),
                ("else", "Else clause"),
                ("while", "While loop"),
                ("do", "Do-while loop"),
                ("for", "For loop"),
                ("switch", "Switch statement"),
                ("case", "Case label"),
                ("default", "Default case"),
                ("break", "Break statement"),
                ("continue", "Continue statement"),
                ("return", "Return statement"),
                // Exception handling
                ("try", "Try block"),
                ("catch", "Catch block"),
                ("finally", "Finally block"),
                ("throw", "Throw statement"),
                ("throws", "Throws clause"),
                // Other keywords
                ("new", "Create new instance"),
                ("this", "This reference"),
                ("super", "Super reference"),
                ("null", "Null literal"),
                ("true", "Boolean true"),
                ("false", "Boolean false"),
                ("package", "Package declaration"),
                ("import", "Import declaration"),
                ("extends", "Extends clause"),
                ("implements", "Implements clause"),
                ("instanceof", "Instance of operator"),
                ("assert", "Assert statement"),
                // Primitive types
                ("boolean", "Boolean type"),
                ("byte", "Byte type"),
                ("char", "Char type"),
                ("short", "Short type"),
                ("int", "Int type"),
                ("long", "Long type"),
                ("float", "Float type"),
                ("double", "Double type"),
                ("void", "Void type"),
            ];

            keywords
                .into_iter()
                .map(|(keyword, detail)| CompletionItem {
                    label: keyword.to_string(),
                    label_details: None,
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(detail.to_string()),
                    documentation: None,
                    preselect: None,
                    sort_text: Some(format!("0_{keyword}")),
                    filter_text: None,
                    insert_text: Some(keyword.to_string()),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    insert_text_mode: None,
                    text_edit: None,
                    additional_text_edits: None,
                    command: None,
                    commit_characters: None,
                    data: None,
                    tags: None,
                    deprecated: None,
                })
                .collect()
        })
        .clone()
}

/// Get Java type-related keyword completions (cached)
pub fn type_keyword_completions() -> Vec<CompletionItem> {
    TYPE_KEYWORD_COMPLETIONS
        .get_or_init(|| {
            let keywords = vec![
                // Type declarations
                ("class", "Declare a class"),
                ("interface", "Declare an interface"),
                ("enum", "Declare an enum"),
                ("record", "Declare a record (Java 14+)"),
                // Primitive types
                ("boolean", "Boolean type"),
                ("byte", "Byte type"),
                ("char", "Char type"),
                ("short", "Short type"),
                ("int", "Int type"),
                ("long", "Long type"),
                ("float", "Float type"),
                ("double", "Double type"),
                ("void", "Void type"),
            ];

            keywords
                .into_iter()
                .map(|(keyword, detail)| CompletionItem {
                    label: keyword.to_string(),
                    label_details: None,
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(detail.to_string()),
                    documentation: None,
                    preselect: None,
                    sort_text: Some(format!("0_{keyword}")),
                    filter_text: None,
                    insert_text: Some(keyword.to_string()),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    insert_text_mode: None,
                    text_edit: None,
                    additional_text_edits: None,
                    command: None,
                    commit_characters: None,
                    data: None,
                    tags: None,
                    deprecated: None,
                })
                .collect()
        })
        .clone()
}

/// Get Java snippet completions (cached)
pub fn snippet_completions() -> Vec<CompletionItem> {
    SNIPPET_COMPLETIONS
        .get_or_init(|| {
            vec![
                // Main method
                ("main", "public static void main(String[] args) {\n\t${0}\n}", "Main method"),
                // System.out.println
                ("sysout", "System.out.println(${1:value});$0", "Print to console"),
                ("syserr", "System.err.println(${1:value});$0", "Print to error stream"),
                // For loops
                ("for", "for (${1:int i = 0}; ${2:i < n}; ${3:i++}) {\n\t${0}\n}", "For loop"),
                ("fori", "for (int ${1:i} = 0; ${1:i} < ${2:length}; ${1:i}++) {\n\t${0}\n}", "For loop with index"),
                ("foreach", "for (${1:Type} ${2:item} : ${3:collection}) {\n\t${0}\n}", "Enhanced for loop"),
                ("iter", "for (${1:Type} ${2:item} : ${3:collection}) {\n\t${0}\n}", "Enhanced for loop (alias)"),
                // Control flow
                ("if", "if (${1:condition}) {\n\t${0}\n}", "If statement"),
                ("ifelse", "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}", "If-else statement"),
                ("while", "while (${1:condition}) {\n\t${0}\n}", "While loop"),
                ("dowhile", "do {\n\t${0}\n} while (${1:condition});", "Do-while loop"),
                ("switch", "switch (${1:expression}) {\n\tcase ${2:value}:\n\t\t${3}\n\t\tbreak;\n\tdefault:\n\t\t${0}\n}", "Switch statement"),
                // Exception handling
                ("try", "try {\n\t${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}\n}", "Try-catch block"),
                ("tryf", "try {\n\t${1}\n} catch (${2:Exception} ${3:e}) {\n\t${4}\n} finally {\n\t${0}\n}", "Try-catch-finally block"),
                ("trycatch", "try {\n\t${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}\n}", "Try-catch block (alias)"),
                // Class templates
                ("class", "public class ${1:ClassName} {\n\t${0}\n}", "Public class"),
                ("interface", "public interface ${1:InterfaceName} {\n\t${0}\n}", "Public interface"),
                ("enum", "public enum ${1:EnumName} {\n\t${0}\n}", "Public enum"),
                // Method templates
                ("pubm", "public ${1:void} ${2:methodName}(${3}) {\n\t${0}\n}", "Public method"),
                ("prim", "private ${1:void} ${2:methodName}(${3}) {\n\t${0}\n}", "Private method"),
                ("prom", "protected ${1:void} ${2:methodName}(${3}) {\n\t${0}\n}", "Protected method"),
                ("pubsm", "public static ${1:void} ${2:methodName}(${3}) {\n\t${0}\n}", "Public static method"),
            ]
            .into_iter()
            .map(|(label, snippet, detail)| CompletionItem {
                label: label.to_string(),
                label_details: None,
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(detail.to_string()),
                documentation: None,
                preselect: None,
                sort_text: Some(format!("2_{label}")),
                filter_text: None,
                insert_text: Some(snippet.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                insert_text_mode: None,
                text_edit: None,
                additional_text_edits: None,
                command: None,
                commit_characters: None,
                data: None,
                tags: None,
                deprecated: None,
            })
            .collect()
        })
        .clone()
}

/// Get wildcard type completions for generic type arguments
/// Provides: ?, ? extends, ? super
pub fn wildcard_type_completions() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "?".to_string(),
            label_details: Some(CompletionItemLabelDetails {
                detail: Some(" (unbounded wildcard)".to_string()),
                description: None,
            }),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some("Unbounded wildcard type".to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "Matches any type. Used when the actual type doesn't matter.\n\n**Example:**\n```java\nList<?> list; // A list of unknown type\n```".to_string(),
            })),
            preselect: Some(true),
            sort_text: Some("0_?".to_string()),
            filter_text: Some("?".to_string()),
            insert_text: Some("?".to_string()),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            insert_text_mode: None,
            text_edit: None,
            additional_text_edits: None,
            command: None,
            commit_characters: None,
            data: None,
            tags: None,
            deprecated: None,
        },
        CompletionItem {
            label: "? extends".to_string(),
            label_details: Some(CompletionItemLabelDetails {
                detail: Some(" (upper bounded wildcard)".to_string()),
                description: None,
            }),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some("Upper bounded wildcard".to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "Matches any subtype of the specified type.\n\n**Example:**\n```java\nList<? extends Number> numbers; // List of Number or any subtype\n```".to_string(),
            })),
            preselect: None,
            sort_text: Some("0_? extends".to_string()),
            filter_text: Some("? extends".to_string()),
            insert_text: Some("? extends ${1:Type}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text_mode: None,
            text_edit: None,
            additional_text_edits: None,
            command: None,
            commit_characters: None,
            data: None,
            tags: None,
            deprecated: None,
        },
        CompletionItem {
            label: "? super".to_string(),
            label_details: Some(CompletionItemLabelDetails {
                detail: Some(" (lower bounded wildcard)".to_string()),
                description: None,
            }),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some("Lower bounded wildcard".to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "Matches any supertype of the specified type.\n\n**Example:**\n```java\nList<? super Integer> numbers; // List of Integer or any supertype\n```".to_string(),
            })),
            preselect: None,
            sort_text: Some("0_? super".to_string()),
            filter_text: Some("? super".to_string()),
            insert_text: Some("? super ${1:Type}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text_mode: None,
            text_edit: None,
            additional_text_edits: None,
            command: None,
            commit_characters: None,
            data: None,
            tags: None,
            deprecated: None,
        },
    ]
}
