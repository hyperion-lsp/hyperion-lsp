// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP lifecycle handlers: initialize, initialized, shutdown.
//!
//! This module contains the heavy logic for LSP lifecycle methods.
//! The `impl LanguageServer` trait methods in `server.rs` delegate here.

mod cache_reconciliation;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use parking_lot::Mutex;
use tower_lsp::lsp_types::MessageType;
use tower_lsp::Client;
use tracing::{debug, info, warn};

use dashmap::DashMap;

use crate::server::DocumentState;
use cache_reconciliation::reconcile_workspace_manifest_off_runtime;

struct RefactoringReadinessCompletion {
    readiness: Arc<crate::server::DestructiveRefactoringReadiness>,
    ticket: Option<crate::server::RefactoringReadinessTicket>,
}

impl RefactoringReadinessCompletion {
    const fn new(
        readiness: Arc<crate::server::DestructiveRefactoringReadiness>,
        ticket: crate::server::RefactoringReadinessTicket,
    ) -> Self {
        Self {
            readiness,
            ticket: Some(ticket),
        }
    }

    fn finish(&mut self, succeeded: bool) {
        if let Some(ticket) = self.ticket.take() {
            let _ = self.readiness.finish(ticket, succeeded);
        }
    }
}

impl Drop for RefactoringReadinessCompletion {
    fn drop(&mut self) {
        self.finish(false);
    }
}

struct WorkspaceSourceIndexCompletionGuard(
    Option<Arc<crate::server::source_indexing::WorkspaceSourceIndexCompletion>>,
);

impl WorkspaceSourceIndexCompletionGuard {
    const fn new(
        completion: Arc<crate::server::source_indexing::WorkspaceSourceIndexCompletion>,
    ) -> Self {
        Self(Some(completion))
    }

    fn set_deferred_build_sources(
        &self,
        entries: std::collections::HashMap<PathBuf, hyperion_workspace::cache::CachedFile>,
    ) {
        if let Some(completion) = &self.0 {
            completion.set_deferred_build_sources(entries);
        }
    }

    fn finish(&mut self) {
        if let Some(completion) = self.0.take() {
            completion.finish();
        }
    }
}

impl Drop for WorkspaceSourceIndexCompletionGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Run the `initialized` lifecycle phase: workspace indexing, JDK indexing,
/// Gradle/Maven integration, and file watcher setup.
///
/// This is extracted from `Backend::initialized()` to keep server.rs focused
/// on trait delegation.
pub(crate) async fn handle_initialized(
    workspace_root: Option<PathBuf>,
    client: Client,
    client_supports_progress: bool,
    workspace_ready: Arc<AtomicBool>,
    // File watcher context
    file_id_registry: Arc<crate::file_registry::FileIdRegistry>,
    document_map: Arc<DashMap<String, DocumentState>>,
    open_files: Arc<DashMap<FileId, String>>,
    var_type_cache: Arc<Mutex<lru::LruCache<(FileId, u32), hyperion_semantics::Type>>>,
    build_refresh: crate::handlers::build_system::BuildRefreshCoordinator,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
    invalidation_runtime: crate::server::diagnostics::InvalidationRuntime,
    file_events_sink: Arc<std::sync::OnceLock<Arc<crate::handlers::file_events::FileEventContext>>>,
) {
    info!("Hyperion LSP Server initialized and ready");

    client
        .log_message(
            MessageType::INFO,
            "Hyperion LSP Server is ready. Welcome to the future of JVM development!".to_string(),
        )
        .await;

    let index_publication = invalidation_runtime.clone();
    let destructive_readiness = Arc::clone(&index_publication.destructive_refactoring);

    let Some(root) = workspace_root else {
        client
            .log_message(
                MessageType::INFO,
                "No workspace root - files will be indexed as they are opened".to_string(),
            )
            .await;

        // There is no workspace build to wait for, so the workspace is ready
        // NOW. Leaving this false made didOpen's best-effort source-root
        // indexing defer forever ("until workspace is ready" — a state that
        // never arrived), so nothing was indexed until the first
        // hover/goto/references forced it with wait=true.
        workspace_ready.store(true, Ordering::SeqCst);
        let sources =
            destructive_readiness.begin(crate::server::RefactoringIndexComponent::Sources);
        let gradle = destructive_readiness.begin(crate::server::RefactoringIndexComponent::Gradle);
        let maven = destructive_readiness.begin(crate::server::RefactoringIndexComponent::Maven);
        let _ = destructive_readiness.finish(sources, false);
        let _ = destructive_readiness.finish(gradle, true);
        let _ = destructive_readiness.finish(maven, true);
        index_publication
            .publish_workspace_revision("empty workspace")
            .await;

        // JDK indexing never needed a workspace root; skipping it here left
        // no-root sessions without java.lang/JDK symbols entirely.
        let jdk = destructive_readiness.begin(crate::server::RefactoringIndexComponent::Jdk);
        spawn_jdk_indexing(
            index_publication,
            client_supports_progress,
            jdk,
            source_providers,
        );
        return;
    };

    client
        .log_message(
            MessageType::INFO,
            "Starting initial workspace indexing...".to_string(),
        )
        .await;

    // Kick off workspace and JDK indexing IN PARALLEL for 2-3x cold start improvement
    let workspace_source_indexing =
        Arc::new(crate::server::source_indexing::WorkspaceSourceIndexCompletion::default());
    let sources = destructive_readiness.begin(crate::server::RefactoringIndexComponent::Sources);
    spawn_workspace_indexing(
        index_publication.clone(),
        root.clone(),
        client_supports_progress,
        Arc::clone(&var_type_cache),
        Arc::clone(&workspace_source_indexing),
        sources,
    );

    let jdk = destructive_readiness.begin(crate::server::RefactoringIndexComponent::Jdk);
    spawn_jdk_indexing(
        index_publication.clone(),
        client_supports_progress,
        jdk,
        Arc::clone(&source_providers),
    );

    crate::handlers::build_system::spawn_initial_build_refresh(
        index_publication,
        crate::handlers::build_system::BuildProgressInputs {
            root: root.clone(),
            supports_progress: client_supports_progress,
            source_providers: Arc::clone(&source_providers),
            workspace_source_indexing,
        },
        build_refresh.clone(),
    );

    start_file_watcher(
        root.clone(),
        Arc::clone(&file_id_registry),
        document_map,
        client.clone(),
        Arc::clone(&open_files),
        var_type_cache,
        build_refresh,
        source_providers,
        invalidation_runtime,
        file_events_sink,
    );
}

/// Save usage statistics and clear document state on shutdown.
pub(crate) fn handle_shutdown(
    workspace_root: Option<PathBuf>,
    workspace_index: &WorkspaceIndex,
    document_map: &DashMap<String, DocumentState>,
) {
    if let Some(root) = workspace_root {
        use hyperion_workspace::IndexCache;
        let cache = IndexCache::new(&root);
        if let Err(e) = cache.save_index(workspace_index) {
            warn!("Failed to save workspace cache on shutdown: {}", e);
        } else {
            info!("Saved workspace cache on shutdown");
        }

        cache.wait_for_pending_saves();
    }

    document_map.clear();

    hyperion_build::gradle::sniff_cache::clear();
}

// ---------------------------------------------------------------------------
// Private spawn helpers
// ---------------------------------------------------------------------------

fn spawn_workspace_indexing(
    publication: crate::server::diagnostics::InvalidationRuntime,
    cache_root: PathBuf,
    supports_progress: bool,
    var_type_cache: Arc<crate::source_publication::VarTypeCache>,
    workspace_source_indexing: Arc<crate::server::source_indexing::WorkspaceSourceIndexCompletion>,
    readiness_ticket: crate::server::RefactoringReadinessTicket,
) {
    tokio::spawn(async move {
        use crate::progress::ProgressReporter;
        use hyperion_workspace::IndexCache;
        use std::time::Instant;

        let workspace_index = Arc::clone(publication.host.index());
        let mut readiness = RefactoringReadinessCompletion::new(
            Arc::clone(&publication.destructive_refactoring),
            readiness_ticket,
        );
        let mut source_index_completion =
            WorkspaceSourceIndexCompletionGuard::new(workspace_source_indexing);
        let workspace_ready = Arc::clone(&publication.workspace_ready);
        let open_files = Arc::clone(&publication.open_files);
        let client = publication.client.clone();

        let progress =
            ProgressReporter::new(client.clone(), "Indexing Workspace", supports_progress);
        progress
            .begin(Some("Loading cache...".to_string()), Some(0))
            .await;

        let start = Instant::now();
        let mut used_cache = false;

        match restore_workspace_cache_off_runtime(cache_root.clone(), Arc::clone(&publication.host))
            .await
        {
            Ok(CacheRestoreOutcome::Full {
                restored_revisions,
                deferred_build_sources,
            }) => {
                source_index_completion.set_deferred_build_sources(deferred_build_sources);
                // Full cache hit - restore all entries
                progress
                    .report(Some("Restoring from cache...".to_string()), Some(50))
                    .await;

                let manifest = match reconcile_workspace_manifest_off_runtime(
                    cache_root.clone(),
                    Arc::clone(&publication.host),
                    restored_revisions,
                )
                .await
                {
                    Ok(Ok(stats)) if stats.files_failed == 0 => stats,
                    Ok(Ok(stats)) => {
                        warn!(
                            "Workspace cache reconciliation failed to index {} source files",
                            stats.files_failed
                        );
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace reconciliation incomplete".to_string()))
                            .await;
                        return;
                    }
                    Ok(Err(error)) => {
                        warn!("Workspace cache reconciliation failed: {error}");
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace reconciliation failed".to_string()))
                            .await;
                        return;
                    }
                    Err(error) => {
                        warn!("Workspace cache reconciliation task failed: {error}");
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace reconciliation failed".to_string()))
                            .await;
                        return;
                    }
                };
                let restored = restore_open_documents_and_publish_epoch(
                    &publication,
                    &cache_root,
                    &open_files,
                    &var_type_cache,
                )
                .await;
                if !restored {
                    warn!("Workspace cache reconciliation could not restore every open buffer");
                    source_index_completion.finish();
                    progress
                        .end(Some("⚠ Open-buffer restoration failed".to_string()))
                        .await;
                    return;
                }
                used_cache = true;

                let stats = workspace_index.stats();

                workspace_ready.store(true, Ordering::SeqCst);
                publication
                    .publish_workspace_diagnostics("workspace cache")
                    .await;
                source_index_completion.finish();
                readiness.finish(true);
                debug!("Workspace ready (from cache)");

                let () = client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "✓ Restored from cache: {} files, {} symbols ({} manifest files checked)",
                            stats.files_indexed, stats.symbols_indexed, manifest.files_scanned
                        ),
                    )
                    .await;
                progress
                    .end(Some(format!("✓ {} files indexed", stats.files_indexed)))
                    .await;
            }
            Ok(CacheRestoreOutcome::Partial {
                stale_files,
                restored_revisions,
                deferred_build_sources,
            }) => {
                source_index_completion.set_deferred_build_sources(deferred_build_sources);
                // Partial cache hit
                info!("Partial cache hit: {} stale files", stale_files.len());
                progress
                    .report(Some("Restoring from cache...".to_string()), Some(25))
                    .await;

                progress
                    .report(
                        Some(format!("Reindexing {} stale files...", stale_files.len())),
                        Some(50),
                    )
                    .await;
                let stale_reindexed = stale_files.iter().filter(|path| path.exists()).count();
                let manifest = match reconcile_workspace_manifest_off_runtime(
                    cache_root.clone(),
                    Arc::clone(&publication.host),
                    restored_revisions,
                )
                .await
                {
                    Ok(Ok(stats)) if stats.files_failed == 0 => stats,
                    Ok(Ok(stats)) => {
                        warn!(
                            "Partial cache reconciliation failed to index {} source files",
                            stats.files_failed
                        );
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace reconciliation incomplete".to_string()))
                            .await;
                        return;
                    }
                    Ok(Err(error)) => {
                        warn!("Partial cache reconciliation failed: {error}");
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace reconciliation failed".to_string()))
                            .await;
                        return;
                    }
                    Err(error) => {
                        warn!("Partial cache reconciliation task failed: {error}");
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace reconciliation failed".to_string()))
                            .await;
                        return;
                    }
                };
                let restored = restore_open_documents_and_publish_epoch(
                    &publication,
                    &cache_root,
                    &open_files,
                    &var_type_cache,
                )
                .await;
                if !restored {
                    warn!("Partial cache reconciliation could not restore every open buffer");
                    source_index_completion.finish();
                    progress
                        .end(Some("⚠ Open-buffer restoration failed".to_string()))
                        .await;
                    return;
                }
                used_cache = true;

                let stats = workspace_index.stats();

                workspace_ready.store(true, Ordering::SeqCst);
                publication
                    .publish_workspace_diagnostics("partial workspace cache")
                    .await;
                source_index_completion.finish();
                readiness.finish(true);
                debug!("Workspace ready (from partial cache)");

                let () = client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "✓ Partial cache restore: {} files, {} symbols ({} stale, {} manifest files checked)",
                            stats.files_indexed,
                            stats.symbols_indexed,
                            stale_reindexed,
                            manifest.files_scanned
                        ),
                    )
                    .await;
                progress
                    .end(Some(format!("✓ {} files indexed", stats.files_indexed)))
                    .await;
            }
            Ok(CacheRestoreOutcome::Miss) => {}
            Err(error) => {
                warn!("Workspace cache restore task failed: {error}");
                source_index_completion.finish();
                let () = client
                    .log_message(
                        MessageType::WARNING,
                        format!("⚠ Workspace cache restore task failed: {error}"),
                    )
                    .await;
                progress
                    .end(Some("⚠ Cache restore failed".to_string()))
                    .await;
                return;
            }
        }

        // If cache wasn't used (miss or error), do full build
        if !used_cache {
            debug!("Cache miss, starting full build");
            progress
                .report(Some("Scanning workspace...".to_string()), Some(10))
                .await;
            let build_result =
                build_workspace_index_off_runtime(Arc::clone(&publication.host)).await;

            match build_result {
                Ok(Ok(stats)) => {
                    if stats.files_failed != 0 {
                        warn!(
                            "Workspace indexing left {} source files unindexed",
                            stats.files_failed
                        );
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Workspace indexing incomplete".to_string()))
                            .await;
                        return;
                    }
                    let restored = restore_open_documents_and_publish_epoch(
                        &publication,
                        &cache_root,
                        &open_files,
                        &var_type_cache,
                    )
                    .await;
                    if !restored {
                        warn!("Workspace indexing could not restore every open buffer");
                        source_index_completion.finish();
                        progress
                            .end(Some("⚠ Open-buffer restoration failed".to_string()))
                            .await;
                        return;
                    }
                    info!(
                        "Workspace indexing complete: {} files, {} symbols",
                        stats.files_indexed, stats.symbols_indexed
                    );

                    workspace_ready.store(true, Ordering::SeqCst);
                    publication
                        .publish_workspace_diagnostics("workspace build")
                        .await;
                    IndexCache::new(&cache_root).save_index_async(Arc::clone(&workspace_index));
                    info!("Index cache save started in background");
                    source_index_completion.finish();
                    readiness.finish(true);
                    debug!("Workspace ready (from full build)");

                    let () = client
                        .log_message(
                            MessageType::INFO,
                            format!(
                                "✓ Workspace indexed: {} files, {} symbols",
                                stats.files_indexed, stats.symbols_indexed
                            ),
                        )
                        .await;

                    progress
                        .end(Some(format!("✓ {} files indexed", stats.files_indexed)))
                        .await;
                }
                Ok(Err(e)) => {
                    warn!("Workspace indexing failed: {}", e);
                    source_index_completion.finish();
                    let () = client
                        .log_message(
                            MessageType::WARNING,
                            format!("⚠ Failed to index workspace: {e}"),
                        )
                        .await;
                    progress.end(Some("⚠ Indexing failed".to_string())).await;
                    return;
                }
                Err(e) => {
                    warn!("Workspace indexing task failed: {}", e);
                    source_index_completion.finish();
                    let () = client
                        .log_message(
                            MessageType::WARNING,
                            format!("⚠ Workspace indexing task failed: {e}"),
                        )
                        .await;
                    progress.end(Some("⚠ Indexing failed".to_string())).await;
                    return;
                }
            }
        }

        let elapsed = start.elapsed();
        info!(
            "Workspace indexing completed in {:.2}s",
            elapsed.as_secs_f64()
        );
    });
}

/// Re-apply editor buffers displaced by startup bulk indexing and publish the
/// Salsa workspace epoch in the same write transaction, so no request can
/// observe the restored index with the previous epoch. Returns whether every
/// open buffer was restored (the epoch is only published then).
async fn restore_open_documents_and_publish_epoch(
    publication: &crate::server::diagnostics::InvalidationRuntime,
    cache_root: &Path,
    open_files: &Arc<DashMap<FileId, String>>,
    var_type_cache: &Arc<crate::source_publication::VarTypeCache>,
) -> bool {
    let cache_root = cache_root.to_path_buf();
    let open_files = Arc::clone(open_files);
    let var_type_cache = Arc::clone(var_type_cache);
    publication
        .host
        .write()
        .await
        .run_blocking(move |txn| {
            let restored = crate::server::source_indexing::restore_open_documents_after_bulk_index(
                &cache_root,
                txn,
                &open_files,
                &var_type_cache,
            );
            if restored {
                txn.bump_workspace_epoch();
            }
            restored
        })
        .await
        .unwrap_or(false)
}

/// Keep the complete scan/reserve/parse/index transaction off Tokio workers.
/// `WorkspaceIndex::build` is deliberately synchronous so it cannot give async
/// callers a false impression that its CPU and filesystem work yields.
async fn build_workspace_index_off_runtime(
    host: Arc<crate::semantic_host::SemanticHost>,
) -> Result<anyhow::Result<hyperion_workspace::IndexStats>, tokio::task::JoinError> {
    // Scanning and parsing dominate and run outside the gate; every bounded
    // chunk of index mutation is its own write transaction so requests and
    // editor notifications interleave with a cold build.
    tokio::task::spawn_blocking(move || {
        let index = Arc::clone(host.index());
        index.build_scoped(|chunk| host.publish_blocking(|_txn| chunk()))
    })
    .await
}

enum CacheRestoreOutcome {
    Miss,
    Full {
        restored_revisions: Vec<hyperion_workspace::RestoredFileRevision>,
        deferred_build_sources:
            std::collections::HashMap<PathBuf, hyperion_workspace::cache::CachedFile>,
    },
    Partial {
        stale_files: Vec<PathBuf>,
        restored_revisions: Vec<hyperion_workspace::RestoredFileRevision>,
        deferred_build_sources:
            std::collections::HashMap<PathBuf, hyperion_workspace::cache::CachedFile>,
    },
}

impl std::fmt::Debug for CacheRestoreOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Miss => formatter.write_str("Miss"),
            Self::Full {
                restored_revisions,
                deferred_build_sources,
            } => formatter
                .debug_struct("Full")
                .field("restored_revisions", &restored_revisions.len())
                .field("deferred_build_sources", &deferred_build_sources.len())
                .finish(),
            Self::Partial {
                stale_files,
                restored_revisions,
                deferred_build_sources,
            } => formatter
                .debug_struct("Partial")
                .field("stale_files", stale_files)
                .field("restored_revisions", &restored_revisions.len())
                .field("deferred_build_sources", &deferred_build_sources.len())
                .finish(),
        }
    }
}

/// Load, validate, filter, and restore one cache snapshot away from Tokio's
/// async workers. The returned outcome contains only the information needed for
/// async progress reporting and any subsequent stale-file reindex.
async fn restore_workspace_cache_off_runtime(
    cache_root: PathBuf,
    host: Arc<crate::semantic_host::SemanticHost>,
) -> Result<CacheRestoreOutcome, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        use hyperion_workspace::IndexCache;
        let workspace_index = Arc::clone(host.index());
        // Cache validation reads and hashes every cached file outside the gate;
        // only the short install step is a write transaction.
        let restore = |entries| {
            workspace_index.restore_from_cache_batch_owned_with_revisions_scoped(
                entries,
                |install| {
                    host.publish_blocking(|_txn| install());
                },
            )
        };

        let cache = IndexCache::new(&cache_root);
        let Some(cached_index) = cache.load() else {
            return CacheRestoreOutcome::Miss;
        };

        let cached_entry_count = cached_index.entries().len();
        debug!("Cache loaded: {} entries", cached_entry_count);
        // Only files visible to the default scan belong to the root source
        // transaction. Build-declared roots can be ignored or external, so
        // leave their cached declarations unpublished until the build scan
        // proves they still belong to the workspace.
        let root_manifest =
            match hyperion_workspace::scanner::WorkspaceScanner::scan_dir(&cache_root) {
                Ok(files) => files
                    .iter()
                    .map(|path| hyperion_workspace::path_lookup_key(path))
                    .collect::<std::collections::HashSet<_>>(),
                Err(error) => {
                    warn!("Cannot classify cached workspace sources: {error}");
                    return CacheRestoreOutcome::Miss;
                }
            };
        let mut stale_files = cache.check_stale_files(&cached_index);
        let (cached_entries, usage_stats) = cached_index.into_parts();
        let (mut deferred_build_sources, mut cached_entries): (std::collections::HashMap<_, _>, _) =
            cached_entries.into_iter().partition(|(path, _)| {
                !root_manifest.contains(&hyperion_workspace::path_lookup_key(path))
            });
        let stale_set: std::collections::HashSet<_> = stale_files.iter().cloned().collect();
        deferred_build_sources.retain(|path, _| !stale_set.contains(path));
        if stale_files.is_empty() {
            info!(
                "Cache hit: restoring {} files from cache",
                cached_entry_count
            );
            let (rejected, restored_revisions) = restore(cached_entries);
            stale_files = rejected;
            host.publish_blocking(|txn| txn.index().load_usage_stats(usage_stats));
            if stale_files.is_empty() {
                return CacheRestoreOutcome::Full {
                    restored_revisions,
                    deferred_build_sources,
                };
            }
            return CacheRestoreOutcome::Partial {
                stale_files,
                restored_revisions,
                deferred_build_sources,
            };
        }

        cached_entries.retain(|path, _| !stale_set.contains(path));
        let (rejected, restored_revisions) = restore(cached_entries);
        stale_files.extend(rejected);
        stale_files.sort_unstable();
        stale_files.dedup();
        // Usage statistics are workspace-level ranking history rather than
        // per-file index records. Preserve them on partial restores just as on
        // full restores; stale files are reindexed below, but their presence
        // must not erase usage data for every valid symbol.
        host.publish_blocking(|txn| txn.index().load_usage_stats(usage_stats));
        CacheRestoreOutcome::Partial {
            stale_files,
            restored_revisions,
            deferred_build_sources,
        }
    })
    .await
}

fn spawn_jdk_indexing(
    publication: crate::server::diagnostics::InvalidationRuntime,
    supports_progress: bool,
    readiness_ticket: crate::server::RefactoringReadinessTicket,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
) {
    tokio::spawn(async move {
        use crate::progress::ProgressReporter;
        use std::time::Instant;

        let _workspace_index = Arc::clone(publication.host.index());
        let mut readiness = RefactoringReadinessCompletion::new(
            Arc::clone(&publication.destructive_refactoring),
            readiness_ticket,
        );
        let client = publication.client.clone();

        let progress = ProgressReporter::new(client.clone(), "Indexing JDK", supports_progress);
        progress
            .begin(Some("Loading standard library...".to_string()), None)
            .await;

        let start = Instant::now();
        let () = client
            .log_message(
                MessageType::INFO,
                "Indexing JDK stdlib (java.lang, java.util, etc.) - parallel with workspace..."
                    .to_string(),
            )
            .await;

        match index_jdk_off_runtime(Arc::clone(&publication.host), source_providers).await {
            Ok(Ok(outcome)) => {
                let published_symbols = jdk_outcome_published_symbols(&outcome);
                let presentation =
                    jdk_outcome_presentation(&outcome, start.elapsed().as_secs_f64());
                if presentation.success {
                    info!("{}", presentation.message);
                } else {
                    warn!("{}", presentation.message);
                }
                let message_type = if presentation.success {
                    MessageType::INFO
                } else {
                    MessageType::WARNING
                };
                let () = client.log_message(message_type, presentation.message).await;
                progress.end(Some(presentation.progress.to_string())).await;
                if published_symbols {
                    // The JDK publication bumped the semantic epoch in its own
                    // final write transaction; only diagnostics remain.
                    publication.publish_workspace_diagnostics("JDK").await;
                }
                if matches!(outcome, crate::jdk_indexer::JdkIndexOutcome::Indexed) {
                    readiness.finish(true);
                }
            }
            Ok(Err(e)) => {
                warn!("JDK stdlib indexing failed: {}", e);
                let () = client
                    .log_message(
                        MessageType::WARNING,
                        format!("⚠ JDK indexing failed: {e} - stdlib navigation may not work"),
                    )
                    .await;
                progress
                    .end(Some("⚠ JDK indexing failed".to_string()))
                    .await;
            }
            Err(e) => {
                warn!("JDK stdlib indexing task failed: {}", e);
                let () = client
                    .log_message(
                        MessageType::WARNING,
                        format!("⚠ JDK indexing task failed: {e} - stdlib navigation may not work"),
                    )
                    .await;
                progress
                    .end(Some("⚠ JDK indexing failed".to_string()))
                    .await;
            }
        }
    });
}

const fn jdk_outcome_published_symbols(outcome: &crate::jdk_indexer::JdkIndexOutcome) -> bool {
    match outcome {
        crate::jdk_indexer::JdkIndexOutcome::Indexed => true,
        crate::jdk_indexer::JdkIndexOutcome::Partial {
            indexed_archives, ..
        } => *indexed_archives > 0,
        crate::jdk_indexer::JdkIndexOutcome::Unavailable { .. } => false,
    }
}

struct JdkOutcomePresentation {
    success: bool,
    message: String,
    progress: &'static str,
}

fn jdk_outcome_presentation(
    outcome: &crate::jdk_indexer::JdkIndexOutcome,
    elapsed_seconds: f64,
) -> JdkOutcomePresentation {
    match outcome {
        crate::jdk_indexer::JdkIndexOutcome::Indexed => JdkOutcomePresentation {
            success: true,
            message: format!("✓ JDK stdlib indexed in {elapsed_seconds:.2}s"),
            progress: "✓ JDK indexed",
        },
        crate::jdk_indexer::JdkIndexOutcome::Unavailable { reason } => JdkOutcomePresentation {
            success: false,
            message: format!("⚠ JDK unavailable: {reason} - stdlib navigation may not work"),
            progress: "⚠ JDK unavailable",
        },
        crate::jdk_indexer::JdkIndexOutcome::Partial {
            indexed_archives,
            failed_archives,
        } => JdkOutcomePresentation {
            success: false,
            message: format!(
                "⚠ JDK partially indexed: {indexed_archives} archives indexed, {failed_archives} failed - some stdlib navigation may not work"
            ),
            progress: "⚠ JDK partially indexed",
        },
    }
}

/// Keep JDK detection, cache I/O, parsing, indexing, and cache writes off
/// Tokio workers. The synchronous core uses Rayon internally for JAR/JMOD
/// parsing; this blocking worker coordinates that work without occupying an
/// async runtime worker.
async fn index_jdk_off_runtime(
    host: Arc<crate::semantic_host::SemanticHost>,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
) -> Result<anyhow::Result<crate::jdk_indexer::JdkIndexOutcome>, tokio::task::JoinError> {
    run_blocking(move || crate::jdk_indexer::index_jdk_stdlib_with_sources(host, source_providers))
        .await
}

async fn run_blocking<F, T>(task: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task).await
}

#[allow(clippy::too_many_arguments)]
fn start_file_watcher(
    root: PathBuf,
    file_id_registry: Arc<crate::file_registry::FileIdRegistry>,
    document_map: Arc<DashMap<String, DocumentState>>,
    client: Client,
    open_files: Arc<DashMap<FileId, String>>,
    var_type_cache: Arc<Mutex<lru::LruCache<(FileId, u32), hyperion_semantics::Type>>>,
    build_refresh: crate::handlers::build_system::BuildRefreshCoordinator,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
    invalidation_runtime: crate::server::diagnostics::InvalidationRuntime,
    file_events_sink: Arc<std::sync::OnceLock<Arc<crate::handlers::file_events::FileEventContext>>>,
) {
    // One reconciler serves both the OS watcher below and client-sent file
    // notifications (`workspace/didChangeWatchedFiles`, `didRenameFiles`, ...).
    // The OS watcher owns change detection because it works for every client
    // and sees writes from any tool; client notifications are a low-latency
    // fast path (and the only source when the OS watcher cannot start). The
    // reconciler is idempotent: an unchanged disk revision is a no-op, so the
    // two sources never double-publish.
    let ctx = Arc::new(crate::handlers::file_events::FileEventContext {
        host: Arc::clone(&invalidation_runtime.host),
        file_id_registry,
        document_map,
        open_files,
        var_type_cache,
        build_refresh,
        workspace_root: root.clone(),
        client,
        source_providers,
        invalidation_runtime,
    });
    let _ = file_events_sink.set(Arc::clone(&ctx));

    match crate::file_watcher::FileWatcher::new_with_readiness(
        root.clone(),
        Arc::clone(&ctx.invalidation_runtime.destructive_refactoring),
    ) {
        Ok(mut watcher) => {
            info!("File system watcher started for: {}", root.display());
            tokio::spawn(async move {
                while let Some(event) = watcher.next_event().await {
                    crate::handlers::file_events::handle_file_event(&ctx, event).await;
                }
                tracing::error!(
                    "File watcher channel closed unexpectedly - external changes will no longer be detected"
                );
                ctx.invalidation_runtime
                    .destructive_refactoring
                    .fail_sources_permanently();
                let () = ctx
                    .client
                    .show_message(
                        MessageType::WARNING,
                        "File watcher stopped unexpectedly. External changes (git, other editors) may not be detected. Consider restarting the LSP server.".to_string(),
                    )
                    .await;
            });
        }
        Err(e) => {
            ctx.invalidation_runtime
                .destructive_refactoring
                .fail_sources_permanently();
            warn!("Failed to start file system watcher: {}", e);
            tokio::spawn(async move {
                let () = ctx
                    .client
                    .log_message(
                        MessageType::WARNING,
                        format!("Could not start file watcher: {e} - external changes may not be detected"),
                    )
                    .await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_workspace_index_off_runtime, handle_shutdown, jdk_outcome_presentation,
        jdk_outcome_published_symbols, reconcile_workspace_manifest_off_runtime,
        restore_workspace_cache_off_runtime, run_blocking, CacheRestoreOutcome,
    };
    use crate::semantic_host::SemanticHost;
    use hyperion_workspace::{IndexCache, WorkspaceIndex};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    struct RuntimeHeartbeat {
        finished: Arc<AtomicBool>,
        ticks: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl RuntimeHeartbeat {
        async fn start() -> Self {
            let finished = Arc::new(AtomicBool::new(false));
            let ticks = Arc::new(AtomicUsize::new(0));
            let task_finished = Arc::clone(&finished);
            let task_ticks = Arc::clone(&ticks);
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                task_ticks.fetch_add(1, Ordering::Relaxed);
                let _ = started_tx.send(());
                while !task_finished.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                    if !task_finished.load(Ordering::Acquire) {
                        task_ticks.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
            started_rx
                .await
                .expect("heartbeat task stopped before starting");
            Self {
                finished,
                ticks,
                task,
            }
        }

        async fn finish(self) -> usize {
            self.finished.store(true, Ordering::Release);
            self.task.await.expect("heartbeat task panicked");
            self.ticks.load(Ordering::Relaxed)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn workspace_build_keeps_tokio_runtime_responsive() {
        let workspace = tempfile::tempdir().unwrap();
        for file_number in 0..200 {
            let methods = (0..80)
                .map(|method_number| {
                    format!("int method{method_number}() {{ return {method_number}; }}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(
                workspace.path().join(format!("Class{file_number}.java")),
                format!("class Class{file_number} {{ {methods} }}"),
            )
            .unwrap();
        }

        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));
        let heartbeat = RuntimeHeartbeat::start().await;

        let stats = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            build_workspace_index_off_runtime(SemanticHost::new(index)),
        )
        .await
        .expect("workspace build timed out")
        .expect("blocking build task panicked")
        .expect("workspace build failed");
        let heartbeats = heartbeat.finish().await;

        assert_eq!(stats.files_indexed, 200);
        assert!(
            heartbeats > 1,
            "Tokio heartbeat did not run while workspace indexing was active"
        );
    }

    fn prepare_workspace_cache() -> (tempfile::TempDir, Vec<PathBuf>) {
        let workspace = tempfile::tempdir().unwrap();
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        let mut paths = Vec::new();
        for file_number in 0..100 {
            let path = workspace.path().join(format!("Cached{file_number}.java"));
            let methods = (0..20)
                .map(|method_number| {
                    format!("int method{method_number}() {{ return {method_number}; }}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let source = format!("class Cached{file_number} {{ {methods} }}");
            std::fs::write(&path, &source).unwrap();
            source_index.index_file_with_source(&path, &source).unwrap();
            paths.push(path);
        }
        source_index.record_symbol_usage("Cached0.method0");
        source_index.record_symbol_usage("Cached1.method0");
        IndexCache::new(workspace.path())
            .save(
                source_index.extract_cache_data(),
                source_index.extract_usage_stats(),
            )
            .unwrap();
        (workspace, paths)
    }

    async fn restore_with_heartbeat(
        cache_root: PathBuf,
        index: Arc<WorkspaceIndex>,
    ) -> (CacheRestoreOutcome, usize) {
        let heartbeat = RuntimeHeartbeat::start().await;

        let outcome = restore_workspace_cache_off_runtime(cache_root, SemanticHost::new(index))
            .await
            .unwrap();
        (outcome, heartbeat.finish().await)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn full_cache_restore_keeps_tokio_runtime_responsive() {
        let (workspace, _) = prepare_workspace_cache();
        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));

        let (outcome, heartbeats) =
            restore_with_heartbeat(workspace.path().to_path_buf(), Arc::clone(&index)).await;

        assert!(matches!(outcome, CacheRestoreOutcome::Full { .. }));
        assert!(heartbeats > 1, "runtime did not tick during cache restore");
        assert!(index.get_symbol("Cached0.method0").is_some());
        assert!(index.get_usage_score("Cached0.method0") > 0.0);
    }

    #[tokio::test]
    async fn generated_sources_wait_for_the_declared_build_source_manifest() {
        let workspace = tempfile::tempdir().unwrap();
        let generated = workspace.path().join("module/build/generated/Example.java");
        std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
        std::fs::write(&generated, "class GeneratedBeforeRestart {}").unwrap();
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        source_index.index_file_sync_for_test(&generated).unwrap();
        IndexCache::new(workspace.path())
            .save(source_index.extract_cache_data(), HashMap::default())
            .unwrap();

        let restored = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));
        let outcome = restore_workspace_cache_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&restored)),
        )
        .await
        .unwrap();
        let CacheRestoreOutcome::Full {
            restored_revisions,
            deferred_build_sources,
        } = outcome
        else {
            panic!("unchanged generated source should be deferred");
        };
        assert!(restored.get_symbol("GeneratedBeforeRestart").is_none());
        assert!(restored_revisions.is_empty());
        assert_eq!(deferred_build_sources.len(), 1);

        let (rejected, _) =
            restored.restore_from_cache_batch_owned_with_revisions(deferred_build_sources);
        assert!(rejected.is_empty());
        let stats = restored.index_files_parallel_skip_cached(&[generated]);
        assert_eq!(stats.files_indexed, 0);
        assert!(restored.get_symbol("GeneratedBeforeRestart").is_some());
    }

    #[tokio::test]
    async fn changed_generated_source_is_reindexed_from_disk() {
        let workspace = tempfile::tempdir().unwrap();
        let generated = workspace.path().join("module/build/generated/Example.java");
        std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
        std::fs::write(&generated, "class OldGenerated {}").unwrap();
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        source_index.index_file_sync_for_test(&generated).unwrap();
        IndexCache::new(workspace.path())
            .save(source_index.extract_cache_data(), HashMap::default())
            .unwrap();
        std::fs::write(&generated, "class NewGenerated {}").unwrap();

        let restored = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));
        let outcome = restore_workspace_cache_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&restored)),
        )
        .await
        .unwrap();
        let CacheRestoreOutcome::Partial {
            deferred_build_sources,
            stale_files,
            ..
        } = outcome
        else {
            panic!("changed generated source should invalidate its cache entry");
        };
        assert!(deferred_build_sources.is_empty());
        assert_eq!(stale_files, vec![generated.clone()]);
        assert!(restored.get_symbol("OldGenerated").is_none());

        let stats = restored.index_files_parallel_skip_cached(&[generated]);
        assert_eq!(stats.files_indexed, 1);
        assert!(restored.get_symbol("NewGenerated").is_some());
    }

    #[tokio::test]
    async fn deleted_generated_source_does_not_return_from_cache() {
        let workspace = tempfile::tempdir().unwrap();
        let generated = workspace.path().join("build/generated/Example.java");
        std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
        std::fs::write(&generated, "class RemovedGenerated {}").unwrap();
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        source_index.index_file_sync_for_test(&generated).unwrap();
        IndexCache::new(workspace.path())
            .save(source_index.extract_cache_data(), HashMap::default())
            .unwrap();
        std::fs::remove_file(&generated).unwrap();

        let restored = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));
        let outcome = restore_workspace_cache_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&restored)),
        )
        .await
        .unwrap();
        let CacheRestoreOutcome::Partial {
            deferred_build_sources,
            stale_files,
            ..
        } = outcome
        else {
            panic!("deleted generated source should invalidate its cache entry");
        };
        assert!(deferred_build_sources.is_empty());
        assert_eq!(stale_files, vec![generated]);
        assert!(restored.get_symbol("RemovedGenerated").is_none());
    }

    #[test]
    fn shutdown_saves_generated_sources_without_usage_statistics() {
        let workspace = tempfile::tempdir().unwrap();
        let generated = workspace.path().join("build/generated/Example.java");
        std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
        std::fs::write(&generated, "class GeneratedAtShutdown {}").unwrap();
        let index = WorkspaceIndex::new(workspace.path().to_path_buf());
        index.index_file_sync_for_test(&generated).unwrap();
        assert!(index.extract_usage_stats().is_empty());

        handle_shutdown(
            Some(workspace.path().to_path_buf()),
            &index,
            &dashmap::DashMap::new(),
        );

        let cached = IndexCache::new(workspace.path()).load().unwrap();
        assert!(cached.entries().contains_key(&generated));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cache_reconciliation_discovers_sources_created_while_server_was_stopped() {
        let (workspace, _) = prepare_workspace_cache();
        let new_source = workspace.path().join("CreatedAfterCache.java");
        std::fs::write(&new_source, "class CreatedAfterCache {}").unwrap();
        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));

        let outcome = restore_workspace_cache_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&index)),
        )
        .await
        .unwrap();
        let restored_revisions = match outcome {
            CacheRestoreOutcome::Full {
                restored_revisions, ..
            } => restored_revisions,
            other => panic!("expected full cache restore, got {other:?}"),
        };
        assert!(index.get_symbol("CreatedAfterCache").is_none());

        let stats = reconcile_workspace_manifest_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&index)),
            restored_revisions,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(stats.files_failed, 0);
        assert!(index.get_symbol("CreatedAfterCache").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cache_reconciliation_removes_restored_sources_excluded_from_fresh_manifest() {
        let (workspace, _) = prepare_workspace_cache();
        std::fs::write(workspace.path().join(".gitignore"), "Cached0.java\n").unwrap();
        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));

        let outcome = restore_workspace_cache_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&index)),
        )
        .await
        .unwrap();
        let (restored_revisions, deferred_build_sources) = match outcome {
            CacheRestoreOutcome::Full {
                restored_revisions,
                deferred_build_sources,
            } => (restored_revisions, deferred_build_sources),
            other => panic!("expected full cache restore, got {other:?}"),
        };
        assert!(index.get_symbol("Cached0").is_none());
        assert!(deferred_build_sources.contains_key(&workspace.path().join("Cached0.java")));

        let stats = reconcile_workspace_manifest_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&index)),
            restored_revisions,
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(stats.files_failed, 0);
        assert!(index.get_symbol("Cached0").is_none());
        assert!(index.get_symbol("Cached1").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cache_reconciliation_preserves_a_newer_in_memory_publication() {
        let (workspace, _) = prepare_workspace_cache();
        let path = workspace.path().join("Cached0.java");
        std::fs::write(workspace.path().join(".gitignore"), "Cached0.java\n").unwrap();
        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));

        let outcome = restore_workspace_cache_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&index)),
        )
        .await
        .unwrap();
        let restored_revisions = match outcome {
            CacheRestoreOutcome::Full {
                restored_revisions, ..
            } => restored_revisions,
            other => panic!("expected full cache restore, got {other:?}"),
        };
        index
            .index_file_with_source(&path, "class EditorWins {}")
            .unwrap();

        reconcile_workspace_manifest_off_runtime(
            workspace.path().to_path_buf(),
            SemanticHost::new(Arc::clone(&index)),
            restored_revisions,
        )
        .await
        .unwrap()
        .unwrap();

        assert!(index.get_symbol("EditorWins").is_some());
        assert!(index.get_symbol("Cached0").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn partial_cache_restore_keeps_tokio_runtime_responsive() {
        let (workspace, paths) = prepare_workspace_cache();
        let deleted = paths[0].clone();
        std::fs::remove_file(&deleted).unwrap();
        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));

        let (outcome, heartbeats) =
            restore_with_heartbeat(workspace.path().to_path_buf(), Arc::clone(&index)).await;

        assert!(matches!(
            outcome,
            CacheRestoreOutcome::Partial { stale_files, .. } if stale_files == vec![deleted]
        ));
        assert!(heartbeats > 1, "runtime did not tick during cache restore");
        assert!(index.get_symbol("Cached0").is_none());
        assert!(index.get_symbol("Cached1").is_some());
        assert!(index.get_usage_score("Cached1.method0") > 0.0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn content_changed_behind_same_mtime_becomes_a_partial_cache_hit() {
        let (workspace, paths) = prepare_workspace_cache();
        let changed = paths[0].clone();
        let cached_mtime = std::fs::metadata(&changed).unwrap().modified().unwrap();
        std::fs::write(&changed, "class ChangedWithoutTimestampUpdate {}").unwrap();
        std::fs::File::open(&changed)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(cached_mtime))
            .unwrap();
        assert_eq!(
            std::fs::metadata(&changed).unwrap().modified().unwrap(),
            cached_mtime,
            "fixture must bypass metadata-only cache validation"
        );

        let index = Arc::new(WorkspaceIndex::new(workspace.path().to_path_buf()));
        let (outcome, heartbeats) =
            restore_with_heartbeat(workspace.path().to_path_buf(), Arc::clone(&index)).await;

        assert!(matches!(
            outcome,
            CacheRestoreOutcome::Partial { stale_files, .. } if stale_files == vec![changed]
        ));
        assert!(heartbeats > 1, "runtime did not tick during cache restore");
        assert!(index.get_symbol("Cached0").is_none());
        assert!(index.get_symbol("Cached1").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn jdk_blocking_boundary_keeps_tokio_runtime_responsive() {
        let heartbeat = RuntimeHeartbeat::start().await;

        run_blocking(|| std::thread::sleep(std::time::Duration::from_millis(50)))
            .await
            .expect("blocking JDK task panicked");
        let heartbeats = heartbeat.finish().await;

        assert!(
            heartbeats > 1,
            "Tokio heartbeat did not run while JDK work used the blocking boundary"
        );
    }

    #[test]
    fn jdk_progress_distinguishes_all_outcomes() {
        use crate::jdk_indexer::JdkIndexOutcome;

        assert_eq!(
            jdk_outcome_presentation(&JdkIndexOutcome::Indexed, 1.0).progress,
            "✓ JDK indexed"
        );
        assert_eq!(
            jdk_outcome_presentation(
                &JdkIndexOutcome::Unavailable {
                    reason: "missing".to_string(),
                },
                1.0,
            )
            .progress,
            "⚠ JDK unavailable"
        );
        assert_eq!(
            jdk_outcome_presentation(
                &JdkIndexOutcome::Partial {
                    indexed_archives: 1,
                    failed_archives: 1,
                },
                1.0,
            )
            .progress,
            "⚠ JDK partially indexed"
        );
    }

    #[test]
    fn jdk_background_publication_only_fires_for_indexed_inputs() {
        use crate::jdk_indexer::JdkIndexOutcome;

        assert!(jdk_outcome_published_symbols(&JdkIndexOutcome::Indexed));
        assert!(jdk_outcome_published_symbols(&JdkIndexOutcome::Partial {
            indexed_archives: 1,
            failed_archives: 2,
        }));
        assert!(!jdk_outcome_published_symbols(&JdkIndexOutcome::Partial {
            indexed_archives: 0,
            failed_archives: 2,
        }));
        assert!(!jdk_outcome_published_symbols(
            &JdkIndexOutcome::Unavailable {
                reason: "missing".to_owned(),
            }
        ));
    }
}
