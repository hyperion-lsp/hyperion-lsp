// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constructor completion generator
//!
//! Generates completions for constructor calls (after 'new' keyword)
//! Provides all available constructors with parameter information.

use super::context::{CompletionGenerator, CompletionGeneratorContext};
use crate::completion::type_utils;
use crate::position_utils::position_to_offset;
use hyperion_workspace::SymbolLocation;
use std::collections::HashSet;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind, SymbolKind,
};
use tracing::debug;

/// Generator for constructor completions
pub struct ConstructorCompletionGenerator;

impl Default for ConstructorCompletionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstructorCompletionGenerator {
    pub const fn new() -> Self {
        Self
    }
}

impl CompletionGenerator for ConstructorCompletionGenerator {
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        // Extract the prefix being typed
        let offset = position_to_offset(context.source_text, context.position)?;
        let before_cursor = &context.source_text[..offset];

        // Find the start of the current identifier
        let identifier_start = before_cursor
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(0, |i| i + 1);

        let prefix = &context.source_text[identifier_start..offset];

        debug!("Constructor completion with prefix: '{}'", prefix);

        // Try to infer expected type from variable declaration
        let expected_type = type_utils::infer_expected_constructor_type(before_cursor);
        debug!("Inferred expected type: {:?}", expected_type);

        // For constructor completion, allow empty prefix but limit results
        let limit = if prefix.is_empty() { 30 } else { 50 };

        // Helper to check if a class is abstract (non-instantiable)
        let is_abstract = |fqn: &str| -> bool {
            context
                .workspace_index
                .get_symbol(fqn)
                .is_some_and(|sym| sym.modifiers.is_abstract)
        };

        let mut results: Vec<_> = Vec::new();
        let mut groups: std::collections::HashMap<String, (u8, u8)> =
            std::collections::HashMap::new();

        // If we inferred an expected type, add it first
        if let Some(ref expected) = expected_type {
            let fqns = context.workspace_index.get_fqns_by_simple_name(expected);
            for fqn in fqns {
                if let Some(loc) = context.workspace_index.find_symbol(&fqn) {
                    // Filter out abstract classes - they cannot be instantiated
                    if matches!(loc.kind, SymbolKind::CLASS | SymbolKind::ENUM)
                        && !is_abstract(&loc.fqn)
                    {
                        debug!(
                            "Adding expected type completion: {} (fqn={})",
                            loc.name, loc.fqn
                        );
                        results.push(loc);
                    }
                }
            }
        }

        if prefix.is_empty() {
            // No prefix: get workspace (non-external) classes directly
            // Instead of search("") which scans + lowercases all 100K+ symbols,
            // iterate symbol_locations with early exit at limit
            let excluded_fqns: HashSet<_> = results
                .iter()
                .map(|location: &SymbolLocation| location.fqn.clone())
                .collect();
            let workspace_classes = context
                .workspace_index
                .get_workspace_instantiable_class_locations(limit, &excluded_fqns);
            debug!(
                "Found {} workspace classes for empty-prefix constructor",
                workspace_classes.len()
            );
            results.extend(workspace_classes);
            debug!(
                "After filtering, {} constructor-eligible classes",
                results.len()
            );
        } else {
            // With prefix: the shared, uncapped type search (prefix and camel
            // humps, JDK internals excluded, java.lang/java.util first).
            let imports = context.workspace_index.get_imports(context.file_id);
            let mut seen_fqns: HashSet<_> = results.iter().map(|loc| loc.fqn.clone()).collect();
            for candidate in crate::completion::type_search::search_types(
                &context.workspace_index,
                imports.as_ref(),
                prefix,
                200,
            ) {
                groups.insert(
                    candidate.symbol.fqn.to_string(),
                    (candidate.group, candidate.hint),
                );
                if !matches!(candidate.symbol.kind, SymbolKind::CLASS | SymbolKind::ENUM)
                    || candidate.symbol.modifiers.is_abstract
                    || candidate.outer.is_some()
                {
                    continue;
                }
                if let Some(location) = context.workspace_index.find_symbol(&candidate.symbol.fqn) {
                    if seen_fqns.insert(location.fqn.clone()) {
                        results.push(location);
                    }
                }
            }
        }

        debug!(
            "Found {} constructor-eligible symbols matching '{}'",
            results.len(),
            prefix
        );

        // Convert to completion items - ONE item per constructor overload
        let completions: Vec<CompletionItem> = results
            .into_iter()
            .take(limit)
            .flat_map(|loc| constructor_items(context, &loc, &groups))
            .collect();

        debug!("Returning {} constructor completions", completions.len());
        Ok(completions)
    }
}

/// The constructor completion items of one class: one per overload, or a
/// single default-constructor item when none is declared. Generic classes get
/// the diamond (`ArrayList<>()`).
fn constructor_items(
    context: &CompletionGeneratorContext<'_>,
    loc: &SymbolLocation,
    groups: &std::collections::HashMap<String, (u8, u8)>,
) -> Vec<CompletionItem> {
    let kind = match loc.kind {
        SymbolKind::ENUM => CompletionItemKind::ENUM,
        _ => CompletionItemKind::CONSTRUCTOR,
    };
    let class_fqn = loc.fqn.to_string();
    let symbol = context.workspace_index.get_symbol(&class_fqn);
    let diamond = if symbol
        .as_ref()
        .is_some_and(|symbol| !symbol.type_parameters.is_empty())
    {
        "<>"
    } else {
        ""
    };
    let deprecated = symbol.as_ref().is_some_and(|symbol| symbol.is_deprecated);
    let description = crate::completion::type_search::package_of(&class_fqn).to_string();
    let (group, hint) = groups
        .get(&class_fqn)
        .copied()
        .unwrap_or((crate::completion::metadata::rank_group::NEAR_TYPE, 100));

    let make = |label: String, detail: String, parameters: &[String], order: usize| {
        let metadata = crate::completion::metadata::CompletionItemMetadata::new(&loc.name)
            .with_fqn(class_fqn.clone())
            .with_import_fqn(class_fqn.clone())
            .with_group(group)
            .with_type_hint(hint);
        let (insert_text, format) = if parameters.is_empty() {
            (
                format!("{}{diamond}()", loc.name),
                InsertTextFormat::PLAIN_TEXT,
            )
        } else if context.snippets {
            let placeholders: Vec<String> = parameters
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let name = p.split_whitespace().last().unwrap_or("arg");
                    format!("${{{}:{}}}", i + 1, name.replace(['$', '}', '\\'], ""))
                })
                .collect();
            (
                format!("{}{diamond}({})$0", loc.name, placeholders.join(", ")),
                InsertTextFormat::SNIPPET,
            )
        } else {
            (
                format!("{}{diamond}()", loc.name),
                InsertTextFormat::PLAIN_TEXT,
            )
        };
        CompletionItem {
            label,
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: Some(description.clone()),
            }),
            kind: Some(kind),
            detail: Some(detail),
            documentation: loc.documentation.as_ref().map(|doc| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: doc.to_string(),
                })
            }),
            preselect: None,
            // Fewer parameters first
            sort_text: Some(format!("1_{}_{order:03}", loc.name)),
            filter_text: Some(loc.name.to_string()),
            insert_text: Some(insert_text),
            insert_text_format: Some(format),
            insert_text_mode: None,
            text_edit: None,
            additional_text_edits: None,
            command: None,
            commit_characters: None,
            data: metadata.to_value(),
            tags: deprecated.then(|| vec![tower_lsp::lsp_types::CompletionItemTag::DEPRECATED]),
            deprecated: deprecated.then_some(true),
        }
    };

    let external = context.workspace_index.is_external_file(loc.file_id);
    let mut constructors = context
        .workspace_index
        .get_constructors_for_class(&class_fqn);
    // Bytecode always declares its constructors, so a jar class with none
    // indexed is not instantiable here; a private one never is.
    if external && constructors.is_empty() {
        return Vec::new();
    }
    let declared = constructors.len();
    constructors.retain(|ctor| {
        !ctor.modifiers.is_private
            && (!external || ctor.modifiers.is_public || ctor.modifiers.is_protected)
    });
    if declared > 0 && constructors.is_empty() {
        return Vec::new();
    }
    if constructors.is_empty() {
        return vec![make(
            format!("{}{diamond}()", loc.name),
            "Default constructor".to_string(),
            &[],
            0,
        )];
    }
    constructors
        .iter()
        .map(|ctor| {
            let parameters: Vec<String> = ctor.parameters.iter().map(|p| p.to_string()).collect();
            let types: Vec<String> = parameters
                .iter()
                .map(|p| {
                    let type_part = p.split_whitespace().next().unwrap_or("Object");
                    crate::completion::generators::member_access::shorten_type(type_part)
                })
                .collect();
            let detail = match parameters.len() {
                0 => "No parameters".to_string(),
                1 => "1 parameter".to_string(),
                n => format!("{n} parameters"),
            };
            make(
                format!("{}{diamond}({})", loc.name, types.join(", ")),
                detail,
                &parameters,
                parameters.len(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::db::{FileId, Language, SourceDatabase};
    use hyperion_workspace::index::WorkspaceIndex;
    use lru::LruCache;
    use parking_lot::Mutex;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn test_constructor_prefix_completion_uses_workspace_index() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let file1 = root.join("SystemConfig.java");
        let file1_source = r"
                package com.example;
                public class SystemConfig {
                    public SystemConfig() {}
                }
            ";
        std::fs::write(&file1, file1_source).unwrap();

        let file2 = root.join("SystemConsole.java");
        let file2_source = r"
                package com.example;
                public class SystemConsole {
                    public SystemConsole() {}
                }
            ";
        std::fs::write(&file2, file2_source).unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file_with_source(&file1, file1_source).unwrap();
        index.index_file_with_source(&file2, file2_source).unwrap();

        let index = Arc::new(index);
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&index));
        let file_id = FileId::new(0);
        let source_text = "new systemco".to_string();
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source_text.clone()));
            db.set_file_language(file_id, Language::Java);
        });

        let txn = host.read_now();
        let ctx = CompletionGeneratorContext::new(
            txn.snapshot(),
            index,
            file_id,
            Position::new(0, source_text.len() as u32),
            &source_text,
            Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(100).unwrap()))),
        );

        let generator = ConstructorCompletionGenerator::new();
        let completions = generator.generate(&ctx).unwrap();
        let labels: Vec<String> = completions.into_iter().map(|c| c.label).collect();

        assert!(labels.iter().any(|l| l.starts_with("SystemConfig(")));
        assert!(labels.iter().any(|l| l.starts_with("SystemConsole(")));
    }
}
