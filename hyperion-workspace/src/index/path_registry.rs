// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Atomic ownership of workspace FileId/path identities.

use crate::{FileId, PathLookupKey};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The display spelling and filesystem identity of one workspace publication.
///
/// The pair is captured and compared atomically so transactional callers cannot
/// combine a pre-move display path with a post-move identity key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredFilePath {
    display_path: Arc<Path>,
    lookup_key: PathLookupKey,
}

impl RegisteredFilePath {
    pub(super) fn new(display_path: PathBuf, lookup_key: PathLookupKey) -> Self {
        let display_path = if display_path == lookup_key.as_path() {
            lookup_key.shared_path()
        } else {
            Arc::from(display_path)
        };
        Self {
            display_path,
            lookup_key,
        }
    }

    /// Original path spelling used for filesystem I/O and user-facing output.
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    /// Filesystem-resolved identity captured with [`Self::display_path`].
    pub fn lookup_key(&self) -> &PathLookupKey {
        &self.lookup_key
    }
}

/// Bidirectional ownership of every workspace path identity.
///
/// Both maps are private to this type so callers cannot update one side of the
/// FileId/path bijection without the other. All mutation validates the old and
/// new inverse before changing either side; corruption is surfaced at its
/// source instead of being silently repaired into a different inconsistency.
#[derive(Debug)]
pub(crate) struct PathRegistry {
    by_file_id: HashMap<FileId, RegisteredFilePath>,
    by_lookup_key: HashMap<PathLookupKey, FileId>,
}

impl PathRegistry {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            by_file_id: HashMap::with_capacity(capacity),
            by_lookup_key: HashMap::with_capacity(capacity),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.by_file_id.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.by_file_id.is_empty()
    }

    pub(super) fn reserve(&mut self, additional: usize) {
        self.by_file_id.reserve(additional);
        self.by_lookup_key.reserve(additional);
    }

    pub(super) fn contains_file_id(&self, file_id: FileId) -> bool {
        self.by_file_id.contains_key(&file_id)
    }

    pub(super) fn get(&self, file_id: FileId) -> Option<&RegisteredFilePath> {
        self.by_file_id.get(&file_id)
    }

    pub(super) fn file_id_for_key(&self, key: &PathLookupKey) -> Option<FileId> {
        self.by_lookup_key.get(key).copied()
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = (&FileId, &RegisteredFilePath)> {
        self.by_file_id.iter()
    }

    pub(super) fn file_ids(&self) -> impl ExactSizeIterator<Item = &FileId> {
        self.by_file_id.keys()
    }

    pub(super) fn bind(&mut self, file_id: FileId, registered: RegisteredFilePath) {
        let key = registered.lookup_key().clone();
        if let Some(other_file_id) = self.by_lookup_key.get(&key).copied() {
            assert_eq!(
                other_file_id, file_id,
                "a path identity cannot be registered to two live FileIds"
            );
        }
        if let Some(previous) = self.by_file_id.get(&file_id) {
            assert_eq!(
                self.by_lookup_key.get(previous.lookup_key()).copied(),
                Some(file_id),
                "a registered FileId must own its reverse path mapping"
            );
        }

        let previous = self.by_file_id.insert(file_id, registered);
        if let Some(previous) = previous {
            if previous.lookup_key() != &key {
                let removed = self.by_lookup_key.remove(previous.lookup_key());
                assert_eq!(removed, Some(file_id));
            }
        }
        let previous_owner = self.by_lookup_key.insert(key, file_id);
        assert!(previous_owner.is_none_or(|owner| owner == file_id));
    }

    pub(super) fn remove(&mut self, file_id: FileId) -> Option<RegisteredFilePath> {
        let registered = self.by_file_id.get(&file_id)?;
        assert_eq!(
            self.by_lookup_key.get(registered.lookup_key()).copied(),
            Some(file_id),
            "a registered FileId must own its reverse path mapping"
        );
        let registered = self
            .by_file_id
            .remove(&file_id)
            .expect("path registration disappeared under exclusive access");
        let removed = self.by_lookup_key.remove(registered.lookup_key());
        assert_eq!(removed, Some(file_id));
        Some(registered)
    }

    #[cfg(test)]
    pub(super) fn assert_consistent(&self) {
        assert_eq!(self.by_file_id.len(), self.by_lookup_key.len());
        for (&file_id, registered) in &self.by_file_id {
            assert_eq!(
                self.by_lookup_key.get(registered.lookup_key()),
                Some(&file_id)
            );
        }
        for (key, &file_id) in &self.by_lookup_key {
            assert_eq!(
                self.by_file_id
                    .get(&file_id)
                    .map(RegisteredFilePath::lookup_key),
                Some(key)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_lookup_key;

    #[test]
    fn occupied_key_is_rejected_without_displacing_the_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Owner.java");
        let key = path_lookup_key(&path);
        let mut registry = PathRegistry::with_capacity(2);
        registry.bind(
            FileId::new(1),
            RegisteredFilePath::new(path.clone(), key.clone()),
        );
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.bind(FileId::new(2), RegisteredFilePath::new(path, key.clone()));
        }));

        assert!(rejected.is_err());
        registry.assert_consistent();
        assert_eq!(registry.file_id_for_key(&key), Some(FileId::new(1)));
        assert!(!registry.contains_file_id(FileId::new(2)));
    }

    #[test]
    fn rebinding_and_removal_preserve_the_exact_inverse() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("First.java");
        let second = directory.path().join("Second.java");
        let first_key = path_lookup_key(&first);
        let second_key = path_lookup_key(&second);
        let file_id = FileId::new(1);
        let mut registry = PathRegistry::with_capacity(1);

        registry.bind(file_id, RegisteredFilePath::new(first, first_key.clone()));
        registry.bind(file_id, RegisteredFilePath::new(second, second_key.clone()));
        registry.assert_consistent();
        assert_eq!(registry.file_id_for_key(&first_key), None);
        assert_eq!(registry.file_id_for_key(&second_key), Some(file_id));

        let removed = registry.remove(file_id).unwrap();
        assert_eq!(removed.lookup_key(), &second_key);
        registry.assert_consistent();
        assert!(registry.is_empty());
    }
}
