// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Will-save-wait-until handler (import organization on save).
//!
//! Open/change/close publication is owned by `server/document_sync.rs`.

use dashmap::DashMap;
use hyperion_semantics::db::{ParseDatabase, SourceDatabase};
use hyperion_workspace::Language;
use tower_lsp::lsp_types::{TextEdit, WillSaveTextDocumentParams};
use tracing::{debug, warn};

use crate::semantic_host::ReadTxn;
use crate::server::DocumentState;

/// Handle `textDocument/willSaveWaitUntil` — organize imports on save.
///
/// Returns text edits to apply before saving, or `None` if no changes needed.
pub(crate) fn handle_will_save_wait_until(
    txn: &ReadTxn,
    document_map: &DashMap<String, DocumentState>,
    params: &WillSaveTextDocumentParams,
) -> Option<Vec<TextEdit>> {
    let uri = params.text_document.uri.to_string();

    debug!("Will save wait until for: {}", uri);

    let (source_text, file_id, change_version) = if let Some(doc_state) = document_map.get(&uri) {
        if doc_state.language != Language::Java || doc_state.desynchronized {
            return None;
        }
        (
            doc_state.text.clone(),
            doc_state.file_id,
            doc_state.change_version,
        )
    } else {
        warn!("Will save requested for unopened document: {}", uri);
        return None;
    };

    if !source_text.contains("import ") {
        return None;
    }

    if txn.file_text(file_id).as_str() != source_text.as_str() {
        return None;
    }
    let root = txn.parse_file(file_id).syntax();

    use hyperion_actions::organize_imports::ImportOrganizer;
    let edits = ImportOrganizer::organize_imports(&root);

    // The parse snapshot may have been taken after a concurrent didChange,
    // while source_text still names the preceding editor revision. Never
    // convert edits from one revision using another revision's line index.
    if !document_map
        .get(&uri)
        .is_some_and(|state| state.file_id == file_id && state.change_version == change_version)
    {
        return None;
    }

    if edits.is_empty() {
        debug!("No import organization needed for {}", uri);
        return None;
    }

    use crate::position_utils::LineIndex;
    let lines = LineIndex::new(&source_text);
    let lsp_edits: Vec<TextEdit> = edits
        .into_iter()
        .map(|edit| TextEdit {
            range: lines.text_range_to_lsp_range(&source_text, edit.range),
            new_text: edit.new_text,
        })
        .collect();

    debug!("Organizing imports on save: {} edits", lsp_edits.len());
    Some(lsp_edits)
}
