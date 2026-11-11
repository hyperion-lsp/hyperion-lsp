// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! File event handling for external file changes.
//!
//! This module processes file system events from the file watcher and
//! updates the workspace index and Salsa database accordingly.
//!
//! ## CRITICAL: TOCTOU Race Prevention
//!
//! The `handle_modified` function prevents a Time-Of-Check-Time-Of-Use race
//! between file watcher updates and LSP `didChange` notifications:
//!
//! ```text
//! WITHOUT LOCK (BROKEN):
//!   T1: File watcher detects change (v1 on disk)
//!   T2: Check "is file open?" -> NO
//!   T3: Release lock
//!   T4: User opens file -> didOpen with v2  [FRESH DATA in editor]
//!   T5: didOpen indexes v2, releases lock
//!   T6: File watcher re-acquires lock, indexes v1  [OVERWRITES v2!]
//!
//! WITH LOCK (CORRECT):
//!   T1: File watcher detects change, acquires lock
//!   T2: Check "is file open?" -> NO (lock held)
//!   T3: Read from disk and index (lock still held)
//!   T4: Release lock
//!   T5: User opens file -> didOpen blocks on lock
//!   T6: didOpen acquires lock, indexes v2
//! ```
//!
//! File-event work runs on the blocking pool and waits for the canonical path
//! and file locks. Contention alone does not prove that an editor has fresher
//! content: bulk indexing or dependent invalidation can hold the same locks.
//! Once the watcher owns them it checks the live editor maps and either yields
//! to that authoritative buffer or publishes the latest disk revision.

mod directory_rename;
mod rename;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_semantics::Type;
use hyperion_workspace::{FileRevisionRemovalOutcome, Language, WorkspaceIndex};
use lru::LruCache;
use parking_lot::Mutex;
use tower_lsp::lsp_types::MessageType;
use tower_lsp::Client;
use tracing::{debug, info, warn};

use crate::file_registry::FileIdRegistry;
use crate::file_watcher::FileChangeEvent;
use crate::semantic_host::WriteTxn;
use crate::server::{schedule_build_system_refresh, DocumentState};
#[cfg(test)]
use crate::source_publication::sync_method_signatures_for_revision;
use crate::source_publication::{
    publish_semantic_revision, retire_absent_semantic_revisions, retire_semantic_revisions,
};
use hyperion_layout::is_build_input;

use self::directory_rename::{
    reconcile_directory_create, reconcile_directory_delete, reconcile_directory_rename,
    DirectoryRenameOutcome,
};
#[cfg(test)]
use self::rename::rename_disk_file;

#[cfg(test)]
use self::directory_rename::{old_path_for_renamed_file, scan_source_tree};

/// Context needed for processing file events
pub struct FileEventContext {
    pub host: Arc<crate::semantic_host::SemanticHost>,
    pub file_id_registry: Arc<FileIdRegistry>,
    pub document_map: Arc<DashMap<String, DocumentState>>,
    /// Reverse index for O(1) "is file open" checks.
    ///
    /// Maps FileId → URI to avoid O(n) iteration over document_map.
    /// Updated by didOpen/didClose handlers in server.rs.
    pub open_files: Arc<DashMap<FileId, String>>,
    pub var_type_cache: Arc<Mutex<LruCache<(FileId, u32), Type>>>,
    pub build_refresh: crate::handlers::build_system::BuildRefreshCoordinator,
    pub workspace_root: PathBuf,
    pub client: Client,
    pub source_providers: Arc<crate::source_provider::SourceProviderChain>,
    pub invalidation_runtime: crate::server::diagnostics::InvalidationRuntime,
}

#[derive(Clone)]
struct SourceMutationContext {
    workspace_index: Arc<WorkspaceIndex>,
    file_id_registry: Arc<FileIdRegistry>,
    document_map: Arc<DashMap<String, DocumentState>>,
    open_files: Arc<DashMap<FileId, String>>,
    var_type_cache: Arc<Mutex<LruCache<(FileId, u32), Type>>>,
}

impl From<&FileEventContext> for SourceMutationContext {
    fn from(ctx: &FileEventContext) -> Self {
        Self {
            workspace_index: Arc::clone(ctx.host.index()),
            file_id_registry: Arc::clone(&ctx.file_id_registry),
            document_map: Arc::clone(&ctx.document_map),
            open_files: Arc::clone(&ctx.open_files),
            var_type_cache: Arc::clone(&ctx.var_type_cache),
        }
    }
}

struct ModifiedFileOutcome {
    /// Primary identity retained for single-file callers and diagnostics.
    file_id: FileId,
    /// Every source revision changed by this reconciliation. A stale rename
    /// can legitimately publish both a recreated old path and its destination.
    changed_file_ids: Vec<FileId>,
    dependent_files: Vec<FileId>,
    /// False when a multi-path reconciliation published useful partial work
    /// but at least one required source still needs a later retry.
    complete: bool,
}

struct RemovedFileOutcome {
    file_id: FileId,
    dependent_files: Vec<FileId>,
}

#[derive(Clone, Copy)]
enum DiskPublicationMode {
    ExistingRevision,
    CreateIfMissing,
}

fn editor_owned_uri(ctx: &SourceMutationContext, file_id: FileId) -> Option<String> {
    ctx.open_files
        .get(&file_id)
        .map(|entry| entry.value().clone())
        .or_else(|| {
            ctx.document_map
                .iter()
                .find(|entry| entry.value().file_id == file_id)
                .map(|entry| entry.key().clone())
        })
}

fn existing_file_id(
    ctx: &SourceMutationContext,
    path: &Path,
    path_key: &hyperion_workspace::PathLookupKey,
    path_is_absent: bool,
) -> Option<FileId> {
    let workspace_id = ctx
        .workspace_index
        .get_file_id_for_key(path_key)
        .or_else(|| {
            path_is_absent
                .then(|| {
                    ctx.workspace_index
                        .get_file_id_for_key_or_display(path_key, path)
                })
                .flatten()
        });
    let registry_id = ctx.file_id_registry.get_from_path_with_key(path, path_key);

    // A fully published editor buffer is the only identity allowed to displace
    // the workspace revision. Otherwise the workspace is authoritative and a
    // later successful publication repairs any stale closed registry binding.
    registry_id
        .filter(|file_id| editor_owned_uri(ctx, *file_id).is_some())
        .or(workspace_id)
        .or(registry_id)
}

fn build_file_has_source_revision(path: &Path) -> bool {
    is_build_input(path) && Language::from_path(path).is_some()
}

#[cfg(test)]
fn existing_or_allocate_file_id(
    workspace_index: &WorkspaceIndex,
    file_id_registry: &FileIdRegistry,
    path: &Path,
) -> FileId {
    workspace_index
        .get_file_id(path)
        .or_else(|| file_id_registry.get_from_path(path))
        .unwrap_or_else(|| file_id_registry.get_or_create_from_path(path))
}

/// Process a single file change event
pub async fn handle_file_event(ctx: &FileEventContext, event: FileChangeEvent) {
    // File-system reconciliation mutates WorkspaceIndex and Salsa under path/
    // file locks. Each handler enters the same fair write transaction as editor
    // and bulk-source publication, but only for the blocking phase that
    // actually mutates: build-file events and events that turn out to concern
    // no tracked source no longer serialize the whole server first.
    match event {
        FileChangeEvent::Created(path) => handle_created(ctx, path).await,
        FileChangeEvent::Modified(path) => handle_modified(ctx, path).await,
        FileChangeEvent::Deleted(path) => handle_deleted(ctx, path).await,
        FileChangeEvent::Renamed(old_path, new_path) => {
            handle_renamed(ctx, old_path, new_path).await;
        }
        FileChangeEvent::DirectoryRenamed { from, to } => {
            handle_directory_renamed(ctx, from, to).await;
        }
        FileChangeEvent::DirectoryCreated(path) => handle_directory_created(ctx, path).await,
        FileChangeEvent::DirectoryDeleted(path) => handle_directory_deleted(ctx, path).await,
    }
}

async fn handle_directory_created(ctx: &FileEventContext, path: PathBuf) {
    info!("External directory created: {}", path.display());
    let mutation_ctx = SourceMutationContext::from(ctx);
    let indexed_path = path.clone();
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation([&path]);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome = reconcile_directory_create(txn, &mutation_ctx, &indexed_path);
            (outcome, source_readiness)
        })
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            warn!("Directory-create reconciliation task failed: {error}");
            return;
        }
    };
    finish_directory_reconciliation(ctx, &path, "created", outcome.0, outcome.1);
}

async fn handle_directory_deleted(ctx: &FileEventContext, path: PathBuf) {
    info!("External directory deleted: {}", path.display());
    let mutation_ctx = SourceMutationContext::from(ctx);
    let indexed_path = path.clone();
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation([&path]);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome = reconcile_directory_delete(txn, &mutation_ctx, &indexed_path);
            (outcome, source_readiness)
        })
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            warn!("Directory-delete reconciliation task failed: {error}");
            return;
        }
    };
    finish_directory_reconciliation(ctx, &path, "deleted", outcome.0, outcome.1);
}

fn finish_directory_reconciliation(
    ctx: &FileEventContext,
    path: &Path,
    operation: &str,
    outcome: DirectoryRenameOutcome,
    source_readiness: crate::server::SourceMutationReadinessGuard,
) {
    if outcome.build_files_changed {
        schedule_build_system_refresh(
            ctx.build_refresh.clone(),
            ctx.workspace_root.clone(),
            ctx.client.clone(),
            ctx.invalidation_runtime.clone(),
            Arc::clone(&ctx.source_providers),
            "directory tree changed",
        );
    }
    if !outcome.scan_complete {
        warn!(
            "Directory {} {} was only partially scanned",
            path.display(),
            operation
        );
    }
    if outcome.changed_file_ids.is_empty() {
        source_readiness.finish(outcome.complete);
    } else if outcome.complete {
        ctx.invalidation_runtime.enqueue_batch_with_readiness(
            outcome.changed_file_ids,
            outcome.dependent_files,
            source_readiness,
        );
    } else {
        source_readiness.finish(false);
        ctx.invalidation_runtime
            .enqueue_batch(outcome.changed_file_ids, outcome.dependent_files);
    }
}

async fn handle_created(ctx: &FileEventContext, path: PathBuf) {
    info!("External file created: {}", path.display());

    if is_build_input(&path) {
        schedule_build_system_refresh(
            ctx.build_refresh.clone(),
            ctx.workspace_root.clone(),
            ctx.client.clone(),
            ctx.invalidation_runtime.clone(),
            Arc::clone(&ctx.source_providers),
            "build file created",
        );
        if !build_file_has_source_revision(&path) {
            return;
        }
    }

    let mutation_ctx = SourceMutationContext::from(ctx);
    let indexed_path = path.clone();
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation([&path]);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome = index_created_file_checked(txn, &mutation_ctx, &indexed_path);
            (outcome, source_readiness)
        })
        .await
    {
        Ok((Ok(outcome), source_readiness)) => (outcome, source_readiness),
        Ok((Err(()), source_readiness)) => {
            source_readiness.finish(false);
            return;
        }
        Err(error) => {
            warn!("Created-file indexing task failed: {error}");
            return;
        }
    };
    let (outcome, source_readiness) = outcome;

    let Some(outcome) = outcome else {
        source_readiness.finish(true);
        return;
    };

    // CRITICAL: Invalidate files that depend on symbols from this new file
    invalidate_and_reindex_dependents(
        ctx,
        outcome.changed_file_ids,
        outcome.dependent_files,
        source_readiness,
    );

    let () = ctx
        .client
        .log_message(
            MessageType::INFO,
            format!("Indexed new file: {}", path.display()),
        )
        .await;
}

async fn handle_modified(ctx: &FileEventContext, path: PathBuf) {
    info!("External file modified: {}", path.display());

    if is_build_input(&path) {
        schedule_build_system_refresh(
            ctx.build_refresh.clone(),
            ctx.workspace_root.clone(),
            ctx.client.clone(),
            ctx.invalidation_runtime.clone(),
            Arc::clone(&ctx.source_providers),
            "build file modified",
        );
        if !build_file_has_source_revision(&path) {
            return;
        }
    }

    let mutation_ctx = SourceMutationContext::from(ctx);
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation([&path]);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome = reindex_modified_file_checked(txn, &mutation_ctx, &path);
            (outcome, source_readiness)
        })
        .await
    {
        Ok((Ok(outcome), source_readiness)) => (outcome, source_readiness),
        Ok((Err(()), source_readiness)) => {
            source_readiness.finish(false);
            return;
        }
        Err(error) => {
            warn!("Modified-file indexing task failed: {error}");
            return;
        }
    };
    let (outcome, source_readiness) = outcome;

    if let Some(outcome) = outcome {
        // Dependents are invalidated after the source+Salsa transaction has
        // published completely and released its blocking locks.
        invalidate_and_reindex_dependents(
            ctx,
            outcome.changed_file_ids,
            outcome.dependent_files,
            source_readiness,
        );
    } else {
        source_readiness.finish(true);
    }
}

#[cfg(test)]
fn reindex_modified_file(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
) -> Option<ModifiedFileOutcome> {
    reindex_modified_file_checked(txn, ctx, path).ok().flatten()
}

#[cfg(test)]
fn index_created_file(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
) -> Option<ModifiedFileOutcome> {
    index_created_file_checked(txn, ctx, path).ok().flatten()
}

fn reindex_modified_file_checked(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
) -> Result<Option<ModifiedFileOutcome>, ()> {
    publish_disk_file_checked(txn, ctx, path, DiskPublicationMode::ExistingRevision)
}

fn index_created_file_checked(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
) -> Result<Option<ModifiedFileOutcome>, ()> {
    publish_disk_file_checked(txn, ctx, path, DiskPublicationMode::CreateIfMissing)
}

fn retire_failed_disk_publication_binding(
    ctx: &SourceMutationContext,
    path_key: &hyperion_workspace::PathLookupKey,
    file_id: FileId,
    binding_created_by_attempt: bool,
) {
    // Run while this attempt still holds both path and FileId locks. A waiting
    // didOpen must observe either the intact old binding or the completed
    // cleanup; it must never have its newly registered identity removed by a
    // cleanup that escaped the transaction.
    if binding_created_by_attempt
        && ctx.workspace_index.get_file_id_for_key(path_key).is_none()
        && editor_owned_uri(ctx, file_id).is_none()
    {
        ctx.file_id_registry
            .remove_expected_path_key(path_key, file_id);
    }
}

fn publish_disk_file_checked(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
    mode: DiskPublicationMode,
) -> Result<Option<ModifiedFileOutcome>, ()> {
    // Acquire the path lock before resolving the FileId. A concurrent rename or
    // deletion may retire this path; after it finishes, the registry lookup
    // below observes that final state instead of resurrecting the old identity.
    let (path_key, path_lock) = ctx.workspace_index.establish_path_lock(path);
    let path_guard = path_lock.lock().unwrap_or_else(|poisoned| {
        warn!(
            "Path lock for disk publication {} was poisoned; continuing with recovered state",
            path.display()
        );
        poisoned.into_inner()
    });

    let existing_workspace_id = ctx.workspace_index.get_file_id_for_key(&path_key);
    let existing_registry_id = ctx.file_id_registry.get_from_path_with_key(path, &path_key);
    let editor_registry_id =
        existing_registry_id.filter(|file_id| editor_owned_uri(ctx, *file_id).is_some());
    let mut registry_binding_created = false;
    let file_id = match (editor_registry_id, existing_workspace_id) {
        (Some(file_id), _) => file_id,
        (None, Some(file_id)) => file_id,
        (None, None) if matches!(mode, DiskPublicationMode::CreateIfMissing) => {
            match existing_registry_id {
                Some(file_id) => file_id,
                None => {
                    registry_binding_created = true;
                    ctx.file_id_registry
                        .get_or_create_from_path_with_key_and_preferred_id(
                            path,
                            path_key.clone(),
                            None,
                        )
                }
            }
        }
        (None, None) => {
            debug!("Modified file is not indexed: {}", path.display());
            let result = match path.try_exists() {
                Ok(false) => Ok(None),
                Ok(true) => Err(()),
                Err(error) => {
                    warn!(
                        "Could not determine whether modified path {} exists: {}",
                        path.display(),
                        error
                    );
                    Err(())
                }
            };
            drop(path_guard);
            drop(path_lock);
            ctx.workspace_index
                .reclaim_path_lock_key_if_unused(&path_key);
            return result;
        }
    };
    let mut file_ids = vec![file_id];
    file_ids.extend(
        [existing_workspace_id, existing_registry_id]
            .into_iter()
            .flatten(),
    );
    file_ids.sort_unstable_by_key(|file_id| file_id.as_u32());
    file_ids.dedup();
    let file_locks: Vec<_> = file_ids
        .iter()
        .map(|file_id| ctx.workspace_index.get_file_lock(*file_id))
        .collect();
    let file_guards: Vec<_> = file_locks
        .iter()
        .map(|lock| {
            lock.lock().unwrap_or_else(|poisoned| {
                warn!(
                    "File lock for disk publication was poisoned; continuing with recovered state"
                );
                poisoned.into_inner()
            })
        })
        .collect();

    if file_ids
        .iter()
        .any(|candidate| *candidate != file_id && editor_owned_uri(ctx, *candidate).is_some())
    {
        warn!(
            "Two editor-owned identities disagree for {}; deferring disk reconciliation",
            path.display()
        );
        return Err(());
    }

    // didOpen installs the reverse mapping before contending for these index
    // locks. The document-map fallback covers the tiny interval between its two
    // map publications. In either case the editor buffer is authoritative.
    let open_uri = editor_owned_uri(ctx, file_id);
    if let Some(open_uri) = open_uri {
        if existing_workspace_id == Some(file_id) {
            debug!(
                "Skipping disk change for editor-owned file: {}",
                path.display()
            );
            return Ok(None);
        }

        // A deliberately inconsistent or interrupted prior publication can
        // leave the editor registry authoritative while WorkspaceIndex still
        // owns the path with another ID. Republish the editor bytes—not disk—
        // while both IDs are locked so the explicit editor-wins policy actually
        // converges every semantic layer.
        let (editor_text, language) = ctx
            .document_map
            .get(&open_uri)
            .filter(|document| document.file_id == file_id)
            .map_or_else(
                || {
                    (
                        txn.db().file_text(file_id),
                        Language::from_path(path).unwrap_or(Language::Java),
                    )
                },
                |document| (Arc::clone(&document.text), document.language),
            );
        let stats = ctx
            .workspace_index
            .index_file_with_key_and_locks_held(path, &path_key, &editor_text, file_id)
            .map_err(|error| {
                warn!(
                    "Failed to reconcile editor-owned identity for {}: {}",
                    path.display(),
                    error
                );
            })?;
        if !publish_semantic_revision(
            txn,
            &ctx.var_type_cache,
            file_id,
            editor_text,
            language,
            &stats.retired_file_ids,
        ) {
            retire_absent_semantic_revisions(
                txn,
                &ctx.var_type_cache,
                &stats.retired_file_ids,
                language,
            );
            return Err(());
        }
        return Ok(Some(ModifiedFileOutcome {
            file_id,
            changed_file_ids: vec![file_id],
            dependent_files: stats.dependent_files,
            complete: true,
        }));
    }

    // Read after acquiring both locks. If this event waited behind another
    // publisher, it deliberately reads the latest disk state rather than the
    // stale state that originally triggered the notification.
    let text = match hyperion_workspace::read_source_file(path) {
        Ok(text) => text,
        Err(error) => {
            warn!("Failed to read source file {}: {}", path.display(), error);
            let stale_create = matches!(mode, DiskPublicationMode::CreateIfMissing)
                && matches!(path.try_exists(), Ok(false));
            retire_failed_disk_publication_binding(
                ctx,
                &path_key,
                file_id,
                registry_binding_created,
            );
            drop(file_guards);
            drop(file_locks);
            drop(path_guard);
            drop(path_lock);
            ctx.workspace_index
                .reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
            return if stale_create { Ok(None) } else { Err(()) };
        }
    };

    // The watcher, the client (`didChangeWatchedFiles`) and git's
    // delete/create/touch bursts routinely report a revision that is already
    // published. Re-publishing it would bump the workspace revision, cancel
    // in-flight diagnostics and re-invalidate every dependent for nothing.
    if existing_workspace_id == Some(file_id)
        && ctx
            .workspace_index
            .get_file_contents(file_id)
            .is_some_and(|indexed| indexed.as_str() == text)
    {
        debug!("Disk revision already published: {}", path.display());
        return Ok(None);
    }

    let mut stats = match ctx
        .workspace_index
        .index_disk_file_with_key_and_locks_held(path, &path_key, &text, file_id)
    {
        Ok(stats) => stats,
        Err(error) => {
            warn!(
                "Failed to index disk revision {}: {}",
                path.display(),
                error
            );
            retire_failed_disk_publication_binding(
                ctx,
                &path_key,
                file_id,
                registry_binding_created,
            );
            drop(file_guards);
            drop(file_locks);
            drop(path_guard);
            drop(path_lock);
            ctx.workspace_index
                .reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
            return Err(());
        }
    };

    // The path lock makes the ownership observation stable against didOpen and
    // didClose. A stale closed side-registry ID must converge to the published
    // workspace identity instead of surviving as a second authority.
    if existing_registry_id.is_some_and(|id| id != file_id && editor_owned_uri(ctx, id).is_none()) {
        if let Some(displaced_id) =
            ctx.file_id_registry
                .reconcile_registered_path_with_key(path, path_key.clone(), file_id)
        {
            stats.retired_file_ids.push(displaced_id);
            stats
                .retired_file_ids
                .sort_unstable_by_key(|id| id.as_u32());
            stats.retired_file_ids.dedup();
        }
    }

    let language = Language::from_path(path).unwrap_or(Language::Java);
    if !publish_semantic_revision(
        txn,
        &ctx.var_type_cache,
        file_id,
        Arc::new(text),
        language,
        &stats.retired_file_ids,
    ) {
        warn!(
            "Workspace revision for {} changed before external semantic publication completed",
            path.display()
        );
        retire_absent_semantic_revisions(
            txn,
            &ctx.var_type_cache,
            &stats.retired_file_ids,
            language,
        );
        return Err(());
    }

    debug!("Published external disk revision: {}", path.display());
    Ok(Some(ModifiedFileOutcome {
        file_id,
        changed_file_ids: vec![file_id],
        dependent_files: stats.dependent_files,
        complete: true,
    }))
}

async fn handle_deleted(ctx: &FileEventContext, path: PathBuf) {
    info!("External file deleted: {}", path.display());

    if is_build_input(&path) {
        schedule_build_system_refresh(
            ctx.build_refresh.clone(),
            ctx.workspace_root.clone(),
            ctx.client.clone(),
            ctx.invalidation_runtime.clone(),
            Arc::clone(&ctx.source_providers),
            "build file deleted",
        );
        if !build_file_has_source_revision(&path) {
            return;
        }
    }

    let mutation_ctx = SourceMutationContext::from(ctx);
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation([&path]);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome = remove_deleted_file_checked(txn, &mutation_ctx, &path);
            (outcome, source_readiness)
        })
        .await
    {
        Ok((Ok(outcome), source_readiness)) => (outcome, source_readiness),
        Ok((Err(()), source_readiness)) => {
            source_readiness.finish(false);
            return;
        }
        Err(error) => {
            warn!("Deleted-file cleanup task failed: {error}");
            return;
        }
    };
    let (outcome, source_readiness) = outcome;

    if let Some(outcome) = outcome {
        invalidate_and_reindex_dependents(
            ctx,
            vec![outcome.file_id],
            outcome.dependent_files,
            source_readiness,
        );
    } else {
        source_readiness.finish(true);
    }
}

#[cfg(test)]
fn remove_deleted_file(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
) -> Option<RemovedFileOutcome> {
    remove_deleted_file_checked(txn, ctx, path).ok().flatten()
}

fn remove_deleted_file_checked(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    path: &Path,
) -> Result<Option<RemovedFileOutcome>, ()> {
    let path_key = hyperion_workspace::path_lookup_key(path);
    let path_existence = path.try_exists();
    let path_is_absent = matches!(path_existence, Ok(false));
    let workspace_id = ctx
        .workspace_index
        .get_file_id_for_key(&path_key)
        .or_else(|| {
            path_is_absent
                .then(|| {
                    ctx.workspace_index
                        .get_file_id_for_key_or_display(&path_key, path)
                })
                .flatten()
        });
    let registry_id = ctx.file_id_registry.get_from_path_with_key(path, &path_key);
    let Some(file_id) = existing_file_id(ctx, path, &path_key, path_is_absent) else {
        debug!("Deleted file is not tracked: {}", path.display());
        return match path_existence {
            Ok(true) => index_created_file_checked(txn, ctx, path).map(|outcome| {
                outcome.map(|outcome| RemovedFileOutcome {
                    file_id: outcome.file_id,
                    dependent_files: outcome.dependent_files,
                })
            }),
            Ok(false) => Ok(None),
            Err(error) => {
                warn!(
                    "Could not determine whether untracked deleted path {} exists: {}",
                    path.display(),
                    error
                );
                Err(())
            }
        };
    };
    let language = Language::from_path(path).unwrap_or(Language::Java);

    // A registry-only ID was never published to WorkspaceIndex. A stale delete
    // must still retire that binding, while a coalesced delete/create must
    // publish the live disk source instead of leaving it permanently
    // unindexed.
    if workspace_id.is_none() {
        let registry_path_key = registry_id
            .and_then(|file_id| ctx.file_id_registry.get_path_key(file_id))
            .unwrap_or_else(|| path_key.clone());
        let (first_path_key, second_path_key) = if registry_path_key <= path_key {
            (&registry_path_key, &path_key)
        } else {
            (&path_key, &registry_path_key)
        };
        let first_path_lock = ctx.workspace_index.get_path_lock_for_key(first_path_key);
        let first_path_guard = first_path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let second_path_lock = (registry_path_key != path_key)
            .then(|| ctx.workspace_index.get_path_lock_for_key(second_path_key));
        let second_path_guard = second_path_lock.as_ref().map(|lock| {
            lock.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        });

        // A concurrent didOpen/create may have installed a workspace revision
        // after the optimistic observation above. Never let registry-only
        // cleanup retire state belonging to that newer publication.
        if ctx
            .workspace_index
            .get_file_id_for_key(&registry_path_key)
            .is_some()
            || ctx.workspace_index.get_file_id_for_key(&path_key).is_some()
        {
            return Ok(None);
        }

        let Some(registry_id) = ctx
            .file_id_registry
            .get_from_path_with_key(path, &registry_path_key)
            .filter(|file_id| {
                ctx.file_id_registry.get_path_key(*file_id).as_ref() == Some(&registry_path_key)
            })
        else {
            return Ok(None);
        };
        if editor_owned_uri(ctx, registry_id).is_some() {
            return Ok(None);
        }

        match path.try_exists() {
            Ok(true) => {
                drop(second_path_guard);
                drop(second_path_lock);
                drop(first_path_guard);
                drop(first_path_lock);
                return index_created_file_checked(txn, ctx, path).map(|outcome| {
                    outcome.map(|outcome| RemovedFileOutcome {
                        file_id: outcome.file_id,
                        dependent_files: outcome.dependent_files,
                    })
                });
            }
            Ok(false) => {}
            Err(error) => {
                warn!(
                    "Could not prove registry-only deleted path {} was absent: {}",
                    path.display(),
                    error
                );
                return Err(());
            }
        }

        let file_lock = ctx.workspace_index.get_file_lock(registry_id);
        let file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // FileId ownership may have changed while this deletion waited behind
        // didOpen. Revalidate every authority only after all path and FileId
        // locks are held.
        let workspace_now_owns_path = ctx
            .workspace_index
            .get_file_id_for_key(&registry_path_key)
            .is_some()
            || ctx.workspace_index.get_file_id_for_key(&path_key).is_some();
        let registry_still_matches = ctx
            .file_id_registry
            .get_from_path_with_key(path, &registry_path_key)
            == Some(registry_id)
            && ctx.file_id_registry.get_path_key(registry_id).as_ref() == Some(&registry_path_key);
        if workspace_now_owns_path
            || editor_owned_uri(ctx, registry_id).is_some()
            || !registry_still_matches
        {
            return Ok(None);
        }
        match path.try_exists() {
            Ok(true) => {
                // The first absence observation happened before waiting for the
                // FileId lock. A create that landed during that wait owns the
                // final state; release the transaction and publish its bytes.
                drop(file_guard);
                drop(file_lock);
                drop(second_path_guard);
                drop(second_path_lock);
                drop(first_path_guard);
                drop(first_path_lock);
                return index_created_file_checked(txn, ctx, path).map(|outcome| {
                    outcome.map(|outcome| RemovedFileOutcome {
                        file_id: outcome.file_id,
                        dependent_files: outcome.dependent_files,
                    })
                });
            }
            Ok(false) => {}
            Err(error) => {
                warn!(
                    "Could not revalidate registry-only deleted path {} after locking: {}",
                    path.display(),
                    error
                );
                return Err(());
            }
        }
        if ctx
            .file_id_registry
            .remove_expected_path_key(&registry_path_key, registry_id)
        {
            retire_semantic_revisions(txn, &ctx.var_type_cache, &[registry_id], language);
            drop(file_guard);
            drop(file_lock);
            drop(second_path_guard);
            drop(second_path_lock);
            drop(first_path_guard);
            drop(first_path_lock);
            ctx.workspace_index
                .reclaim_unpublished_revision_locks_for_key(&registry_path_key, registry_id);
            if registry_path_key != path_key {
                ctx.workspace_index
                    .reclaim_path_lock_key_if_unused(&path_key);
            }
            return Ok(Some(RemovedFileOutcome {
                file_id: registry_id,
                dependent_files: Vec::new(),
            }));
        }
        return Ok(None);
    }

    let recreated_on_disk = Cell::new(false);
    let path_observation_failed = Cell::new(false);
    let published_path_key = workspace_id
        .and_then(|file_id| ctx.workspace_index.get_file_path_key(file_id))
        .unwrap_or_else(|| path_key.clone());
    let expected_registry_id = ctx
        .file_id_registry
        .get_from_path_with_key(path, &published_path_key);

    let removal = ctx.workspace_index.remove_file_revision(
        path,
        file_id,
        || {
            if editor_owned_uri(ctx, file_id).is_some() {
                debug!(
                    "Keeping editor-owned revision after disk deletion: {}",
                    path.display()
                );
                false
            } else {
                match path.try_exists() {
                    Ok(true) => {
                        // Debounced delete/create sequences can arrive after
                        // the path has already been recreated. Retiring the
                        // current identity here would make a stale delete win.
                        recreated_on_disk.set(true);
                        false
                    }
                    Ok(false) => true,
                    Err(error) => {
                        warn!(
                            "Could not prove deleted path {} was absent: {}",
                            path.display(),
                            error
                        );
                        path_observation_failed.set(true);
                        false
                    }
                }
            }
        },
        || {
            // WorkspaceIndex, Salsa, signatures, the editor registry, and
            // completion caches are one retired revision while the path/file
            // locks exclude a concurrent didOpen or newer watcher update.
            let stale_registry_id = expected_registry_id.filter(|registry_id| {
                ctx.file_id_registry
                    .remove_expected_path_key(&published_path_key, *registry_id)
            });
            let mut retired_ids = vec![file_id];
            if let Some(registry_id) = stale_registry_id.filter(|id| *id != file_id) {
                retired_ids.push(registry_id);
            }
            retire_semantic_revisions(txn, &ctx.var_type_cache, &retired_ids, language);
        },
    );

    let dependent_files = match removal {
        FileRevisionRemovalOutcome::Removed { dependent_files } => dependent_files,
        FileRevisionRemovalOutcome::Retained if recreated_on_disk.get() => {
            debug!(
                "Re-indexing file recreated before delete event was handled: {}",
                path.display()
            );
            return reindex_modified_file_checked(txn, ctx, path).map(|outcome| {
                outcome.map(|outcome| RemovedFileOutcome {
                    file_id: outcome.file_id,
                    dependent_files: outcome.dependent_files,
                })
            });
        }
        FileRevisionRemovalOutcome::Retained if path_observation_failed.get() => return Err(()),
        FileRevisionRemovalOutcome::RevisionChanged | FileRevisionRemovalOutcome::Retained => {
            return Ok(None);
        }
    };

    debug!("Removed deleted file from index: {}", path.display());
    Ok(Some(RemovedFileOutcome {
        file_id,
        dependent_files,
    }))
}

async fn handle_renamed(ctx: &FileEventContext, old_path: PathBuf, new_path: PathBuf) {
    info!(
        "External file renamed: {} -> {}",
        old_path.display(),
        new_path.display()
    );

    let old_is_source = Language::from_path(&old_path).is_some();
    let new_is_source = Language::from_path(&new_path).is_some();
    if is_build_input(&old_path) || is_build_input(&new_path) {
        schedule_build_system_refresh(
            ctx.build_refresh.clone(),
            ctx.workspace_root.clone(),
            ctx.client.clone(),
            ctx.invalidation_runtime.clone(),
            Arc::clone(&ctx.source_providers),
            "build file renamed",
        );
        if !old_is_source && !new_is_source {
            return;
        }
    }

    let mutation_ctx = SourceMutationContext::from(ctx);
    let indexed_old_path = old_path.clone();
    let indexed_new_path = new_path.clone();
    let source_paths: Vec<_> = [(&old_path, old_is_source), (&new_path, new_is_source)]
        .into_iter()
        .filter_map(|(path, is_source)| is_source.then_some(path))
        .collect();
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation(source_paths);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome = match (old_is_source, new_is_source) {
                (true, true) => self::rename::rename_disk_file_checked(
                    txn,
                    &mutation_ctx,
                    &indexed_old_path,
                    &indexed_new_path,
                ),
                (true, false) => remove_deleted_file_checked(txn, &mutation_ctx, &indexed_old_path)
                    .map(|outcome| {
                        outcome.map(|outcome| ModifiedFileOutcome {
                            file_id: outcome.file_id,
                            changed_file_ids: vec![outcome.file_id],
                            dependent_files: outcome.dependent_files,
                            complete: true,
                        })
                    }),
                (false, true) => index_created_file_checked(txn, &mutation_ctx, &indexed_new_path),
                (false, false) => Ok(None),
            };
            (outcome, source_readiness)
        })
        .await
    {
        Ok((Ok(outcome), source_readiness)) => (outcome, source_readiness),
        Ok((Err(()), source_readiness)) => {
            source_readiness.finish(false);
            return;
        }
        Err(error) => {
            warn!("Renamed-file indexing task failed: {error}");
            return;
        }
    };
    let (outcome, source_readiness) = outcome;

    if let Some(outcome) = outcome {
        debug!(
            "Completed rename: {} -> {}",
            old_path.display(),
            new_path.display()
        );
        if outcome.complete {
            invalidate_and_reindex_dependents(
                ctx,
                outcome.changed_file_ids,
                outcome.dependent_files,
                source_readiness,
            );
        } else {
            source_readiness.finish(false);
            ctx.invalidation_runtime
                .enqueue_batch(outcome.changed_file_ids, outcome.dependent_files);
        }
    } else {
        source_readiness.finish(true);
    }
}

/// Invalidate and re-index files that depend on the given file.
///
/// When a file changes externally (git checkout, build system, etc.), any files that import
/// symbols from it need to be re-indexed to pick up the changes. This prevents stale symbols
/// in dependent files.
///
/// ## Implementation
///
/// The indexing transaction supplies an old+new dependent snapshot before
/// removed declaration identities disappear. The shared invalidation engine
/// walks that graph transitively, using authoritative in-memory text for open
/// buffers and disk text for closed files. The changed file itself is skipped.
///
/// ## Example
///
/// ```text
/// // User.java defines "com.example.User"
/// package com.example;
/// public class User {
///     String name;  // Field added externally
/// }
///
/// // Main.java imports User
/// package com.example;
/// public class Main {
///     void test() {
///         User u = new User();
///         u.name;  // Must see the new 'name' field
///     }
/// }
/// ```
///
/// Without transitive invalidation, Main.java would still have the old User symbol table
/// without the `name` field, causing "symbol not found" errors.
fn invalidate_and_reindex_dependents(
    ctx: &FileEventContext,
    changed_file_ids: Vec<FileId>,
    dependent_files: Vec<FileId>,
    readiness: crate::server::SourceMutationReadinessGuard,
) {
    if changed_file_ids.len() == 1 {
        ctx.invalidation_runtime.enqueue_with_readiness(
            changed_file_ids[0],
            dependent_files,
            readiness,
        );
    } else {
        ctx.invalidation_runtime.enqueue_batch_with_readiness(
            changed_file_ids,
            dependent_files,
            readiness,
        );
    }
}

/// Handle directory rename events
///
/// When a directory is renamed (e.g., com/example -> com/company during package refactoring),
/// all files within need to be removed from the old paths and re-indexed at new paths.
/// This ensures package names and imports stay consistent.
async fn handle_directory_renamed(ctx: &FileEventContext, from: PathBuf, to: PathBuf) {
    info!(
        "External directory renamed: {} -> {}",
        from.display(),
        to.display()
    );

    let mutation_ctx = SourceMutationContext::from(ctx);
    let indexed_from = from.clone();
    let indexed_to = to.clone();
    let source_readiness = ctx
        .invalidation_runtime
        .destructive_refactoring
        .begin_source_mutation([&from, &to]);
    let txn = ctx.host.write().await;
    let outcome = match txn
        .run_blocking(move |txn| {
            let outcome =
                reconcile_directory_rename(txn, &mutation_ctx, &indexed_from, &indexed_to);
            (outcome, source_readiness)
        })
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            warn!("Directory-rename reconciliation task failed: {error}");
            return;
        }
    };
    let (outcome, source_readiness) = outcome;

    if outcome.build_files_changed {
        schedule_build_system_refresh(
            ctx.build_refresh.clone(),
            ctx.workspace_root.clone(),
            ctx.client.clone(),
            ctx.invalidation_runtime.clone(),
            Arc::clone(&ctx.source_providers),
            "directory containing build files renamed",
        );
    }

    if !outcome.scan_complete {
        warn!(
            "Directory rename {} -> {} was only partially scanned; retained unmatched old revisions",
            from.display(),
            to.display()
        );
    }
    info!(
        "Completed directory rename processing: {} source revisions changed",
        outcome.revisions_changed
    );

    if outcome.changed_file_ids.is_empty() {
        source_readiness.finish(outcome.complete);
    } else if outcome.complete {
        ctx.invalidation_runtime.enqueue_batch_with_readiness(
            outcome.changed_file_ids,
            outcome.dependent_files,
            source_readiness,
        );
    } else {
        source_readiness.finish(false);
        ctx.invalidation_runtime
            .enqueue_batch(outcome.changed_file_ids, outcome.dependent_files);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use std::fs;
    use std::num::NonZeroUsize;
    use tempfile::TempDir;

    fn test_mutation_context(
        workspace_index: Arc<WorkspaceIndex>,
    ) -> (SourceMutationContext, Arc<SemanticHost>) {
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        let ctx = SourceMutationContext {
            workspace_index,
            file_id_registry: Arc::new(FileIdRegistry::new()),
            document_map: Arc::new(DashMap::new()),
            open_files: Arc::new(DashMap::new()),
            var_type_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(8).unwrap()))),
        };
        (ctx, host)
    }

    #[test]
    fn gradle_build_files_keep_source_revisions_but_pom_does_not() {
        assert!(build_file_has_source_revision(Path::new("build.gradle")));
        assert!(build_file_has_source_revision(Path::new(
            "build.gradle.kts"
        )));
        assert!(build_file_has_source_revision(Path::new("settings.gradle")));
        assert!(!build_file_has_source_revision(Path::new("pom.xml")));

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("build.gradle");
        fs::write(&path, "plugins { id 'java' }").unwrap();
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let outcome = index_created_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("Gradle script should publish a source revision");
        assert_eq!(workspace_index.get_file_id(&path), Some(outcome.file_id));

        fs::remove_file(&path).unwrap();
        remove_deleted_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("Gradle script deletion should retire its source revision");
        assert_eq!(workspace_index.get_file_id(&path), None);
    }

    #[test]
    fn published_workspace_identity_precedes_editor_registry_fallback() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("ColdIndexed.java");
        let source = "class ColdIndexed {}";
        fs::write(&path, source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let workspace_id = FileId::new(33);
        workspace_index
            .index_file_with_source_and_id(&path, source, workspace_id)
            .unwrap();

        let (mutation_ctx, _host) = test_mutation_context(Arc::clone(&workspace_index));
        let unrelated_editor_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        assert_ne!(workspace_id, unrelated_editor_id);
        let path_key = hyperion_workspace::path_lookup_key(&path);
        assert_eq!(
            existing_file_id(&mutation_ctx, &path, &path_key, false),
            Some(workspace_id)
        );
        assert_eq!(
            existing_or_allocate_file_id(&workspace_index, &mutation_ctx.file_id_registry, &path),
            workspace_id
        );
    }

    #[test]
    fn editor_owned_registry_identity_precedes_a_disagreeing_workspace_revision() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("OpenDisagreement.java");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let workspace_id = FileId::new(34);
        workspace_index
            .index_file_with_source_and_id(&path, "class Cold {}", workspace_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let editor_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        let editor_source = "class EditorWins {}";
        fs::write(&path, "class DiskMustNotWin {}").unwrap();
        host.with_db(|db| db.set_file_text_tracked(editor_id, Arc::new(editor_source.to_owned())));
        mutation_ctx
            .open_files
            .insert(editor_id, "file:///OpenDisagreement.java".to_owned());

        assert_ne!(workspace_id, editor_id);
        let path_key = hyperion_workspace::path_lookup_key(&path);
        assert_eq!(
            existing_file_id(&mutation_ctx, &path, &path_key, false),
            Some(editor_id)
        );

        let outcome = reindex_modified_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("editor-owned disagreement should converge");
        assert_eq!(outcome.file_id, editor_id);
        assert_eq!(workspace_index.get_file_id(&path), Some(editor_id));
        assert!(workspace_index.find_symbol("EditorWins").is_some());
        assert!(workspace_index.find_symbol("DiskMustNotWin").is_none());
        assert!(workspace_index.find_symbol("Cold").is_none());
    }

    #[test]
    fn modified_event_waits_for_publication_lock_and_reads_latest_disk_revision() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Contended.java");
        let old_source = "class OldRevision {}";
        let new_source = "class NewRevision {}";
        fs::write(&path, old_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        // Cold workspace scans allocate low-domain IDs without registering
        // them in the editor registry.
        let file_id = FileId::new(31);
        workspace_index
            .index_file_with_source_and_id(&path, old_source, file_id)
            .unwrap();

        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));

        let path_lock = workspace_index.get_path_lock(&path);
        let path_guard = path_lock.lock().unwrap();
        fs::write(&path, new_source).unwrap();

        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker_ctx = mutation_ctx.clone();
        let worker_host = Arc::clone(&host);
        let worker = std::thread::spawn(move || {
            let outcome =
                reindex_modified_file(&mut worker_host.write_now(), &worker_ctx, &worker_path);
            finished_tx.send(outcome).unwrap();
        });

        assert!(matches!(
            finished_rx.recv_timeout(std::time::Duration::from_millis(25)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(path_guard);

        let outcome = finished_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("modified event should finish after contention clears")
            .expect("modified event should publish its revision");
        worker.join().unwrap();

        assert_eq!(outcome.file_id, file_id);
        assert!(workspace_index.find_symbol("NewRevision").is_some());
        assert!(workspace_index.find_symbol("OldRevision").is_none());
        assert_eq!(host.read_now().file_text(file_id).as_str(), new_source);
    }

    #[test]
    fn created_event_waits_for_publication_lock_and_publishes_one_identity() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Created.java");
        let source = "package created; class Created {}";
        fs::write(&path, source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let path_lock = workspace_index.get_path_lock(&path);
        let path_guard = path_lock.lock().unwrap();

        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker_ctx = mutation_ctx.clone();
        let worker_host = Arc::clone(&host);
        let worker = std::thread::spawn(move || {
            let outcome =
                index_created_file(&mut worker_host.write_now(), &worker_ctx, &worker_path);
            finished_tx.send(outcome).unwrap();
        });

        assert!(matches!(
            finished_rx.recv_timeout(std::time::Duration::from_millis(25)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(path_guard);

        let outcome = finished_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("created event should finish after contention clears")
            .expect("created event should publish its revision");
        worker.join().unwrap();

        assert_eq!(workspace_index.get_file_id(&path), Some(outcome.file_id));
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&path),
            Some(outcome.file_id)
        );
        assert!(workspace_index.find_symbol("created.Created").is_some());
        assert_eq!(host.read_now().file_text(outcome.file_id).as_str(), source);
    }

    #[test]
    fn failed_created_event_retires_its_unpublished_identity() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Missing.java");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));

        assert!(index_created_file(&mut host.write_now(), &mutation_ctx, &path).is_none());
        assert!(workspace_index.get_file_id(&path).is_none());
        assert!(mutation_ctx.file_id_registry.get_from_path(&path).is_none());
    }

    #[test]
    fn failed_created_event_preserves_a_preexisting_registry_identity() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("StillEditorKnown.java");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let existing_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);

        assert!(index_created_file(&mut host.write_now(), &mutation_ctx, &path).is_none());
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&path),
            Some(existing_id)
        );
        assert!(workspace_index.get_file_id(&path).is_none());
    }

    #[test]
    fn delete_retires_a_closed_registry_only_identity() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("NeverPublished.java");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let registry_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        host.with_db(|db| {
            db.set_file_text_tracked(registry_id, Arc::new("class Ghost {}".to_owned()));
        });

        let outcome = remove_deleted_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("registry-only deletion should retire its stale identity");

        assert_eq!(outcome.file_id, registry_id);
        assert!(outcome.dependent_files.is_empty());
        assert!(mutation_ctx
            .file_id_registry
            .get_path(registry_id)
            .is_none());
        assert!(host.read_now().file_text(registry_id).is_empty());
    }

    #[test]
    fn registry_only_delete_yields_to_a_concurrent_did_open_publication() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("OpenedDuringDelete.java");
        let source = "class OpenedDuringDelete {}";
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let file_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        let path_lock = workspace_index.get_path_lock(&path);
        let path_guard = path_lock.lock().unwrap();

        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker_ctx = mutation_ctx.clone();
        let worker = std::thread::spawn(move || {
            finished_tx
                .send(remove_deleted_file(
                    &mut host.write_now(),
                    &worker_ctx,
                    &worker_path,
                ))
                .unwrap();
        });
        assert!(matches!(
            finished_rx.recv_timeout(std::time::Duration::from_millis(25)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        let file_lock = workspace_index.get_file_lock(file_id);
        let file_guard = file_lock.lock().unwrap();
        workspace_index
            .index_file_with_locks_held(&path, source, file_id)
            .unwrap();
        mutation_ctx
            .open_files
            .insert(file_id, "file:///OpenedDuringDelete.java".to_owned());
        drop(file_guard);
        drop(file_lock);
        drop(path_guard);
        drop(path_lock);

        assert!(finished_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("delete should finish after didOpen publishes")
            .is_none());
        worker.join().unwrap();
        assert_eq!(workspace_index.get_file_id(&path), Some(file_id));
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&path),
            Some(file_id)
        );
        assert!(workspace_index.find_symbol("OpenedDuringDelete").is_some());
    }

    #[test]
    fn registry_only_delete_rechecks_a_create_while_waiting_for_the_file_lock() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("RecreatedWhileDeleteWaits.java");
        let source = "class RecreatedWhileDeleteWaits {}";
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let file_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        let file_lock = workspace_index.get_file_lock(file_id);
        let file_guard = file_lock.lock().unwrap();
        let path_lock = workspace_index.get_path_lock(&path);

        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker_ctx = mutation_ctx.clone();
        let worker = std::thread::spawn(move || {
            finished_tx
                .send(remove_deleted_file(
                    &mut host.write_now(),
                    &worker_ctx,
                    &worker_path,
                ))
                .unwrap();
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while path_lock.try_lock().is_ok() {
            assert!(
                std::time::Instant::now() < deadline,
                "delete did not acquire its path lock"
            );
            std::thread::yield_now();
        }
        assert!(matches!(
            finished_rx.recv_timeout(std::time::Duration::from_millis(25)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        fs::write(&path, source).unwrap();
        drop(file_guard);
        drop(file_lock);

        let outcome = finished_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("delete should publish the recreated file")
            .expect("recreated source should produce an update outcome");
        worker.join().unwrap();
        drop(path_lock);

        assert_eq!(outcome.file_id, file_id);
        assert_eq!(workspace_index.get_file_id(&path), Some(file_id));
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&path),
            Some(file_id)
        );
        assert!(workspace_index
            .find_symbol("RecreatedWhileDeleteWaits")
            .is_some());
    }

    #[test]
    fn delayed_delete_publishes_a_recreated_registry_only_path() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("RecreatedRegistryOnly.java");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let registry_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        fs::write(&path, "class RecreatedRegistryOnly {}").unwrap();

        let outcome = remove_deleted_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("the recreated source should be published");

        assert_eq!(outcome.file_id, registry_id);
        assert_eq!(workspace_index.get_file_id(&path), Some(registry_id));
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&path),
            Some(registry_id)
        );
        assert!(workspace_index
            .find_symbol("RecreatedRegistryOnly")
            .is_some());
    }

    #[test]
    fn disk_publication_repairs_a_closed_workspace_registry_disagreement() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Disagreed.java");
        let old_source = "class OldWorkspaceRevision {}";
        let new_source = "class CurrentWorkspaceRevision {}";
        fs::write(&path, new_source).unwrap();
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let workspace_id = FileId::new(203);
        workspace_index
            .index_file_with_source_and_id(&path, old_source, workspace_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let stale_registry_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        host.with_db(|db| {
            db.set_file_text_tracked(
                stale_registry_id,
                Arc::new("class StaleRegistryRevision {}".to_owned()),
            );
        });

        let outcome = reindex_modified_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("the published workspace identity should win");

        assert_eq!(outcome.file_id, workspace_id);
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&path),
            Some(workspace_id)
        );
        assert!(mutation_ctx
            .file_id_registry
            .get_path(stale_registry_id)
            .is_none());
        assert!(host.read_now().file_text(stale_registry_id).is_empty());
        assert!(workspace_index
            .find_symbol("CurrentWorkspaceRevision")
            .is_some());
        assert!(workspace_index
            .find_symbol("OldWorkspaceRevision")
            .is_none());
    }

    #[test]
    fn created_event_does_not_overwrite_an_editor_owned_revision() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("OpenCreated.java");
        let editor_source = "class EditorRevision {}";
        let disk_source = "class DiskRevision {}";
        fs::write(&path, disk_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let file_id = mutation_ctx.file_id_registry.get_or_create_from_path(&path);
        workspace_index
            .index_file_with_source_and_id(&path, editor_source, file_id)
            .unwrap();
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(editor_source.to_owned())));
        mutation_ctx
            .open_files
            .insert(file_id, "file:///OpenCreated.java".to_owned());

        assert!(index_created_file(&mut host.write_now(), &mutation_ctx, &path).is_none());
        assert!(workspace_index.find_symbol("EditorRevision").is_some());
        assert!(workspace_index.find_symbol("DiskRevision").is_none());
        assert_eq!(host.read_now().file_text(file_id).as_str(), editor_source);
    }

    #[test]
    fn modified_event_does_not_resurrect_a_retired_workspace_identity() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Retired.java");
        let old_source = "class RetiredRevision {}";
        let recreated_source = "class RecreatedBeforeRegistryCleanup {}";
        fs::write(&path, old_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(32);
        workspace_index
            .index_file_with_source_and_id(&path, old_source, file_id)
            .unwrap();
        workspace_index.remove_file(file_id);
        fs::write(&path, recreated_source).unwrap();

        // A delayed modification notification for a deleted/recreated path must
        // not republish the retired workspace identity.
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));

        assert!(reindex_modified_file(&mut host.write_now(), &mutation_ctx, &path).is_none());
        assert!(workspace_index.get_file_id(&path).is_none());
        assert!(workspace_index
            .find_symbol("RecreatedBeforeRegistryCleanup")
            .is_none());
    }

    #[test]
    fn delete_event_reindexes_a_path_recreated_before_delivery() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Recreated.java");
        let old_source = "class BeforeDelete {}";
        let recreated_source = "class AfterRecreate {}";
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(34);
        workspace_index
            .index_file_with_source_and_id(&path, old_source, file_id)
            .unwrap();
        fs::write(&path, recreated_source).unwrap();

        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(old_source.to_owned())));

        let outcome = remove_deleted_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("a recreated path should publish its current disk revision");

        assert_eq!(outcome.file_id, file_id);
        assert!(workspace_index.find_symbol("BeforeDelete").is_none());
        assert!(workspace_index.find_symbol("AfterRecreate").is_some());
        assert_eq!(
            host.read_now().file_text(file_id).as_str(),
            recreated_source
        );
    }

    #[test]
    fn delete_event_keeps_an_editor_owned_revision() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Open.java");
        let source = "class OpenRevision {}";
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(35);
        workspace_index
            .index_file_with_source_and_id(&path, source, file_id)
            .unwrap();

        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(source.to_owned())));
        mutation_ctx
            .open_files
            .insert(file_id, "file:///Open.java".to_owned());

        assert!(remove_deleted_file(&mut host.write_now(), &mutation_ctx, &path).is_none());
        assert_eq!(workspace_index.get_file_id(&path), Some(file_id));
        assert!(workspace_index.find_symbol("OpenRevision").is_some());
        assert_eq!(host.read_now().file_text(file_id).as_str(), source);
    }

    #[test]
    fn delete_event_retires_a_closed_revision_from_every_semantic_layer() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Deleted.java");
        let source = concat!(
            "package deleted; ",
            "class Deleted { String value() { return \"gone\"; } }"
        );
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(36);
        workspace_index
            .index_file_with_source_and_id(&path, source, file_id)
            .unwrap();

        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        assert_eq!(
            mutation_ctx
                .file_id_registry
                .get_or_create_from_path_with_preferred_id(&path, Some(file_id)),
            file_id
        );
        host.with_db(|db| {
            db.set_file_text_tracked(file_id, Arc::new(source.to_owned()));
            db.set_file_language(file_id, Language::Java);
        });
        assert!(sync_method_signatures_for_revision(
            &mut host.write_now(),
            file_id,
            Language::Java
        ));
        assert!(host
            .member_fact_cache()
            .get_signature("deleted.Deleted.value()")
            .is_some());

        let outcome = remove_deleted_file(&mut host.write_now(), &mutation_ctx, &path)
            .expect("an absent closed path should be retired");

        assert_eq!(outcome.file_id, file_id);
        assert!(workspace_index.get_file_id(&path).is_none());
        assert!(workspace_index.find_symbol("deleted.Deleted").is_none());
        assert!(mutation_ctx.file_id_registry.get_path(file_id).is_none());
        assert!(host.read_now().file_text(file_id).is_empty());
        assert!(host
            .member_fact_cache()
            .get_signature("deleted.Deleted.value()")
            .is_none());
    }

    #[test]
    fn registry_only_rename_retires_the_old_binding_and_publishes_destination() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("RegistryOnlyOld.java");
        let new_path = temp.path().join("RegistryOnlyNew.java");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let stale_id = mutation_ctx
            .file_id_registry
            .get_or_create_from_path(&old_path);
        host.with_db(|db| {
            db.set_file_text_tracked(stale_id, Arc::new("class RegistryOnlyGhost {}".to_owned()));
        });
        fs::write(&new_path, "class RegistryOnlyNew {}").unwrap();

        let outcome = rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path)
            .expect("destination should be published as a create");

        assert!(mutation_ctx
            .file_id_registry
            .get_from_path(&old_path)
            .is_none());
        assert!(mutation_ctx.file_id_registry.get_path(stale_id).is_none());
        assert!(host.read_now().file_text(stale_id).is_empty());
        assert_eq!(
            workspace_index.get_file_id(&new_path),
            Some(outcome.file_id)
        );
        assert!(workspace_index.find_symbol("RegistryOnlyNew").is_some());
    }

    #[test]
    fn stale_rename_reindexes_a_recreated_old_path_and_publishes_the_destination() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("old/Recreated.java");
        let new_path = temp.path().join("new/Moved.java");
        fs::create_dir_all(old_path.parent().unwrap()).unwrap();
        fs::create_dir_all(new_path.parent().unwrap()).unwrap();
        let original_source = "class OriginalBeforeRename {}";
        let recreated_source = "class RecreatedAfterRename {}";
        fs::write(&old_path, original_source).unwrap();
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let old_id = FileId::new(213);
        workspace_index
            .index_file_with_source_and_id(&old_path, original_source, old_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));

        fs::write(&new_path, original_source).unwrap();
        fs::write(&old_path, recreated_source).unwrap();
        let outcome = rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path)
            .expect("stale rename should reconcile both live sources");

        let destination_id = workspace_index
            .get_file_id(&new_path)
            .expect("rename destination should be published");
        assert!(outcome.complete);
        assert_eq!(outcome.file_id, destination_id);
        assert_eq!(outcome.changed_file_ids.len(), 2);
        assert!(outcome.changed_file_ids.contains(&old_id));
        assert!(outcome.changed_file_ids.contains(&destination_id));
        assert_ne!(destination_id, old_id);
        assert_eq!(workspace_index.get_file_id(&old_path), Some(old_id));
        assert!(workspace_index
            .find_symbol("RecreatedAfterRename")
            .is_some());
        assert!(workspace_index
            .find_symbol("OriginalBeforeRename")
            .is_some());
    }

    #[cfg(unix)]
    #[test]
    fn broken_old_symlink_alias_rename_recovers_and_moves_its_stored_revision() {
        let temp = TempDir::new().unwrap();
        let backing_path = temp.path().join("Backing.java");
        let old_path = temp.path().join("OldAlias.java");
        let new_path = temp.path().join("NewAlias.java");
        let source = "class SymlinkBackedRevision {}";
        fs::write(&backing_path, source).unwrap();
        std::os::unix::fs::symlink(&backing_path, &old_path).unwrap();
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(239);
        workspace_index
            .index_file_with_source_and_id(&old_path, source, file_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        assert_eq!(
            mutation_ctx
                .file_id_registry
                .get_or_create_from_path_with_preferred_id(&old_path, Some(file_id)),
            file_id
        );
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(source.to_owned())));

        fs::rename(&old_path, &new_path).unwrap();
        let outcome = rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path)
            .expect("the vanished alias spelling should recover its stored key");

        assert_eq!(outcome.file_id, file_id);
        assert_eq!(workspace_index.get_file_id(&old_path), None);
        assert_eq!(workspace_index.get_file_id(&new_path), Some(file_id));
        assert_eq!(
            workspace_index.get_file_path(file_id),
            Some(new_path.clone())
        );
        assert_eq!(mutation_ctx.file_id_registry.get_from_path(&old_path), None);
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&new_path),
            Some(file_id)
        );
        assert!(workspace_index
            .find_symbol("SymlinkBackedRevision")
            .is_some());
    }

    #[test]
    fn rename_moves_one_closed_revision_across_every_semantic_layer() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("Old.java");
        let new_path = temp.path().join("New.java");
        let old_source = concat!(
            "package before; class Old { ",
            "String oldValue() { return \"old\"; } }"
        );
        let new_source = concat!(
            "package after; class New { ",
            "String newValue() { return \"new\"; } }"
        );
        fs::write(&old_path, old_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(40);
        workspace_index
            .index_file_with_source_and_id(&old_path, old_source, file_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        assert_eq!(
            mutation_ctx
                .file_id_registry
                .get_or_create_from_path_with_preferred_id(&old_path, Some(file_id)),
            file_id
        );
        host.with_db(|db| {
            db.set_file_text_tracked(file_id, Arc::new(old_source.to_owned()));
            db.set_file_language(file_id, Language::Java);
        });
        assert!(sync_method_signatures_for_revision(
            &mut host.write_now(),
            file_id,
            Language::Java
        ));

        fs::rename(&old_path, &new_path).unwrap();
        fs::write(&new_path, new_source).unwrap();
        let outcome = rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path)
            .expect("closed rename should publish");

        assert_eq!(outcome.file_id, file_id);
        assert_eq!(workspace_index.get_file_id(&old_path), None);
        assert_eq!(workspace_index.get_file_id(&new_path), Some(file_id));
        assert!(workspace_index.find_symbol("before.Old").is_none());
        assert!(workspace_index.find_symbol("after.New").is_some());
        assert_eq!(mutation_ctx.file_id_registry.get_from_path(&old_path), None);
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&new_path),
            Some(file_id)
        );
        assert_eq!(host.read_now().file_text(file_id).as_str(), new_source);
        assert!(host
            .member_fact_cache()
            .get_signature("before.Old.oldValue()")
            .is_none());
        assert!(host
            .member_fact_cache()
            .get_signature("after.New.newValue()")
            .is_some());
    }

    #[test]
    fn rename_retires_a_closed_destination_from_every_semantic_layer() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("Moved.java");
        let new_path = temp.path().join("Destination.java");
        let moved_source = "package moved; class Moved { String value() { return \"moved\"; } }";
        let destination_source = concat!(
            "package replaced; class Destination { ",
            "String stale() { return \"stale\"; } }"
        );
        fs::write(&old_path, moved_source).unwrap();
        fs::write(&new_path, destination_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let moved_id = FileId::new(41);
        let destination_id = FileId::new(42);
        workspace_index
            .index_file_with_source_and_id(&old_path, moved_source, moved_id)
            .unwrap();
        workspace_index
            .index_file_with_source_and_id(&new_path, destination_source, destination_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        assert_eq!(
            mutation_ctx
                .file_id_registry
                .get_or_create_from_path_with_preferred_id(&old_path, Some(moved_id)),
            moved_id
        );
        assert_eq!(
            mutation_ctx
                .file_id_registry
                .get_or_create_from_path_with_preferred_id(&new_path, Some(destination_id)),
            destination_id
        );
        host.with_db(|db| {
            db.set_file_text_tracked(moved_id, Arc::new(moved_source.to_owned()));
            db.set_file_language(moved_id, Language::Java);
            db.set_file_text_tracked(destination_id, Arc::new(destination_source.to_owned()));
            db.set_file_language(destination_id, Language::Java);
        });
        assert!(sync_method_signatures_for_revision(
            &mut host.write_now(),
            destination_id,
            Language::Java
        ));

        fs::rename(&old_path, &new_path).unwrap();
        let outcome = rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path)
            .expect("replacement rename should publish");

        assert_eq!(outcome.file_id, moved_id);
        assert_eq!(workspace_index.get_file_id(&new_path), Some(moved_id));
        assert!(workspace_index.find_symbol("moved.Moved").is_some());
        assert!(workspace_index
            .find_symbol("replaced.Destination")
            .is_none());
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&new_path),
            Some(moved_id)
        );
        assert!(mutation_ctx
            .file_id_registry
            .get_path(destination_id)
            .is_none());
        assert!(host.read_now().file_text(destination_id).is_empty());
        assert!(host
            .member_fact_cache()
            .get_signature("replaced.Destination.stale()")
            .is_none());
    }

    #[test]
    fn rename_does_not_duplicate_an_editor_owned_source_at_its_disk_destination() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("Open.java");
        let new_path = temp.path().join("Moved.java");
        let editor_source = "package opened; class OpenRevision {}";
        fs::write(&old_path, editor_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let file_id = mutation_ctx
            .file_id_registry
            .get_or_create_from_path(&old_path);
        workspace_index
            .index_file_with_source_and_id(&old_path, editor_source, file_id)
            .unwrap();
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(editor_source.to_owned())));
        mutation_ctx
            .open_files
            .insert(file_id, "file:///Open.java".to_owned());
        fs::rename(&old_path, &new_path).unwrap();

        assert!(
            rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path).is_none()
        );
        assert_eq!(workspace_index.get_file_id(&old_path), Some(file_id));
        assert_eq!(workspace_index.get_file_id(&new_path), None);
        assert!(workspace_index.find_symbol("opened.OpenRevision").is_some());
        assert_eq!(host.read_now().file_text(file_id).as_str(), editor_source);
        assert!(mutation_ctx
            .file_id_registry
            .get_from_path(&new_path)
            .is_none());
    }

    #[test]
    fn rename_never_overwrites_an_editor_owned_destination() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("Closed.java");
        let new_path = temp.path().join("OpenDestination.java");
        let old_source = "class ClosedRevision {}";
        let editor_source = "class EditorDestination {}";
        fs::write(&old_path, old_source).unwrap();
        fs::write(&new_path, editor_source).unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let old_id = FileId::new(43);
        workspace_index
            .index_file_with_source_and_id(&old_path, old_source, old_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        let destination_id = mutation_ctx
            .file_id_registry
            .get_or_create_from_path(&new_path);
        workspace_index
            .index_file_with_source_and_id(&new_path, editor_source, destination_id)
            .unwrap();
        host.with_db(|db| {
            db.set_file_text_tracked(old_id, Arc::new(old_source.to_owned()));
            db.set_file_text_tracked(destination_id, Arc::new(editor_source.to_owned()));
        });
        mutation_ctx
            .open_files
            .insert(destination_id, "file:///OpenDestination.java".to_owned());
        fs::rename(&old_path, &new_path).unwrap();

        let outcome = rename_disk_file(&mut host.write_now(), &mutation_ctx, &old_path, &new_path)
            .expect("the vanished closed source should be retired");

        assert_eq!(outcome.file_id, old_id);
        assert_eq!(workspace_index.get_file_id(&old_path), None);
        assert_eq!(workspace_index.get_file_id(&new_path), Some(destination_id));
        assert!(workspace_index.find_symbol("ClosedRevision").is_none());
        assert!(workspace_index.find_symbol("EditorDestination").is_some());
        assert!(host.read_now().file_text(old_id).is_empty());
        assert_eq!(
            host.read_now().file_text(destination_id).as_str(),
            editor_source
        );
    }

    #[test]
    fn unreadable_rename_destination_preserves_the_last_good_revision() {
        let temp = TempDir::new().unwrap();
        let old_path = temp.path().join("LastGood.java");
        let missing_new_path = temp.path().join("Missing.java");
        let source = "class LastGood {}";
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(44);
        workspace_index
            .index_file_with_source_and_id(&old_path, source, file_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(source.to_owned())));

        assert!(rename_disk_file(
            &mut host.write_now(),
            &mutation_ctx,
            &old_path,
            &missing_new_path
        )
        .is_none());
        assert_eq!(workspace_index.get_file_id(&old_path), Some(file_id));
        assert_eq!(workspace_index.get_file_id(&missing_new_path), None);
        assert!(workspace_index.find_symbol("LastGood").is_some());
        assert_eq!(host.read_now().file_text(file_id).as_str(), source);
    }

    #[test]
    fn directory_rename_reconciles_moved_removed_and_created_sources() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old/pkg");
        let to = temp.path().join("new/pkg");
        let old_a = from.join("A.java");
        let old_b = from.join("nested/B.java");
        let old_removed = from.join("Removed.java");
        fs::create_dir_all(old_b.parent().unwrap()).unwrap();
        fs::write(&old_a, "class A {}").unwrap();
        fs::write(&old_b, "class B {}").unwrap();
        fs::write(&old_removed, "class Removed {}").unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let a_id = FileId::new(45);
        let b_id = FileId::new(46);
        let removed_id = FileId::new(47);
        for (path, source, file_id) in [
            (&old_a, "class A {}", a_id),
            (&old_b, "class B {}", b_id),
            (&old_removed, "class Removed {}", removed_id),
        ] {
            workspace_index
                .index_file_with_source_and_id(path, source, file_id)
                .unwrap();
        }
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        for (path, source, file_id) in [
            (&old_a, "class A {}", a_id),
            (&old_b, "class B {}", b_id),
            (&old_removed, "class Removed {}", removed_id),
        ] {
            assert_eq!(
                mutation_ctx
                    .file_id_registry
                    .get_or_create_from_path_with_preferred_id(path, Some(file_id)),
                file_id
            );
            host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(source.to_owned())));
        }

        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::rename(&from, &to).unwrap();
        let new_a = to.join("A.java");
        let new_b = to.join("nested/B.java");
        let new_removed = to.join("Removed.java");
        fs::remove_file(&new_removed).unwrap();
        let added = to.join("Added.java");
        fs::write(&added, "class Added {}").unwrap();

        let outcome = reconcile_directory_rename(&mut host.write_now(), &mutation_ctx, &from, &to);

        assert!(outcome.scan_complete);
        assert_eq!(outcome.revisions_changed, 4);
        assert_eq!(workspace_index.get_file_id(&new_a), Some(a_id));
        assert_eq!(workspace_index.get_file_id(&new_b), Some(b_id));
        assert_eq!(workspace_index.get_file_id(&old_a), None);
        assert_eq!(workspace_index.get_file_id(&old_b), None);
        assert_eq!(workspace_index.get_file_id(&old_removed), None);
        assert!(workspace_index.find_symbol("Removed").is_none());
        assert!(host.read_now().file_text(removed_id).is_empty());
        assert!(mutation_ctx.file_id_registry.get_path(removed_id).is_none());
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&new_a),
            Some(a_id)
        );
        assert_eq!(
            mutation_ctx.file_id_registry.get_from_path(&new_b),
            Some(b_id)
        );

        let added_id = workspace_index
            .get_file_id(&added)
            .expect("new destination-only file should be indexed");
        assert!(workspace_index.find_symbol("Added").is_some());
        assert_eq!(
            host.read_now().file_text(added_id).as_str(),
            "class Added {}"
        );
        assert!(outcome.changed_file_ids.contains(&a_id));
        assert!(outcome.changed_file_ids.contains(&b_id));
        assert!(outcome.changed_file_ids.contains(&removed_id));
        assert!(outcome.changed_file_ids.contains(&added_id));
    }

    #[test]
    fn incomplete_directory_scan_retains_unmatched_old_revisions() {
        let temp = TempDir::new().unwrap();
        let from = temp.path().join("old/pkg");
        let missing_to = temp.path().join("missing/pkg");
        let old_path = from.join("LastGood.java");
        let source = "class DirectoryLastGood {}";
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let file_id = FileId::new(48);
        workspace_index
            .index_file_with_source_and_id(&old_path, source, file_id)
            .unwrap();
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        host.with_db(|db| db.set_file_text_tracked(file_id, Arc::new(source.to_owned())));

        let outcome =
            reconcile_directory_rename(&mut host.write_now(), &mutation_ctx, &from, &missing_to);

        assert!(!outcome.scan_complete);
        assert_eq!(outcome.revisions_changed, 0);
        assert!(outcome.changed_file_ids.is_empty());
        assert_eq!(workspace_index.get_file_id(&old_path), Some(file_id));
        assert!(workspace_index.find_symbol("DirectoryLastGood").is_some());
        assert_eq!(host.read_now().file_text(file_id).as_str(), source);
    }

    #[test]
    fn source_tree_scan_finds_nested_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let deep = root.join("a/b/c");
        fs::create_dir_all(&deep).unwrap();

        let java_file = root.join("Top.java");
        let kt_file = deep.join("Nested.kt");
        let scala_file = root.join("a/Also.scala");
        let gradle_file = root.join("build.gradle");
        let txt_file = deep.join("ignore.txt");

        fs::write(&java_file, "class Top {}").unwrap();
        fs::write(&kt_file, "class Nested").unwrap();
        fs::write(&scala_file, "class Also").unwrap();
        fs::write(&gradle_file, "plugins { id 'java' }").unwrap();
        fs::write(&txt_file, "ignore").unwrap();

        let scan = scan_source_tree(root, root);

        assert!(scan.complete);
        assert!(scan.files.contains(&java_file));
        assert!(scan.files.contains(&kt_file));
        assert!(scan.files.contains(&scala_file));
        assert!(scan.files.contains(&gradle_file));
        assert!(!scan.files.contains(&txt_file));
        assert!(scan.build_files_found);
    }

    #[test]
    fn source_tree_scan_matches_workspace_hidden_and_build_output_policy() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let visible = root.join("src/main/java/Visible.java");
        let hidden = root.join(".hidden/Hidden.java");
        let target = root.join("target/generated/Target.java");
        let build = root.join("build/generated/Build.java");
        let ignored = root.join("ignored/Ignored.java");
        for path in [&visible, &hidden, &target, &build, &ignored] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "class Source {}").unwrap();
        }
        fs::write(root.join(".gitignore"), "ignored/\n").unwrap();

        let scan = scan_source_tree(root, root);

        assert!(scan.complete);
        assert!(scan.files.contains(&visible));
        assert!(!scan.files.contains(&hidden));
        assert!(!scan.files.contains(&target));
        assert!(!scan.files.contains(&build));
        assert!(!scan.files.contains(&ignored));
    }

    #[test]
    fn directory_events_keep_the_workspace_exclusion_boundary() {
        let temp = TempDir::new().unwrap();
        let workspace_root = temp.path().join("build/checkout");
        let workspace_index = Arc::new(WorkspaceIndex::new(workspace_root.clone()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));
        for directory in [
            "src",
            "build/classes",
            "target/classes",
            "bin",
            "dist",
            "node_modules",
        ] {
            let subtree = workspace_root.join(directory);
            fs::create_dir_all(&subtree).unwrap();
            let source_path = subtree.join("EventSource.java");
            fs::write(&source_path, "class EventSource {}").unwrap();
            let created =
                reconcile_directory_create(&mut host.write_now(), &mutation_ctx, &subtree);
            assert!(created.complete, "{directory}");
            assert_eq!(
                created.revisions_changed,
                usize::from(directory == "src"),
                "{directory}"
            );
            assert_eq!(
                workspace_index.get_file_id(&source_path).is_some(),
                directory == "src",
                "{directory}"
            );
        }
        assert!(workspace_index.find_symbol("EventSource").is_some());

        // Moving real sources into outputs must retire their old index entries
        // without reintroducing the same declarations under the output path.
        let from = workspace_root.join("src");
        let to = workspace_root.join("out/moved");
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::rename(&from, &to).unwrap();
        let renamed = reconcile_directory_rename(&mut host.write_now(), &mutation_ctx, &from, &to);
        assert!(renamed.complete);
        assert!(workspace_index.find_symbol("EventSource").is_none());
        assert!(workspace_index
            .get_file_id(&to.join("EventSource.java"))
            .is_none());
    }

    #[test]
    fn directory_create_and_delete_reconcile_descendants_without_child_events() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("new-module");
        let source_path = root.join("src/main/java/example/OnlyDirectoryEvent.java");
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::write(
            &source_path,
            "package example; public class OnlyDirectoryEvent {}",
        )
        .unwrap();
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let (mutation_ctx, host) = test_mutation_context(Arc::clone(&workspace_index));

        let created = reconcile_directory_create(&mut host.write_now(), &mutation_ctx, &root);
        assert!(created.complete);
        assert_eq!(created.revisions_changed, 1);
        assert!(workspace_index
            .find_symbol("example.OnlyDirectoryEvent")
            .is_some());

        fs::remove_dir_all(&root).unwrap();
        let deleted = reconcile_directory_delete(&mut host.write_now(), &mutation_ctx, &root);
        assert!(deleted.complete);
        assert_eq!(deleted.revisions_changed, 1);
        assert!(workspace_index
            .find_symbol("example.OnlyDirectoryEvent")
            .is_none());
    }

    #[test]
    fn old_path_for_renamed_file_maps_relative_path() {
        let from = PathBuf::from("/tmp/old/com/example");
        let to = PathBuf::from("/tmp/new/com/company");
        let new_path = PathBuf::from("/tmp/new/com/company/deep/Foo.java");

        let old = old_path_for_renamed_file(&from, &to, &new_path).unwrap();
        assert_eq!(old, PathBuf::from("/tmp/old/com/example/deep/Foo.java"));
    }

    #[test]
    fn old_path_for_renamed_file_rejects_unrelated_paths() {
        let from = PathBuf::from("/tmp/old");
        let to = PathBuf::from("/tmp/new");
        let outside = PathBuf::from("/tmp/other/Foo.java");

        assert!(old_path_for_renamed_file(&from, &to, &outside).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn old_path_for_renamed_file_uses_resolved_suffix_across_symlink_depth() {
        let temp = TempDir::new().unwrap();
        let real_destination = temp.path().join("expanded/deep/destination");
        fs::create_dir_all(real_destination.join("pkg")).unwrap();
        let destination_alias = temp.path().join("to");
        std::os::unix::fs::symlink(&real_destination, &destination_alias).unwrap();
        let old_root = temp.path().join("from");
        let scanned_path = real_destination.join("pkg/Moved.java");

        assert_eq!(
            old_path_for_renamed_file(&old_root, &destination_alias, &scanned_path),
            Some(old_root.join("pkg/Moved.java"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn old_path_for_renamed_file_rejects_lexical_and_symlink_escapes() {
        let temp = TempDir::new().unwrap();
        let destination = temp.path().join("destination");
        let outside = temp.path().join("outside");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&outside).unwrap();
        let escape = destination.join("escape");
        std::os::unix::fs::symlink(&outside, &escape).unwrap();
        let old_root = temp.path().join("from");

        assert_eq!(
            old_path_for_renamed_file(
                &old_root,
                &destination,
                &destination.join("../outside/Moved.java"),
            ),
            None
        );
        assert_eq!(
            old_path_for_renamed_file(&old_root, &destination, &escape.join("Escaped.java"),),
            None
        );
    }
}
