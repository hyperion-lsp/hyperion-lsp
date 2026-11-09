// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::{path_lookup_key, PathLookupKey};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The virtual identity used by cached, installation-wide JDK publications.
///
/// Existing public APIs represent every external source as a `PathBuf`, so the
/// historical `PathBuf::from("jdk")` sentinel remains recognized here. Keeping
/// that compatibility decision in one constructor prevents ordinary archive
/// code from accidentally treating virtual names as filesystem paths.
const JDK_VIRTUAL_SOURCE: &str = "jdk";

/// Exact identity of one external declaration source.
///
/// Archive paths use the same filesystem-aware key as workspace source files.
/// Virtual sources occupy a separate namespace, except for the legacy `"jdk"`
/// path sentinel recognized internally. No archive can collide with a virtual
/// name through lossy string conversion.
///
/// Values returned by [`crate::WorkspaceIndex`] can be retained by source
/// providers without reducing filesystem paths to UTF-8 strings. Providers
/// may also construct the matching archive or virtual identity explicitly.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExternalSourceIdentity(ExternalSourceIdentityKind);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ExternalSourceIdentityKind {
    Virtual(Arc<str>),
    Archive(PathLookupKey),
}

/// One archive path and the exact filesystem identity captured for it.
///
/// Keep this value through a classpath refresh instead of reconstructing an
/// identity from the path at each stage. If a symlink is retargeted after
/// capture, revision validation fails closed rather than silently assigning
/// the new target to the old publication.
#[derive(Clone, Debug)]
pub struct ExternalSourceArchive {
    identity: ExternalSourceIdentity,
    path: Arc<Path>,
    expected_file_version: Option<crate::ArchiveFileVersion>,
    expected_content: Option<crate::ArchiveContentIdentity>,
}

/// Immutable bytes for one captured external archive identity.
#[derive(Debug)]
pub struct ExternalArchiveSnapshot {
    archive: ExternalSourceArchive,
    snapshot: crate::ArchiveSnapshot,
}

impl ExternalSourceArchive {
    #[must_use]
    pub fn capture(path: &Path) -> Self {
        let identity = ExternalSourceIdentity::for_archive_path(path);
        let archive_key = identity
            .archive_key()
            .expect("an explicitly constructed archive identity must remain an archive");
        let path = if archive_key.as_path() == path {
            archive_key.shared_path()
        } else {
            Arc::from(path)
        };
        Self {
            identity,
            path,
            expected_file_version: None,
            expected_content: None,
        }
    }

    /// Capture a cheap exact regular-file generation without reading, hashing,
    /// or copying its contents. A later [`Self::snapshot_contents`] accepts only
    /// this same file generation and revalidates it around the byte copy.
    pub fn capture_staged(path: &Path, maximum_bytes: u64) -> Result<Self, String> {
        Self::capture(path).into_staged(maximum_bytes)
    }

    /// Bind this archive's already-canonical identity to its current cheap file
    /// version. This is intended for dependency plans which parse the archive
    /// later and want to avoid a throwaway preliminary content snapshot.
    pub fn into_staged(mut self, maximum_bytes: u64) -> Result<Self, String> {
        let path = self
            .identity
            .archive_path()
            .expect("an ExternalSourceArchive always has an archive identity");
        self.expected_file_version = Some(crate::ArchiveFileVersion::capture_exact(
            path,
            maximum_bytes,
        )?);
        Ok(self)
    }

    /// Capture both the stable path key and the exact bytes selected by a
    /// dependency resolver. Later snapshots fail closed if the same pathname is
    /// replaced before parsing or publication.
    pub fn capture_versioned(path: &Path, maximum_bytes: u64) -> Result<Self, String> {
        Self::capture(path).into_versioned(maximum_bytes)
    }

    /// Bind an already-captured canonical identity to its current bytes.
    ///
    /// This deliberately opens the identity's canonical target rather than
    /// recapturing `self.path`, which may be a resolver-provided symlink alias.
    pub fn into_versioned(mut self, maximum_bytes: u64) -> Result<Self, String> {
        let snapshot = self.snapshot_contents(maximum_bytes)?;
        self.expected_content = Some(snapshot.content_identity());
        Ok(self)
    }

    /// Capture a versioned archive and retain the immutable bytes used to
    /// identify that version.
    ///
    /// This is the preferred constructor when the caller will parse the
    /// archive immediately. It avoids hashing one read and parsing a later
    /// read: the returned plan identity and snapshot describe the same private
    /// file byte-for-byte.
    pub fn capture_versioned_snapshot(
        path: &Path,
        maximum_bytes: u64,
    ) -> Result<(Self, ExternalArchiveSnapshot), String> {
        Self::capture(path).into_versioned_snapshot(maximum_bytes)
    }

    /// Bind an already-captured canonical identity to one retained immutable
    /// snapshot of its current bytes.
    pub fn into_versioned_snapshot(
        mut self,
        maximum_bytes: u64,
    ) -> Result<(Self, ExternalArchiveSnapshot), String> {
        let snapshot = self.snapshot_contents(maximum_bytes)?;
        self.expected_content = Some(snapshot.content_identity());
        let captured = ExternalArchiveSnapshot {
            archive: self.clone(),
            snapshot,
        };
        Ok((self, captured))
    }

    #[must_use]
    pub fn identity(&self) -> &ExternalSourceIdentity {
        &self.identity
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn expected_content(&self) -> Option<crate::ArchiveContentIdentity> {
        self.expected_content
    }

    #[must_use]
    pub const fn expected_file_version(&self) -> Option<crate::ArchiveFileVersion> {
        self.expected_file_version
    }

    /// Whether this archive's canonical path still selects the exact regular
    /// file generation captured by [`Self::capture_staged`].
    ///
    /// This is metadata-only: it does not read or hash archive contents. An
    /// archive that was not staged with a strong platform file identity fails
    /// closed rather than being treated as current.
    #[must_use]
    pub fn staged_version_is_current(&self) -> bool {
        let Some(expected) = self.expected_file_version else {
            return false;
        };
        let path = self
            .identity
            .archive_path()
            .expect("an ExternalSourceArchive always has an archive identity");
        expected.exact_path_is_current(path)
    }

    /// Copy the captured canonical target into a bounded private snapshot.
    ///
    /// Source aliases are used only for display. Opening the identity's
    /// canonical target prevents a later symlink retarget from changing which
    /// bytes this transaction owns.
    pub fn snapshot(&self, maximum_bytes: u64) -> Result<ExternalArchiveSnapshot, String> {
        let snapshot = self.snapshot_contents(maximum_bytes)?;
        Ok(ExternalArchiveSnapshot {
            archive: self.clone(),
            snapshot,
        })
    }

    /// Copy this archive's captured canonical target without registering it as
    /// a declaration source.
    ///
    /// Optional attachments such as Javadoc archives need the same alias-safe
    /// byte ownership as binary archives, but have no workspace `FileId` of
    /// their own.
    pub fn snapshot_contents(&self, maximum_bytes: u64) -> Result<crate::ArchiveSnapshot, String> {
        let path = self
            .identity
            .archive_path()
            .expect("an ExternalSourceArchive always has an archive identity");
        let snapshot = match self.expected_file_version {
            Some(expected) => {
                crate::ArchiveSnapshot::capture_exact_versioned(path, maximum_bytes, expected)?
            }
            None => crate::ArchiveSnapshot::capture_exact(path, maximum_bytes)?,
        };
        if self
            .expected_content
            .is_some_and(|expected| expected != snapshot.content_identity())
        {
            return Err(format!(
                "archive {} changed after dependency resolution",
                self.path.display()
            ));
        }
        Ok(snapshot)
    }
}

impl ExternalArchiveSnapshot {
    #[must_use]
    pub fn archive(&self) -> &ExternalSourceArchive {
        &self.archive
    }

    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.snapshot.bytes()
    }

    /// Stable identity of the exact private bytes this snapshot owns.
    #[must_use]
    pub const fn content_identity(&self) -> crate::ArchiveContentIdentity {
        self.snapshot.content_identity()
    }

    pub(super) fn into_parts(self) -> (ExternalSourceArchive, crate::ArchiveSnapshot) {
        (self.archive, self.snapshot)
    }
}

impl ExternalSourceIdentity {
    /// Construct an exact filesystem archive identity.
    ///
    /// This always creates an archive, including for a relative path named
    /// `"jdk"`; the historical virtual sentinel is not part of this API.
    pub fn for_archive_path(path: &Path) -> Self {
        Self(ExternalSourceIdentityKind::Archive(path_lookup_key(path)))
    }

    /// Construct an explicit virtual external-source identity.
    pub fn for_virtual(name: &str) -> Self {
        Self(ExternalSourceIdentityKind::Virtual(Arc::from(name)))
    }

    pub(super) fn for_legacy_path(path: &Path) -> Self {
        if path == Path::new(JDK_VIRTUAL_SOURCE) {
            Self::for_virtual(JDK_VIRTUAL_SOURCE)
        } else {
            Self::for_archive_path(path)
        }
    }

    /// Filesystem-aware key for an archive source, if this is not virtual.
    pub fn archive_key(&self) -> Option<&PathLookupKey> {
        match &self.0 {
            ExternalSourceIdentityKind::Archive(key) => Some(key),
            ExternalSourceIdentityKind::Virtual(_) => None,
        }
    }

    /// Exact normalized archive path represented by this identity.
    pub fn archive_path(&self) -> Option<&Path> {
        self.archive_key().map(PathLookupKey::as_path)
    }

    /// Virtual source name, if this identity does not represent an archive.
    pub fn virtual_name(&self) -> Option<&str> {
        match &self.0 {
            ExternalSourceIdentityKind::Virtual(name) => Some(name),
            ExternalSourceIdentityKind::Archive(_) => None,
        }
    }
}

/// Exact identity plus the spelling retained for diagnostics and legacy
/// string-facing callers.
///
/// Equality and hashing happen only through [`ExternalSourceIdentity`]. The
/// display path therefore never participates in identity, and can preserve
/// non-UTF-8 filesystem spelling without a lossy intermediate conversion.
#[derive(Clone, Debug)]
pub(crate) struct RegisteredExternalSource {
    key: ExternalSourceIdentity,
    display: ExternalSourceDisplay,
}

#[derive(Clone, Debug)]
enum ExternalSourceDisplay {
    Virtual(Arc<str>),
    Archive(Arc<Path>),
}

impl RegisteredExternalSource {
    pub(super) fn for_archive(archive: &ExternalSourceArchive) -> Self {
        Self {
            key: archive.identity.clone(),
            display: ExternalSourceDisplay::Archive(Arc::clone(&archive.path)),
        }
    }

    pub(super) fn for_legacy_path(path: &Path) -> Self {
        if path == Path::new(JDK_VIRTUAL_SOURCE) {
            return Self::virtual_source(JDK_VIRTUAL_SOURCE);
        }

        let key = ExternalSourceIdentity::for_archive_path(path);
        let archive_key = key
            .archive_key()
            .expect("an explicitly constructed archive identity must remain an archive");
        let display = if archive_key.as_path() == path {
            archive_key.shared_path()
        } else {
            Arc::from(path)
        };
        Self {
            key,
            display: ExternalSourceDisplay::Archive(display),
        }
    }

    pub(super) fn virtual_source(name: &str) -> Self {
        let name: Arc<str> = Arc::from(name);
        Self {
            key: ExternalSourceIdentity(ExternalSourceIdentityKind::Virtual(Arc::clone(&name))),
            display: ExternalSourceDisplay::Virtual(name),
        }
    }

    pub(super) fn key(&self) -> &ExternalSourceIdentity {
        &self.key
    }

    pub(super) fn display_name(&self) -> String {
        match &self.display {
            ExternalSourceDisplay::Virtual(name) => name.to_string(),
            ExternalSourceDisplay::Archive(path) => path.to_string_lossy().into_owned(),
        }
    }

    pub(super) fn display_path(&self) -> Option<PathBuf> {
        match &self.display {
            ExternalSourceDisplay::Virtual(_) => None,
            ExternalSourceDisplay::Archive(path) => Some(path.to_path_buf()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_archive_rejects_same_path_content_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("library.jar");
        std::fs::write(&archive_path, b"generation one").unwrap();
        let archive = ExternalSourceArchive::capture_versioned(&archive_path, 1024).unwrap();

        let replacement = temp.path().join("replacement.jar");
        std::fs::write(&replacement, b"generation two").unwrap();
        std::fs::rename(replacement, &archive_path).unwrap();

        let error = archive.snapshot(1024).unwrap_err();
        assert!(error.contains("changed after dependency resolution"));
    }

    #[test]
    fn staged_archive_enforces_the_per_file_byte_bound_without_copying() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("oversized.jar");
        std::fs::write(&archive_path, b"five!").unwrap();

        let error = ExternalSourceArchive::capture_staged(&archive_path, 4).unwrap_err();

        assert!(error.contains("5 bytes (maximum 4)"));
    }

    #[test]
    fn staged_archive_rejects_regular_file_replacement_even_with_identical_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("library.jar");
        std::fs::write(&archive_path, b"same bytes").unwrap();
        let archive = ExternalSourceArchive::capture_staged(&archive_path, 1024).unwrap();
        assert!(archive.staged_version_is_current());

        let replacement = temp.path().join("replacement.jar");
        std::fs::write(&replacement, b"same bytes").unwrap();
        #[cfg(windows)]
        std::fs::remove_file(&archive_path).unwrap();
        std::fs::rename(replacement, &archive_path).unwrap();

        assert!(!archive.staged_version_is_current());
        let error = archive.snapshot_contents(1024).unwrap_err();
        assert!(error.contains("changed after dependency resolution"));
    }

    #[test]
    fn staged_archive_rejects_same_length_in_place_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("library.jar");
        std::fs::write(&archive_path, b"generation-A").unwrap();
        let archive = ExternalSourceArchive::capture_staged(&archive_path, 1024).unwrap();

        std::fs::write(&archive_path, b"generation-B").unwrap();

        let error = archive.snapshot_contents(1024).unwrap_err();
        assert!(error.contains("changed after dependency resolution"));
    }

    #[cfg(unix)]
    #[test]
    fn staged_archive_accepts_a_stable_symlink_but_ignores_alias_retargeting() {
        use std::io::Read;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.jar");
        let second = temp.path().join("second.jar");
        let alias = temp.path().join("library.jar");
        std::fs::write(&first, b"first generation").unwrap();
        std::fs::write(&second, b"second generation").unwrap();
        symlink(&first, &alias).unwrap();
        let archive = ExternalSourceArchive::capture_staged(&alias, 1024).unwrap();

        std::fs::remove_file(&alias).unwrap();
        symlink(&second, &alias).unwrap();

        let mut snapshot = archive.snapshot_contents(1024).unwrap().into_file();
        let mut bytes = Vec::new();
        snapshot.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"first generation");
    }

    #[cfg(unix)]
    #[test]
    fn captured_canonical_target_cannot_be_replaced_by_a_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.jar");
        let other = temp.path().join("other.jar");
        let alias = temp.path().join("alias.jar");
        std::fs::write(&target, b"target").unwrap();
        std::fs::write(&other, b"other").unwrap();
        symlink(&target, &alias).unwrap();
        let archive = ExternalSourceArchive::capture_staged(&alias, 1024).unwrap();

        std::fs::remove_file(&target).unwrap();
        symlink(&other, &target).unwrap();

        assert!(archive.snapshot(1024).is_err());
    }
}
