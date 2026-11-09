// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Source publication transactions for workspace and build-system scans.
//!
//! A transaction indexes disk sources, restores authoritative editor buffers,
//! publishes the Salsa workspace epoch, and only then releases source readiness.
//! The conversion from the diagnostic invalidation runtime lives in
//! `server::diagnostics`, keeping that runtime outside this module.

use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::Language;
use tracing::{info, warn};

use crate::semantic_host::WriteTxn;

/// Re-apply editor buffers after a bulk disk scan.
///
/// Bulk indexing intentionally reads from disk, while an open document's
/// authoritative text lives in Salsa. The per-file lock makes the ordering
/// explicit: if `didChange` has already updated Salsa, this reads that version;
/// if it arrives later, its own index update runs after this one and wins.
pub(crate) fn restore_open_documents_after_bulk_index(
    root: &Path,
    txn: &mut WriteTxn,
    open_files: &Arc<DashMap<FileId, String>>,
    var_type_cache: &crate::source_publication::VarTypeCache,
) -> bool {
    let root_key = hyperion_workspace::path_lookup_key(root);
    restore_open_documents_after_bulk_index_matching(txn, open_files, var_type_cache, |path| {
        hyperion_workspace::path_lookup_key(path).starts_with(&root_key)
    })
}

fn restore_open_documents_after_bulk_index_matching(
    txn: &mut WriteTxn,
    open_files: &Arc<DashMap<FileId, String>>,
    var_type_cache: &crate::source_publication::VarTypeCache,
    should_restore: impl Fn(&Path) -> bool,
) -> bool {
    let workspace_index = Arc::clone(txn.index());
    let open_documents: Vec<_> = open_files
        .iter()
        .filter_map(|entry| {
            let uri = tower_lsp::lsp_types::Url::parse(entry.value()).ok()?;
            let path = uri.to_file_path().ok()?;
            should_restore(&path).then(|| (*entry.key(), entry.value().clone(), path))
        })
        .collect();

    let mut all_restored = true;
    for (file_id, expected_uri, path) in open_documents {
        let path_key = workspace_index
            .get_file_path_key(file_id)
            .unwrap_or_else(|| hyperion_workspace::path_lookup_key(&path));
        let path_lock = workspace_index.get_path_lock_for_key(&path_key);
        let _path_guard = path_lock.lock().unwrap_or_else(|poisoned| {
            warn!(
                "Path lock for open document {} was poisoned; restoring editor buffer anyway",
                path.display()
            );
            poisoned.into_inner()
        });
        let file_lock = workspace_index.get_file_lock(file_id);
        let _guard = file_lock.lock().unwrap_or_else(|poisoned| {
            warn!(
                "File lock for open document {:?} was poisoned; restoring editor buffer anyway",
                file_id
            );
            poisoned.into_inner()
        });

        let still_open = open_files
            .get(&file_id)
            .is_some_and(|uri| *uri == expected_uri);
        if !still_open {
            continue;
        }

        let source = txn.db().file_text(file_id);
        match workspace_index.index_file_with_key_and_locks_held(&path, &path_key, &source, file_id)
        {
            Ok(stats) => {
                let language = Language::from_path(&path).unwrap_or(Language::Java);
                if !crate::source_publication::publish_semantic_revision(
                    txn,
                    var_type_cache,
                    file_id,
                    Arc::clone(&source),
                    language,
                    &stats.retired_file_ids,
                ) {
                    all_restored = false;
                    warn!(
                        "Restored open document {} could not publish its exact semantic revision",
                        path.display()
                    );
                }
            }
            Err(err) => {
                all_restored = false;
                warn!(
                    "Failed to restore open document after bulk indexing {}: {}",
                    path.display(),
                    err
                );
            }
        }
    }
    all_restored
}

#[derive(Clone)]
pub(crate) struct BulkSourceIndexContext {
    pub(super) host: Arc<crate::semantic_host::SemanticHost>,
    pub(super) open_files: Arc<DashMap<FileId, String>>,
    pub(super) var_type_cache: Arc<crate::source_publication::VarTypeCache>,
    pub(super) destructive_refactoring: Arc<crate::server::DestructiveRefactoringReadiness>,
    pub(super) workspace_source_indexing: Option<Arc<WorkspaceSourceIndexCompletion>>,
    pub(super) build_progress: Option<crate::progress::ProgressReporter>,
}

impl BulkSourceIndexContext {
    pub(crate) fn with_build_progress(
        mut self,
        progress: crate::progress::ProgressReporter,
    ) -> Self {
        self.build_progress = Some(progress);
        self
    }

    pub(crate) const fn build_progress(&self) -> Option<&crate::progress::ProgressReporter> {
        self.build_progress.as_ref()
    }

    pub(crate) async fn report_build_progress(&self, message: impl Into<String>) {
        if let Some(progress) = &self.build_progress {
            progress.report(Some(message.into()), None).await;
        }
    }

    /// Order an overlapping build-system source pass after the initial root
    /// source transaction. Dependency discovery and archive indexing remain
    /// parallel; only the disk publications that share source paths wait.
    pub(crate) fn after_workspace_source_indexing(
        mut self,
        completion: Arc<WorkspaceSourceIndexCompletion>,
    ) -> Self {
        self.workspace_source_indexing = Some(completion);
        self
    }
}

#[derive(Clone, Copy)]
pub(super) enum BulkSourceScanPolicy {
    /// Respect the nested repository's own ignore rules.
    ScopedGitignore,
    /// Source roots declared by an open file or build model are authoritative,
    /// including generated trees which are normally ignored.
    IgnoreGitignore,
}

/// A reserved root-scoped source publication.
///
/// Reservation happens before an async delay or blocking-pool handoff. The
/// readiness guard is then moved into the blocking transaction, so aborting the
/// outer Tokio task cannot declare the workspace complete while detached work
/// is still mutating the index.
pub(super) struct BulkSourceIndexTransaction {
    pub(super) context: BulkSourceIndexContext,
    source_roots: Vec<std::path::PathBuf>,
    readiness: crate::server::state::SourceMutationReadinessGuard,
}

impl BulkSourceIndexTransaction {
    pub(super) fn reserve(
        context: BulkSourceIndexContext,
        mut source_roots: Vec<std::path::PathBuf>,
    ) -> Self {
        source_roots.sort_unstable();
        source_roots.dedup();
        let readiness = context
            .destructive_refactoring
            .begin_source_mutation(&source_roots);
        Self {
            context,
            source_roots,
            readiness,
        }
    }

    pub(super) fn run(
        self,
        txn: &mut WriteTxn,
        force_reindex: bool,
        scan_policy: BulkSourceScanPolicy,
    ) -> BulkSourceIndexOutcome {
        self.run_with_progress(txn, force_reindex, scan_policy, |_| {})
    }

    fn run_with_progress(
        self,
        txn: &mut WriteTxn,
        force_reindex: bool,
        scan_policy: BulkSourceScanPolicy,
        mut report: impl FnMut(String),
    ) -> BulkSourceIndexOutcome {
        let Self {
            context,
            source_roots,
            readiness,
        } = self;
        let workspace_index = Arc::clone(txn.index());
        let scan_started = std::time::Instant::now();
        let root_count = source_roots.len();
        report(format!("Scanning {root_count} build source roots..."));
        let mut files = Vec::new();
        let mut source_scan_failures = 0;
        for source_root in source_roots {
            let scan = match scan_policy {
                BulkSourceScanPolicy::ScopedGitignore => {
                    hyperion_workspace::scanner::WorkspaceScanner::scan_dir_scoped_gitignore(
                        &source_root,
                    )
                }
                BulkSourceScanPolicy::IgnoreGitignore => {
                    hyperion_workspace::scanner::WorkspaceScanner::scan_dir_ignore_gitignore(
                        &source_root,
                    )
                }
            };
            match scan {
                Ok(found) => files.extend(found),
                Err(error) => {
                    source_scan_failures += 1;
                    warn!(
                        "Failed to scan bulk source root {}: {}",
                        source_root.display(),
                        error
                    );
                }
            }
        }
        let file_sightings = files.len();
        files.sort_unstable();
        files.dedup();
        let indexed_paths: std::collections::HashSet<_> = files
            .iter()
            .map(|path| hyperion_workspace::path_lookup_key(path))
            .collect();
        info!(
            roots = root_count,
            file_sightings,
            unique_files = files.len(),
            failed_roots = source_scan_failures,
            elapsed_ms = scan_started.elapsed().as_millis(),
            "Build source scanning complete"
        );
        if let Some(completion) = &context.workspace_source_indexing {
            let mut cached = completion.take_deferred_build_sources();
            cached.retain(|path, _| {
                indexed_paths.contains(&hyperion_workspace::path_lookup_key(path))
            });
            if !force_reindex && !cached.is_empty() {
                let retained = cached.len();
                report(format!("Restoring {retained} cached build source files..."));
                let restore_started = std::time::Instant::now();
                let (rejected, _) =
                    workspace_index.restore_from_cache_batch_owned_with_revisions(cached);
                info!(
                    retained,
                    rejected = rejected.len(),
                    elapsed_ms = restore_started.elapsed().as_millis(),
                    "Validated deferred build-source cache entries"
                );
            }
        }
        report(format!(
            "Checking and indexing {} build source files...",
            files.len()
        ));
        let index_started = std::time::Instant::now();
        let stats = if force_reindex {
            workspace_index.index_files_parallel(&files)
        } else {
            workspace_index.index_files_parallel_skip_cached(&files)
        };
        let skipped = stats
            .files_scanned
            .saturating_sub(stats.files_indexed + stats.files_failed);
        info!(
            indexed = stats.files_indexed,
            skipped,
            failed = stats.files_failed,
            elapsed_ms = index_started.elapsed().as_millis(),
            "Build source indexing complete"
        );
        report(format!(
            "Restoring open buffers ({} indexed, {skipped} unchanged, {} failed)...",
            stats.files_indexed, stats.files_failed
        ));
        let restore_started = std::time::Instant::now();
        let open_documents_restored = restore_open_documents_after_bulk_index_matching(
            txn,
            &context.open_files,
            &context.var_type_cache,
            |path| indexed_paths.contains(&hyperion_workspace::path_lookup_key(path)),
        );

        // WorkspaceIndex is mutable external state from Salsa's point of view.
        // Publish its new epoch before the readiness guard may report success.
        txn.bump_workspace_epoch();
        info!(
            open_documents_restored,
            elapsed_ms = restore_started.elapsed().as_millis(),
            "Build source restoration and epoch publication complete"
        );
        let complete =
            source_scan_failures == 0 && stats.files_failed == 0 && open_documents_restored;
        readiness.finish(complete);

        BulkSourceIndexOutcome {
            stats,
            source_scan_failures,
            open_documents_restored,
        }
    }
}

/// One-shot completion for the initial workspace source transaction.
///
/// Unlike `workspace_ready`, this is completed on failure and cancellation as
/// well as success. Build-system source scans can therefore avoid racing cache
/// reconciliation without hanging forever after a failed root scan.
#[derive(Default)]
pub(crate) struct WorkspaceSourceIndexCompletion {
    finished: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
    deferred_build_sources: parking_lot::Mutex<
        std::collections::HashMap<std::path::PathBuf, hyperion_workspace::cache::CachedFile>,
    >,
}

impl WorkspaceSourceIndexCompletion {
    pub(crate) fn set_deferred_build_sources(
        &self,
        entries: std::collections::HashMap<
            std::path::PathBuf,
            hyperion_workspace::cache::CachedFile,
        >,
    ) {
        *self.deferred_build_sources.lock() = entries;
    }

    fn take_deferred_build_sources(
        &self,
    ) -> std::collections::HashMap<std::path::PathBuf, hyperion_workspace::cache::CachedFile> {
        std::mem::take(&mut *self.deferred_build_sources.lock())
    }

    pub(crate) fn finish(&self) {
        if !self
            .finished
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            self.notify.notify_waiters();
        }
    }

    async fn wait(&self) {
        loop {
            if self.finished.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            let notified = self.notify.notified();
            if self.finished.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

pub(crate) struct BulkSourceIndexOutcome {
    pub(crate) stats: hyperion_workspace::IndexStats,
    pub(crate) source_scan_failures: usize,
    pub(crate) open_documents_restored: bool,
}

/// Scan declared build-system source roots, index their disk revisions, and
/// restore any authoritative editor buffers displaced by the bulk pass. The
/// whole filesystem/parse transaction stays off Tokio workers.
pub(crate) async fn index_build_source_roots_off_runtime(
    context: BulkSourceIndexContext,
    source_roots: Vec<std::path::PathBuf>,
    force_reindex: bool,
) -> Result<BulkSourceIndexOutcome, tokio::task::JoinError> {
    let transaction = BulkSourceIndexTransaction::reserve(context, source_roots);
    if let Some(completion) = &transaction.context.workspace_source_indexing {
        transaction
            .context
            .report_build_progress("Waiting for workspace source indexing...")
            .await;
        completion.wait().await;
    }
    transaction
        .context
        .report_build_progress("Waiting to publish build source files...")
        .await;
    let txn = transaction.context.host.write().await;
    let progress = transaction.context.build_progress().cloned();
    // The worker emits up to four phase transitions. Keep delivery on the async
    // caller so cancellation cannot leave detached progress notifications
    // updating a superseding refresh while the disk transaction finishes.
    let (phase_sender, mut phase_receiver) = tokio::sync::mpsc::channel(4);
    let work = txn.run_blocking(move |txn| {
        transaction.run_with_progress(
            txn,
            force_reindex,
            BulkSourceScanPolicy::IgnoreGitignore,
            |message| {
                let _ = phase_sender.try_send(message);
            },
        )
    });
    while let Some(message) = phase_receiver.recv().await {
        if let Some(progress) = &progress {
            progress.report(Some(message), None).await;
        }
    }
    work.await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::nullability::{extract_method_signatures, WorkspaceMemberFactRevision};
    use hyperion_semantics::{ParseDatabase, SemanticDatabase, Type};
    use std::num::NonZeroUsize;

    #[test]
    fn bulk_source_transaction_fences_readiness_until_salsa_epoch_publication() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let source_root = temp.path().join("src/main/java");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::write(source_root.join("Bulk.java"), "class Bulk {}").unwrap();
        let index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        let host = SemanticHost::new(Arc::clone(&index));
        let readiness = Arc::new(crate::server::DestructiveRefactoringReadiness::new());
        for component in [
            crate::server::RefactoringIndexComponent::Sources,
            crate::server::RefactoringIndexComponent::Jdk,
            crate::server::RefactoringIndexComponent::Gradle,
            crate::server::RefactoringIndexComponent::Maven,
        ] {
            let ticket = readiness.begin(component);
            assert!(readiness.finish(ticket, true));
        }
        let context = BulkSourceIndexContext {
            host: Arc::clone(&host),
            open_files: Arc::new(DashMap::new()),
            var_type_cache: Arc::new(crate::source_publication::VarTypeCache::new(
                lru::LruCache::new(NonZeroUsize::new(8).unwrap()),
            )),
            destructive_refactoring: Arc::clone(&readiness),
            workspace_source_indexing: None,
            build_progress: None,
        };
        let before = host.read_now().workspace_version();
        let transaction = BulkSourceIndexTransaction::reserve(context, vec![source_root.clone()]);
        assert!(!readiness.is_ready());

        let outcome = transaction.run(
            &mut host.write_now(),
            false,
            BulkSourceScanPolicy::IgnoreGitignore,
        );

        assert_eq!(outcome.source_scan_failures, 0);
        assert_eq!(outcome.stats.files_failed, 0);
        assert!(host.read_now().workspace_version() > before);
        assert!(readiness.is_ready());
        assert!(index.find_symbol("Bulk").is_some());
    }

    #[test]
    fn build_source_cache_uses_only_current_declared_disk_revisions() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("module/build/generated");
        let old_root = temp.path().join("retired/build/generated");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&old_root).unwrap();
        let unchanged = source_root.join("Unchanged.java");
        let changed = source_root.join("Changed.java");
        let deleted = source_root.join("Deleted.java");
        let retired = old_root.join("Retired.java");
        let open = source_root.join("Open.java");
        for (path, source) in [
            (&unchanged, "class Unchanged {}"),
            (&changed, "class BeforeChange {}"),
            (&deleted, "class Deleted {}"),
            (&retired, "class Retired {}"),
            (&open, "class DiskVersion {}"),
        ] {
            std::fs::write(path, source).unwrap();
        }
        let prior = hyperion_workspace::WorkspaceIndex::new(temp.path().to_path_buf());
        for path in [&unchanged, &changed, &deleted, &retired, &open] {
            prior.index_file_sync_for_test(path).unwrap();
        }
        let cached = prior.extract_cache_data();
        std::fs::write(&changed, "class AfterChange {}").unwrap();
        std::fs::remove_file(&deleted).unwrap();

        let index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        let host = SemanticHost::new(Arc::clone(&index));
        let editor_text = Arc::new("class EditorVersion {}".to_owned());
        index.index_file_with_source(&open, &editor_text).unwrap();
        let open_id = index.get_file_id(&open).unwrap();
        host.with_db(|db| {
            db.register_file(open_id, Arc::clone(&editor_text), Language::Java);
        });
        let open_files = Arc::new(DashMap::new());
        open_files.insert(
            open_id,
            tower_lsp::lsp_types::Url::from_file_path(&open)
                .unwrap()
                .to_string(),
        );
        let completion = Arc::new(WorkspaceSourceIndexCompletion::default());
        completion.set_deferred_build_sources(cached);
        completion.finish();
        let readiness = Arc::new(crate::server::DestructiveRefactoringReadiness::new());
        let context = BulkSourceIndexContext {
            host: Arc::clone(&host),
            open_files,
            var_type_cache: Arc::new(crate::source_publication::VarTypeCache::new(
                lru::LruCache::new(NonZeroUsize::new(8).unwrap()),
            )),
            destructive_refactoring: readiness,
            workspace_source_indexing: Some(completion),
            build_progress: None,
        };

        let outcome = BulkSourceIndexTransaction::reserve(context, vec![source_root]).run(
            &mut host.write_now(),
            false,
            BulkSourceScanPolicy::IgnoreGitignore,
        );

        assert_eq!(outcome.stats.files_failed, 0);
        assert_eq!(outcome.stats.files_indexed, 2);
        assert!(index.get_symbol("Unchanged").is_some());
        assert!(index.get_symbol("AfterChange").is_some());
        assert!(index.get_symbol("EditorVersion").is_some());
        for absent in ["BeforeChange", "Deleted", "Retired", "DiskVersion"] {
            assert!(index.get_symbol(absent).is_none(), "{absent} survived");
        }
    }

    #[test]
    fn bulk_index_file_id_and_disk_text_cannot_displace_an_open_document() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let path = temp.path().join("Example.java");
        let index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        let open_file_id = FileId::new(41);
        let stale_bulk_id = FileId::new(7);
        let editor_source = Arc::new("class EditorVersion {}".to_owned());
        let stale_bulk_source = Arc::new(
            concat!(
                "package stale; ",
                "class DiskVersion { String ghost() { return \"old\"; } }"
            )
            .to_owned(),
        );

        let host = SemanticHost::new(Arc::clone(&index));
        let var_type_cache = crate::source_publication::VarTypeCache::new(lru::LruCache::new(
            NonZeroUsize::new(8).expect("non-zero cache capacity"),
        ));
        host.with_db(|db| {
            db.register_file(
                stale_bulk_id,
                Arc::clone(&stale_bulk_source),
                Language::Java,
            );
            db.set_file_text_tracked(open_file_id, Arc::clone(&editor_source));
        });
        {
            let txn = host.read_now();
            let parse_tree = txn.parse_file(stale_bulk_id);
            let signatures = extract_method_signatures(&parse_tree.syntax(), stale_bulk_id);
            txn.method_signature_cache()
                .update_file_signatures(stale_bulk_id, signatures);
        }
        assert!(host
            .member_fact_cache()
            .get_signature("stale.DiskVersion.ghost()")
            .is_some());
        var_type_cache.lock().put((stale_bulk_id, 3), Type::Unknown);

        let open_files = Arc::new(DashMap::new());
        let uri = tower_lsp::lsp_types::Url::from_file_path(&path)
            .expect("file URI")
            .to_string();
        open_files.insert(open_file_id, uri);

        // Reproduce the losing side of the startup race: the bulk scanner reserved
        // an ID before didOpen, then completed after the editor's authoritative index.
        index
            .index_file_with_source_and_id(&path, &stale_bulk_source, stale_bulk_id)
            .expect("simulate stale bulk index completion");
        assert_eq!(index.get_file_id(&path), Some(stale_bulk_id));

        restore_open_documents_after_bulk_index(
            temp.path(),
            &mut host.write_now(),
            &open_files,
            &var_type_cache,
        );

        assert_eq!(index.get_file_id(&path), Some(open_file_id));
        assert_eq!(
            index.get_file_path(open_file_id).as_deref(),
            Some(path.as_path())
        );
        assert!(index.get_file_path(stale_bulk_id).is_none());
        assert!(index.find_symbol("EditorVersion").is_some());
        assert!(index.find_symbol("DiskVersion").is_none());
        assert_eq!(
            host.read_now().file_text(open_file_id).as_str(),
            editor_source.as_str()
        );
        let file_revision = index
            .current_file_revision(open_file_id)
            .expect("restored file revision");
        let indexed = index
            .get_indexed_revision_content(file_revision)
            .expect("restored indexed source");
        let hierarchy = index
            .file_supertypes_for_revision(open_file_id, &indexed.content)
            .expect("restored exact hierarchy");
        let member_revision = WorkspaceMemberFactRevision::new(
            hierarchy.file_revision,
            hierarchy.workspace_revision,
            hierarchy.fingerprint,
        );
        assert!(
            host.member_fact_cache()
                .contains_workspace_member_fact_revision(open_file_id, member_revision),
            "bulk restore must republish member facts under the restored workspace token"
        );
        assert!(host.read_now().file_text(stale_bulk_id).is_empty());
        assert!(host
            .member_fact_cache()
            .get_signature("stale.DiskVersion.ghost()")
            .is_none());
        assert!(var_type_cache.lock().peek(&(stale_bulk_id, 3)).is_none());
    }
}
