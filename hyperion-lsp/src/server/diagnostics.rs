// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::atomic::Ordering;
use std::sync::Arc;

use dashmap::DashMap;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::Language;
use tokio::time::{sleep, Duration};
use tower_lsp::lsp_types::MessageType;
use tracing::{debug, info, warn};

use super::source_indexing::{
    BulkSourceIndexContext, BulkSourceIndexTransaction, BulkSourceScanPolicy,
};
use super::{find_git_root, find_nearest_source_root, DocumentState, IndexGate};
use crate::semantic_host::WriteTxn;
use crate::server::state::DiagnosticPublicationState;
use crate::server::Backend;

use std::path::Path;

/// Retry budget for open-file diagnostic refreshes superseded by concurrent
/// source publications (about a second at 40 ms per round).
const MAX_DEFERRED_REFRESH_ROUNDS: usize = 25;

/// Shared state and services for the single dependent-invalidation worker.
///
/// Cloning this value is cheap: every mutable or heavyweight component is
/// shared behind an `Arc`. Keeping the live document map here is important;
/// copying a `DashMap` when a job starts gives a long-running worker stale open
/// buffers and can publish disk text over a later editor revision.
#[derive(Clone)]
pub struct InvalidationRuntime {
    pub(crate) coordinator: Arc<parking_lot::Mutex<crate::server::state::InvalidationCoordinator>>,
    pub(crate) host: Arc<crate::semantic_host::SemanticHost>,
    pub(crate) project_classpaths: crate::project_classpath::ProjectClasspathStore,
    pub(crate) document_map: Arc<DashMap<String, DocumentState>>,
    pub(crate) open_files: Arc<DashMap<FileId, String>>,
    pub(crate) editor_owned_files: Arc<dashmap::DashSet<FileId>>,
    pub(crate) var_type_cache: Arc<crate::source_publication::VarTypeCache>,
    pub(crate) client: tower_lsp::Client,
    pub(crate) security_provider: Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>,
    pub(crate) diagnostic_options:
        Arc<parking_lot::RwLock<crate::diagnostic_engine::DiagnosticOptions>>,
    pub(crate) workspace_ready: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) destructive_refactoring: Arc<crate::server::DestructiveRefactoringReadiness>,
    pub(crate) diagnostic_publication: Arc<DiagnosticPublicationState>,
}

impl InvalidationRuntime {
    /// Create a standalone invalidation runtime.
    ///
    /// The server normally shares its coordinator through [`Backend`]. This
    /// constructor is useful for embedding the public file-event handler in a
    /// smaller host that owns the same index/database/document components.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: Arc<crate::semantic_host::SemanticHost>,
        document_map: Arc<DashMap<String, DocumentState>>,
        open_files: Arc<DashMap<FileId, String>>,
        editor_owned_files: Arc<dashmap::DashSet<FileId>>,
        client: tower_lsp::Client,
        security_provider: Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>,
    ) -> Self {
        Self {
            coordinator: Arc::new(parking_lot::Mutex::new(
                crate::server::state::InvalidationCoordinator::new(),
            )),
            host,
            project_classpaths: crate::project_classpath::compatibility_store(),
            document_map,
            open_files,
            editor_owned_files,
            var_type_cache: Arc::new(parking_lot::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(1).expect("non-zero standalone cache capacity"),
            ))),
            client,
            security_provider,
            diagnostic_options: Arc::new(parking_lot::RwLock::new(
                crate::diagnostic_engine::DiagnosticOptions::default(),
            )),
            workspace_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            destructive_refactoring: Arc::new(crate::server::DestructiveRefactoringReadiness::new()),
            diagnostic_publication: Arc::new(DiagnosticPublicationState::new()),
        }
    }

    /// Publish a completed mutation of the external workspace index to Salsa.
    ///
    /// Build-system and SDK indexing can affect any registered file, including
    /// files that had unresolved names before a dependency appeared or that
    /// still resolve a dependency which was just removed. The global epoch is
    /// therefore appropriate here; ordinary source edits use the narrower
    /// dependent-file invalidation queue instead.
    pub(crate) async fn publish_workspace_revision(&self, source: &str) {
        self.publish_workspace_epoch().await;
        self.publish_workspace_diagnostics(source).await;
    }

    /// Make an already-completed external-index mutation visible to Salsa.
    /// Startup calls this before flipping `workspace_ready`, closing the small
    /// window where requests could otherwise observe ready=true with the old
    /// semantic epoch. Callers that publish the index mutation under their own
    /// [`WriteTxn`](crate::semantic_host::WriteTxn) bump the epoch in that same
    /// transaction instead.
    pub(crate) async fn publish_workspace_epoch(&self) {
        self.host.write().await.bump_workspace_epoch();
    }

    pub(crate) async fn publish_workspace_diagnostics(&self, source: &str) {
        loop {
            let workspace_revision = self.host.index().current_workspace_revision();
            let mut open_uris: Vec<_> = self
                .open_files
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            open_uris.sort_unstable();
            open_uris.dedup();
            let publications: Vec<_> = open_uris
                .into_iter()
                .filter_map(|uri| {
                    let generation = self.diagnostic_publication.supersede_open_uri(&uri)?;
                    Some((uri, generation))
                })
                .collect();

            if !self.workspace_ready.load(Ordering::Acquire) {
                debug!(
                    "Published {source} index revision; diagnostics wait for workspace readiness"
                );
                return;
            }
            if self.host.index().current_workspace_revision() != workspace_revision {
                tokio::task::yield_now().await;
                continue;
            }

            let diagnostic_options = *self.diagnostic_options.read();
            for (uri, generation) in publications {
                super::parse_and_publish_diagnostics_for_uri(
                    &uri,
                    &self.document_map,
                    &self.host,
                    &self.client,
                    Some(&self.security_provider),
                    diagnostic_options,
                    Some(&self.workspace_ready),
                    &self.diagnostic_publication,
                    generation,
                    workspace_revision,
                )
                .await;
            }
            if self.host.index().current_workspace_revision() == workspace_revision {
                return;
            }
            debug!("Workspace changed during {source} diagnostics; retrying exact revision");
            tokio::task::yield_now().await;
        }
    }

    /// Add an immutable old+new dependent snapshot to the global queue.
    ///
    /// There is exactly one worker. A file enqueued again after the worker has
    /// drained its current batch remains pending and is processed once more
    /// against the newest workspace revision. This gives supersession without
    /// losing dependents that only existed in an older declaration graph.
    pub(crate) fn enqueue(&self, changed_file_id: FileId, dependent_files: Vec<FileId>) {
        self.enqueue_batch_inner(vec![changed_file_id], dependent_files, None);
    }

    pub(crate) fn enqueue_with_readiness(
        &self,
        changed_file_id: FileId,
        dependent_files: Vec<FileId>,
        readiness: crate::server::state::SourceMutationReadinessGuard,
    ) {
        self.enqueue_batch_inner(vec![changed_file_id], dependent_files, Some(readiness));
    }

    /// Enqueue one completed multi-file publication as a single invalidation
    /// generation. Every changed identity is excluded from the dependent walk;
    /// all other captured dependents are coalesced before the worker starts.
    pub(crate) fn enqueue_batch(
        &self,
        changed_file_ids: Vec<FileId>,
        dependent_files: Vec<FileId>,
    ) {
        self.enqueue_batch_inner(changed_file_ids, dependent_files, None);
    }

    pub(crate) fn enqueue_batch_with_readiness(
        &self,
        changed_file_ids: Vec<FileId>,
        dependent_files: Vec<FileId>,
        readiness: crate::server::state::SourceMutationReadinessGuard,
    ) {
        self.enqueue_batch_inner(changed_file_ids, dependent_files, Some(readiness));
    }

    fn enqueue_batch_inner(
        &self,
        mut changed_file_ids: Vec<FileId>,
        dependent_files: Vec<FileId>,
        readiness: Option<crate::server::state::SourceMutationReadinessGuard>,
    ) {
        changed_file_ids.sort_unstable_by_key(|file_id| file_id.as_u32());
        changed_file_ids.dedup();
        if changed_file_ids.is_empty() {
            if let Some(readiness) = readiness {
                readiness.finish(true);
            }
            return;
        }

        let changed_file_ids_set: std::collections::HashSet<_> =
            changed_file_ids.iter().copied().collect();
        let mut new_files: Vec<_> = dependent_files
            .into_iter()
            .filter(|file_id| !changed_file_ids_set.contains(file_id))
            .collect();
        new_files.sort_unstable_by_key(|file_id| file_id.as_u32());
        new_files.dedup();
        if new_files.is_empty() {
            if let Some(readiness) = readiness {
                readiness.finish(true);
            }
            return;
        }

        let mut coordinator = self.coordinator.lock();
        for changed_file_id in changed_file_ids {
            coordinator.enqueue(changed_file_id, new_files.iter().copied());
        }
        if let Some(readiness) = readiness {
            coordinator.enqueue_readiness(readiness);
        }

        if coordinator.worker_is_running() {
            return;
        }

        let runtime = self.clone();
        let worker = tokio::spawn(async move {
            runtime.run_worker().await;
        });
        coordinator.replace_worker(worker);
    }

    async fn run_worker(self) {
        // Open files whose refreshed diagnostics may not have been published:
        // a refresh is superseded whenever the workspace revision moves under
        // it (any other file event) or a source publisher is active, and a
        // superseding event that changes no dependent set enqueues nothing.
        // Without this carry-over the open caller would keep stale diagnostics
        // until it was edited (e.g. after `git checkout` of an unopened file).
        let mut deferred: Vec<String> = Vec::new();
        let mut deferred_rounds = 0usize;
        loop {
            enum Next<B> {
                Batch(B),
                RetryDeferred,
                Done,
            }
            let next = {
                let mut coordinator = self.coordinator.lock();
                match coordinator.take_batch() {
                    Some(batch) => Next::Batch(batch),
                    None if deferred.is_empty()
                        || deferred_rounds >= MAX_DEFERRED_REFRESH_ROUNDS =>
                    {
                        // Drop our own completed handle. Enqueue holds the same
                        // lock, so it either leaves work for this loop or
                        // starts a successor.
                        coordinator.clear_worker();
                        Next::Done
                    }
                    None => Next::RetryDeferred,
                }
            };
            let (changed_file_ids, dependent_files, readiness) = match next {
                Next::Batch(batch) => batch,
                Next::Done => return,
                Next::RetryDeferred => {
                    deferred_rounds += 1;
                    sleep(Duration::from_millis(40)).await;
                    let uris = std::mem::take(&mut deferred);
                    deferred = self.publish_open_uris(uris).await;
                    continue;
                }
            };

            let document_map = Arc::clone(&self.document_map);
            let open_files = Arc::clone(&self.open_files);
            let editor_owned_files = Arc::clone(&self.editor_owned_files);
            let coordinator = Arc::clone(&self.coordinator);
            let mut progress = Some(InvalidationProgress::new(
                &changed_file_ids,
                dependent_files,
            ));
            // One slice at a time under the write transaction: notifications
            // and requests queued for it run between slices instead of after
            // the whole transitive re-index.
            while progress.as_ref().is_some_and(|p| !p.is_finished()) {
                let Some(mut running) = progress.take() else {
                    break;
                };
                let txn = self.host.write().await;
                let document_map = Arc::clone(&document_map);
                let open_files = Arc::clone(&open_files);
                let editor_owned_files = Arc::clone(&editor_owned_files);
                let coordinator = Arc::clone(&coordinator);
                let slice = txn
                    .run_blocking(move |txn| {
                        run_transitive_invalidation_slice(
                            &mut running,
                            Some(INVALIDATION_SLICE),
                            txn,
                            &document_map,
                            &open_files,
                            |file_id| coordinator.lock().should_defer(file_id),
                            |file_id| editor_owned_files.contains(&file_id),
                        );
                        running
                    })
                    .await;
                match slice {
                    Ok(next) => progress = Some(next),
                    Err(error) => warn!("Transitive invalidation worker panicked: {error}"),
                }
                tokio::task::yield_now().await;
            }
            let invalidation = progress.map_or(
                InvalidationBatchOutcome {
                    open_file_uris: Vec::new(),
                    complete: false,
                },
                InvalidationProgress::into_outcome,
            );
            for readiness in readiness {
                readiness.finish(invalidation.complete);
            }
            deferred.extend(invalidation.open_file_uris);
            deferred.sort_unstable();
            deferred.dedup();
            deferred_rounds = 0;

            if self.coordinator.lock().has_pending() {
                continue;
            }

            // Clear stale diagnostics only after the corresponding open buffer
            // has been re-indexed. New queue entries can accumulate while these
            // async client notifications run and are drained by the next loop.
            let uris = std::mem::take(&mut deferred);
            deferred = self.publish_open_uris(uris).await;
        }
    }

    /// Republish diagnostics for `uris`; returns those whose result may have
    /// been superseded and must be retried.
    async fn publish_open_uris(&self, uris: Vec<String>) -> Vec<String> {
        let diagnostic_options = *self.diagnostic_options.read();
        let mut retry = Vec::new();
        let mut iter = uris.into_iter();
        while let Some(uri) = iter.next() {
            let workspace_revision = self.host.index().current_workspace_revision();
            let Some(generation) = self.diagnostic_publication.supersede_open_uri(&uri) else {
                continue;
            };
            super::parse_and_publish_diagnostics_for_uri(
                &uri,
                &self.document_map,
                &self.host,
                &self.client,
                Some(&self.security_provider),
                diagnostic_options,
                Some(&self.workspace_ready),
                &self.diagnostic_publication,
                generation,
                workspace_revision,
            )
            .await;
            if self.host.index().current_workspace_revision() != workspace_revision
                || self.host.write_in_progress()
                || self.coordinator.lock().has_pending()
            {
                retry.push(uri);
                retry.extend(iter);
                break;
            }
        }
        retry
    }
}

impl From<&InvalidationRuntime> for BulkSourceIndexContext {
    fn from(runtime: &InvalidationRuntime) -> Self {
        Self {
            host: Arc::clone(&runtime.host),
            open_files: Arc::clone(&runtime.open_files),
            var_type_cache: Arc::clone(&runtime.var_type_cache),
            destructive_refactoring: Arc::clone(&runtime.destructive_refactoring),
            workspace_source_indexing: None,
            build_progress: None,
        }
    }
}

impl Backend {
    /// Logs a message to the client's output channel.
    ///
    /// This is useful for debugging and providing feedback to users during development.
    ///
    /// # Arguments
    ///
    /// * `typ` - The message type (Error, Warning, Info, or Log)
    /// * `message` - The message text to display
    pub(crate) async fn log_message(&self, typ: MessageType, message: String) {
        self.client.log_message(typ, message).await;
    }

    pub(crate) async fn maybe_index_nested_git_repo(&self, file_path: &Path) {
        let Some(workspace_root) = self.workspace.root.read().clone() else {
            return;
        };

        let workspace_git_root =
            find_git_root(&workspace_root).unwrap_or_else(|| workspace_root.clone());
        let Some(file_git_root) = find_git_root(file_path) else {
            return;
        };
        let workspace_git_key = hyperion_workspace::path_lookup_key(&workspace_git_root);
        let file_git_key = hyperion_workspace::path_lookup_key(&file_git_root);

        // If the file is in the same repo as the workspace root, nothing special.
        if file_git_key == workspace_git_key {
            return;
        }

        // Only index each nested repo root once.
        if self
            .workspace
            .indexed_repos
            .insert(file_git_key.clone(), ())
            .is_some()
        {
            return;
        }

        let transaction = BulkSourceIndexTransaction::reserve(
            BulkSourceIndexContext::from(&self.invalidation_runtime()),
            vec![file_git_root.clone()],
        );
        let indexed_repos = Arc::clone(&self.workspace.indexed_repos);
        tokio::spawn(async move {
            // Let fast source-root indexing run first; nested repos can be huge and would
            // otherwise contend for CPU with the first navigation request.
            sleep(Duration::from_secs(1)).await;

            let txn = transaction.context.host.write().await;

            let retry_key = file_git_key;
            let indexed_repos_after_join = Arc::clone(&indexed_repos);
            let result = txn
                .run_blocking(move |txn| {
                    info!(
                        "Indexing nested git repo root (scoped gitignore): {}",
                        file_git_root.display()
                    );
                    // Keep nested-repo indexing cheap: rely on cache + skip-cached
                    // for bulk repos. The transaction restores authoritative editor
                    // buffers and publishes the semantic epoch before readiness.
                    let outcome =
                        transaction.run(txn, false, BulkSourceScanPolicy::ScopedGitignore);
                    outcome.source_scan_failures == 0
                        && outcome.stats.files_failed == 0
                        && outcome.open_documents_restored
                })
                .await;
            if !matches!(result, Ok(true)) {
                indexed_repos_after_join.remove(&retry_key);
            }
        });
    }

    pub(crate) async fn maybe_index_open_file_source_root(&self, file_path: &Path) {
        let _ = self
            .ensure_open_file_source_root_indexed(file_path, false)
            .await;
    }

    pub(crate) async fn ensure_open_file_source_root_indexed(
        &self,
        file_path: &Path,
        wait: bool,
    ) -> bool {
        // During initial workspace build, eager source-root reindexing can contend heavily
        // with first interactive requests (hover/goto). Defer best-effort background
        // source-root indexing until the workspace is marked ready.
        if !wait && !self.workspace.ready.load(Ordering::Acquire) {
            debug!(
                "Deferring source-root indexing for {} until workspace is ready",
                file_path.display()
            );
            return false;
        }

        let Some(source_root) = find_nearest_source_root(file_path) else {
            return false;
        };
        let source_root_key = hyperion_workspace::path_lookup_key(&source_root);

        // A failed gate used to be dropped and rebuilt here, which re-spawned a
        // full source-root index and re-armed the wait below on *every*
        // interactive request. One file that cannot be indexed then made every
        // goto-definition and find-references pay the whole timeout plus a
        // competing re-index, permanently. The gate now owns the retry policy:
        // it re-arms a bounded number of times and only after a cooldown, and
        // otherwise stays failed so the wait returns immediately.
        let (gate, spawn_indexing) = match self.workspace.source_root_gates.entry(source_root_key) {
            dashmap::mapref::entry::Entry::Occupied(entry) => {
                let gate = Arc::clone(entry.get());
                let retrying = gate.claim_retry();
                (gate, retrying)
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let gate = Arc::new(IndexGate::new());
                gate.begin_initial_attempt();
                entry.insert(Arc::clone(&gate));
                (gate, true)
            }
        };

        if spawn_indexing {
            let transaction = BulkSourceIndexTransaction::reserve(
                BulkSourceIndexContext::from(&self.invalidation_runtime()),
                vec![source_root.clone()],
            );
            let gate_clone = Arc::clone(&gate);
            let delay_indexing = !wait;
            tokio::spawn(async move {
                // Give first hover/goto requests a short head start after didOpen.
                if delay_indexing {
                    sleep(Duration::from_millis(500)).await;
                }

                let txn = transaction.context.host.write().await;

                let gate_for_join_error = Arc::clone(&gate_clone);
                let join = txn
                    .run_blocking(move |txn| {
                        info!(
                            "Indexing opened-file source root (ignore gitignore): {}",
                            source_root.display()
                        );
                        // Use skip-cached indexing here to keep first references/rename responsive.
                        // Current reference queries re-extract usages from exact source bytes, so
                        // this pass only needs files that are new or whose content hash changed.
                        let outcome =
                            transaction.run(txn, false, BulkSourceScanPolicy::IgnoreGitignore);
                        gate_clone.mark_done(
                            outcome.source_scan_failures == 0
                                && outcome.stats.files_failed == 0
                                && outcome.open_documents_restored,
                        );
                    })
                    .await;

                if let Err(err) = join {
                    tracing::warn!("Opened-file source-root indexing task failed: {}", err);
                    gate_for_join_error.mark_done(false);
                }
            });
        }

        if wait {
            gate.wait_done(Duration::from_secs(3)).await
        } else {
            false
        }
    }

    /// Invalidate the immutable dependent set captured by the indexing
    /// transaction before historical reverse edges were removed.
    pub(crate) fn invalidate_dependent_file_ids(
        &self,
        changed_file_id: FileId,
        dependent_files: Vec<FileId>,
    ) {
        self.invalidation_runtime()
            .enqueue(changed_file_id, dependent_files);
    }

    pub(crate) fn invalidate_dependent_file_ids_with_readiness(
        &self,
        changed_file_id: FileId,
        dependent_files: Vec<FileId>,
        readiness: crate::server::state::SourceMutationReadinessGuard,
    ) {
        self.invalidation_runtime().enqueue_with_readiness(
            changed_file_id,
            dependent_files,
            readiness,
        );
    }

    pub(crate) fn invalidation_runtime(&self) -> InvalidationRuntime {
        InvalidationRuntime {
            coordinator: Arc::clone(&self.background_tasks.invalidation),
            host: Arc::clone(&self.workspace.host),
            project_classpaths: self.workspace.project_classpaths.clone(),
            document_map: Arc::clone(&self.documents.by_uri),
            open_files: Arc::clone(&self.documents.open_by_id),
            editor_owned_files: Arc::clone(&self.documents.editor_owned),
            var_type_cache: Arc::clone(self.services.completion.var_type_cache()),
            client: self.client.clone(),
            security_provider: Arc::clone(&self.services.security),
            diagnostic_options: Arc::clone(&self.workspace.diagnostic_options),
            workspace_ready: Arc::clone(&self.workspace.ready),
            destructive_refactoring: Arc::clone(&self.workspace.destructive_refactoring),
            diagnostic_publication: Arc::clone(&self.background_tasks.diagnostic_publication),
        }
    }
}

/// Background transitive invalidation: BFS re-indexes all files that depend on the changed file.
///
/// PERF: This is a synchronous function designed to run inside `spawn_blocking`.
/// It does blocking I/O (disk reads for closed files) and CPU-heavy work (parsing
/// + symbol extraction) that would starve the tokio async runtime if run directly.
///
/// Returns the URIs of open editor files that were re-indexed, so callers can
/// republish diagnostics for them.
pub fn run_transitive_invalidation(
    changed_file_id: FileId,
    txn: &mut WriteTxn,
    document_map: &DashMap<String, DocumentState>,
    open_files: &Arc<DashMap<FileId, String>>,
) -> Vec<String> {
    let dependent_files = txn.index().get_dependent_files_for_file(changed_file_id);
    run_transitive_invalidation_for_dependents(
        changed_file_id,
        dependent_files,
        txn,
        document_map,
        open_files,
    )
}

pub(crate) fn run_transitive_invalidation_for_dependents(
    changed_file_id: FileId,
    dependent_files: Vec<FileId>,
    txn: &mut WriteTxn,
    document_map: &DashMap<String, DocumentState>,
    open_files: &Arc<DashMap<FileId, String>>,
) -> Vec<String> {
    run_transitive_invalidation_batch(
        &[changed_file_id],
        dependent_files,
        txn,
        document_map,
        open_files,
        |_| false,
        |file_id| open_files.contains_key(&file_id),
    )
    .open_file_uris
}

#[derive(Debug)]
enum DependentTextRevision {
    /// Text read from disk while the file was not editor-owned.
    Disk(Arc<String>),
    /// The exact editor revision observed while holding the file's index lock.
    /// Pointer identity is used as a lightweight revision token at publication.
    Editor(Arc<String>),
}

#[derive(Debug)]
struct DependentDatabaseUpdate {
    file_id: FileId,
    text: DependentTextRevision,
    language: Language,
}

struct InvalidationBatchOutcome {
    open_file_uris: Vec<String>,
    complete: bool,
}

/// How long one slice of a transitive invalidation may hold the write
/// transaction. Re-indexing the dependents of a hub type takes seconds; done in
/// one transaction it blocked every notification and request behind it
/// so the worker re-acquires the transaction between slices.
const INVALIDATION_SLICE: Duration = Duration::from_millis(30);

/// State of a transitive invalidation that runs in permit-sized slices.
struct InvalidationProgress {
    to_invalidate: std::collections::VecDeque<FileId>,
    invalidated: std::collections::HashSet<FileId>,
    changed_file_ids: std::collections::HashSet<FileId>,
    open_file_uris: Vec<String>,
    complete: bool,
}

impl InvalidationProgress {
    fn new(changed_file_ids: &[FileId], dependent_files: Vec<FileId>) -> Self {
        let changed_file_ids: std::collections::HashSet<_> =
            changed_file_ids.iter().copied().collect();
        let mut to_invalidate: std::collections::VecDeque<FileId> = dependent_files.into();
        to_invalidate.retain(|id| !changed_file_ids.contains(id));
        Self {
            to_invalidate,
            invalidated: std::collections::HashSet::new(),
            changed_file_ids,
            open_file_uris: Vec::new(),
            complete: true,
        }
    }

    fn is_finished(&self) -> bool {
        self.to_invalidate.is_empty()
    }

    fn into_outcome(mut self) -> InvalidationBatchOutcome {
        self.open_file_uris.sort_unstable();
        self.open_file_uris.dedup();
        InvalidationBatchOutcome {
            open_file_uris: self.open_file_uris,
            complete: self.complete,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_transitive_invalidation_batch(
    changed_file_ids: &[FileId],
    dependent_files: Vec<FileId>,
    txn: &mut WriteTxn,
    document_map: &DashMap<String, DocumentState>,
    open_files: &Arc<DashMap<FileId, String>>,
    should_defer: impl Fn(FileId) -> bool,
    is_editor_owned: impl Fn(FileId) -> bool,
) -> InvalidationBatchOutcome {
    let mut progress = InvalidationProgress::new(changed_file_ids, dependent_files);
    run_transitive_invalidation_slice(
        &mut progress,
        None,
        txn,
        document_map,
        open_files,
        should_defer,
        is_editor_owned,
    );
    progress.into_outcome()
}

/// Re-index queued dependents until `budget` is spent (`None`: until done).
#[allow(clippy::too_many_arguments)]
fn run_transitive_invalidation_slice(
    progress: &mut InvalidationProgress,
    budget: Option<Duration>,
    txn: &mut WriteTxn,
    document_map: &DashMap<String, DocumentState>,
    open_files: &Arc<DashMap<FileId, String>>,
    should_defer: impl Fn(FileId) -> bool,
    is_editor_owned: impl Fn(FileId) -> bool,
) {
    let started = std::time::Instant::now();
    let workspace_index = Arc::clone(txn.index());
    let InvalidationProgress {
        to_invalidate,
        invalidated,
        changed_file_ids,
        open_file_uris,
        complete,
    } = progress;

    while let Some(dep_file_id) = to_invalidate.pop_front() {
        if budget.is_some_and(|budget| started.elapsed() >= budget) {
            to_invalidate.push_front(dep_file_id);
            return;
        }
        // A notification that arrived after this batch was drained owns the
        // final publication for this file. Leave it in the live queue instead
        // of doing duplicate work against an older workspace revision.
        if should_defer(dep_file_id) {
            debug!(
                "Deferring dependent {:?} to a newer invalidation batch",
                dep_file_id
            );
            continue;
        }
        if !invalidated.insert(dep_file_id) {
            continue;
        }

        let Some(dep_registered_path) = workspace_index.get_registered_file_path(dep_file_id)
        else {
            warn!(
                "Could not find indexed path for dependent FileId {:?}",
                dep_file_id
            );
            continue;
        };
        let dep_path = dep_registered_path.display_path();
        let dep_path_key = dep_registered_path.lookup_key();

        // Serialize the editor-state snapshot with every disk/source publisher.
        // Path-before-file is the workspace-wide lock order. If invalidation
        // captured text before these locks, it could wait behind a newer
        // didChange and then overwrite the fresh index with stale content.
        let path_lock = workspace_index.get_path_lock_for_key(dep_path_key);
        let _path_guard = path_lock.lock().unwrap_or_else(|poisoned| {
            warn!(
                "Path lock for dependent {} was poisoned; continuing with recovered state",
                dep_path.display()
            );
            poisoned.into_inner()
        });
        let file_lock = workspace_index.get_file_lock(dep_file_id);
        let per_file_guard = file_lock.lock().unwrap_or_else(|poisoned| {
            warn!(
                "File lock for dependent {:?} was poisoned; continuing with recovered state",
                dep_file_id
            );
            poisoned.into_inner()
        });

        if !workspace_index.is_registered_file_path_current(dep_file_id, &dep_registered_path) {
            debug!(
                "Skipping retired or relocated dependent {:?} at {}",
                dep_file_id,
                dep_path.display()
            );
            continue;
        }

        // Try in-memory content first (file may be open in editor)
        // Also capture the URI for diagnostic republishing
        // O(1) lookup via open_files reverse index instead of O(n) document_map scan
        let open_doc = open_files.get(&dep_file_id).and_then(|uri_ref| {
            let uri = uri_ref.value().clone();
            document_map
                .get(&uri)
                .map(|doc| (uri, doc.value().text.clone()))
        });

        let is_open_in_editor = open_doc.is_some();
        let in_memory_text = open_doc.as_ref().map(|(_, text)| text.clone());

        let text: Arc<String> = match in_memory_text {
            Some(text) => {
                debug!(
                    "Re-indexing open dependent file: {} (in-memory)",
                    dep_path.display()
                );
                text
            }
            None => match hyperion_workspace::read_source_file(dep_path) {
                Ok(text) => {
                    debug!(
                        "Re-indexing closed dependent file: {} (from disk)",
                        dep_path.display()
                    );
                    Arc::new(text)
                }
                Err(e) => {
                    warn!(
                        "Failed to read dependent file {} from disk: {}",
                        dep_path.display(),
                        e
                    );
                    *complete = false;
                    continue;
                }
            },
        };

        let indexed = if is_open_in_editor {
            workspace_index.index_file_with_key_and_locks_held(
                dep_path,
                dep_path_key,
                &text,
                dep_file_id,
            )
        } else {
            workspace_index.index_disk_file_with_key_and_locks_held(
                dep_path,
                dep_path_key,
                &text,
                dep_file_id,
            )
        };
        let stats = match indexed {
            Ok(stats) => stats,
            Err(e) => {
                warn!(
                    "Failed to re-index dependent file {}: {}",
                    dep_path.display(),
                    e
                );
                *complete = false;
                continue;
            }
        };
        // Track open files for diagnostic republishing
        if let Some((uri, _)) = &open_doc {
            open_file_uris.push(uri.clone());
        }

        let language = Language::from_path(dep_path).unwrap_or(Language::Java);
        let text = if is_open_in_editor {
            DependentTextRevision::Editor(text)
        } else {
            DependentTextRevision::Disk(text)
        };
        let db_update = DependentDatabaseUpdate {
            file_id: dep_file_id,
            text,
            language,
        };

        // Publish Salsa while the path and file locks still protect the exact
        // workspace revision we just indexed. Delaying these writes until the
        // end of the whole BFS allowed a later delete or reindex to publish in
        // WorkspaceIndex first, only for this stale captured buffer to overwrite
        // its Salsa input afterward.
        //
        // Re-setting an unchanged input is intentional: it invalidates this
        // dependent's queries after the mutable workspace index changed without
        // bumping the workspace-wide Salsa epoch.
        publish_dependent_update(db_update, txn, &is_editor_owned);
        drop(per_file_guard);

        // Continue from the transaction's immutable old+new dependent set.
        // Looking up the file's declarations after publication loses callers
        // of declarations that disappeared while this dependent was re-read.
        for transitive_dep in stats.dependent_files {
            if !invalidated.contains(&transitive_dep) {
                to_invalidate.push_back(transitive_dep);
            }
        }
    }

    // Re-setting each dependent's text/language invalidates `file_semantics`
    // for that specific file via Salsa's input dependency tracking. We
    // deliberately do NOT bump `workspace_version` here: every
    // `file_semantics` body reads that input, so a bump would invalidate the
    // cached semantics for every file in the database. See
    // `file_semantics_invalidation_tests.rs`.

    let _ = changed_file_ids;
    debug!(
        "Background transitive invalidation complete: {} files re-indexed, {} open files need diagnostics refresh",
        invalidated.len(),
        open_file_uris.len(),
    );
}

fn publish_dependent_update(
    update: DependentDatabaseUpdate,
    txn: &mut WriteTxn,
    is_editor_owned: &impl Fn(FileId) -> bool,
) {
    let mut db_guard = txn.db();
    let text = match update.text {
        DependentTextRevision::Disk(text) => {
            // didOpen publishes the ownership marker before acquiring this
            // database lock. Either disk lands first and didOpen overwrites
            // it, or editor ownership is visible and stale disk text loses.
            if is_editor_owned(update.file_id) {
                debug!(
                    "Skipping disk Salsa update for {:?}: file opened during invalidation",
                    update.file_id
                );
                return;
            }
            text
        }
        DependentTextRevision::Editor(text) => {
            // didChange publishes a new Arc into Salsa for every revision.
            // Touch only the exact revision we indexed; a pointer mismatch
            // means a newer editor update already invalidated the queries.
            let Some(current) = db_guard.try_file_text(update.file_id) else {
                debug!(
                    "Skipping editor Salsa touch for {:?}: file is no longer registered",
                    update.file_id
                );
                return;
            };
            if !Arc::ptr_eq(&current, &text) {
                debug!(
                    "Skipping stale editor Salsa touch for {:?}: a newer revision won",
                    update.file_id
                );
                return;
            }
            text
        }
    };
    db_guard.set_file_text_tracked(update.file_id, text);
    db_guard.set_file_language(update.file_id, update.language);
}

#[cfg(test)]
mod open_document_restore_tests {
    use super::*;
    use crate::semantic_host::SemanticHost;

    #[test]
    fn captured_dependents_survive_removed_declarations_transitively() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let api_path = temp.path().join("api/Target.java");
        let middle_path = temp.path().join("app/Middle.java");
        let leaf_path = temp.path().join("leaf/Leaf.java");
        std::fs::create_dir_all(api_path.parent().expect("api parent")).expect("create api");
        std::fs::create_dir_all(middle_path.parent().expect("middle parent")).expect("create app");
        std::fs::create_dir_all(leaf_path.parent().expect("leaf parent")).expect("create leaf");

        let old_api = "package api; public class Target {}";
        let old_middle = "package app; import api.Target; public class Middle { Target value; }";
        let old_leaf = "package leaf; import app.Middle; public class Leaf { Middle value; }";
        std::fs::write(&api_path, old_api).expect("write api");
        std::fs::write(&middle_path, old_middle).expect("write middle");
        std::fs::write(&leaf_path, old_leaf).expect("write leaf");

        let index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        // Model IDs allocated by the workspace scanner. These paths are not in
        // the editor registry, but they must still participate in invalidation.
        let api_id = FileId::new(11);
        let middle_id = FileId::new(12);
        let leaf_id = FileId::new(13);
        index
            .index_file_with_source_and_id(&api_path, old_api, api_id)
            .expect("index api");
        index
            .index_file_with_source_and_id(&middle_path, old_middle, middle_id)
            .expect("index middle");
        index
            .index_file_with_source_and_id(&leaf_path, old_leaf, leaf_id)
            .expect("index leaf");

        let new_api = "package api; public class Renamed {}";
        let new_middle =
            "package app; import api.Renamed; public class RenamedMiddle { Renamed value; }";
        let new_leaf = "package leaf; import app.RenamedMiddle; public class ReindexedLeaf { RenamedMiddle value; }";
        std::fs::write(&api_path, new_api).expect("rewrite api");
        std::fs::write(&middle_path, new_middle).expect("rewrite middle");
        std::fs::write(&leaf_path, new_leaf).expect("rewrite leaf");

        let stats = index
            .index_file_with_source_and_id(&api_path, new_api, api_id)
            .expect("rename api declaration");
        assert_eq!(stats.dependent_files, vec![middle_id]);
        assert_eq!(
            index.get_dependent_files("api.Target"),
            vec![middle_id],
            "declaration removal must not delete the importer's still-live edge"
        );

        let document_map = DashMap::new();
        let open_files = Arc::new(DashMap::new());
        let host = SemanticHost::new(Arc::clone(&index));
        let refreshed_open_files = run_transitive_invalidation_for_dependents(
            api_id,
            stats.dependent_files,
            &mut host.write_now(),
            &document_map,
            &open_files,
        );

        assert!(refreshed_open_files.is_empty());
        assert!(index.find_symbol("app.Middle").is_none());
        assert!(index.find_symbol("app.RenamedMiddle").is_some());
        assert!(index.find_symbol("leaf.Leaf").is_none());
        assert!(index.find_symbol("leaf.ReindexedLeaf").is_some());
        assert!(
            index.get_dependent_files("api.Target").is_empty(),
            "reindexing the importer must retire its old edge"
        );
        assert_eq!(index.get_file_id(&middle_path), Some(middle_id));
        assert_eq!(index.get_file_id(&leaf_path), Some(leaf_id));
    }

    #[test]
    fn delayed_closed_file_update_cannot_overwrite_an_open_editor_buffer() {
        let file_id = FileId::new(91);
        let host = SemanticHost::for_tests();
        host.with_db(|db| {
            db.set_file_text_tracked(file_id, Arc::new("class EditorBuffer {}".to_owned()));
        });

        // didOpen establishes editor ownership before contending for db.lock().
        let editor_owned_files = dashmap::DashSet::new();
        editor_owned_files.insert(file_id);

        publish_dependent_update(
            DependentDatabaseUpdate {
                file_id,
                text: DependentTextRevision::Disk(Arc::new("class StaleDiskBuffer {}".to_owned())),
                language: Language::Java,
            },
            &mut host.write_now(),
            &|candidate| editor_owned_files.contains(&candidate),
        );

        assert_eq!(
            host.read_now().file_text(file_id).as_str(),
            "class EditorBuffer {}"
        );
    }

    #[test]
    fn delayed_editor_touch_cannot_overwrite_a_newer_revision() {
        let file_id = FileId::new(92);
        let captured = Arc::new("class CapturedRevision {}".to_owned());
        let host = SemanticHost::for_tests();
        host.with_db(|db| {
            db.set_file_text_tracked(file_id, Arc::new("class NewerRevision {}".to_owned()));
        });

        publish_dependent_update(
            DependentDatabaseUpdate {
                file_id,
                text: DependentTextRevision::Editor(captured),
                language: Language::Java,
            },
            &mut host.write_now(),
            &|_| true,
        );

        assert_eq!(
            host.read_now().file_text(file_id).as_str(),
            "class NewerRevision {}"
        );
    }

    #[test]
    fn dependent_salsa_publication_stays_inside_the_index_revision_locks() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let path = temp.path().join("Dependent.java");
        let previous_source = "class PreviousDependent {}";
        let source = "class PublishedDependent {}";
        std::fs::write(&path, previous_source).expect("write dependent");

        let index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        let dependent_id = FileId::new(21);
        let changed_id = FileId::new(22);
        index
            .index_file_with_source_and_id(&path, previous_source, dependent_id)
            .expect("index previous dependent revision");
        std::fs::write(&path, source).expect("rewrite dependent");
        let document_map = DashMap::new();
        let open_files = Arc::new(DashMap::new());
        let host = SemanticHost::new(Arc::clone(&index));
        let path_lock = index.get_path_lock(&path);
        let file_lock = index.get_file_lock(dependent_id);

        // The ownership probe runs inside the Salsa publication, after the
        // dependent's index revision is visible: both publication locks must
        // still be held there.
        let probed = std::cell::Cell::new(false);
        run_transitive_invalidation_batch(
            &[changed_id],
            vec![dependent_id],
            &mut host.write_now(),
            &document_map,
            &open_files,
            |_| false,
            |_| {
                assert!(
                    index.find_symbol("PublishedDependent").is_some(),
                    "the dependent's index revision must be published before its Salsa write"
                );
                assert!(
                    path_lock.try_lock().is_err(),
                    "path lock was released before Salsa publication"
                );
                assert!(
                    file_lock.try_lock().is_err(),
                    "file lock was released before Salsa publication"
                );
                probed.set(true);
                false
            },
        );

        assert!(probed.get(), "the Salsa publication must run");
        assert_eq!(host.read_now().file_text(dependent_id).as_str(), source);
    }
}
