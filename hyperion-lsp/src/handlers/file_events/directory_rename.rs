// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Reconciliation for external directory renames.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use hyperion_semantics::db::FileId;
use tracing::warn;

use super::rename::rename_disk_file_checked;
use super::{index_created_file_checked, remove_deleted_file_checked, SourceMutationContext};
use crate::semantic_host::WriteTxn;

/// Watcher reconciliation is a recovery path, not an unbounded replacement
/// for workspace startup. Crossing this limit leaves source readiness failed
/// so a destructive operation can never mistake the partial repair for proof.
const MAX_DIRECTORY_RECONCILIATION_ENTRIES: usize = 100_000;

pub(super) struct DirectoryRenameOutcome {
    pub changed_file_ids: Vec<FileId>,
    pub dependent_files: Vec<FileId>,
    pub revisions_changed: usize,
    pub scan_complete: bool,
    /// Destination traversal and every required source publication completed.
    pub complete: bool,
    pub build_files_changed: bool,
}

pub(super) struct SourceTreeScan {
    pub files: Vec<PathBuf>,
    pub complete: bool,
    pub build_files_found: bool,
}

impl DirectoryRenameOutcome {
    fn absorb(
        changed_file_ids: &mut HashSet<FileId>,
        dependent_files: &mut HashSet<FileId>,
        revisions_changed: &mut usize,
        file_ids: Vec<FileId>,
        dependents: Vec<FileId>,
    ) {
        for file_id in file_ids {
            if changed_file_ids.insert(file_id) {
                *revisions_changed += 1;
            }
        }
        dependent_files.extend(dependents);
    }
}

pub(super) fn reconcile_directory_rename(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    from: &Path,
    to: &Path,
) -> DirectoryRenameOutcome {
    let scan = scan_source_tree(to, &ctx.workspace_index.root());
    let (indexed_old_files, truncated) = ctx
        .workspace_index
        .workspace_files_under_bounded(from, MAX_DIRECTORY_RECONCILIATION_ENTRIES);
    let indexed_old_files_complete = !truncated;
    if !indexed_old_files_complete {
        warn!(
            "Directory rename {} contains more than {} indexed files; reconciliation is partial",
            from.display(),
            MAX_DIRECTORY_RECONCILIATION_ENTRIES
        );
    }
    let build_files_changed = !indexed_old_files_complete
        || scan.build_files_found
        || indexed_old_files
            .iter()
            .any(|(_, path)| hyperion_layout::is_build_input(path));
    let new_file_keys: HashSet<_> = scan
        .files
        .iter()
        .map(|path| hyperion_workspace::path_lookup_key(path))
        .collect();
    let mut matched_new_paths = HashSet::with_capacity(indexed_old_files.len());
    let mut changed_file_ids = HashSet::new();
    let mut dependent_files = HashSet::new();
    let mut revisions_changed = 0;
    let mut complete = scan.complete && indexed_old_files_complete;

    for (_, old_path) in indexed_old_files {
        let Some(new_path) = new_path_for_renamed_file(from, to, &old_path) else {
            warn!(
                "Indexed path {} was not below renamed directory {}",
                old_path.display(),
                from.display()
            );
            complete = false;
            continue;
        };
        let new_key = hyperion_workspace::path_lookup_key(&new_path);

        if new_file_keys.contains(&new_key) {
            match rename_disk_file_checked(txn, ctx, &old_path, &new_path) {
                Ok(Some(outcome)) => {
                    if outcome.complete {
                        matched_new_paths.insert(new_key);
                    } else {
                        complete = false;
                    }
                    DirectoryRenameOutcome::absorb(
                        &mut changed_file_ids,
                        &mut dependent_files,
                        &mut revisions_changed,
                        outcome.changed_file_ids,
                        outcome.dependent_files,
                    );
                }
                Ok(None) => {
                    matched_new_paths.insert(new_key);
                }
                // Leave the destination unmatched so the create pass gets one
                // independent retry. Readiness remains incomplete even if that
                // retry succeeds because the old-side transaction deferred.
                Err(()) => complete = false,
            }
        } else if scan.complete {
            // A complete destination scan proves this indexed old path has no
            // moved counterpart. An incomplete scan must retain the last good
            // revision rather than converting a transient I/O error to delete.
            match remove_deleted_file_checked(txn, ctx, &old_path) {
                Ok(Some(outcome)) => DirectoryRenameOutcome::absorb(
                    &mut changed_file_ids,
                    &mut dependent_files,
                    &mut revisions_changed,
                    vec![outcome.file_id],
                    outcome.dependent_files,
                ),
                Ok(None) => {}
                Err(()) => complete = false,
            }
        }
    }

    // Files present only in the destination are genuine creates (or files the
    // old index had never successfully published).
    for new_path in scan
        .files
        .iter()
        .filter(|path| !matched_new_paths.contains(&hyperion_workspace::path_lookup_key(path)))
    {
        match index_created_file_checked(txn, ctx, new_path) {
            Ok(Some(outcome)) => DirectoryRenameOutcome::absorb(
                &mut changed_file_ids,
                &mut dependent_files,
                &mut revisions_changed,
                outcome.changed_file_ids,
                outcome.dependent_files,
            ),
            Ok(None) => {}
            Err(()) => complete = false,
        }
    }

    dependent_files.retain(|file_id| !changed_file_ids.contains(file_id));
    let mut changed_file_ids: Vec<_> = changed_file_ids.into_iter().collect();
    changed_file_ids.sort_unstable_by_key(|file_id| file_id.as_u32());
    let mut dependent_files: Vec<_> = dependent_files.into_iter().collect();
    dependent_files.sort_unstable_by_key(|file_id| file_id.as_u32());

    DirectoryRenameOutcome {
        changed_file_ids,
        dependent_files,
        revisions_changed,
        scan_complete: scan.complete,
        complete,
        build_files_changed,
    }
}

/// Reconcile a directory create when the watcher did not provide individual
/// child events. Only the new subtree is scanned; unrelated workspace paths are
/// never touched.
pub(super) fn reconcile_directory_create(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    root: &Path,
) -> DirectoryRenameOutcome {
    let scan = scan_source_tree(root, &ctx.workspace_index.root());
    let mut changed_file_ids = HashSet::new();
    let mut dependent_files = HashSet::new();
    let mut revisions_changed = 0;
    let mut complete = scan.complete;

    for path in &scan.files {
        match index_created_file_checked(txn, ctx, path) {
            Ok(Some(outcome)) => DirectoryRenameOutcome::absorb(
                &mut changed_file_ids,
                &mut dependent_files,
                &mut revisions_changed,
                outcome.changed_file_ids,
                outcome.dependent_files,
            ),
            Ok(None) => {}
            Err(()) => complete = false,
        }
    }

    finish_directory_outcome(
        changed_file_ids,
        dependent_files,
        revisions_changed,
        scan.complete,
        complete,
        scan.build_files_found,
    )
}

/// Reconcile a directory removal from the authoritative set of indexed paths.
/// The directory no longer exists, so a disk traversal cannot recover child
/// paths and watcher backends may not emit per-file removals.
pub(super) fn reconcile_directory_delete(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    root: &Path,
) -> DirectoryRenameOutcome {
    let (indexed_files, truncated) = ctx
        .workspace_index
        .workspace_files_under_bounded(root, MAX_DIRECTORY_RECONCILIATION_ENTRIES);
    let mut changed_file_ids = HashSet::new();
    let mut dependent_files = HashSet::new();
    let mut revisions_changed = 0;
    let mut complete = !truncated;
    if !complete {
        warn!(
            "Directory removal {} contains more than {} indexed files; reconciliation is partial",
            root.display(),
            MAX_DIRECTORY_RECONCILIATION_ENTRIES
        );
    }

    for (_, path) in indexed_files {
        match remove_deleted_file_checked(txn, ctx, &path) {
            Ok(Some(outcome)) => DirectoryRenameOutcome::absorb(
                &mut changed_file_ids,
                &mut dependent_files,
                &mut revisions_changed,
                vec![outcome.file_id],
                outcome.dependent_files,
            ),
            Ok(None) => {}
            Err(()) => complete = false,
        }
    }

    // The removed tree is unavailable for inspection. Refreshing the build
    // model is conservative but necessary: it may have contained a nested POM,
    // settings script, or version catalog not represented in WorkspaceIndex.
    finish_directory_outcome(
        changed_file_ids,
        dependent_files,
        revisions_changed,
        true,
        complete,
        true,
    )
}

fn finish_directory_outcome(
    changed_file_ids: HashSet<FileId>,
    mut dependent_files: HashSet<FileId>,
    revisions_changed: usize,
    scan_complete: bool,
    complete: bool,
    build_files_changed: bool,
) -> DirectoryRenameOutcome {
    dependent_files.retain(|file_id| !changed_file_ids.contains(file_id));
    let mut changed_file_ids: Vec<_> = changed_file_ids.into_iter().collect();
    changed_file_ids.sort_unstable_by_key(|file_id| file_id.as_u32());
    let mut dependent_files: Vec<_> = dependent_files.into_iter().collect();
    dependent_files.sort_unstable_by_key(|file_id| file_id.as_u32());
    DirectoryRenameOutcome {
        changed_file_ids,
        dependent_files,
        revisions_changed,
        scan_complete,
        complete,
        build_files_changed,
    }
}

pub(super) fn scan_source_tree(root: &Path, workspace_root: &Path) -> SourceTreeScan {
    let mut files =
        match hyperion_workspace::scanner::WorkspaceScanner::scan_dir_with_exclusion_root(
            root,
            workspace_root,
        ) {
            Ok(files) => files,
            Err(error) => {
                warn!(
                    "Directory reconciliation scan of {} was incomplete: {}",
                    root.display(),
                    error
                );
                return SourceTreeScan {
                    files: Vec::new(),
                    complete: false,
                    // A failed traversal cannot prove that no Maven/Gradle input
                    // was present. Refreshing is the conservative recovery path.
                    build_files_found: true,
                };
            }
        };

    let complete = files.len() <= MAX_DIRECTORY_RECONCILIATION_ENTRIES;
    if !complete {
        warn!(
            "Directory reconciliation for {} exceeded the {}-source safety bound",
            root.display(),
            MAX_DIRECTORY_RECONCILIATION_ENTRIES
        );
        files.truncate(MAX_DIRECTORY_RECONCILIATION_ENTRIES);
    }

    // WorkspaceScanner deliberately returns source files only, so it cannot
    // prove the absence of a nested pom.xml or Gradle properties file. A
    // directory event is rare and a conservative build-model refresh is much
    // safer than retaining a stale classpath.
    SourceTreeScan {
        files,
        complete,
        build_files_found: true,
    }
}

#[cfg(test)]
pub(super) fn old_path_for_renamed_file(
    from: &Path,
    to: &Path,
    new_path: &Path,
) -> Option<PathBuf> {
    let relative = relative_path_for_lookup(new_path, to)?;
    Some(from.join(relative))
}

fn new_path_for_renamed_file(from: &Path, to: &Path, old_path: &Path) -> Option<PathBuf> {
    let relative = relative_path_for_lookup(old_path, from)?;
    Some(to.join(relative))
}

fn relative_path_for_lookup(path: &Path, base: &Path) -> Option<PathBuf> {
    // Watchers and the initial scan can disagree on symlink, normalization, or
    // spelling aliases. Both containment and the suffix must come from the
    // resolved keys: a symlink can expand to a different component depth, so
    // skipping the raw base's component count can manufacture a wrong path.
    let path_key = hyperion_workspace::path_lookup_key(path);
    let base_key = hyperion_workspace::path_lookup_key(base);
    let resolved_suffix = path_key.strip_prefix(&base_key).ok()?.to_path_buf();

    // Preserve watcher spelling only after typed containment has been proven,
    // and never carry traversal/root components into the reconstructed path.
    let raw_suffix = path.strip_prefix(base).ok().filter(|suffix| {
        suffix.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    });
    Some(raw_suffix.map_or(resolved_suffix, Path::to_path_buf))
}
