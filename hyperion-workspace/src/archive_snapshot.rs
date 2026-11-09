// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded immutable archive snapshots.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Cheap, strong identity of one regular archive file generation.
///
/// This deliberately describes the opened file rather than just its pathname.
/// Dependency resolution can retain it without copying or hashing the archive,
/// then require a later immutable snapshot to observe the same generation both
/// before and after copying the bytes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchiveFileVersion {
    platform: PlatformFileVersion,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PlatformFileVersion {
    device: u64,
    inode: u64,
    bytes: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PlatformFileVersion {
    volume_serial_number: u32,
    file_index: u64,
    bytes: u64,
    creation_time: u64,
    last_write_time: u64,
}

#[cfg(not(any(unix, windows)))]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PlatformFileVersion {
    bytes: u64,
    created: Option<std::time::SystemTime>,
    modified: Option<std::time::SystemTime>,
    readonly: bool,
}

impl ArchiveFileVersion {
    /// Capture the generation currently selected by `path`, following a stable
    /// pre-existing symlink exactly once.
    pub fn capture(path: &Path, maximum_bytes: u64) -> Result<Self, String> {
        let source = open_regular_file(path)
            .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
        Self::from_open_file(path, maximum_bytes, &source)
    }

    pub(crate) fn capture_exact(path: &Path, maximum_bytes: u64) -> Result<Self, String> {
        let source = open_regular_file_exact(path)
            .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
        Self::from_open_file(path, maximum_bytes, &source)
    }

    /// Check the generation currently selected by an already-canonical path
    /// without reading its contents.
    pub(crate) fn exact_path_is_current(self, path: &Path) -> bool {
        Self::capture_exact(path, self.bytes()).is_ok_and(|current| current == self)
    }

    fn from_open_file(path: &Path, maximum_bytes: u64, source: &File) -> Result<Self, String> {
        let metadata = source
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("archive {} is not a regular file", path.display()));
        }
        let version = Self::from_metadata(&metadata).map_err(|error| {
            format!(
                "failed to identify archive generation for {}: {error}",
                path.display()
            )
        })?;
        if !version.is_strong_for_staging() {
            return Err(format!(
                "archive {} is on a platform without a strong cheap file identity; use a content snapshot instead",
                path.display()
            ));
        }
        if version.bytes() > maximum_bytes {
            return Err(format!(
                "archive {} is {} bytes (maximum {maximum_bytes})",
                path.display(),
                version.bytes()
            ));
        }
        Ok(version)
    }

    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.platform.bytes()
    }

    #[cfg(any(unix, windows))]
    const fn is_strong_for_staging(self) -> bool {
        true
    }

    #[cfg(not(any(unix, windows)))]
    const fn is_strong_for_staging(_self: Self) -> bool {
        // Timestamps are useful for detecting ordinary mutation around a
        // one-shot snapshot, but without a stable file ID they cannot prove
        // that a pathname still names the same file generation.
        false
    }

    #[cfg(unix)]
    fn from_metadata(metadata: &std::fs::Metadata) -> Result<Self, &'static str> {
        use std::os::unix::fs::MetadataExt;

        Ok(Self {
            platform: PlatformFileVersion {
                device: metadata.dev(),
                inode: metadata.ino(),
                bytes: metadata.size(),
                modified_seconds: metadata.mtime(),
                modified_nanoseconds: metadata.mtime_nsec(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            },
        })
    }

    #[cfg(windows)]
    fn from_metadata(metadata: &std::fs::Metadata) -> Result<Self, &'static str> {
        use std::os::windows::fs::MetadataExt;

        let volume_serial_number = metadata
            .volume_serial_number()
            .ok_or("volume serial number is unavailable")?;
        let file_index = metadata.file_index().ok_or("file index is unavailable")?;
        Ok(Self {
            platform: PlatformFileVersion {
                volume_serial_number,
                file_index,
                bytes: metadata.file_size(),
                creation_time: metadata.creation_time(),
                last_write_time: metadata.last_write_time(),
            },
        })
    }

    #[cfg(not(any(unix, windows)))]
    fn from_metadata(metadata: &std::fs::Metadata) -> Result<Self, &'static str> {
        // There is no portable inode/file-ID API. Ordinary one-shot snapshots
        // can still compare every field the filesystem provides. Staged
        // generation capture separately rejects this platform and directs the
        // caller to the content-hashed API.
        Ok(Self {
            platform: PlatformFileVersion {
                bytes: metadata.len(),
                created: metadata.created().ok(),
                modified: metadata.modified().ok(),
                readonly: metadata.permissions().readonly(),
            },
        })
    }
}

impl PlatformFileVersion {
    const fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Stable byte identity of one bounded archive snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveContentIdentity {
    bytes: u64,
    sha256: [u8; 32],
}

impl ArchiveContentIdentity {
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub const fn sha256(self) -> [u8; 32] {
        self.sha256
    }
}

/// A private, move-only copy of one archive.
///
/// An open descriptor is not immutable when another process truncates the
/// original file in place. Copying into an anonymous temporary file makes the
/// bytes stable for the complete extraction transaction and leaves no path to
/// clean up after a crash.
#[derive(Debug)]
pub struct ArchiveSnapshot {
    display_path: PathBuf,
    file: File,
    content_identity: ArchiveContentIdentity,
}

impl ArchiveSnapshot {
    pub fn capture(path: &Path, maximum_bytes: u64) -> Result<Self, String> {
        let source = open_regular_file(path)
            .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
        Self::capture_open_file(path, maximum_bytes, source, None)
    }

    /// Snapshot a path which is already the canonical identity selected by a
    /// caller's transaction.
    ///
    /// Unlike [`Self::capture`], this never follows a replacement leaf
    /// symlink. Re-canonicalizing here would let an archive captured as `A`
    /// silently become `B` if `A` were replaced by a symlink between planning
    /// and extraction.
    pub(crate) fn capture_exact(path: &Path, maximum_bytes: u64) -> Result<Self, String> {
        let source = open_regular_file_exact(path)
            .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
        Self::capture_open_file(path, maximum_bytes, source, None)
    }

    /// Snapshot exactly the regular-file generation staged earlier by a
    /// dependency resolver. The expected version is checked on the opened
    /// descriptor both before and after the copy, so pathname replacement and
    /// in-place mutation fail closed without a preliminary content read.
    pub(crate) fn capture_exact_versioned(
        path: &Path,
        maximum_bytes: u64,
        expected: ArchiveFileVersion,
    ) -> Result<Self, String> {
        let source = open_regular_file_exact(path)
            .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
        Self::capture_open_file(path, maximum_bytes, source, Some(expected))
    }

    fn capture_open_file(
        path: &Path,
        maximum_bytes: u64,
        source: File,
        expected: Option<ArchiveFileVersion>,
    ) -> Result<Self, String> {
        let metadata = source
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("archive {} is not a regular file", path.display()));
        }
        let declared_size = metadata.len();
        if declared_size > maximum_bytes {
            return Err(format!(
                "archive {} is {declared_size} bytes (maximum {maximum_bytes})",
                path.display()
            ));
        }
        let initial_version = ArchiveFileVersion::from_metadata(&metadata).map_err(|error| {
            format!(
                "failed to identify archive generation for {}: {error}",
                path.display()
            )
        })?;
        if expected.is_some_and(|expected| expected != initial_version) {
            return Err(format!(
                "archive {} changed after dependency resolution",
                path.display()
            ));
        }

        let mut file = tempfile::tempfile().map_err(|error| {
            format!(
                "failed to create a private snapshot for {}: {error}",
                path.display()
            )
        })?;
        let mut source = source.take(maximum_bytes.saturating_add(1));
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| format!("failed to snapshot {}: {error}", path.display()))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|error| format!("failed to snapshot {}: {error}", path.display()))?;
            hasher.update(&buffer[..read]);
            bytes = bytes
                .checked_add(read as u64)
                .ok_or_else(|| format!("archive {} byte count overflowed", path.display()))?;
        }
        if bytes > maximum_bytes {
            return Err(format!(
                "archive {} grew beyond {maximum_bytes} bytes while being snapshotted",
                path.display()
            ));
        }
        if bytes != declared_size {
            return Err(format!(
                "archive {} changed size while being snapshotted (expected {declared_size}, read {bytes})",
                path.display()
            ));
        }
        let final_metadata = source
            .get_ref()
            .metadata()
            .map_err(|error| format!("failed to re-inspect {}: {error}", path.display()))?;
        let final_version =
            ArchiveFileVersion::from_metadata(&final_metadata).map_err(|error| {
                format!(
                    "failed to re-identify archive generation for {}: {error}",
                    path.display()
                )
            })?;
        if initial_version != final_version
            || expected.is_some_and(|expected| expected != final_version)
        {
            return Err(format!(
                "archive {} changed while being snapshotted",
                path.display()
            ));
        }
        file.flush()
            .map_err(|error| format!("failed to flush snapshot for {}: {error}", path.display()))?;
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            format!("failed to rewind snapshot for {}: {error}", path.display())
        })?;
        Ok(Self {
            display_path: path.to_path_buf(),
            file,
            content_identity: ArchiveContentIdentity {
                bytes,
                sha256: hasher.finalize().into(),
            },
        })
    }

    #[must_use]
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.content_identity.bytes
    }

    #[must_use]
    pub const fn content_identity(&self) -> ArchiveContentIdentity {
        self.content_identity
    }

    /// Consume the snapshot and transfer its private file to an archive reader.
    pub fn into_file(self) -> File {
        self.file
    }
}

fn open_regular_file(path: &Path) -> std::io::Result<File> {
    // Following a pre-existing symlink is intentional: JDK installations and
    // artifact caches commonly expose archives through symlinks. Resolve that
    // alias once, then reject a last-component substitution at open time.
    let resolved = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&resolved)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "archive is not a regular file",
        ));
    }

    open_regular_file_exact(&resolved)
}

fn open_regular_file_exact(path: &Path) -> std::io::Result<File> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "archive is not a regular file",
        ));
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A path can be replaced by a FIFO after the metadata probe. A
        // non-blocking open lets the descriptor check above reject that race
        // without waiting forever for a writer. Regular-file reads are
        // unaffected by O_NONBLOCK.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Do not follow a replacement leaf reparse point between the metadata
        // probe and open. The opened handle's metadata is checked again below.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn fifo_is_rejected_without_waiting_for_a_writer() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::sync::mpsc;
        use std::time::Duration;

        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("archive.fifo");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `fifo_name` is a live NUL-terminated path and the mode is a
        // valid permission mask. The return value is checked before use.
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);

        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            result_tx
                .send(ArchiveSnapshot::capture(&fifo, 1024))
                .unwrap();
        });
        let error = result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("FIFO validation must not wait for a writer")
            .unwrap_err();
        worker.join().unwrap();

        assert!(error.contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn stable_symlink_to_regular_archive_is_accepted() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("archive.jar");
        let alias = temp.path().join("archive-link.jar");
        std::fs::write(&archive, b"immutable archive bytes").unwrap();
        symlink(&archive, &alias).unwrap();

        let snapshot = ArchiveSnapshot::capture(&alias, 1024).unwrap();

        assert_eq!(snapshot.bytes(), 23);
        assert_eq!(snapshot.display_path(), alias);
    }
}
