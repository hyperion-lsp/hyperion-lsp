// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use hyperion_semantics::db::FileId;
#[cfg(test)]
use hyperion_semantics::db::SourceDatabase;
use hyperion_semantics::Type;
use hyperion_workspace::{FileRevisionRemovalOutcome, Language};
use lru::LruCache;
use parking_lot::Mutex;
use tokio::time::{sleep, Duration};
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    MessageType, TextEdit, Url, WillSaveTextDocumentParams,
};
use tracing::{debug, trace, warn};

use super::{parse_and_publish_diagnostics_for_uri, DocumentState};
use crate::position_utils::apply_incremental_change;
use crate::semantic_host::WriteTxn;
use crate::server::state::DiagnosticPublicationState;
use crate::server::Backend;
use crate::source_publication::{publish_semantic_revision, retire_semantic_revisions};

#[derive(Clone)]
struct DocumentPublicationContext {
    workspace_index: Arc<hyperion_workspace::WorkspaceIndex>,
    file_ids: Arc<crate::file_registry::FileIdRegistry>,
    by_uri: Arc<DashMap<String, DocumentState>>,
    open_by_id: Arc<DashMap<FileId, String>>,
    editor_owned: Arc<DashSet<FileId>>,
    next_change_version: Arc<std::sync::atomic::AtomicU64>,
    diagnostic_publication: Arc<DiagnosticPublicationState>,
    var_type_cache: Arc<Mutex<LruCache<(FileId, u32), Type>>>,
}

impl From<&Backend> for DocumentPublicationContext {
    fn from(backend: &Backend) -> Self {
        Self {
            workspace_index: Arc::clone(&backend.workspace.index),
            file_ids: Arc::clone(&backend.documents.file_ids),
            by_uri: Arc::clone(&backend.documents.by_uri),
            open_by_id: Arc::clone(&backend.documents.open_by_id),
            editor_owned: Arc::clone(&backend.documents.editor_owned),
            next_change_version: Arc::clone(&backend.documents.next_change_version),
            diagnostic_publication: Arc::clone(&backend.background_tasks.diagnostic_publication),
            var_type_cache: Arc::clone(backend.services.completion.var_type_cache()),
        }
    }
}

struct OpenDocumentOutcome {
    file_id: FileId,
    dependent_files: Vec<FileId>,
    diagnostic_publications: Vec<DiagnosticPublication>,
}

struct ChangedDocumentOutcome {
    file_id: FileId,
    dependent_files: Vec<FileId>,
    text_len: usize,
    diagnostic_publications: Vec<DiagnosticPublication>,
}

enum ChangeDocumentResult {
    Missing,
    Stale,
    Failed,
    Published(ChangedDocumentOutcome),
}

#[derive(Debug, PartialEq, Eq)]
struct DiagnosticPublication {
    uri: String,
    generation: u64,
}

const _: () = assert!(std::mem::size_of::<DiagnosticPublication>() <= 32);

struct ClosedDocumentOutcome {
    file_id: FileId,
    dependent_files: Option<Vec<FileId>>,
    /// Whether the closed path was reconciled to an exact disk/deletion
    /// revision in both the workspace index and Salsa.
    exact: bool,
}

enum CloseDocumentResult {
    Missing,
    Stale,
    Closed(ClosedDocumentOutcome),
}

enum CloseDiskAttempt {
    Closed(ClosedDocumentOutcome),
    NeedsWorkspaceRemoval,
    Stale,
}

#[cfg(test)]
fn publish_semantic_source(
    txn: &mut WriteTxn,
    ctx: &DocumentPublicationContext,
    file_id: FileId,
    text: Arc<String>,
    language: Language,
) {
    assert!(publish_semantic_revision(
        txn,
        &ctx.var_type_cache,
        file_id,
        text,
        language,
        &[],
    ));
}

fn retire_semantic_source(
    txn: &mut WriteTxn,
    ctx: &DocumentPublicationContext,
    file_id: FileId,
    language: Language,
) {
    retire_semantic_revisions(txn, &ctx.var_type_cache, &[file_id], language);
}

fn retire_displaced_document_ownership(
    ctx: &DocumentPublicationContext,
    retired_file_ids: &[FileId],
) {
    for file_id in retired_file_ids {
        ctx.file_ids.remove(*file_id);
        ctx.editor_owned.remove(file_id);
        if let Some((_, uri)) = ctx.open_by_id.remove(file_id) {
            let uri_still_names_retired_id = ctx
                .by_uri
                .get(&uri)
                .is_some_and(|entry| entry.file_id == *file_id);
            if uri_still_names_retired_id {
                ctx.by_uri.remove(&uri);
                ctx.diagnostic_publication.supersede_uri(&uri);
            }
        }
    }
}

/// Supersede and return replacement work as one operation. A source publication
/// advances workspace-wide name resolution, so even files which are not reverse
/// dependents can rebind a formerly unresolved simple name. Returning every new
/// generation prevents callers from invalidating an open URI without scheduling
/// a replacement analysis for it.
fn supersede_open_diagnostics(ctx: &DocumentPublicationContext) -> Vec<DiagnosticPublication> {
    let mut uris: Vec<_> = ctx.by_uri.iter().map(|entry| entry.key().clone()).collect();
    uris.sort_unstable();
    uris.dedup();
    uris.into_iter()
        .filter_map(|uri| {
            Some(DiagnosticPublication {
                generation: ctx.diagnostic_publication.supersede_open_uri(&uri)?,
                uri,
            })
        })
        .collect()
}

fn mark_document_desynchronized(
    entry: &mut DocumentState,
    version: i32,
    next_change_version: &std::sync::atomic::AtomicU64,
) {
    entry.version = version;
    entry.desynchronized = true;
    entry.change_version = next_change_version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

fn publish_document_change(
    txn: &mut WriteTxn,
    ctx: &DocumentPublicationContext,
    uri: &str,
    version: i32,
    changes: Vec<tower_lsp::lsp_types::TextDocumentContentChangeEvent>,
) -> ChangeDocumentResult {
    let Some(initial_file_id) = ctx.by_uri.get(uri).map(|entry| entry.file_id) else {
        return ChangeDocumentResult::Missing;
    };
    let Some(registered_path) = ctx
        .workspace_index
        .get_registered_file_path(initial_file_id)
    else {
        warn!(
            "Open document {:?} has no registered or indexed path: {}",
            initial_file_id, uri
        );
        return ChangeDocumentResult::Missing;
    };

    let path = registered_path.display_path();
    let path_key = registered_path.lookup_key();
    let path_lock = ctx.workspace_index.get_path_lock_for_key(path_key);
    let _path_guard = path_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file_lock = ctx.workspace_index.get_file_lock(initial_file_id);
    let _file_guard = file_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    if !ctx
        .workspace_index
        .is_registered_file_path_current(initial_file_id, &registered_path)
    {
        return ChangeDocumentResult::Stale;
    }

    // Read what the edit is based on, then release the document-map shard. The
    // URI publication mutex and the path/file locks held here exclude every
    // other mutation of this entry, so the shard is only locked for these two
    // short critical sections and never across the index or Salsa publication.
    struct EditBase {
        old_text: Arc<String>,
        language: Language,
        desynchronized: bool,
        change_version: u64,
    }
    let Some(base) = ctx.by_uri.get(uri).and_then(|entry| {
        if entry.file_id != initial_file_id || version <= entry.version {
            warn!(
                "Ignoring stale didChange for {}: received version {}, current version {}",
                uri, version, entry.version
            );
            return None;
        }
        Some(EditBase {
            old_text: Arc::clone(&entry.text),
            language: entry.language,
            desynchronized: entry.desynchronized,
            change_version: entry.change_version,
        })
    }) else {
        return if ctx.by_uri.contains_key(uri) {
            ChangeDocumentResult::Stale
        } else {
            ChangeDocumentResult::Missing
        };
    };
    let EditBase {
        old_text,
        language,
        desynchronized,
        change_version: base_change_version,
    } = base;
    let mark_desynchronized = || {
        if let Some(mut entry) = ctx.by_uri.get_mut(uri) {
            if entry.file_id == initial_file_id && entry.change_version == base_change_version {
                mark_document_desynchronized(&mut entry, version, &ctx.next_change_version);
            }
        }
    };

    let mut new_text = old_text.as_str().to_owned();
    let mut text_edit = None;
    let content_changes_len = changes.len();
    let changes = if desynchronized {
        let Some(first_full_change) = changes.iter().position(|change| change.range.is_none())
        else {
            mark_desynchronized();
            warn!(
                "Ignoring ranged didChange for desynchronized document {uri}; awaiting full text"
            );
            return ChangeDocumentResult::Failed;
        };
        changes.into_iter().skip(first_full_change).collect()
    } else {
        changes
    };
    for change in changes {
        if let Some(range) = change.range {
            if text_edit.is_none() && content_changes_len == 1 {
                match crate::position_utils::lsp_range_to_text_edit(&old_text, range, &change.text)
                {
                    Ok(edit) => {
                        text_edit = Some(edit);
                        debug!(
                            "Captured text edit for incremental parsing: range {:?}, inserted {} bytes",
                            range,
                            change.text.len()
                        );
                    }
                    Err(error) => {
                        debug!("Failed to convert LSP range to TextEdit: {}", error);
                    }
                }
            }

            if let Err(error) = apply_incremental_change(&mut new_text, range, &change.text) {
                // A failed edit makes the entire notification unusable. In
                // particular, the disk is not the authority for an open,
                // possibly unsaved document. Keep the last coherent editor
                // revision until a full-text change or a new didOpen arrives.
                warn!("Rejecting didChange with invalid incremental range for {uri}: {error}");
                mark_desynchronized();
                return ChangeDocumentResult::Failed;
            }
        } else {
            new_text = change.text;
            text_edit = None;
        }
    }

    let text = Arc::new(new_text);
    let (dependent_files, retired_file_ids) = match ctx
        .workspace_index
        .index_file_with_edit_and_key_and_locks_held(
            path,
            path_key,
            &text,
            initial_file_id,
            text_edit.as_ref(),
            text_edit.as_ref().map(|_| old_text.as_str()),
        ) {
        Ok(stats) => {
            debug!(
                "Re-indexed {} in workspace using {} parsing - {} symbols updated",
                path.display(),
                if text_edit.is_some() {
                    "incremental"
                } else {
                    "full"
                },
                stats.symbols_added
            );
            (stats.dependent_files, stats.retired_file_ids)
        }
        Err(error) => {
            warn!(
                "Failed to update workspace index for {}: {}",
                path.display(),
                error
            );
            mark_desynchronized();
            return ChangeDocumentResult::Failed;
        }
    };

    if !publish_semantic_revision(
        txn,
        &ctx.var_type_cache,
        initial_file_id,
        Arc::clone(&text),
        language,
        &retired_file_ids,
    ) {
        warn!(
            "Workspace revision for {} changed before semantic publication completed",
            path.display()
        );
        mark_desynchronized();
        return ChangeDocumentResult::Failed;
    }

    // The document map is the request-facing publication gate. Once its text
    // changes, the workspace index, Salsa, signatures, and completion caches
    // already describe the same revision.
    {
        let Some(mut entry) = ctx.by_uri.get_mut(uri) else {
            return ChangeDocumentResult::Missing;
        };
        if entry.file_id != initial_file_id || entry.change_version != base_change_version {
            return ChangeDocumentResult::Stale;
        }
        entry.text = Arc::clone(&text);
        entry.version = version;
        entry.desynchronized = false;
        entry.change_version = ctx
            .next_change_version
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    retire_displaced_document_ownership(ctx, &retired_file_ids);
    let diagnostic_publications = supersede_open_diagnostics(ctx);

    ChangeDocumentResult::Published(ChangedDocumentOutcome {
        file_id: initial_file_id,
        dependent_files,
        text_len: text.len(),
        diagnostic_publications,
    })
}

#[allow(clippy::too_many_arguments)]
fn publish_open_document(
    txn: &mut WriteTxn,
    ctx: &DocumentPublicationContext,
    path: &std::path::Path,
    uri: String,
    language_id: String,
    language: Language,
    version: i32,
    text: Arc<String>,
) -> Option<OpenDocumentOutcome> {
    let (path_key, path_lock) = ctx.workspace_index.establish_path_lock(path);
    let _path_guard = path_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let preferred_id = ctx.workspace_index.get_file_id_for_key(&path_key);
    let file_id = ctx
        .file_ids
        .get_or_create_from_path_with_key_and_preferred_id(path, path_key.clone(), preferred_id);
    let file_lock = ctx.workspace_index.get_file_lock(file_id);
    let _file_guard = file_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    ctx.diagnostic_publication.supersede_for_open(&uri, version);

    let unchanged_indexed_revision = ctx.workspace_index.get_file_id_for_key(&path_key)
        == Some(file_id)
        && ctx.workspace_index.is_content_unchanged(file_id, &text);
    let (dependent_files, retired_file_ids) = if unchanged_indexed_revision
        && ctx
            .workspace_index
            .adopt_caller_owned_revision_with_key_and_locks_held(path, &path_key, &text, file_id)
    {
        // Even when didOpen matches the cold disk revision byte-for-byte, the
        // editor now owns those bytes. Adoption changes only provenance, so we
        // can preserve the existing parse/index products. The revision token
        // still advances so pre-open snapshots cannot survive the authority
        // handoff, but declarations did not change and therefore no dependent
        // source needs re-indexing.
        (Vec::new(), Vec::new())
    } else {
        match ctx
            .workspace_index
            .index_file_with_key_and_locks_held(path, &path_key, &text, file_id)
        {
            Ok(stats) => {
                debug!(
                    "Indexed {} - {} symbols added",
                    path.display(),
                    stats.symbols_added
                );
                let dependent_files =
                    if unchanged_indexed_revision && stats.retired_file_ids.is_empty() {
                        // The bounded source/parse caches can evict a cold-indexed
                        // buffer before didOpen wins the file lock. Rebuilding is
                        // then necessary to establish editor authority, but the
                        // already-matching publication hash proves that this is not
                        // a declaration change and its dependents remain valid.
                        Vec::new()
                    } else {
                        stats.dependent_files
                    };
                (dependent_files, stats.retired_file_ids)
            }
            Err(error) => {
                warn!("Failed to index {}: {}", path.display(), error);
                return None;
            }
        }
    };

    if !publish_semantic_revision(
        txn,
        &ctx.var_type_cache,
        file_id,
        Arc::clone(&text),
        language,
        &retired_file_ids,
    ) {
        warn!(
            "Workspace revision for {} changed before semantic publication completed",
            path.display()
        );
        return None;
    }
    retire_displaced_document_ownership(ctx, &retired_file_ids);

    let change_version = ctx
        .next_change_version
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let state = DocumentState {
        uri: uri.clone(),
        language_id,
        language,
        version,
        desynchronized: false,
        text,
        file_id,
        change_version,
    };

    // Publish the complete URI/id pair only after index and Salsa are ready.
    // If a client opens the same URI twice without a close, retire the prior
    // ownership maps instead of leaking a ghost open document.
    if let Some(previous_uri) = ctx
        .open_by_id
        .get(&file_id)
        .map(|entry| entry.value().clone())
    {
        if previous_uri != uri {
            ctx.by_uri.remove(&previous_uri);
        }
    }
    if let Some(previous) = ctx.by_uri.insert(uri.clone(), state) {
        if previous.file_id != file_id {
            ctx.open_by_id.remove(&previous.file_id);
            ctx.editor_owned.remove(&previous.file_id);
        }
    }
    ctx.editor_owned.insert(file_id);
    ctx.open_by_id.insert(file_id, uri.clone());
    let diagnostic_publications = supersede_open_diagnostics(ctx);

    Some(OpenDocumentOutcome {
        file_id,
        dependent_files,
        diagnostic_publications,
    })
}

fn document_generation_matches(
    ctx: &DocumentPublicationContext,
    uri: &str,
    file_id: FileId,
    change_version: u64,
) -> bool {
    ctx.by_uri
        .get(uri)
        .is_some_and(|entry| entry.file_id == file_id && entry.change_version == change_version)
}

/// Remove the request-facing ownership maps while the source publication locks
/// exclude a concurrent open/change of the same path.
fn close_document_ownership(
    ctx: &DocumentPublicationContext,
    uri: &str,
    file_id: FileId,
    change_version: u64,
) -> bool {
    if !document_generation_matches(ctx, uri, file_id, change_version) {
        return false;
    }

    ctx.by_uri.remove(uri);
    if ctx
        .open_by_id
        .get(&file_id)
        .is_some_and(|entry| entry.value() == uri)
    {
        ctx.open_by_id.remove(&file_id);
    }
    ctx.editor_owned.remove(&file_id);
    true
}

fn reconcile_close_from_disk(
    txn: &mut WriteTxn,
    ctx: &DocumentPublicationContext,
    path: &Path,
    uri: &str,
    file_id: FileId,
    change_version: u64,
    language: Language,
    is_workspace_file: bool,
) -> CloseDiskAttempt {
    let path_key = ctx
        .workspace_index
        .get_file_path_key(file_id)
        .unwrap_or_else(|| hyperion_workspace::path_lookup_key(path));
    let path_lock = ctx.workspace_index.get_path_lock_for_key(&path_key);
    let _path_guard = path_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file_lock = ctx.workspace_index.get_file_lock(file_id);
    let _file_guard = file_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    if !document_generation_matches(ctx, uri, file_id, change_version) {
        return CloseDiskAttempt::Stale;
    }

    let disk_text = match hyperion_workspace::read_source_file(path) {
        Ok(text) => text,
        Err(error) if is_workspace_file && matches!(path.try_exists(), Ok(false)) => {
            debug!(
                "Workspace file disappeared before close reconciliation: {} ({})",
                path.display(),
                error
            );
            return CloseDiskAttempt::NeedsWorkspaceRemoval;
        }
        Err(error) => {
            // Non-workspace sources may be ephemeral extracted files. Keep the
            // complete editor-published semantic revision if no disk revision
            // is available. For unreadable workspace files, retaining the last
            // good revision is safer than publishing empty or partial state.
            warn!(
                "Keeping last published revision while closing unreadable source {}: {}",
                path.display(),
                error
            );
            if !close_document_ownership(ctx, uri, file_id, change_version) {
                return CloseDiskAttempt::Stale;
            }
            if !is_workspace_file {
                ctx.file_ids.remove(file_id);
            }
            return CloseDiskAttempt::Closed(ClosedDocumentOutcome {
                file_id,
                dependent_files: None,
                exact: false,
            });
        }
    };

    let disk_text = Arc::new(disk_text);
    let (disk_published, dependent_files, retired_file_ids) = if ctx
        .workspace_index
        .get_file_id_for_key(&path_key)
        == Some(file_id)
        && ctx
            .workspace_index
            .is_content_unchanged(file_id, &disk_text)
    {
        (
            ctx.workspace_index
                .adopt_disk_revision_with_key_and_locks_held(path, &path_key, &disk_text, file_id),
            None,
            Vec::new(),
        )
    } else {
        match ctx
            .workspace_index
            .index_disk_file_with_key_and_locks_held(path, &path_key, &disk_text, file_id)
        {
            Ok(stats) => (true, Some(stats.dependent_files), stats.retired_file_ids),
            Err(error) => {
                warn!(
                    "Failed to restore disk revision while closing {}: {}",
                    path.display(),
                    error
                );
                (false, None, Vec::new())
            }
        }
    };

    // Only advance Salsa to disk when the matching workspace revision is
    // published (or was already identical). A failed parse keeps the complete
    // last-good editor revision in both layers.
    let semantic_published = if disk_published {
        let published = publish_semantic_revision(
            txn,
            &ctx.var_type_cache,
            file_id,
            Arc::clone(&disk_text),
            language,
            &retired_file_ids,
        );
        retire_displaced_document_ownership(ctx, &retired_file_ids);
        published
    } else {
        false
    };

    if !close_document_ownership(ctx, uri, file_id, change_version) {
        return CloseDiskAttempt::Stale;
    }
    if !is_workspace_file {
        ctx.file_ids.remove(file_id);
    }

    CloseDiskAttempt::Closed(ClosedDocumentOutcome {
        file_id,
        dependent_files,
        exact: disk_published && semantic_published,
    })
}

fn close_document(
    txn: &mut WriteTxn,
    ctx: &DocumentPublicationContext,
    workspace_root: Option<&Path>,
    uri: &str,
) -> CloseDocumentResult {
    let Some(state) = ctx
        .by_uri
        .get(uri)
        .map(|entry| (entry.file_id, entry.change_version, entry.language))
    else {
        return CloseDocumentResult::Missing;
    };
    let (file_id, change_version, language) = state;
    let Some(path) = ctx
        .file_ids
        .get_path(file_id)
        .or_else(|| ctx.workspace_index.get_file_path(file_id))
        .or_else(|| Url::parse(uri).ok()?.to_file_path().ok())
    else {
        warn!(
            "Closing document {:?} has no registered or indexed path: {}",
            file_id, uri
        );
        return CloseDocumentResult::Missing;
    };
    // The server's configured root is the authority for close semantics. The
    // workspace index can temporarily retain an older root while initialization
    // or a workspace-folder update is being published. Compare typed identities,
    // using the file's stored key when available so a broken/retargeted display
    // alias cannot change whether its existing revision belongs to the workspace.
    let is_workspace_file = workspace_root.is_some_and(|root| {
        let root_key = hyperion_workspace::path_lookup_key(root);
        let path_key = ctx
            .workspace_index
            .get_file_path_key(file_id)
            .unwrap_or_else(|| hyperion_workspace::path_lookup_key(&path));
        path_key.starts_with(&root_key)
    });

    // A file can flip between absent and recreated while a debounced watcher
    // batch is in flight. Retry a bounded number of exact locked observations;
    // subsequent watcher events reconcile the retained revision if churn never
    // settles during this close.
    for _ in 0..8 {
        match reconcile_close_from_disk(
            txn,
            ctx,
            &path,
            uri,
            file_id,
            change_version,
            language,
            is_workspace_file,
        ) {
            CloseDiskAttempt::Closed(outcome) => return CloseDocumentResult::Closed(outcome),
            CloseDiskAttempt::Stale => return CloseDocumentResult::Stale,
            CloseDiskAttempt::NeedsWorkspaceRemoval => {}
        }

        let recreated = Cell::new(false);
        let ownership_closed = Cell::new(false);
        let path_observation_failed = Cell::new(false);
        let removal = ctx.workspace_index.remove_file_revision(
            &path,
            file_id,
            || {
                if !document_generation_matches(ctx, uri, file_id, change_version) {
                    return false;
                }
                match path.try_exists() {
                    Ok(true) => {
                        recreated.set(true);
                        return false;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        // A permission or transient I/O error is not proof that
                        // the workspace source disappeared. Close editor
                        // ownership, but retain the last complete revision.
                        warn!(
                            "Could not prove closing workspace path {} was absent: {}; retaining last revision",
                            path.display(),
                            error
                        );
                        if close_document_ownership(ctx, uri, file_id, change_version) {
                            ownership_closed.set(true);
                            path_observation_failed.set(true);
                        }
                        return false;
                    }
                }
                if close_document_ownership(ctx, uri, file_id, change_version) {
                    ownership_closed.set(true);
                    true
                } else {
                    false
                }
            },
            || {
                ctx.file_ids.remove(file_id);
                retire_semantic_source(txn, ctx, file_id, language);
            },
        );

        match removal {
            FileRevisionRemovalOutcome::Removed { dependent_files } => {
                debug_assert!(ownership_closed.get());
                return CloseDocumentResult::Closed(ClosedDocumentOutcome {
                    file_id,
                    dependent_files: Some(dependent_files),
                    exact: true,
                });
            }
            FileRevisionRemovalOutcome::Retained if recreated.get() => continue,
            FileRevisionRemovalOutcome::Retained
                if path_observation_failed.get() && ownership_closed.get() =>
            {
                return CloseDocumentResult::Closed(ClosedDocumentOutcome {
                    file_id,
                    dependent_files: None,
                    exact: false,
                });
            }
            FileRevisionRemovalOutcome::Retained => return CloseDocumentResult::Stale,
            FileRevisionRemovalOutcome::RevisionChanged => break,
        }
    }

    warn!(
        "Source path kept changing while closing {}; retaining the last complete revision",
        path.display()
    );
    let path_key = ctx
        .workspace_index
        .get_file_path_key(file_id)
        .unwrap_or_else(|| hyperion_workspace::path_lookup_key(&path));
    let path_lock = ctx.workspace_index.get_path_lock_for_key(&path_key);
    let _path_guard = path_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file_lock = ctx.workspace_index.get_file_lock(file_id);
    let _file_guard = file_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if ctx
        .workspace_index
        .get_file_path_key(file_id)
        .is_some_and(|current_key| current_key != path_key)
    {
        return CloseDocumentResult::Stale;
    }
    if !close_document_ownership(ctx, uri, file_id, change_version) {
        return CloseDocumentResult::Stale;
    }
    if !is_workspace_file {
        ctx.file_ids.remove(file_id);
    }
    CloseDocumentResult::Closed(ClosedDocumentOutcome {
        file_id,
        dependent_files: None,
        exact: false,
    })
}

impl Backend {
    fn schedule_diagnostic_publication(&self, publication: DiagnosticPublication, delay: Duration) {
        let DiagnosticPublication { uri, generation } = publication;
        let uri_clone = uri.clone();
        let client = self.client.clone();
        let document_map = self.documents.by_uri.clone();
        let host = Arc::clone(&self.workspace.host);
        let debouncer_arc = Arc::clone(&self.background_tasks.parsing);
        let workspace_ready = Arc::clone(&self.workspace.ready);
        let security_provider = Arc::clone(&self.services.security);
        let diagnostic_options = *self.workspace.diagnostic_options.read();
        let diagnostic_publication = Arc::clone(&self.background_tasks.diagnostic_publication);

        // Replacing and registering the task under one lock prevents an older
        // completion from deleting the newly scheduled handle.
        let mut debouncer = self.background_tasks.parsing.lock();
        if !diagnostic_publication.is_generation_current(&uri, generation) {
            return;
        }
        let task_seq = debouncer.get(&uri).map_or(1, |(seq, _)| seq + 1);
        if let Some((_, old_handle)) = debouncer.remove(&uri) {
            old_handle.abort();
            debug!("Cancelled pending parse for: {}", uri);
        }

        let handle = tokio::spawn(async move {
            if !delay.is_zero() {
                sleep(delay).await;
            }

            if document_map.contains_key(&uri_clone) {
                parse_and_publish_diagnostics_for_uri(
                    &uri_clone,
                    &document_map,
                    &host,
                    &client,
                    Some(&security_provider),
                    diagnostic_options,
                    Some(&workspace_ready),
                    &diagnostic_publication,
                    generation,
                    host.index().current_workspace_revision(),
                )
                .await;
            } else {
                debug!("Document no longer open: {}", uri_clone);
            }

            let mut debouncer = debouncer_arc.lock();
            if debouncer
                .get(&uri_clone)
                .is_some_and(|(current_seq, _)| *current_seq == task_seq)
            {
                debouncer.remove(&uri_clone);
                debug!("Task {} completed and removed itself", task_seq);
            }
        });

        debouncer.insert(uri, (task_seq, handle));
    }

    pub(crate) async fn handle_did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let uri = document.uri.to_string();
        let language_id = document.language_id;
        let version = document.version;
        let text = Arc::new(document.text);
        let uri_publication = self
            .background_tasks
            .diagnostic_publication
            .uri_publication(&uri);
        let document_publication_guard = uri_publication.publication.lock().await;

        debug!(
            "Document opened: {} (language: {}, version: {}, length: {} bytes)",
            uri,
            language_id,
            version,
            text.len()
        );

        let Ok(file_path) = document.uri.to_file_path() else {
            debug!("Ignoring document with unsupported URI: {}", document.uri);
            return;
        };
        let Some(language) = Language::from_path(&file_path) else {
            debug!(
                "Ignoring document with unsupported URI or extension: {}",
                document.uri
            );
            return;
        };

        debug!("Setting language for {}: {:?}", uri, language);

        let txn = self.workspace.host.write().await;
        let publication_ctx = DocumentPublicationContext::from(self);
        let publication_uri = uri.clone();
        let publication_language_id = language_id.clone();
        let publication_text = Arc::clone(&text);
        let publication_path = file_path.clone();
        let source_readiness = self
            .workspace
            .destructive_refactoring
            .begin_source_mutation([&file_path]);
        let outcome = match txn
            .run_blocking(move |txn| {
                let outcome = publish_open_document(
                    txn,
                    &publication_ctx,
                    &publication_path,
                    publication_uri,
                    publication_language_id,
                    language,
                    version,
                    publication_text,
                );
                (outcome, source_readiness)
            })
            .await
        {
            Ok((Some(outcome), source_readiness)) => (outcome, source_readiness),
            Ok((None, source_readiness)) => {
                source_readiness.finish(false);
                warn!("didOpen could not publish an exact indexed revision for {uri}");
                return;
            }
            Err(error) => {
                warn!("didOpen publication task failed for {uri}: {error}");
                return;
            }
        };
        let (mut outcome, source_readiness) = outcome;
        drop(document_publication_guard);

        if self.workspace.index.cold_declaration_pass_active()
            && !outcome.dependent_files.is_empty()
        {
            debug!(
                "Cold-build usage pass subsumes {} didOpen dependents for {}",
                outcome.dependent_files.len(),
                uri
            );
            outcome.dependent_files.clear();
        }

        // Hand readiness to the invalidation worker before any client-facing
        // await. Cancellation of this notification must not orphan a complete
        // source publication without its dependent re-index work.
        self.invalidate_dependent_file_ids_with_readiness(
            outcome.file_id,
            outcome.dependent_files,
            source_readiness,
        );

        self.log_message(
            MessageType::LOG,
            format!("Opened {language_id} document: {uri}"),
        )
        .await;

        // Prioritize fast source-root indexing for navigation requests (references/rename).
        self.maybe_index_open_file_source_root(&file_path).await;

        // If this file belongs to a nested Git repo, index that repo using a gitignore scope
        // that does not inherit ignore rules from above the repo root.
        self.maybe_index_nested_git_repo(&file_path).await;

        // The opened document remains immediate; every other open URI whose
        // authority was superseded receives a debounced replacement.
        let mut opened_publication = None;
        for publication in outcome.diagnostic_publications {
            if publication.uri == uri {
                opened_publication = Some(publication);
            } else {
                self.schedule_diagnostic_publication(publication, Duration::from_millis(75));
            }
        }
        if let Some(publication) = opened_publication {
            // A notification must never wait for semantic analysis. Large or
            // pathological files used to keep tower-lsp inside didOpen until
            // every diagnostic phase completed, backpressuring stdin and
            // making the entire server appear frozen. The existing scheduler
            // preserves generation fencing and cancellation while allowing
            // the notification handler to return as soon as source publication
            // and source-root discovery are complete.
            self.schedule_diagnostic_publication(publication, Duration::ZERO);
        } else {
            warn!("didOpen published {uri} without reserving replacement diagnostics");
        }
    }

    pub(crate) async fn handle_did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let version = params.text_document.version;
        let source_path = params.text_document.uri.to_file_path().ok();
        let uri_publication = self
            .background_tasks
            .diagnostic_publication
            .uri_publication(&uri);
        let document_publication_guard = uri_publication.publication.lock().await;

        trace!(
            uri = %uri,
            version,
            changes = params.content_changes.len(),
            "did_change received"
        );
        for (i, change) in params.content_changes.iter().enumerate() {
            let text_preview = change.text.char_indices().nth(50).map_or_else(
                || change.text.clone(),
                |(end, _)| format!("{}...", &change.text[..end]),
            );
            trace!(
                index = i,
                range = ?change.range,
                text = %text_preview,
                "did_change content"
            );
        }

        debug!(
            "Document changed: {} (version: {}, {} changes)",
            uri,
            version,
            params.content_changes.len()
        );

        let Some(_diagnostic_generation) = self
            .background_tasks
            .diagnostic_publication
            .supersede_for_change(&uri, version)
        else {
            debug!("Ignoring stale didChange diagnostic generation for {uri}: {version}");
            return;
        };
        let change_ctx = DocumentPublicationContext::from(self);
        let change_uri = uri.clone();
        let changes = params.content_changes;
        let txn = self.workspace.host.write().await;
        let source_readiness = source_path.map(|path| {
            self.workspace
                .destructive_refactoring
                .begin_source_mutation([path])
        });
        let change_result = txn
            .run_blocking(move |txn| {
                let result =
                    publish_document_change(txn, &change_ctx, &change_uri, version, changes);
                (result, source_readiness)
            })
            .await;
        drop(document_publication_guard);

        let diagnostic_publications = match change_result {
            Ok((ChangeDocumentResult::Published(outcome), source_readiness)) => {
                debug!(
                    "Document state updated: {} (new length: {} bytes)",
                    uri, outcome.text_len
                );
                if let Some(readiness) = source_readiness {
                    self.invalidate_dependent_file_ids_with_readiness(
                        outcome.file_id,
                        outcome.dependent_files,
                        readiness,
                    );
                }
                outcome.diagnostic_publications
            }
            Ok((ChangeDocumentResult::Missing, source_readiness)) => {
                if let Some(readiness) = source_readiness {
                    // No source revision was published. In particular, a
                    // spurious didChange must not recover an earlier watcher
                    // failure for the same path.
                    readiness.finish(false);
                }
                warn!("Received didChange for unopened document: {}", uri);
                Vec::new()
            }
            Ok((ChangeDocumentResult::Stale, source_readiness)) => {
                if let Some(readiness) = source_readiness {
                    readiness.finish(true);
                }
                Vec::new()
            }
            Ok((ChangeDocumentResult::Failed, source_readiness)) => {
                if let Some(readiness) = source_readiness {
                    readiness.finish(false);
                }
                warn!("didChange could not publish an exact indexed revision for {uri}");
                Vec::new()
            }
            Err(error) => {
                warn!("didChange publication task failed for {uri}: {error}");
                Vec::new()
            }
        };

        for publication in diagnostic_publications {
            self.schedule_diagnostic_publication(publication, Duration::from_millis(75));
        }
    }

    pub(crate) async fn handle_did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let uri_publication = self
            .background_tasks
            .diagnostic_publication
            .uri_publication(&uri);
        let document_publication_guard = uri_publication.publication.lock().await;
        let was_open = self.documents.by_uri.contains_key(&uri);
        let close_generation = self
            .background_tasks
            .diagnostic_publication
            .supersede_for_close(&uri);

        debug!("Document closed: {}", uri);

        // Cancel any pending parse task for this document
        {
            let mut debouncer = self.background_tasks.parsing.lock();
            if let Some((seq, handle)) = debouncer.remove(&uri) {
                handle.abort();
                debug!(
                    "Cancelled pending parse task {} for closed document: {}",
                    seq, uri
                );
            }
        }

        let close_ctx = DocumentPublicationContext::from(self);
        let close_uri = uri.clone();
        let workspace_root = self.workspace.root.read().clone();
        let txn = self.workspace.host.write().await;
        let source_readiness = was_open
            .then(|| Url::parse(&uri).ok()?.to_file_path().ok())
            .flatten()
            .map(|path| {
                self.workspace
                    .destructive_refactoring
                    .begin_source_mutation([path])
            });
        let result = txn
            .run_blocking(move |txn| {
                let result = close_document(txn, &close_ctx, workspace_root.as_deref(), &close_uri);
                (result, source_readiness)
            })
            .await;

        let mut log_closed = false;
        let should_clear = match result {
            Ok((CloseDocumentResult::Closed(outcome), source_readiness)) => {
                if let Some(dependent_files) = outcome.dependent_files {
                    if outcome.exact {
                        if let Some(readiness) = source_readiness {
                            self.invalidate_dependent_file_ids_with_readiness(
                                outcome.file_id,
                                dependent_files,
                                readiness,
                            );
                        } else {
                            self.invalidate_dependent_file_ids(outcome.file_id, dependent_files);
                        }
                    } else {
                        if let Some(readiness) = source_readiness {
                            readiness.finish(false);
                        }
                        self.invalidate_dependent_file_ids(outcome.file_id, dependent_files);
                    }
                } else if let Some(readiness) = source_readiness {
                    readiness.finish(outcome.exact);
                }
                log_closed = true;
                true
            }
            Ok((CloseDocumentResult::Missing, source_readiness)) => {
                if let Some(readiness) = source_readiness {
                    // A concurrent ownership transition prevented any exact
                    // close publication. Keep readiness conservative; unknown
                    // didClose notifications never reserve a guard at all.
                    readiness.finish(false);
                }
                warn!("Received didClose for unknown document: {}", uri);
                true
            }
            Ok((CloseDocumentResult::Stale, source_readiness)) => {
                if let Some(readiness) = source_readiness {
                    readiness.finish(true);
                }
                debug!("Ignored stale didClose for a newer document revision: {uri}");
                false
            }
            Err(error) => {
                warn!("didClose publication task failed for {uri}: {error}");
                false
            }
        };

        if should_clear
            && !self.documents.by_uri.contains_key(&uri)
            && self
                .background_tasks
                .diagnostic_publication
                .is_closed_generation_current(&uri, close_generation)
            && !self.documents.by_uri.contains_key(&uri)
        {
            if let Ok(url) = Url::parse(&uri) {
                *uri_publication.published.lock() = None;
                self.client.publish_diagnostics(url, vec![], None).await;
                debug!("Cleared diagnostics for closed document: {}", uri);
            }
        }
        drop(document_publication_guard);
        if log_closed {
            self.log_message(MessageType::LOG, format!("Closed document: {uri}"))
                .await;
        }
    }

    pub(crate) async fn handle_will_save_wait_until(
        &self,
        params: &WillSaveTextDocumentParams,
    ) -> Option<Vec<TextEdit>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        crate::handlers::notifications::handle_will_save_wait_until(
            &txn,
            &self.documents.by_uri,
            params,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use tempfile::tempdir;
    use tower_lsp::lsp_types::{
        GotoDefinitionParams, GotoDefinitionResponse, InitializeParams, InitializeResult, Position,
        Range, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        TextDocumentPositionParams, TextDocumentSaveReason, VersionedTextDocumentIdentifier,
    };
    use tower_lsp::{LanguageServer, LspService};

    fn test_client() -> tower_lsp::Client {
        struct Dummy;

        #[tower_lsp::async_trait]
        impl LanguageServer for Dummy {
            async fn initialize(
                &self,
                _: InitializeParams,
            ) -> tower_lsp::jsonrpc::Result<InitializeResult> {
                Ok(InitializeResult::default())
            }

            async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
                Ok(())
            }
        }

        let holder = Arc::new(StdMutex::new(None));
        let captured = Arc::clone(&holder);
        let (_service, _socket) = LspService::new(move |client| {
            *captured.lock().expect("client holder poisoned") = Some(client.clone());
            Dummy
        });
        let client = holder
            .lock()
            .expect("client holder poisoned")
            .take()
            .expect("client was captured");
        client
    }

    fn change_params(
        uri: Url,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> DidChangeTextDocumentParams {
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes: changes,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invalid_incremental_batch_preserves_unsaved_editor_revision() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Editor.java");
        let disk_source = "class DiskRevision {}";
        let editor_source = "class EditorRevision {}";
        let recovered_source = "class RecoveredRevision {}";
        std::fs::write(&path, disk_source).expect("write disk source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());

        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_owned(),
                    version: 1,
                    text: editor_source.to_owned(),
                },
            })
            .await;

        backend
            .handle_did_change(change_params(
                uri.clone(),
                2,
                vec![
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(0, 6), Position::new(0, 6))),
                        range_length: None,
                        text: "Partial".to_owned(),
                    },
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(50, 0), Position::new(50, 1))),
                        range_length: None,
                        text: "Invalid".to_owned(),
                    },
                ],
            ))
            .await;

        let state = backend
            .documents
            .by_uri
            .get(uri.as_str())
            .expect("open state");
        let file_id = state.file_id;
        assert_eq!(state.version, 2);
        assert!(state.desynchronized);
        assert_eq!(state.text.as_str(), editor_source);
        drop(state);
        assert!(backend.java_document(&uri, "Code actions").is_none());
        assert!(backend.has_desynchronized_open_document());
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            editor_source
        );
        assert!(backend
            .workspace
            .index
            .find_symbol("EditorRevision")
            .is_some());
        assert!(backend
            .workspace
            .index
            .find_symbol("DiskRevision")
            .is_none());
        assert!(backend
            .workspace
            .index
            .find_symbol("PartialEditorRevision")
            .is_none());

        backend
            .handle_did_change(change_params(
                uri.clone(),
                3,
                vec![TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(0, 6), Position::new(0, 20))),
                    range_length: None,
                    text: "WrongRevision".to_owned(),
                }],
            ))
            .await;
        let state = backend
            .documents
            .by_uri
            .get(uri.as_str())
            .expect("open state");
        assert_eq!(state.version, 3);
        assert!(state.desynchronized);
        assert_eq!(state.text.as_str(), editor_source);
        drop(state);
        assert!(backend
            .workspace
            .index
            .find_symbol("WrongRevision")
            .is_none());

        backend
            .handle_did_change(change_params(
                uri.clone(),
                4,
                vec![
                    TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: "class Recovered {}".to_owned(),
                    },
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(0, 15), Position::new(0, 15))),
                        range_length: None,
                        text: "Revision".to_owned(),
                    },
                ],
            ))
            .await;
        let state = backend
            .documents
            .by_uri
            .get(uri.as_str())
            .expect("open state");
        assert_eq!(state.version, 4);
        assert!(!state.desynchronized);
        assert_eq!(state.text.as_str(), recovered_source);
        drop(state);
        assert!(backend.java_document(&uri, "Code actions").is_some());
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            recovered_source
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn will_save_does_not_apply_java_import_organizer_to_kotlin() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Example.kt");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "kotlin".to_owned(),
                    version: 1,
                    text: "import kotlin.collections.List\nclass Example".to_owned(),
                },
            })
            .await;

        assert!(backend
            .handle_will_save_wait_until(&WillSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                reason: TextDocumentSaveReason::MANUAL,
            })
            .await
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn did_open_adopts_and_republishes_a_cold_workspace_id() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Cold.java");
        let disk_source = "class ColdDiskRevision {}";
        let editor_source = "class ColdEditorRevision {}";
        std::fs::write(&path, disk_source).expect("write source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        let workspace_id = hyperion_semantics::db::FileId::new(37);
        backend
            .workspace
            .index
            .index_file_with_source_and_id(&path, disk_source, workspace_id)
            .expect("cold-index source");

        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_owned(),
                    version: 1,
                    text: editor_source.to_owned(),
                },
            })
            .await;

        let uri_key = uri.to_string();
        let state = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("published document state");
        assert_eq!(state.file_id, workspace_id);
        drop(state);
        assert_eq!(
            backend.documents.file_ids.get_from_path(&path),
            Some(workspace_id)
        );
        assert_eq!(
            backend.workspace.index.get_file_id(&path),
            Some(workspace_id)
        );
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(workspace_id)
                .as_str(),
            editor_source
        );
        assert!(backend
            .workspace
            .index
            .find_symbol("ColdEditorRevision")
            .is_some());
        assert!(backend
            .workspace
            .index
            .find_symbol("ColdDiskRevision")
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unchanged_did_open_claims_the_cold_disk_revision() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Unchanged.java");
        let opened_source = "class OpenedRevision {}";
        let later_disk_source = "class LaterDiskRevision {}";
        std::fs::write(&path, opened_source).expect("write source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        backend
            .workspace
            .index
            .index_file(&path)
            .await
            .expect("cold-index disk revision");
        let workspace_id = backend
            .workspace
            .index
            .get_file_id(&path)
            .expect("cold workspace ID");

        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "java".to_owned(),
                    version: 1,
                    text: opened_source.to_owned(),
                },
            })
            .await;

        std::fs::write(&path, later_disk_source).expect("rewrite disk source");
        // Exact indexed reads prefer retained parse products. Evict both
        // bounded caches so validated retrieval must consult provenance before
        // considering the now-different disk bytes.
        for ordinal in 0..=1_000 {
            backend
                .workspace
                .index
                .index_file_with_source(
                    &temp.path().join(format!("evict/Evict{ordinal}.java")),
                    &format!("class Evict{ordinal} {{}}"),
                )
                .expect("fill bounded source caches");
        }
        backend.workspace.index.clear_file_cache();

        assert!(backend
            .workspace
            .index
            .get_validated_file_contents(workspace_id)
            .is_none());
        assert!(backend
            .workspace
            .index
            .find_symbol("OpenedRevision")
            .is_some());
        assert!(backend
            .workspace
            .index
            .find_symbol("LaterDiskRevision")
            .is_none());
    }

    #[test]
    fn unchanged_did_open_does_not_invalidate_semantic_dependents() {
        let temp = tempdir().expect("temp workspace");
        let declaration_path = temp.path().join("p/Declaration.java");
        let dependent_path = temp.path().join("p/Dependent.java");
        let declaration_source = "package p; public class Declaration {}";
        let dependent_source = "package p; class Dependent { Declaration value; }";
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        backend
            .workspace
            .index
            .index_file_with_source(&declaration_path, declaration_source)
            .expect("index declaration");
        backend
            .workspace
            .index
            .index_file_with_source(&dependent_path, dependent_source)
            .expect("index dependent");
        let declaration_id = backend
            .workspace
            .index
            .get_file_id(&declaration_path)
            .expect("declaration file id");
        assert_eq!(
            backend
                .workspace
                .index
                .get_dependent_files_for_file(declaration_id),
            vec![backend
                .workspace
                .index
                .get_file_id(&dependent_path)
                .expect("dependent file id")],
            "fixture must expose the invalidation edge"
        );

        let outcome = publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &declaration_path,
            Url::from_file_path(&declaration_path)
                .expect("declaration URI")
                .to_string(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new(declaration_source.to_owned()),
        )
        .expect("adopt unchanged editor source");

        assert!(
            outcome.dependent_files.is_empty(),
            "provenance-only adoption must not re-index unchanged dependents"
        );
    }

    #[test]
    fn unchanged_did_open_after_source_cache_eviction_skips_dependents() {
        let temp = tempdir().expect("temp workspace");
        let declaration_path = temp.path().join("p/Declaration.java");
        let dependent_path = temp.path().join("p/Dependent.java");
        let declaration_source = "package p; public class Declaration {}";
        let dependent_source = "package p; class Dependent { Declaration value; }";
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        backend
            .workspace
            .index
            .index_file_with_source(&declaration_path, declaration_source)
            .expect("index declaration");
        backend
            .workspace
            .index
            .index_file_with_source(&dependent_path, dependent_source)
            .expect("index dependent");
        let declaration_id = backend
            .workspace
            .index
            .get_file_id(&declaration_path)
            .expect("declaration file id");
        assert!(!backend
            .workspace
            .index
            .get_dependent_files_for_file(declaration_id)
            .is_empty());

        for ordinal in 0..=1_000 {
            backend
                .workspace
                .index
                .index_file_with_source(
                    &temp.path().join(format!("evict/Evict{ordinal}.java")),
                    &format!("class Evict{ordinal} {{}}"),
                )
                .expect("fill bounded source caches");
        }
        backend.workspace.index.clear_file_cache();
        assert!(backend
            .workspace
            .index
            .is_content_unchanged(declaration_id, declaration_source));

        let outcome = publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &declaration_path,
            Url::from_file_path(&declaration_path)
                .expect("declaration URI")
                .to_string(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new(declaration_source.to_owned()),
        )
        .expect("rebuild evicted unchanged source");

        assert!(
            outcome.dependent_files.is_empty(),
            "an unchanged rebuild after cache eviction must not invalidate dependents"
        );
    }

    #[test]
    fn did_open_retires_a_displaced_cold_workspace_identity() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Identity.java");
        let disk_source = concat!(
            "package cold; ",
            "class Identity { String stale() { return \"old\"; } }"
        );
        let editor_source = concat!(
            "package hot; ",
            "class Identity { String fresh() { return \"new\"; } }"
        );
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        let workspace_id = FileId::new(41);
        backend
            .workspace
            .index
            .index_file_with_source_and_id(&path, disk_source, workspace_id)
            .expect("cold-index source");
        publish_semantic_source(
            &mut backend.workspace.host.write_now(),
            &ctx,
            workspace_id,
            Arc::new(disk_source.to_owned()),
            Language::Java,
        );
        ctx.var_type_cache
            .lock()
            .put((workspace_id, 7), Type::Unknown);
        assert!(backend
            .workspace
            .host
            .member_fact_cache()
            .get_signature("cold.Identity.stale()")
            .is_some());

        // Simulate an editor binding created before the cold workspace index
        // became visible. Existing editor ownership must win, displacing the
        // workspace identity from every semantic mirror.
        let editor_id = backend.documents.file_ids.get_or_create_from_path(&path);
        assert_ne!(editor_id, workspace_id);
        let outcome = publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &path,
            "file:///Identity.java".to_owned(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new(editor_source.to_owned()),
        )
        .expect("publish editor source");

        assert_eq!(outcome.file_id, editor_id);
        assert_eq!(backend.workspace.index.get_file_id(&path), Some(editor_id));
        assert!(backend
            .workspace
            .index
            .get_file_path(workspace_id)
            .is_none());
        assert!(backend
            .workspace
            .host
            .read_now()
            .file_text(workspace_id)
            .is_empty());
        assert!(ctx.var_type_cache.lock().peek(&(workspace_id, 7)).is_none());
        let db = backend.workspace.host.member_fact_cache();
        assert!(db.get_signature("cold.Identity.stale()").is_none());
        assert!(db.get_signature("hot.Identity.fresh()").is_some());
    }

    #[test]
    fn publishing_non_java_source_retires_prior_java_signatures() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Identity.java");
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        let file_id = FileId::new(43);
        let java_source = concat!(
            "package mixed; ",
            "class Identity { String stale() { return \"old\"; } }"
        );

        backend
            .workspace
            .index
            .index_file_with_source_and_id(&path, java_source, file_id)
            .expect("index Java revision");
        publish_semantic_source(
            &mut backend.workspace.host.write_now(),
            &ctx,
            file_id,
            Arc::new(java_source.to_owned()),
            Language::Java,
        );
        assert!(backend
            .workspace
            .host
            .member_fact_cache()
            .get_signature("mixed.Identity.stale()")
            .is_some());

        let kotlin_source = "class Identity";
        backend
            .workspace
            .index
            .index_file_with_source_and_id(&path, kotlin_source, file_id)
            .expect("index Kotlin revision");
        publish_semantic_source(
            &mut backend.workspace.host.write_now(),
            &ctx,
            file_id,
            Arc::new(kotlin_source.to_owned()),
            Language::Kotlin,
        );

        assert!(backend
            .workspace
            .host
            .member_fact_cache()
            .get_signature("mixed.Identity.stale()")
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn closing_a_retained_external_source_keeps_its_semantic_revision_complete() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("ExternalProvider.java");
        let source = concat!(
            "package external; ",
            "public class ExternalProvider { public String value() { return \"x\"; } }"
        );
        std::fs::write(&path, source).expect("write source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_owned(),
                    version: 1,
                    text: source.to_owned(),
                },
            })
            .await;

        let uri_key = uri.to_string();
        let file_id = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("open state")
            .file_id;
        assert!(backend
            .workspace
            .host
            .member_fact_cache()
            .get_signature("external.ExternalProvider.value()")
            .is_some());

        backend
            .handle_did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier::new(uri),
            })
            .await;

        assert!(!backend.documents.by_uri.contains_key(&uri_key));
        assert!(backend.documents.file_ids.get_path(file_id).is_none());
        assert!(backend
            .workspace
            .index
            .find_symbol("external.ExternalProvider")
            .is_some());
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            source
        );
        assert!(
            backend
                .workspace
                .host
                .member_fact_cache()
                .get_signature("external.ExternalProvider.value()")
                .is_some(),
            "retained symbols/text must not lose their matching method signatures"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn closing_an_unsaved_workspace_buffer_restores_the_disk_revision() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Restored.java");
        let disk_source = "class DiskRevision {}";
        let editor_source = "class UnsavedEditorRevision {}";
        std::fs::write(&path, disk_source).expect("write source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        *backend.workspace.root.write() = Some(temp.path().to_path_buf());

        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_owned(),
                    version: 1,
                    text: editor_source.to_owned(),
                },
            })
            .await;
        let uri_key = uri.to_string();
        let file_id = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("open state")
            .file_id;
        assert!(backend
            .workspace
            .index
            .find_symbol("UnsavedEditorRevision")
            .is_some());

        backend
            .handle_did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier::new(uri),
            })
            .await;

        assert!(!backend.documents.by_uri.contains_key(&uri_key));
        assert!(backend
            .workspace
            .index
            .find_symbol("UnsavedEditorRevision")
            .is_none());
        assert!(backend
            .workspace
            .index
            .find_symbol("DiskRevision")
            .is_some());
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            disk_source
        );
        assert_eq!(
            backend.documents.file_ids.get_from_path(&path),
            Some(file_id)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn closing_a_deleted_workspace_buffer_retires_its_semantic_revision() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Removed.java");
        let source = concat!(
            "package removed; ",
            "class Removed { String value() { return \"gone\"; } }"
        );
        std::fs::write(&path, source).expect("write source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        *backend.workspace.root.write() = Some(temp.path().to_path_buf());

        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_owned(),
                    version: 1,
                    text: source.to_owned(),
                },
            })
            .await;
        let uri_key = uri.to_string();
        let file_id = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("open state")
            .file_id;
        std::fs::remove_file(&path).expect("delete source");

        backend
            .handle_did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier::new(uri),
            })
            .await;

        assert!(!backend.documents.by_uri.contains_key(&uri_key));
        assert!(backend.workspace.index.get_file_id(&path).is_none());
        assert!(backend
            .workspace
            .index
            .find_symbol("removed.Removed")
            .is_none());
        assert!(backend.documents.file_ids.get_path(file_id).is_none());
        assert!(backend
            .workspace
            .host
            .read()
            .await
            .file_text(file_id)
            .is_empty());
        assert!(backend
            .workspace
            .host
            .member_fact_cache()
            .get_signature("removed.Removed.value()")
            .is_none());
    }

    #[test]
    fn stale_close_generation_cannot_remove_a_newer_reopen() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Reopened.java");
        let disk_source = "class Reopened {}";
        std::fs::write(&path, disk_source).expect("write source");
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        let uri = "file:///Reopened.java".to_owned();

        let first = publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &path,
            uri.clone(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new("class FirstOpen {}".to_owned()),
        )
        .expect("publish first open");
        let stale_generation = ctx.by_uri.get(&uri).expect("first state").change_version;
        publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &path,
            uri.clone(),
            "java".to_owned(),
            Language::Java,
            2,
            Arc::new("class SecondOpen {}".to_owned()),
        )
        .expect("publish second open");

        assert!(matches!(
            reconcile_close_from_disk(
                &mut backend.workspace.host.write_now(),
                &ctx,
                &path,
                &uri,
                first.file_id,
                stale_generation,
                Language::Java,
                false,
            ),
            CloseDiskAttempt::Stale
        ));
        let state = ctx.by_uri.get(&uri).expect("newer reopen must remain open");
        assert_eq!(state.version, 2);
        assert!(ctx.editor_owned.contains(&first.file_id));
        assert_eq!(
            ctx.open_by_id
                .get(&first.file_id)
                .expect("reverse owner")
                .value(),
            &uri
        );
    }

    #[test]
    fn reopening_one_file_under_a_new_uri_retires_the_old_uri_owner() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Aliased.java");
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        let old_uri = "file:///old-spelling/Aliased.java".to_owned();
        let new_uri = "file:///new-spelling/Aliased.java".to_owned();

        let first = publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &path,
            old_uri.clone(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new("class FirstAlias {}".to_owned()),
        )
        .expect("publish first alias");
        let second = publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &path,
            new_uri.clone(),
            "java".to_owned(),
            Language::Java,
            2,
            Arc::new("class SecondAlias {}".to_owned()),
        )
        .expect("publish second alias");

        assert_eq!(first.file_id, second.file_id);
        assert!(!ctx.by_uri.contains_key(&old_uri));
        assert!(ctx.by_uri.contains_key(&new_uri));
        assert_eq!(
            ctx.open_by_id
                .get(&second.file_id)
                .expect("one reverse owner")
                .value(),
            &new_uri
        );
        assert!(ctx.editor_owned.contains(&second.file_id));
    }

    #[test]
    fn document_change_returns_replacements_for_every_superseded_open_uri() {
        let temp = tempdir().expect("temp workspace");
        let changed_path = temp.path().join("Changed.java");
        let unrelated_path = temp.path().join("Unrelated.java");
        let changed_uri = Url::from_file_path(&changed_path)
            .expect("changed URI")
            .to_string();
        let unrelated_uri = Url::from_file_path(&unrelated_path)
            .expect("unrelated URI")
            .to_string();
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);

        publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &changed_path,
            changed_uri.clone(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new("class Before {}".to_owned()),
        )
        .expect("publish changed document");
        publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &unrelated_path,
            unrelated_uri.clone(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new("class Unrelated {}".to_owned()),
        )
        .expect("publish unrelated document");
        ctx.diagnostic_publication
            .supersede_for_change(&changed_uri, 2)
            .expect("reserve changed generation");

        let ChangeDocumentResult::Published(outcome) = publish_document_change(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &changed_uri,
            2,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "class After {}".to_owned(),
            }],
        ) else {
            panic!("change publication must succeed");
        };

        let expected_uris = vec![changed_uri.clone(), unrelated_uri.clone()];
        assert_eq!(
            outcome
                .diagnostic_publications
                .iter()
                .map(|publication| publication.uri.clone())
                .collect::<Vec<_>>(),
            expected_uris
        );
        for publication in outcome.diagnostic_publications {
            assert!(
                ctx.diagnostic_publication
                    .is_generation_current(&publication.uri, publication.generation),
                "returned replacement generation must remain authoritative for {}",
                publication.uri
            );
        }
    }

    #[test]
    fn workspace_refresh_cannot_resurrect_diagnostics_while_close_waits() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("Closing.java");
        let uri = Url::from_file_path(&path).expect("closing URI").to_string();
        let backend = Backend::new(test_client());
        let ctx = DocumentPublicationContext::from(&backend);
        publish_open_document(
            &mut backend.workspace.host.write_now(),
            &ctx,
            &path,
            uri.clone(),
            "java".to_owned(),
            Language::Java,
            1,
            Arc::new("class Closing {}".to_owned()),
        )
        .expect("publish closing document");

        // didClose retires authority before it waits for the source-publication
        // lock. The URI remains in by_uri during that wait, exactly as it does in
        // the production race with a workspace refresh.
        let close_generation = ctx.diagnostic_publication.supersede_for_close(&uri);
        assert!(ctx.by_uri.contains_key(&uri));
        assert!(supersede_open_diagnostics(&ctx).is_empty());
        assert!(
            ctx.diagnostic_publication
                .is_closed_generation_current(&uri, close_generation),
            "workspace refresh must leave the pending close clear authoritative"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn did_change_keeps_the_document_map_on_the_last_complete_revision() {
        let temp = tempdir().expect("temp workspace");
        let path = temp.path().join("AtomicChange.java");
        let old_source = "class BeforeAtomicChange {}";
        let new_source = "class AfterAtomicChange {}";
        std::fs::write(&path, old_source).expect("write source");
        let uri = Url::from_file_path(&path).expect("file URI");
        let backend = Backend::new(test_client());
        backend
            .handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_owned(),
                    version: 1,
                    text: old_source.to_owned(),
                },
            })
            .await;

        let uri_key = uri.to_string();
        let file_id = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("open state")
            .file_id;
        let path_lock = backend.workspace.index.get_path_lock(&path);
        let path_guard = path_lock.lock().unwrap();

        let change_ctx = DocumentPublicationContext::from(&backend);
        let worker_uri = uri_key.clone();
        change_ctx
            .diagnostic_publication
            .supersede_for_change(&worker_uri, 2)
            .expect("new document generation");
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let worker_host = Arc::clone(&backend.workspace.host);
        let worker = std::thread::spawn(move || {
            let result = publish_document_change(
                &mut worker_host.write_now(),
                &change_ctx,
                &worker_uri,
                2,
                vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: new_source.to_owned(),
                }],
            );
            finished_tx.send(result).unwrap();
        });

        assert!(matches!(
            finished_rx.recv_timeout(std::time::Duration::from_millis(25)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(backend
            .background_tasks
            .diagnostic_publication
            .supersede_open_uri(&uri_key)
            .is_some());
        assert_eq!(
            backend
                .documents
                .by_uri
                .get(&uri_key)
                .expect("old state remains visible")
                .text
                .as_str(),
            old_source
        );
        // The blocked publication holds the write transaction, so no reader can
        // observe a half-published revision; the index still shows the old one.
        assert!(backend
            .workspace
            .index
            .find_symbol("BeforeAtomicChange")
            .is_some());

        drop(path_guard);
        assert!(matches!(
            finished_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("change should finish after contention clears"),
            ChangeDocumentResult::Published(_)
        ));
        worker.join().unwrap();

        let state = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("new state published");
        assert_eq!(state.version, 2);
        assert_eq!(state.text.as_str(), new_source);
        drop(state);
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            new_source
        );
        assert!(backend
            .workspace
            .index
            .find_symbol("AfterAtomicChange")
            .is_some());
        assert!(backend
            .workspace
            .index
            .find_symbol("BeforeAtomicChange")
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn document_changes_preserve_utf16_crlf_versions_and_fresh_index_state() {
        let temp = tempdir().expect("temp workspace");
        let source_dir = temp.path().join("src/main/java/test");
        std::fs::create_dir_all(&source_dir).expect("create source directory");
        let path = source_dir.join("Sample.java");
        let source = concat!(
            "package test;\r\n",
            "public class Sample {\r\n",
            "    String marker = \"😀x\";\r\n",
            "    void oldName() {}\r\n",
            "}\r\n",
        );
        std::fs::write(&path, source).expect("write source file");
        let uri = Url::from_file_path(&path).expect("file URI");
        let use_path = source_dir.join("Use.java");
        let use_source = concat!(
            "package test;\r\n",
            "public class Use {\r\n",
            "    void call(Sample sample) { sample.oldName(); }\r\n",
            "}\r\n",
        );
        std::fs::write(&use_path, use_source).expect("write dependent source file");
        let use_uri = Url::from_file_path(&use_path).expect("dependent file URI");
        let backend = Backend::new(test_client());

        tokio::time::timeout(
            Duration::from_secs(5),
            backend.handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "java".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            }),
        )
        .await
        .expect("opening declaration document should not stall");
        tokio::time::timeout(
            Duration::from_secs(5),
            backend.handle_did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: use_uri.clone(),
                    language_id: "java".to_string(),
                    version: 1,
                    text: use_source.to_string(),
                },
            }),
        )
        .await
        .expect("opening dependent document should not stall");

        let edited_line = source.split("\r\n").nth(2).expect("line containing emoji");
        let x_byte = edited_line.find('x').expect("x after emoji");
        let x_utf16 = edited_line[..x_byte].encode_utf16().count() as u32;
        let replacement = format!("{}😀done", "a".repeat(49));
        tokio::time::timeout(
            Duration::from_secs(5),
            backend.handle_did_change(change_params(
                uri.clone(),
                2,
                vec![TextDocumentContentChangeEvent {
                    range: Some(Range::new(
                        Position::new(2, x_utf16),
                        Position::new(2, x_utf16 + 1),
                    )),
                    range_length: Some(1),
                    text: replacement.clone(),
                }],
            )),
        )
        .await
        .expect("incremental declaration change should not stall");

        let after_incremental = source.replacen('x', &replacement, 1);
        let uri_key = uri.to_string();
        let state = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("open document state");
        assert_eq!(state.version, 2);
        assert_eq!(state.text.as_str(), after_incremental);
        let file_id = state.file_id;
        drop(state);
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            after_incremental
        );

        let latest = after_incremental.replace("oldName", "newName");
        tokio::time::timeout(
            Duration::from_secs(5),
            backend.handle_did_change(change_params(
                uri.clone(),
                4,
                vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: latest.clone(),
                }],
            )),
        )
        .await
        .expect("full declaration change should not stall");
        assert!(
            backend
                .workspace
                .index
                .get_symbol("test.Sample.newName")
                .is_some(),
            "sample symbols after invalidation: {:?}",
            backend.workspace.index.get_file_symbols(file_id)
        );
        assert!(backend
            .workspace
            .index
            .get_symbol("test.Sample.oldName")
            .is_none());

        let latest_use = use_source.replace("oldName", "newName");
        tokio::time::timeout(
            Duration::from_secs(5),
            backend.handle_did_change(change_params(
                use_uri.clone(),
                2,
                vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: latest_use.clone(),
                }],
            )),
        )
        .await
        .expect("dependent change should not stall");
        let use_line = latest_use
            .split("\r\n")
            .nth(2)
            .expect("dependent call line");
        let new_name_byte = use_line.find("newName").expect("renamed call");
        let new_name_utf16 = use_line[..new_name_byte].encode_utf16().count() as u32;
        let definition = tokio::time::timeout(
            Duration::from_secs(7),
            backend.handle_goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams::new(
                    TextDocumentIdentifier::new(use_uri),
                    Position::new(2, new_name_utf16),
                ),
                work_done_progress_params: tower_lsp::lsp_types::WorkDoneProgressParams::default(),
                partial_result_params: tower_lsp::lsp_types::PartialResultParams::default(),
            }),
        )
        .await
        .expect("goto definition should not stall");
        let Some(GotoDefinitionResponse::Scalar(location)) = definition else {
            panic!("unsaved renamed call should resolve immediately");
        };
        assert_eq!(location.uri, uri);

        tokio::time::timeout(
            Duration::from_secs(5),
            backend.handle_did_change(change_params(
                uri,
                3,
                vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: source.to_string(),
                }],
            )),
        )
        .await
        .expect("stale declaration change should not stall");

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if backend.background_tasks.invalidation.lock().is_empty() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background invalidation should settle");

        let state = backend
            .documents
            .by_uri
            .get(&uri_key)
            .expect("document remains open");
        assert_eq!(state.version, 4, "stale versions must be ignored");
        assert_eq!(state.text.as_str(), latest);
        assert_eq!(
            backend
                .workspace
                .host
                .read()
                .await
                .file_text(file_id)
                .as_str(),
            latest
        );
        assert!(
            backend
                .workspace
                .index
                .get_symbol("test.Sample.newName")
                .is_some(),
            "sample symbols after settled invalidation: {:?}",
            backend.workspace.index.get_file_symbols(file_id)
        );
        assert!(backend
            .workspace
            .index
            .get_symbol("test.Sample.oldName")
            .is_none());
    }
}
