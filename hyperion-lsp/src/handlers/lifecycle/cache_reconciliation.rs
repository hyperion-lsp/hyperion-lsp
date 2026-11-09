// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Revision-exact reconciliation of restored cache state with a fresh manifest.

use crate::semantic_host::SemanticHost;
use hyperion_workspace::{IndexStats, PathLookupKey, RestoredFileRevision, SourceRevisionOrigin};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Files (or retirements) per write transaction while reconciling a large
/// restored workspace: bounded so requests and editor notifications interleave
/// with startup reconciliation.
const RECONCILE_STEP: usize = 200;

/// Reconcile restored cache entries off Tokio workers.
pub(super) async fn reconcile_workspace_manifest_off_runtime(
    cache_root: PathBuf,
    host: Arc<SemanticHost>,
    restored_revisions: Vec<RestoredFileRevision>,
) -> Result<anyhow::Result<IndexStats>, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        reconcile_workspace_manifest(&cache_root, &host, &restored_revisions, || {})
    })
    .await
}

fn retire_missing_restored_revisions(
    host: &Arc<SemanticHost>,
    restored_revisions: &[RestoredFileRevision],
    manifest: &HashSet<PathLookupKey>,
) {
    let missing: Vec<_> = restored_revisions
        .iter()
        .filter(|restored| !manifest.contains(restored.registered_path.lookup_key()))
        .collect();
    for step in missing.chunks(RECONCILE_STEP) {
        host.publish_blocking(|txn| {
            let workspace_index = txn.index();
            for restored in step {
                let revision = restored.revision;
                let _ = workspace_index.remove_exact_file_revision(
                    revision,
                    || {
                        workspace_index.get_revision_origin(revision.file_id())
                            == Some(SourceRevisionOrigin::Disk)
                    },
                    || {},
                );
            }
        });
    }
}

fn reconcile_workspace_manifest(
    cache_root: &Path,
    host: &Arc<SemanticHost>,
    restored_revisions: &[RestoredFileRevision],
    after_initial_scan: impl FnOnce(),
) -> anyhow::Result<IndexStats> {
    let initial_files = hyperion_workspace::scanner::WorkspaceScanner::scan_dir(cache_root)?;
    let initial_manifest = initial_files
        .iter()
        .map(|path| hyperion_workspace::path_lookup_key(path))
        .collect();
    after_initial_scan();
    retire_missing_restored_revisions(host, restored_revisions, &initial_manifest);

    // A create or delete may land while the first scan and exact retirements
    // run. Re-scan after them: newly live files enter the indexing set, while a
    // cached revision that vanished during reconciliation is retired against
    // the final manifest before readiness is published.
    let files = hyperion_workspace::scanner::WorkspaceScanner::scan_dir(cache_root)?;
    let manifest = files
        .iter()
        .map(|path| hyperion_workspace::path_lookup_key(path))
        .collect();
    retire_missing_restored_revisions(host, restored_revisions, &manifest);
    let mut total = IndexStats {
        files_scanned: 0,
        files_indexed: 0,
        files_failed: 0,
        symbols_indexed: 0,
    };
    for step in files.chunks(RECONCILE_STEP) {
        let stats = host.publish_blocking(|txn| txn.index().index_files_parallel_skip_cached(step));
        total.files_scanned += stats.files_scanned;
        total.files_indexed += stats.files_indexed;
        total.files_failed += stats.files_failed;
        total.symbols_indexed += stats.symbols_indexed;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_workspace::WorkspaceIndex;

    #[test]
    fn indexes_a_source_recreated_after_the_initial_scan() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("Recreated.java");
        std::fs::write(&path, "class CachedBeforeDelete {}").unwrap();
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        source_index.index_file_sync_for_test(&path).unwrap();
        let cache = source_index.extract_cache_data();
        let index = WorkspaceIndex::new(workspace.path().to_path_buf());
        let (rejected, restored_revisions) = index.restore_from_cache_batch_with_revisions(&cache);
        assert!(rejected.is_empty());
        std::fs::remove_file(&path).unwrap();

        let stats = reconcile_workspace_manifest(
            workspace.path(),
            &SemanticHost::new(Arc::new(index.clone())),
            &restored_revisions,
            || {
                std::fs::write(&path, "class RecreatedAfterScan {}").unwrap();
            },
        )
        .unwrap();

        assert_eq!(stats.files_failed, 0);
        assert!(index.get_symbol("CachedBeforeDelete").is_none());
        assert!(index.get_symbol("RecreatedAfterScan").is_some());
    }

    #[test]
    fn retires_a_source_deleted_after_the_initial_scan() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("DeletedDuringScan.java");
        std::fs::write(&path, "class DeletedDuringScan {}").unwrap();
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        source_index.index_file_sync_for_test(&path).unwrap();
        let cache = source_index.extract_cache_data();
        let index = WorkspaceIndex::new(workspace.path().to_path_buf());
        let (rejected, restored_revisions) = index.restore_from_cache_batch_with_revisions(&cache);
        assert!(rejected.is_empty());

        let stats = reconcile_workspace_manifest(
            workspace.path(),
            &SemanticHost::new(Arc::new(index.clone())),
            &restored_revisions,
            || {
                std::fs::remove_file(&path).unwrap();
            },
        )
        .unwrap();

        assert_eq!(stats.files_failed, 0);
        assert!(index.get_symbol("DeletedDuringScan").is_none());
        assert!(index.get_file_id(&path).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn retires_the_stored_identity_after_a_symlink_retarget() {
        let workspace = tempfile::tempdir().unwrap();
        let first = workspace.path().join("first");
        let second = workspace.path().join("second");
        let alias = workspace.path().join("alias");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::fs::write(first.join("Target.java"), "class CachedAliasTarget {}").unwrap();
        std::fs::write(second.join("Target.java"), "class RetargetedAliasTarget {}").unwrap();
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let alias_path = alias.join("Target.java");
        let source_index = WorkspaceIndex::new(workspace.path().to_path_buf());
        source_index.index_file_sync_for_test(&alias_path).unwrap();
        let cache = source_index.extract_cache_data();
        let index = WorkspaceIndex::new(workspace.path().to_path_buf());
        let (rejected, restored_revisions) = index.restore_from_cache_batch_with_revisions(&cache);
        assert!(rejected.is_empty());

        std::fs::remove_file(&alias).unwrap();
        std::fs::remove_file(first.join("Target.java")).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        reconcile_workspace_manifest(
            workspace.path(),
            &SemanticHost::new(Arc::new(index.clone())),
            &restored_revisions,
            || {},
        )
        .unwrap();

        assert!(index.get_symbol("CachedAliasTarget").is_none());
        assert!(index.get_symbol("RetargetedAliasTarget").is_some());
    }
}
