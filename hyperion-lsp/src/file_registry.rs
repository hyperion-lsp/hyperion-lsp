// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Editor FileId Registry
//!
//! This module owns FileIds and URI/path mappings for editor buffers. Each open
//! file's ID is passed unchanged to Salsa and the workspace index.
//!
//! ## FileId Architecture
//!
//! Hyperion uses disjoint allocation domains within one `u32` namespace:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     FileIdRegistry                           │
//! │  Source of truth for editor Path ↔ FileId mappings          │
//! │  Counter starts at 2^30, increments for each open file      │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!        ┌─────────────────────┼─────────────────────┐
//!        │                     │                     │
//!        ▼                     ▼                     ▼
//! ┌─────────────┐      ┌─────────────┐      ┌─────────────┐
//! │  Database   │      │ WorkspaceIdx│      │   Symbols   │
//! │  (Salsa)    │      │             │      │             │
//! │             │      │             │      │             │
//! │ Stores file │      │ Stores file │      │ References  │
//! │ text for    │      │ symbols and │      │ files by    │
//! │ parsing     │      │ references  │      │ FileId      │
//! └─────────────┘      └─────────────┘      └─────────────┘
//! ```
//!
//! ## How It Works
//!
//! 1. **LSP Server**: When a document is opened (did_open) or changed:
//!    - Gets FileId from registry: `file_id_registry.get_or_create_from_uri(uri)`
//!    - Updates database: `db.set_file_text(file_id, text)`
//!    - Indexes in workspace: `workspace_index.index_file_with_source_and_id(path, source, file_id)`
//!
//! 2. **Same FileId Used Everywhere**: The FileId from the registry is used for:
//!    - Database queries (parsing, semantic analysis)
//!    - Workspace lookups (symbols, references)
//!    - LSP responses (locations, ranges)
//!
//! 3. **Allocation domains**:
//!    - Workspace sources allocate upward from 1
//!    - Editor buffers allocate upward from 2^30
//!    - External archives allocate downward from `u32::MAX`
//!
//! ## Previous Problem (Solved)
//!
//! Previously, FileIds were allocated independently:
//! - LSP server: allocated when documents were opened
//! - Workspace index: allocated when files were indexed
//!
//! This caused the same file to have DIFFERENT FileIds in different systems,
//! leading to coordination bugs and confusing code.
//!
//! ## Current Solution
//!
//! The FileIdRegistry provides:
//! - Editor-owned FileId allocation from a reserved domain
//! - Thread-safe operations using RwLock
//! - Support for both URI and PathBuf lookups
//! - Bidirectional mappings (FileId ↔ Path)
//! - Automatic canonicalization for consistent path handling
//!
//! ## Note on WorkspaceIndex
//!
//! WorkspaceIndex can allocate IDs during background scans. Its path lock and
//! disjoint low-numbered domain prevent those scans from racing editor-owned IDs.

use hyperion_workspace::{FileId, PathLookupKey};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

/// Editor-owned files allocate from a domain disjoint from workspace scans.
///
/// `WorkspaceIndex` allocates source files upward from 1 and external archives
/// downward from `u32::MAX`. Starting editor buffers at one quarter of the range
/// leaves over a billion IDs on either side before the domains could meet.
const EDITOR_FILE_ID_START: u32 = 1 << 30;

/// Resolve a path through the workspace's filesystem-aware identity policy.
///
/// Existing aliases converge through canonicalization. Missing suffixes remain
/// stable across create/delete cycles. Existing spelling aliases converge only
/// through the filesystem's own resolution; no userland case fold is guessed.
fn registry_key(path: &Path) -> PathLookupKey {
    hyperion_workspace::path_lookup_key(path)
}

/// Central registry for FileId allocation and URI/Path mappings.
///
/// This registry ensures that each file has a unique FileId across the entire
/// LSP server and workspace index, preventing duplication and coordination issues.
#[derive(Debug)]
pub struct FileIdRegistry {
    inner: RwLock<RegistryInner>,
}

#[derive(Debug)]
struct RegistryInner {
    /// Next FileId to allocate
    next_id: u32,

    /// Path → FileId mapping
    path_to_id: HashMap<PathLookupKey, FileId>,

    /// FileId → stable lookup key and display path (reverse lookup).
    ///
    /// Retaining the key is important: recomputing it after a deletion or
    /// symlink change can observe a different filesystem identity and leave a
    /// stale forward binding behind.
    id_to_path: HashMap<FileId, RegisteredPath>,
}

#[derive(Debug)]
struct RegisteredPath {
    key: PathLookupKey,
    display_path: PathBuf,
}

impl RegistryInner {
    fn id_for_exact_display_path(&self, path: &Path) -> Option<FileId> {
        let mut matches = self.id_to_path.iter().filter_map(|(id, registered)| {
            (registered.display_path.as_path() == path).then_some(*id)
        });
        let only = matches.next()?;
        matches.next().is_none().then_some(only)
    }

    fn remove_id(&mut self, id: FileId) -> Option<RegisteredPath> {
        let registered = self.id_to_path.remove(&id)?;
        if self.path_to_id.get(&registered.key).copied() == Some(id) {
            self.path_to_id.remove(&registered.key);
        }
        Some(registered)
    }

    fn bind(&mut self, key: PathLookupKey, display_path: PathBuf, id: FileId) {
        // Preserve the bijection even if a caller is repairing an already
        // inconsistent registry. Both displaced sides are retired before the
        // new pair becomes visible under the write lock.
        if let Some(displaced_id) = self.path_to_id.get(&key).copied() {
            if displaced_id != id {
                self.remove_id(displaced_id);
            }
        }
        self.remove_id(id);
        self.path_to_id.insert(key.clone(), id);
        self.id_to_path
            .insert(id, RegisteredPath { key, display_path });
    }
}

impl FileIdRegistry {
    /// Creates a registry whose editor-owned IDs start in their disjoint domain.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(RegistryInner {
                next_id: EDITOR_FILE_ID_START,
                path_to_id: HashMap::new(),
                id_to_path: HashMap::new(),
            }),
        }
    }

    /// Gets or creates a FileId for the given URI.
    ///
    /// If the URI has already been registered, returns the existing FileId.
    /// Otherwise, allocates a new FileId and registers the mapping.
    ///
    /// # Arguments
    ///
    /// * `uri` - The LSP URI (e.g., "file:///path/to/file.java")
    ///
    /// # Returns
    ///
    /// The FileId for this URI, or None if the URI cannot be converted to a file path.
    pub fn get_or_create_from_uri(&self, uri: &Url) -> Option<FileId> {
        let path = uri.to_file_path().ok()?;
        Some(self.get_or_create_from_path(&path))
    }

    /// Gets or creates a FileId for the given path.
    ///
    /// If the path has already been registered, returns the existing FileId.
    /// Otherwise, allocates a new FileId and registers the mapping.
    ///
    /// The path is canonicalized to ensure consistent handling of relative paths,
    /// symlinks, and path separators.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path
    ///
    /// # Returns
    ///
    /// The FileId for this path.
    pub fn get_or_create_from_path(&self, path: &Path) -> FileId {
        self.get_or_create_from_path_with_preferred_id(path, None)
    }

    /// Gets or creates a FileId, reusing an already-published workspace ID when
    /// the path has not yet entered the editor registry.
    ///
    /// The caller must serialize this decision with workspace path publication.
    /// An existing registry binding always wins; replacing it would orphan any
    /// open document that already owns that ID.
    pub(crate) fn get_or_create_from_path_with_preferred_id(
        &self,
        path: &Path,
        preferred_id: Option<FileId>,
    ) -> FileId {
        let key = registry_key(path);
        self.get_or_create_from_path_with_key_and_preferred_id(path, key, preferred_id)
    }

    /// Key-preserving variant for callers that already established the
    /// workspace path lock. Recomputing here could select a retargeted symlink
    /// identity different from the lock the caller holds.
    pub(crate) fn get_or_create_from_path_with_key_and_preferred_id(
        &self,
        path: &Path,
        key: PathLookupKey,
        preferred_id: Option<FileId>,
    ) -> FileId {
        // Keep the caller's spelling for URI/display round-tripping, but use a
        // separately resolved identity key for lookup. The workspace resolver
        // keeps missing suffixes stable and canonicalizes only the deepest
        // existing ancestor.
        let display_path = path.to_path_buf();
        let path_existence = path.try_exists();

        // Fast path: check if we already have this path
        {
            let inner = self.inner.read();
            if let Some(&id) = inner.path_to_id.get(&key) {
                return id;
            }
        }

        // Slow path: allocate new FileId
        let mut inner = self.inner.write();

        // Double-check in case another thread allocated while we were waiting for write lock
        if let Some(&id) = inner.path_to_id.get(&key) {
            return id;
        }

        // A deleted/broken alias can no longer reproduce its stored resolved
        // key. Reuse that exact spelling while it remains absent. If the alias
        // now resolves to a different live owner, preserve the old resolved
        // binding and allocate/adopt the new identity below.
        if let Some(stale_id) = inner.id_for_exact_display_path(path) {
            if matches!(path_existence, Ok(false)) {
                // This is a new publication for the same absent URI spelling.
                // Transfer its editor-owned identity to the established
                // missing-path key so registry and workspace transactions lock
                // the same authority from this point onward.
                inner.bind(key, display_path, stale_id);
                return stale_id;
            }
            // A live or inaccessible spelling may now resolve to a different
            // owner. Keep the old key/id reachable and publish a second key;
            // exact-display fallback fails closed if that spelling later
            // becomes ambiguous.
        }

        // A cold workspace scan may already have published this path under a
        // low-domain ID. Adopt it unless that ID is already bound to a different
        // editor path (which would violate the registry's bijection).
        let id = preferred_id
            .filter(|id| !inner.id_to_path.contains_key(id))
            .unwrap_or_else(|| {
                let id = FileId::new(inner.next_id);
                inner.next_id = inner
                    .next_id
                    .checked_add(1)
                    .expect("editor FileId domain exhausted");
                id
            });

        // Keep the original display spelling separate from the resolved key.
        inner.bind(key, display_path, id);

        id
    }

    /// Gets the FileId for a URI, if it has been registered.
    ///
    /// # Arguments
    ///
    /// * `uri` - The LSP URI
    ///
    /// # Returns
    ///
    /// The FileId, or None if the URI hasn't been registered or can't be converted to a path.
    pub fn get_from_uri(&self, uri: &Url) -> Option<FileId> {
        let path = uri.to_file_path().ok()?;
        self.get_from_path(&path)
    }

    /// Gets the FileId for a path, if it has been registered.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path
    ///
    /// # Returns
    ///
    /// The FileId, or None if the path hasn't been registered.
    pub fn get_from_path(&self, path: &Path) -> Option<FileId> {
        // Resolve the filesystem observation before acquiring the registry
        // lock. Permission and I/O errors are uncertainty, never proof that an
        // inaccessible identity is absent.
        let key = registry_key(path);
        self.get_from_path_with_key(path, &key)
    }

    pub(crate) fn get_from_path_with_key(
        &self,
        path: &Path,
        key: &PathLookupKey,
    ) -> Option<FileId> {
        let path_is_absent = matches!(path.try_exists(), Ok(false));
        let inner = self.inner.read();
        inner.path_to_id.get(key).copied().or_else(|| {
            // Miss-only fallback for a deleted file or broken symlink. A live
            // retargeted alias is a new filesystem identity and must not be
            // captured by its stale display spelling.
            path_is_absent
                .then(|| inner.id_for_exact_display_path(path))
                .flatten()
        })
    }

    /// Gets the path for a FileId.
    ///
    /// # Arguments
    ///
    /// * `id` - The FileId
    ///
    /// # Returns
    ///
    /// The path, or None if the FileId hasn't been registered.
    pub fn get_path(&self, id: FileId) -> Option<PathBuf> {
        let inner = self.inner.read();
        inner
            .id_to_path
            .get(&id)
            .map(|registered| registered.display_path.clone())
    }

    /// Gets the identity key currently stored for a FileId.
    ///
    /// Transaction callers use this after an absent exact-display fallback so
    /// they can lock and then revalidate the stored identity, rather than using
    /// a newly recomputed missing-path key.
    pub(crate) fn get_path_key(&self, id: FileId) -> Option<PathLookupKey> {
        self.inner
            .read()
            .id_to_path
            .get(&id)
            .map(|registered| registered.key.clone())
    }

    /// Gets the URI for a FileId.
    ///
    /// # Arguments
    ///
    /// * `id` - The FileId
    ///
    /// # Returns
    ///
    /// The URI, or None if the FileId hasn't been registered or can't be converted to a URI.
    pub fn get_uri(&self, id: FileId) -> Option<Url> {
        let path = self.get_path(id)?;
        Url::from_file_path(path).ok()
    }

    /// Removes a FileId registration.
    ///
    /// This should be called when a file is deleted or no longer tracked.
    ///
    /// # Arguments
    ///
    /// * `id` - The FileId to remove
    pub fn remove(&self, id: FileId) {
        self.inner.write().remove_id(id);
    }

    /// Remove the identity currently bound to `path` and return it.
    ///
    /// Callers use this for a closed, registry-only path after proving that no
    /// workspace revision exists. Editor-owned bindings must be filtered by the
    /// caller while holding the workspace path-publication lock.
    #[cfg(test)]
    pub(crate) fn remove_path(&self, path: &Path) -> Option<FileId> {
        let key = registry_key(path);
        self.remove_path_with_key(path, &key)
    }

    #[cfg(test)]
    pub(crate) fn remove_path_with_key(&self, path: &Path, key: &PathLookupKey) -> Option<FileId> {
        let path_is_absent = matches!(path.try_exists(), Ok(false));
        let mut inner = self.inner.write();
        // The resolved owner always wins. Only on a key miss may an exact
        // stored spelling retire a binding whose symlink/case identity is no
        // longer observable.
        let id = inner.path_to_id.get(key).copied().or_else(|| {
            path_is_absent
                .then(|| inner.id_for_exact_display_path(path))
                .flatten()
        })?;
        inner.remove_id(id);
        Some(id)
    }

    /// Remove exactly the captured key/ID pair, without filesystem or display
    /// fallback. This is the commit primitive for callers that already hold
    /// every path and FileId lock protecting a cross-layer transaction.
    pub(crate) fn remove_expected_path_key(
        &self,
        key: &PathLookupKey,
        expected_id: FileId,
    ) -> bool {
        let mut inner = self.inner.write();
        if inner.path_to_id.get(key).copied() != Some(expected_id)
            || inner.id_to_path.get(&expected_id).map(|path| &path.key) != Some(key)
        {
            return false;
        }
        inner.remove_id(expected_id);
        true
    }

    /// Replace a stale closed registry binding with an already-published
    /// workspace identity.
    ///
    /// The registry remains sparse when the path was never editor-registered.
    /// The caller must first prove that the existing binding is not
    /// editor-owned while holding the workspace path lock. The displaced ID is
    /// returned so its semantic side state can be retired.
    pub(crate) fn reconcile_registered_path_with_key(
        &self,
        path: &Path,
        key: PathLookupKey,
        published_id: FileId,
    ) -> Option<FileId> {
        let mut inner = self.inner.write();
        // Reconciliation follows a successfully published live path, so only
        // its established key may identify the stale closed registry entry.
        // A display alias may still name an older resolved owner.
        let registered_id = inner.path_to_id.get(&key).copied()?;
        if registered_id == published_id {
            if let Some(registered) = inner.id_to_path.get_mut(&published_id) {
                registered.display_path = path.to_path_buf();
            } else {
                inner.bind(key, path.to_path_buf(), published_id);
            }
            return None;
        }

        inner.remove_id(registered_id);
        inner.bind(key, path.to_path_buf(), published_id);
        Some(registered_id)
    }

    /// Mirror an already-serialized workspace move without registering cold
    /// workspace identities that were never editor-owned.
    ///
    /// Any closed destination binding is displaced. If `file_id` owned the old
    /// binding, that same identity is moved to `new_path`; otherwise the
    /// registry remains sparse and the workspace index continues to own path
    /// lookup for this file.
    #[cfg(test)]
    pub(crate) fn relocate_published_path(
        &self,
        old_path: &Path,
        new_path: &Path,
        file_id: FileId,
    ) -> Vec<FileId> {
        let old_key = registry_key(old_path);
        let new_key = registry_key(new_path);
        self.relocate_published_path_with_keys(old_path, &old_key, new_path, &new_key, file_id)
    }

    pub(crate) fn relocate_published_path_with_keys(
        &self,
        old_path: &Path,
        old_key: &PathLookupKey,
        new_path: &Path,
        new_key: &PathLookupKey,
        file_id: FileId,
    ) -> Vec<FileId> {
        let old_path_is_absent = matches!(old_path.try_exists(), Ok(false));
        let mut inner = self.inner.write();
        let old_registered_id = inner.path_to_id.get(old_key).copied().or_else(|| {
            old_path_is_absent
                .then(|| inner.id_for_exact_display_path(old_path))
                .flatten()
        });
        // The new path was read and published before this call. Never let an
        // older identity with the same display spelling override its resolved
        // destination key.
        let destination_registered_id = inner.path_to_id.get(new_key).copied();
        let should_keep_registered = old_registered_id.is_some();
        let mut displaced = Vec::with_capacity(2);

        for registered_id in [old_registered_id, destination_registered_id]
            .into_iter()
            .flatten()
        {
            if registered_id != file_id && !displaced.contains(&registered_id) {
                displaced.push(registered_id);
            }
            inner.remove_id(registered_id);
        }

        if should_keep_registered {
            inner.bind(new_key.clone(), new_path.to_path_buf(), file_id);
        }

        displaced
    }

    /// Gets the total number of registered files.
    pub fn len(&self) -> usize {
        self.inner.read().id_to_path.len()
    }

    /// Checks if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for FileIdRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_get_or_create() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/test.java");

        let id1 = registry.get_or_create_from_path(&path);
        let id2 = registry.get_or_create_from_path(&path);

        // Should return the same ID for the same path
        assert_eq!(id1, id2);
        assert_eq!(id1.as_u32(), EDITOR_FILE_ID_START);
    }

    #[test]
    fn adopts_a_preferred_workspace_id_for_an_unregistered_path() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/cold-indexed.java");
        let workspace_id = FileId::new(17);

        assert_eq!(
            registry.get_or_create_from_path_with_preferred_id(&path, Some(workspace_id)),
            workspace_id
        );
        assert_eq!(registry.get_from_path(&path), Some(workspace_id));
        assert_eq!(
            registry.get_path(workspace_id).as_deref(),
            Some(path.as_path())
        );
    }

    #[test]
    fn existing_path_binding_precedes_a_later_preference() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/already-open.java");
        let editor_id = registry.get_or_create_from_path(&path);

        assert_eq!(
            registry.get_or_create_from_path_with_preferred_id(&path, Some(FileId::new(23))),
            editor_id
        );
    }

    #[test]
    fn test_different_paths_different_ids() {
        let registry = FileIdRegistry::new();
        let path1 = PathBuf::from("/tmp/test1.java");
        let path2 = PathBuf::from("/tmp/test2.java");

        let id1 = registry.get_or_create_from_path(&path1);
        let id2 = registry.get_or_create_from_path(&path2);

        // Different paths should get different IDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_get_path() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/test.java");

        let id = registry.get_or_create_from_path(&path);
        let retrieved_path = registry.get_path(id);

        assert!(retrieved_path.is_some());
        // Paths should match (might be canonicalized)
        assert!(retrieved_path.unwrap().ends_with("test.java"));
    }

    #[test]
    fn test_get_from_path_nonexistent() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/nonexistent.java");

        let id = registry.get_from_path(&path);
        assert!(id.is_none());
    }

    #[test]
    fn test_remove() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/test.java");

        let id = registry.get_or_create_from_path(&path);
        assert_eq!(registry.len(), 1);

        registry.remove(id);
        assert_eq!(registry.len(), 0);
        assert!(registry.get_path(id).is_none());
    }

    #[test]
    fn published_move_relocates_an_editor_binding_and_retires_destination() {
        let registry = FileIdRegistry::new();
        let old_path = PathBuf::from("/tmp/old.java");
        let new_path = PathBuf::from("/tmp/new.java");
        let moved_id = registry.get_or_create_from_path(&old_path);
        let displaced_id = registry.get_or_create_from_path(&new_path);

        assert_eq!(
            registry.relocate_published_path(&old_path, &new_path, moved_id),
            vec![displaced_id]
        );

        assert!(registry.get_from_path(&old_path).is_none());
        assert_eq!(registry.get_from_path(&new_path), Some(moved_id));
        assert_eq!(registry.get_path(moved_id), Some(new_path));
        assert!(registry.get_path(displaced_id).is_none());
    }

    #[test]
    fn published_move_keeps_a_cold_workspace_identity_unregistered() {
        let registry = FileIdRegistry::new();
        let old_path = PathBuf::from("/tmp/cold-old.java");
        let new_path = PathBuf::from("/tmp/cold-new.java");
        let workspace_id = FileId::new(41);

        assert!(registry
            .relocate_published_path(&old_path, &new_path, workspace_id)
            .is_empty());

        assert!(registry.is_empty());
        assert!(registry.get_path(workspace_id).is_none());
    }

    #[test]
    fn path_case_identity_follows_the_test_volume() {
        use std::fs::OpenOptions;

        let directory = tempfile::tempdir().unwrap();
        let upper = directory.path().join("RegistryCase.java");
        let lower = directory.path().join("registrycase.java");
        std::fs::write(&upper, b"class Upper {}").unwrap();
        let case_sensitive = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lower)
            .is_ok();
        let registry = FileIdRegistry::new();
        let id1 = registry.get_or_create_from_path(&upper);
        let id2 = registry.get_or_create_from_path(&lower);
        if case_sensitive {
            assert_ne!(id1, id2);
            assert_eq!(registry.len(), 2);
        } else {
            assert_eq!(id1, id2);
            assert_eq!(registry.len(), 1);
        }
    }

    #[test]
    fn same_key_relocation_repairs_a_stale_closed_binding() {
        let registry = FileIdRegistry::new();
        let path = PathBuf::from("/tmp/case-only.java");
        let stale_id = registry.get_or_create_from_path(&path);
        let published_id = FileId::new(57);

        assert_eq!(
            registry.relocate_published_path(&path, &path, published_id),
            vec![stale_id]
        );
        assert_eq!(registry.get_from_path(&path), Some(published_id));
        assert!(registry.get_path(stale_id).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_alias_can_retire_its_stored_binding() {
        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("real");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("Aliased.java"), b"class Aliased {}").unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let aliased_file = alias.join("Aliased.java");
        let registry = FileIdRegistry::new();
        let file_id = registry.get_or_create_from_path(&aliased_file);

        std::fs::remove_file(&alias).unwrap();

        assert_eq!(registry.get_from_path(&aliased_file), Some(file_id));
        assert_eq!(registry.remove_path(&aliased_file), Some(file_id));
        assert!(registry.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn reopening_a_broken_alias_rebinds_its_id_to_the_current_missing_key() {
        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("real");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        let real_file = real.join("Aliased.java");
        std::fs::write(&real_file, b"class Aliased {}").unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let aliased_file = alias.join("Aliased.java");
        let registry = FileIdRegistry::new();
        let original_id = registry.get_or_create_from_path(&aliased_file);

        std::fs::remove_file(&alias).unwrap();
        let reopened_id = registry.get_or_create_from_path(&aliased_file);

        assert_eq!(reopened_id, original_id);
        assert_eq!(registry.get_from_path(&aliased_file), Some(original_id));
        assert_eq!(registry.get_from_path(&real_file), None);
        assert_eq!(registry.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn retargeted_alias_resolves_to_the_current_owner_before_display_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let new = directory.path().join("new");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("Owner.java"), b"class Old {}").unwrap();
        std::fs::write(new.join("Owner.java"), b"class New {}").unwrap();
        std::os::unix::fs::symlink(&old, &alias).unwrap();

        let registry = FileIdRegistry::new();
        let stale_id = registry.get_or_create_from_path(&alias.join("Owner.java"));
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&new, &alias).unwrap();
        let current_id = registry.get_or_create_from_path(&new.join("Owner.java"));

        assert_ne!(stale_id, current_id);
        assert_eq!(
            registry.get_from_path(&alias.join("Owner.java")),
            Some(current_id)
        );
    }

    #[cfg(unix)]
    #[test]
    fn established_key_lookup_does_not_follow_a_retargeted_alias() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let new = directory.path().join("new");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("Owner.java"), b"class Old {}").unwrap();
        std::fs::write(new.join("Owner.java"), b"class New {}").unwrap();
        std::os::unix::fs::symlink(&old, &alias).unwrap();
        let aliased_file = alias.join("Owner.java");
        let established_key = registry_key(&aliased_file);
        let registry = FileIdRegistry::new();
        let old_id = registry.get_or_create_from_path_with_key_and_preferred_id(
            &aliased_file,
            established_key.clone(),
            None,
        );

        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&new, &alias).unwrap();

        assert_eq!(
            registry.get_from_path_with_key(&aliased_file, &established_key),
            Some(old_id)
        );
        assert_eq!(registry.get_from_path(&aliased_file), None);
        assert_eq!(
            registry.remove_path_with_key(&aliased_file, &established_key),
            Some(old_id)
        );
        assert!(registry.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn retarget_registration_retains_both_real_owners_and_ambiguous_fallback_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let new = directory.path().join("new");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let old_file = old.join("Owner.java");
        let new_file = new.join("Owner.java");
        std::fs::write(&old_file, b"class Old {}").unwrap();
        std::fs::write(&new_file, b"class New {}").unwrap();
        std::os::unix::fs::symlink(&old, &alias).unwrap();
        let aliased_file = alias.join("Owner.java");
        let registry = FileIdRegistry::new();
        let old_id = registry.get_or_create_from_path(&aliased_file);

        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&new, &alias).unwrap();
        let new_id = registry.get_or_create_from_path(&aliased_file);

        assert_ne!(old_id, new_id);
        assert_eq!(registry.get_from_path(&old_file), Some(old_id));
        assert_eq!(registry.get_from_path(&new_file), Some(new_id));

        std::fs::remove_file(&alias).unwrap();
        assert_eq!(registry.get_from_path(&aliased_file), None);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let registry = Arc::new(FileIdRegistry::new());
        let mut handles = vec![];

        // Spawn multiple threads that all try to register the same path
        for i in 0..10 {
            let registry = Arc::clone(&registry);
            let handle = thread::spawn(move || {
                let path = PathBuf::from(format!("/tmp/test{}.java", i % 3));
                registry.get_or_create_from_path(&path)
            });
            handles.push(handle);
        }

        // Collect all FileIds
        let ids: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Should only have 3 unique IDs (since we used i % 3)
        let mut unique_ids: Vec<_> = ids.clone();
        unique_ids.sort_by_key(|id| id.as_u32());
        unique_ids.dedup();

        assert_eq!(unique_ids.len(), 3);
    }
}
