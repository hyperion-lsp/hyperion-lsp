// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol completion generator
//!
//! Generates completions for symbols defined in the current file
//! (classes, methods, fields, variables, etc.)

use super::context::{CompletionGenerator, CompletionGeneratorContext};
use hyperion_semantics::db::{ParseDatabase, SemanticDatabase, SymbolKind};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
};

/// Generator for file-scoped symbol completions
pub struct SymbolCompletionGenerator;

impl Default for SymbolCompletionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolCompletionGenerator {
    pub const fn new() -> Self {
        Self
    }

    /// Extract documentation for a symbol by parsing its source file
    fn extract_symbol_documentation(
        &self,
        symbol: &hyperion_semantics::db::Symbol,
        context: &CompletionGeneratorContext<'_>,
    ) -> Option<Documentation> {
        use crate::hover::documentation::extract_documentation;

        // Use the pre-acquired snapshot from context
        let parse_tree = context.snapshot.parse_file(symbol.file);
        let root = parse_tree.syntax();

        // Find the node at the symbol's offset
        let offset = rowan::TextSize::from(symbol.offset);
        let token = root.token_at_offset(offset).right_biased()?;

        // Walk up to find the declaration node
        let mut node = token.parent();
        while let Some(current) = node {
            // Check if this is a declaration node
            use hyperion_parser::syntax::SyntaxKind::{
                CLASS_DECL, CONSTRUCTOR_DECL, ENUM_DECL, FIELD_DECL, INTERFACE_DECL, METHOD_DECL,
            };
            match current.kind() {
                CLASS_DECL | INTERFACE_DECL | ENUM_DECL | METHOD_DECL | CONSTRUCTOR_DECL
                | FIELD_DECL => {
                    // Found a declaration, extract its documentation
                    if let Some(doc_text) = extract_documentation(&current) {
                        return Some(Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: doc_text,
                        }));
                    }
                    return None;
                }
                _ => {
                    node = current.parent();
                }
            }
        }

        None
    }
}

impl CompletionGenerator for SymbolCompletionGenerator {
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        // Use the pre-acquired snapshot from context
        let file_symbols = context.snapshot.file_symbols(context.file_id);

        let completions = file_symbols
            .symbols
            .iter()
            .map(|symbol| {
                let kind = match symbol.kind {
                    SymbolKind::Class => CompletionItemKind::CLASS,
                    SymbolKind::Interface => CompletionItemKind::INTERFACE,
                    SymbolKind::Enum => CompletionItemKind::ENUM,
                    SymbolKind::Method => CompletionItemKind::METHOD,
                    SymbolKind::Field => CompletionItemKind::FIELD,
                    SymbolKind::Variable => CompletionItemKind::VARIABLE,
                    SymbolKind::Parameter => CompletionItemKind::VARIABLE,
                    SymbolKind::Package => CompletionItemKind::MODULE,
                };

                let kind_str = format!("{:?}", symbol.kind);

                // Extract documentation for this symbol
                let documentation = self.extract_symbol_documentation(symbol, context);

                // Create metadata for ranking cache
                // Note: File-scoped symbols don't have FQN readily available,
                // but we can still cache alphabetical score and type info
                let metadata =
                    crate::completion::metadata::CompletionItemMetadata::new(&symbol.name)
                        .with_semantic_id(format!(
                            "local:{:?}:{}:{:?}",
                            context.file_id, symbol.offset, symbol.kind
                        ));

                CompletionItem {
                    label: symbol.name.to_string(),
                    label_details: None,
                    kind: Some(kind),
                    detail: Some(format!("{} - {}", kind_str, symbol.name)),
                    documentation,
                    preselect: None,
                    sort_text: Some(format!("1_{}", symbol.name)), // Sort symbols after keywords
                    filter_text: None,
                    insert_text: Some(symbol.name.to_string()),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    insert_text_mode: None,
                    text_edit: None,
                    additional_text_edits: None,
                    command: None,
                    commit_characters: None,
                    data: metadata.to_value(),
                    tags: if symbol.is_deprecated {
                        Some(vec![lsp_types::CompletionItemTag::DEPRECATED])
                    } else {
                        None
                    },
                    deprecated: Some(symbol.is_deprecated),
                }
            })
            .collect();

        Ok(completions)
    }
}
