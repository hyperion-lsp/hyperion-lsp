// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin-specific code completion
//!
//! This module provides completion items for Kotlin language constructs:
//! - Keywords (class, fun, val, var, etc.)
//! - Type keywords (String, Int, Boolean, etc.)
//! - Snippets (main, when, data class, etc.)

use std::sync::OnceLock;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Cached Kotlin keyword completions
static KOTLIN_KEYWORD_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Cached Kotlin type-related keyword completions
static KOTLIN_TYPE_KEYWORD_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Cached Kotlin snippet completions
static KOTLIN_SNIPPET_COMPLETIONS: OnceLock<Vec<CompletionItem>> = OnceLock::new();

/// Get Kotlin keyword completions (cached)
pub fn keyword_completions() -> Vec<CompletionItem> {
    KOTLIN_KEYWORD_COMPLETIONS
        .get_or_init(|| {
            let keywords = vec![
                // Type declarations
                ("class", "Declare a class"),
                ("interface", "Declare an interface"),
                ("object", "Declare an object (singleton)"),
                ("data class", "Declare a data class"),
                ("sealed class", "Declare a sealed class"),
                ("enum class", "Declare an enum class"),
                ("annotation class", "Declare an annotation class"),
                // Modifiers
                ("public", "Public visibility modifier"),
                ("private", "Private visibility modifier"),
                ("protected", "Protected visibility modifier"),
                ("internal", "Internal visibility modifier (module-level)"),
                ("open", "Open modifier (allows inheritance)"),
                ("abstract", "Abstract modifier"),
                ("final", "Final modifier (default for classes)"),
                ("override", "Override modifier"),
                ("suspend", "Suspend modifier (coroutine function)"),
                ("inline", "Inline modifier"),
                ("infix", "Infix modifier"),
                ("operator", "Operator modifier"),
                ("lateinit", "Late-initialized property"),
                ("const", "Compile-time constant"),
                ("companion", "Companion object modifier"),
                // Property declarations
                ("val", "Read-only property"),
                ("var", "Mutable property"),
                // Function declaration
                ("fun", "Function declaration"),
                // Control flow
                ("if", "If expression"),
                ("else", "Else clause"),
                ("when", "When expression (pattern matching)"),
                ("while", "While loop"),
                ("do", "Do-while loop"),
                ("for", "For loop"),
                ("break", "Break statement"),
                ("continue", "Continue statement"),
                ("return", "Return statement"),
                // Exception handling
                ("try", "Try block"),
                ("catch", "Catch block"),
                ("finally", "Finally block"),
                ("throw", "Throw expression"),
                // Type checks and casts
                ("is", "Type check operator"),
                ("!is", "Negated type check operator"),
                ("as", "Type cast operator"),
                ("as?", "Safe type cast operator"),
                ("in", "In operator (membership/range)"),
                ("!in", "Not in operator"),
                // Other keywords
                ("this", "This reference"),
                ("super", "Super reference"),
                ("null", "Null literal"),
                ("true", "Boolean true"),
                ("false", "Boolean false"),
                ("package", "Package declaration"),
                ("import", "Import declaration"),
                ("typealias", "Type alias declaration"),
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

/// Get Kotlin type-related keyword completions (cached)
pub fn type_keyword_completions() -> Vec<CompletionItem> {
    KOTLIN_TYPE_KEYWORD_COMPLETIONS
        .get_or_init(|| {
            let keywords = vec![
                // Type declarations
                ("class", "Declare a class"),
                ("interface", "Declare an interface"),
                ("object", "Declare an object"),
                ("data class", "Declare a data class"),
                ("sealed class", "Declare a sealed class"),
                ("enum class", "Declare an enum class"),
                // Basic types
                ("String", "String type"),
                ("Int", "32-bit integer type"),
                ("Long", "64-bit integer type"),
                ("Short", "16-bit integer type"),
                ("Byte", "8-bit integer type"),
                ("Double", "64-bit floating point type"),
                ("Float", "32-bit floating point type"),
                ("Boolean", "Boolean type"),
                ("Char", "Character type"),
                ("Unit", "Unit type (void equivalent)"),
                ("Any", "Root of Kotlin type hierarchy"),
                ("Nothing", "Bottom type (never returns)"),
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

/// Get Kotlin snippet completions (cached)
pub fn snippet_completions() -> Vec<CompletionItem> {
    KOTLIN_SNIPPET_COMPLETIONS
        .get_or_init(|| {
            vec![
                // Main function
                ("main", "fun main() {\n\t${0}\n}", "Main function"),
                (
                    "maina",
                    "fun main(args: Array<String>) {\n\t${0}\n}",
                    "Main function with args",
                ),
                // Print statements
                ("println", "println(${1:value})", "Print line to console"),
                ("print", "print(${1:value})", "Print to console"),
                // Control flow
                ("if", "if (${1:condition}) {\n\t${0}\n}", "If expression"),
                (
                    "ifelse",
                    "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}",
                    "If-else expression",
                ),
                (
                    "when",
                    "when (${1:expression}) {\n\t${2:value} -> ${3}\n\telse -> ${0}\n}",
                    "When expression",
                ),
                (
                    "while",
                    "while (${1:condition}) {\n\t${0}\n}",
                    "While loop",
                ),
                (
                    "for",
                    "for (${1:item} in ${2:collection}) {\n\t${0}\n}",
                    "For loop",
                ),
                (
                    "fori",
                    "for (${1:i} in 0 until ${2:n}) {\n\t${0}\n}",
                    "For loop with index",
                ),
                // Exception handling
                (
                    "try",
                    "try {\n\t${1}\n} catch (${2:e}: ${3:Exception}) {\n\t${0}\n}",
                    "Try-catch block",
                ),
                (
                    "tryf",
                    "try {\n\t${1}\n} catch (${2:e}: ${3:Exception}) {\n\t${4}\n} finally {\n\t${0}\n}",
                    "Try-catch-finally block",
                ),
                // Class templates
                ("class", "class ${1:ClassName} {\n\t${0}\n}", "Class declaration"),
                (
                    "data",
                    "data class ${1:ClassName}(${2:val property: Type})",
                    "Data class declaration",
                ),
                ("sealed", "sealed class ${1:ClassName}", "Sealed class declaration"),
                (
                    "object",
                    "object ${1:ObjectName} {\n\t${0}\n}",
                    "Object declaration",
                ),
                ("companion", "companion object {\n\t${0}\n}", "Companion object"),
                (
                    "interface",
                    "interface ${1:InterfaceName} {\n\t${0}\n}",
                    "Interface declaration",
                ),
                // Function templates
                (
                    "fun",
                    "fun ${1:functionName}(${2}): ${3:Unit} {\n\t${0}\n}",
                    "Function declaration",
                ),
                (
                    "funp",
                    "private fun ${1:functionName}(${2}): ${3:Unit} {\n\t${0}\n}",
                    "Private function",
                ),
                (
                    "suspend",
                    "suspend fun ${1:functionName}(${2}): ${3:Unit} {\n\t${0}\n}",
                    "Suspend function",
                ),
                // Property templates
                ("val", "val ${1:name}: ${2:Type} = ${0}", "Read-only property"),
                ("var", "var ${1:name}: ${2:Type} = ${0}", "Mutable property"),
                ("lazy", "val ${1:name} by lazy {\n\t${0}\n}", "Lazy property"),
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
