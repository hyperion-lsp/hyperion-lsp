// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Atomic publication for external single-file renames.

use std::path::Path;
use std::sync::Arc;

use hyperion_workspace::Language;
use tracing::warn;

use super::{
    editor_owned_uri, index_created_file_checked, remove_deleted_file_checked, ModifiedFileOutcome,
    SourceMutationContext,
};
use crate::semantic_host::WriteTxn;
use crate::source_publication::{
    publish_semantic_revision, retire_absent_semantic_revisions, retire_semantic_revisions,
};

fn publish_recreated_source_and_destination(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    old_path: &Path,
    new_path: &Path,
) -> Result<Option<ModifiedFileOutcome>, ()> {
    let old_outcome = index_created_file_checked(txn, ctx, old_path);
    let new_outcome = index_created_file_checked(txn, ctx, new_path);
    match (old_outcome, new_outcome) {
        (Ok(Some(mut old)), Ok(Some(mut new))) => {
            new.changed_file_ids.append(&mut old.changed_file_ids);
            new.changed_file_ids
                .sort_unstable_by_key(|file_id| file_id.as_u32());
            new.changed_file_ids.dedup();
            new.dependent_files.append(&mut old.dependent_files);
            new.dependent_files
                .sort_unstable_by_key(|file_id| file_id.as_u32());
            new.dependent_files.dedup();
            Ok(Some(new))
        }
        (Ok(Some(outcome)), Ok(None)) | (Ok(None), Ok(Some(outcome))) => Ok(Some(outcome)),
        (Ok(Some(mut outcome)), Err(())) | (Err(()), Ok(Some(mut outcome))) => {
            outcome.complete = false;
            Ok(Some(outcome))
        }
        (Ok(None), Ok(None)) => Ok(None),
        (Err(()) | Ok(None), Err(())) | (Err(()), Ok(None)) => Err(()),
    }
}

#[cfg(test)]
pub(super) fn rename_disk_file(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    old_path: &Path,
    new_path: &Path,
) -> Option<ModifiedFileOutcome> {
    rename_disk_file_checked(txn, ctx, old_path, new_path)
        .ok()
        .flatten()
}

pub(super) fn rename_disk_file_checked(
    txn: &mut WriteTxn,
    ctx: &SourceMutationContext,
    old_path: &Path,
    new_path: &Path,
) -> Result<Option<ModifiedFileOutcome>, ()> {
    // A broken old alias can have both a newly observed missing-path key and a
    // stored resolved key. Acquire every distinct path identity in lexical
    // order so didOpen, deletion, and overlapping renames cannot interleave
    // publication through the key we did not happen to observe first.
    let observed_old_key = hyperion_workspace::path_lookup_key(old_path);
    let new_key = hyperion_workspace::path_lookup_key(new_path);
    let old_path_is_absent = matches!(old_path.try_exists(), Ok(false));
    let observed_old_workspace_id = ctx
        .workspace_index
        .get_file_id_for_key(&observed_old_key)
        .or_else(|| {
            old_path_is_absent
                .then(|| {
                    ctx.workspace_index
                        .get_file_id_for_key_or_display(&observed_old_key, old_path)
                })
                .flatten()
        });
    let observed_old_registry_id = ctx
        .file_id_registry
        .get_from_path_with_key(old_path, &observed_old_key);
    // A vanished symlink alias no longer reproduces its registered key. Once
    // exact display fallback identifies the old revision, lock the immutable
    // stored key that actually protects it.
    let old_key = observed_old_workspace_id
        .and_then(|file_id| ctx.workspace_index.get_file_path_key(file_id))
        .or_else(|| {
            observed_old_registry_id.and_then(|file_id| ctx.file_id_registry.get_path_key(file_id))
        })
        .unwrap_or_else(|| observed_old_key.clone());
    let mut path_keys = vec![observed_old_key.clone(), old_key.clone(), new_key.clone()];
    path_keys.sort_unstable();
    path_keys.dedup();
    let path_locks: Vec<_> = path_keys
        .iter()
        .map(|key| ctx.workspace_index.get_path_lock_for_key(key))
        .collect();
    let path_guards: Vec<_> = path_locks
        .iter()
        .map(|lock| {
            lock.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        })
        .collect();

    if old_key != new_key {
        match old_path.try_exists() {
            Ok(false) => {}
            Ok(true) => {
                // The old path was recreated before this watcher event ran.
                // Preserve it as a separate source and publish only the live
                // destination; destructively moving the old revision would
                // make the stale rename notification win over newer disk state.
                drop(path_guards);
                drop(path_locks);
                return publish_recreated_source_and_destination(txn, ctx, old_path, new_path);
            }
            Err(error) => {
                warn!(
                    "Could not prove renamed source {} was absent: {}",
                    old_path.display(),
                    error
                );
                return Err(());
            }
        }
    }

    let old_workspace_id = ctx
        .workspace_index
        .get_file_id_for_key(&old_key)
        .filter(|file_id| {
            Some(*file_id) == observed_old_workspace_id
                && ctx.workspace_index.get_file_path_key(*file_id).as_ref() == Some(&old_key)
        });
    let old_registry_id = ctx
        .file_id_registry
        .get_from_path_with_key(old_path, &old_key)
        .filter(|file_id| {
            Some(*file_id) == observed_old_registry_id
                && ctx.file_id_registry.get_path_key(*file_id).as_ref() == Some(&old_key)
        });
    let destination_workspace_id = ctx.workspace_index.get_file_id_for_key(&new_key);
    let destination_registry_id = ctx
        .file_id_registry
        .get_from_path_with_key(new_path, &new_key);

    let old_is_editor_owned = [old_workspace_id, old_registry_id]
        .into_iter()
        .flatten()
        .any(|file_id| editor_owned_uri(ctx, file_id).is_some());
    let destination_is_editor_owned = [destination_workspace_id, destination_registry_id]
        .into_iter()
        .flatten()
        .any(|file_id| editor_owned_uri(ctx, file_id).is_some());

    if old_is_editor_owned {
        // The watcher cannot safely duplicate one logical moved source while
        // the old URI still owns an editor buffer: equal FQNs would make one
        // file's declaration silently displace the other. The client's
        // didClose/didOpen sequence will publish its authoritative URI change.
        drop(path_guards);
        drop(path_locks);
        ctx.workspace_index
            .reclaim_path_lock_key_if_unused(&new_key);
        return Err(());
    }

    if old_workspace_id.is_none() {
        // A coalesced rename may arrive after the old revision was already
        // retired. Retire any closed registry-only identity before treating the
        // live destination as a create; otherwise the vanished old path remains
        // a second authority forever.
        if let Some(registry_id) = old_registry_id {
            let file_lock = ctx.workspace_index.get_file_lock(registry_id);
            let file_guard = file_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let workspace_now_owns_old = ctx
                .workspace_index
                .get_file_id_for_key(&observed_old_key)
                .is_some()
                || ctx.workspace_index.get_file_id_for_key(&old_key).is_some();
            let registry_still_matches = ctx
                .file_id_registry
                .get_from_path_with_key(old_path, &old_key)
                == Some(registry_id)
                && ctx.file_id_registry.get_path_key(registry_id).as_ref() == Some(&old_key);
            if workspace_now_owns_old
                || editor_owned_uri(ctx, registry_id).is_some()
                || !registry_still_matches
            {
                drop(file_guard);
                drop(file_lock);
                drop(path_guards);
                drop(path_locks);
                return Err(());
            }
            match old_path.try_exists() {
                Ok(false) => {
                    if ctx
                        .file_id_registry
                        .remove_expected_path_key(&old_key, registry_id)
                    {
                        let language = Language::from_path(old_path).unwrap_or(Language::Java);
                        retire_semantic_revisions(
                            txn,
                            &ctx.var_type_cache,
                            &[registry_id],
                            language,
                        );
                    }
                }
                Ok(true) => {}
                Err(error) => {
                    warn!(
                        "Could not prove renamed registry-only source {} was absent: {}",
                        old_path.display(),
                        error
                    );
                    return Err(());
                }
            }
            drop(file_guard);
            drop(file_lock);
        }
        drop(path_guards);
        drop(path_locks);
        ctx.workspace_index
            .reclaim_path_lock_key_if_unused(&old_key);
        if observed_old_key != old_key {
            ctx.workspace_index
                .reclaim_path_lock_key_if_unused(&observed_old_key);
        }
        return index_created_file_checked(txn, ctx, new_path);
    }

    if destination_is_editor_owned {
        // The destination editor buffer is authoritative. Retire only the
        // vanished closed source; never replace the open destination from disk.
        drop(path_guards);
        drop(path_locks);
        return remove_deleted_file_checked(txn, ctx, old_path).map(|outcome| {
            outcome.map(|outcome| ModifiedFileOutcome {
                file_id: outcome.file_id,
                changed_file_ids: vec![outcome.file_id],
                dependent_files: outcome.dependent_files,
                complete: true,
            })
        });
    }

    let moved_file_id = old_workspace_id.expect("checked above");
    let mut file_ids = vec![moved_file_id];
    file_ids.extend(
        [
            old_registry_id,
            destination_workspace_id,
            destination_registry_id,
        ]
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
            lock.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        })
        .collect();

    if file_ids
        .iter()
        .any(|file_id| editor_owned_uri(ctx, *file_id).is_some())
    {
        drop(file_guards);
        drop(file_locks);
        drop(path_guards);
        drop(path_locks);
        return Err(());
    }

    // Read only after both path identities and all involved FileIds are stable.
    // A failed read or extraction leaves the complete old revision published.
    let text = match hyperion_workspace::read_source_file(new_path) {
        Ok(text) => text,
        Err(error) => {
            warn!(
                "Keeping previous revision after renamed source {} could not be read: {}",
                new_path.display(),
                error
            );
            drop(file_guards);
            drop(file_locks);
            drop(path_guards);
            drop(path_locks);
            ctx.workspace_index
                .reclaim_unpublished_revision_locks_for_key(&new_key, moved_file_id);
            return Err(());
        }
    };

    if old_key != new_key {
        match old_path.try_exists() {
            Ok(false) => {}
            Ok(true) => {
                drop(file_guards);
                drop(file_locks);
                drop(path_guards);
                drop(path_locks);
                return publish_recreated_source_and_destination(txn, ctx, old_path, new_path);
            }
            Err(error) => {
                warn!(
                    "Could not revalidate renamed source absence for {}: {}",
                    old_path.display(),
                    error
                );
                return Err(());
            }
        }
    }

    let mut stats = match ctx
        .workspace_index
        .index_moved_disk_file_with_keys_and_locks_held(
            old_path,
            &old_key,
            new_path,
            &new_key,
            &text,
            moved_file_id,
        ) {
        Ok(stats) => stats,
        Err(error) => {
            warn!(
                "Keeping previous revision after rename publication failed {} -> {}: {}",
                old_path.display(),
                new_path.display(),
                error
            );
            drop(file_guards);
            drop(file_locks);
            drop(path_guards);
            drop(path_locks);
            ctx.workspace_index
                .reclaim_unpublished_revision_locks_for_key(&new_key, moved_file_id);
            return Err(());
        }
    };

    let language = Language::from_path(new_path).unwrap_or(Language::Java);
    // The workspace move is already committed. Relocate the editor identity
    // before the fallible semantic publication so a retry starts from one
    // coherent path/FileId mapping even when Salsa rejects this revision.
    let registry_displaced_ids = ctx.file_id_registry.relocate_published_path_with_keys(
        old_path,
        &old_key,
        new_path,
        &new_key,
        moved_file_id,
    );
    stats
        .retired_file_ids
        .extend(registry_displaced_ids.iter().copied());
    stats
        .retired_file_ids
        .sort_unstable_by_key(|id| id.as_u32());
    stats.retired_file_ids.dedup();
    if !publish_semantic_revision(
        txn,
        &ctx.var_type_cache,
        moved_file_id,
        Arc::new(text),
        language,
        &stats.retired_file_ids,
    ) {
        warn!(
            "Workspace revision changed before renamed semantic publication completed: {} -> {}",
            old_path.display(),
            new_path.display()
        );
        let retired_file_ids = stats.retired_file_ids;
        drop(file_guards);
        drop(file_locks);
        drop(path_guards);
        drop(path_locks);
        ctx.workspace_index
            .reclaim_unpublished_revision_locks_for_key(&old_key, moved_file_id);
        for retired_file_id in &retired_file_ids {
            ctx.workspace_index
                .reclaim_unpublished_revision_locks_for_key(&new_key, *retired_file_id);
        }
        retire_absent_semantic_revisions(txn, &ctx.var_type_cache, &retired_file_ids, language);
        return Err(());
    }

    let retired_file_ids = stats.retired_file_ids;
    let outcome = ModifiedFileOutcome {
        file_id: moved_file_id,
        changed_file_ids: vec![moved_file_id],
        dependent_files: stats.dependent_files,
        complete: true,
    };

    drop(file_guards);
    drop(file_locks);
    drop(path_guards);
    drop(path_locks);

    ctx.workspace_index
        .reclaim_unpublished_revision_locks_for_key(&old_key, moved_file_id);
    for retired_file_id in retired_file_ids {
        ctx.workspace_index
            .reclaim_unpublished_revision_locks_for_key(&new_key, retired_file_id);
    }
    Ok(Some(outcome))
}
