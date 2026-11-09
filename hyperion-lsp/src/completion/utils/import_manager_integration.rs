// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Import manager integration for auto-importing types
//!
//! Provides utilities for adding import statements to completion items.

use crate::completion::metadata::CompletionItemMetadata;
use crate::import_manager::ImportManager;
use hyperion_semantics::db::FileId;
use std::sync::Arc;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};
use tracing::debug;

/// Add import edits to completion items that need them
///
/// Adds imports for types (classes, interfaces, enums) using:
/// 1. FQN from completion metadata (preferred - already resolved)
/// 2. Workspace lookup by simple name (fallback - only if unambiguous)
pub fn add_import_edits(
    completions: &mut [CompletionItem],
    file_id: FileId,
    source_text: &str,
    workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
) {
    let import_manager = ImportManager::new(workspace_index.clone());
    let mut insertion_position = None;
    let is_kotlin = workspace_index.get_file_path(file_id).is_some_and(|path| {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("kt" | "kts")
        )
    });

    for item in completions.iter_mut() {
        // Only add imports for type completions (not methods/fields/variables)
        if !matches!(
            item.kind,
            Some(
                CompletionItemKind::CLASS
                    | CompletionItemKind::INTERFACE
                    | CompletionItemKind::ENUM
                    | CompletionItemKind::CONSTRUCTOR
            )
        ) {
            continue;
        }

        let metadata = CompletionItemMetadata::from_item(item);
        // A nested type is written through its outer type, so the outer type
        // is what has to be imported.
        let import_target = metadata.as_ref().and_then(|m| m.import_fqn.clone());
        let simple_name_owned = import_target
            .as_deref()
            .and_then(|fqn| fqn.rsplit('.').next())
            .map_or_else(|| item.label.clone(), str::to_string);
        let simple_name = &simple_name_owned;

        // Try to get FQN from metadata first (already resolved during completion generation)
        // This handles the case where multiple classes have the same simple name
        let metadata_fqn = import_target.or_else(|| metadata.and_then(|metadata| metadata.fqn));
        let fqn = select_auto_import_fqn(metadata_fqn, || {
            workspace_index.get_fqns_by_simple_name(simple_name)
        });

        if let Some(fqn) = fqn {
            if import_manager.needs_import(file_id, simple_name, &fqn) {
                if is_kotlin {
                    item.additional_text_edits = Some(vec![
                        import_manager.generate_kotlin_import_edit(&fqn, source_text)
                    ]);
                    continue;
                }
                let position = *insertion_position
                    .get_or_insert_with(|| import_manager.find_import_insertion_point(source_text));
                if let Some(position) = position {
                    let edit = import_manager.generate_import_edit_at(&fqn, position);
                    debug!(
                        "Adding import edit for {} ({}): {:?}",
                        simple_name, fqn, edit
                    );
                    item.additional_text_edits = Some(vec![edit]);
                }
            }
        }
    }
}

fn unique_auto_import_fqn(mut candidates: Vec<String>) -> Option<String> {
    candidates.sort();
    candidates.dedup();
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn select_auto_import_fqn(
    metadata_fqn: Option<String>,
    fallback: impl FnOnce() -> Vec<String>,
) -> Option<String> {
    metadata_fqn.or_else(|| unique_auto_import_fqn(fallback()))
}

#[cfg(test)]
mod tests {
    use super::{select_auto_import_fqn, unique_auto_import_fqn};

    #[test]
    fn ambiguous_date_without_metadata_does_not_choose_an_import() {
        let candidates = vec!["java.util.Date".to_string(), "java.sql.Date".to_string()];
        assert_eq!(unique_auto_import_fqn(candidates.clone()), None);

        let mut reversed = candidates;
        reversed.reverse();
        assert_eq!(unique_auto_import_fqn(reversed), None);
    }

    #[test]
    fn exact_duplicate_candidates_are_unambiguous() {
        assert_eq!(
            unique_auto_import_fqn(vec![
                "java.util.Date".to_string(),
                "java.util.Date".to_string(),
            ]),
            Some("java.util.Date".to_string())
        );
    }

    #[test]
    fn metadata_fqn_does_not_evaluate_workspace_fallback() {
        let selected = select_auto_import_fqn(Some("java.util.Date".to_string()), || {
            panic!("metadata-backed imports must not query the workspace index")
        });
        assert_eq!(selected.as_deref(), Some("java.util.Date"));
    }
}
