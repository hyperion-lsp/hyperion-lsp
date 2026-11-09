// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Workspace type completion generator
//!
//! Generates completions for types from the entire workspace
//! (classes, interfaces, enums).

use super::context::{CompletionGenerator, CompletionGeneratorContext};
use crate::completion::metadata::CompletionItemMetadata;
use crate::completion::type_search::{completion_kind, search_types, TypeCandidate};
use crate::completion::utils::extract_identifier_prefix;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemLabelDetails, CompletionItemTag, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind,
};
use tracing::debug;

/// How many types one request offers. Ranking happens before this cut, so it
/// only bounds payload size, never which class wins.
const MAX_TYPE_ITEMS: usize = 100;

/// Generator for type completions from the whole index: workspace sources,
/// dependency jars and the JDK.
pub struct WorkspaceTypeCompletionGenerator {
    annotations_only: bool,
}

impl Default for WorkspaceTypeCompletionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceTypeCompletionGenerator {
    pub const fn new() -> Self {
        Self {
            annotations_only: false,
        }
    }

    /// Offer only annotation types (after `@`).
    pub const fn annotations() -> Self {
        Self {
            annotations_only: true,
        }
    }
}

/// Build the completion item for one matched type.
pub fn type_item(candidate: &TypeCandidate) -> CompletionItem {
    let symbol = &candidate.symbol;
    let name = symbol.simple_name.to_string();
    let fqn = symbol.fqn.to_string();
    let mut metadata = CompletionItemMetadata::new(&name)
        .with_fqn(fqn.clone())
        .with_group(candidate.group)
        .with_type_hint(candidate.hint);
    // A nested type is inserted as `Outer.Inner` and imported through `Outer`.
    let insert_text = if candidate.outer.is_some() {
        let outer_path = fqn
            .strip_suffix(&format!(".{name}"))
            .unwrap_or(&fqn)
            .to_string();
        let top_level = top_level_type(&outer_path);
        let top_simple = top_level.rsplit('.').next().unwrap_or(&top_level);
        metadata = metadata.with_import_fqn(top_level.clone());
        format!("{}{}.{}", top_simple, &outer_path[top_level.len()..], name)
    } else {
        name.clone()
    };
    let description = candidate
        .outer
        .as_ref()
        .map_or_else(|| candidate.package().to_string(), ToString::to_string);
    CompletionItem {
        label: name,
        label_details: Some(CompletionItemLabelDetails {
            detail: None,
            description: Some(description),
        }),
        kind: Some(completion_kind(symbol)),
        detail: Some(fqn),
        documentation: symbol.documentation.as_ref().map(|doc| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            })
        }),
        preselect: None,
        sort_text: None,
        filter_text: None,
        insert_text: Some(insert_text),
        insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
        insert_text_mode: None,
        text_edit: None,
        additional_text_edits: None, // Filled by add_import_edits
        command: None,
        commit_characters: None,
        data: metadata.to_value(),
        tags: symbol
            .is_deprecated
            .then(|| vec![CompletionItemTag::DEPRECATED]),
        deprecated: symbol.is_deprecated.then_some(true),
    }
}

/// Outermost type of a (possibly nested) type FQN.
fn top_level_type(fqn: &str) -> String {
    let package = crate::completion::type_search::package_of(fqn);
    let rest = fqn
        .strip_prefix(package)
        .unwrap_or(fqn)
        .trim_start_matches('.');
    let outer = rest.split('.').next().unwrap_or(rest);
    if package.is_empty() {
        outer.to_string()
    } else {
        format!("{package}.{outer}")
    }
}

impl CompletionGenerator for WorkspaceTypeCompletionGenerator {
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        let prefix = extract_identifier_prefix(context.source_text, context.position);
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let imports = context.workspace_index.get_imports(context.file_id);
        let mut candidates = search_types(
            &context.workspace_index,
            imports.as_ref(),
            &prefix,
            if self.annotations_only {
                usize::MAX
            } else {
                MAX_TYPE_ITEMS
            },
        );
        if self.annotations_only {
            candidates.retain(TypeCandidate::is_annotation);
            candidates.truncate(MAX_TYPE_ITEMS);
        }
        debug!(
            "Type search for '{}': {} candidates",
            prefix,
            candidates.len()
        );
        Ok(candidates.iter().map(type_item).collect())
    }
}
