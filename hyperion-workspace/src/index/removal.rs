// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Revision-exact source retirement transactions.

use super::*;

impl WorkspaceIndex {
    /// Retire one exact source publication without re-resolving its display
    /// path through the current filesystem.
    ///
    /// Cache-manifest reconciliation already owns a revision token and must be
    /// able to remove that old identity after a symlink retarget. Both the
    /// token and its atomic display/key pair are revalidated after acquiring
    /// the stored path lock and FileId lock. A newer publication always wins.
    pub fn remove_exact_file_revision<P, F>(
        &self,
        revision: FileRevisionToken,
        should_remove: P,
        after_removal: F,
    ) -> FileRevisionRemovalOutcome
    where
        P: FnOnce() -> bool,
        F: FnOnce(),
    {
        let file_id = revision.file_id();
        let registered_path = {
            let inner = self.inner.read();
            if inner.revision_token(file_id) != Some(revision) {
                return FileRevisionRemovalOutcome::RevisionChanged;
            }
            let Some(registered_path) = inner.registered_path(file_id).cloned() else {
                return FileRevisionRemovalOutcome::RevisionChanged;
            };
            registered_path
        };
        let path_key = registered_path.lookup_key().clone();
        let path_lock = self.get_path_lock_for_key(&path_key);
        let path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file_lock = self.get_file_lock(file_id);
        let file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let revision_is_current = {
            let inner = self.inner.read();
            inner.revision_token(file_id) == Some(revision)
                && inner.registered_path(file_id) == Some(&registered_path)
        };
        if !revision_is_current {
            drop(file_guard);
            drop(file_lock);
            drop(path_guard);
            drop(path_lock);
            self.remove_file_lock_if_unused(file_id);
            self.remove_path_lock_key_if_unused(&path_key);
            return FileRevisionRemovalOutcome::RevisionChanged;
        }
        if !should_remove() {
            drop(file_guard);
            drop(file_lock);
            drop(path_guard);
            drop(path_lock);
            return FileRevisionRemovalOutcome::Retained;
        }

        let dependent_files = self.get_dependent_files_for_file(file_id);
        let removed_key = self.remove_file_with_locks_held(file_id);
        after_removal();

        drop(file_guard);
        drop(file_lock);
        drop(path_guard);
        drop(path_lock);
        self.remove_file_lock_if_unused(file_id);
        self.remove_path_lock_key_if_unused(&path_key);
        assert_eq!(removed_key.as_ref(), Some(&path_key));

        debug!(
            "Removed exact revision {:?} from workspace index at {}",
            revision,
            registered_path.display_path().display()
        );
        FileRevisionRemovalOutcome::Removed { dependent_files }
    }
}
