// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Session-scoped materialization of immutable archive source.

use fs2::FileExt;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

/// Complete materialized source retained for one LSP session.
///
/// Staged writes participate in the same limits, so Hyperion itself uses at
/// most 256 MiB and 4,096 materialized/staging source files per session,
/// including an in-flight publication. The lease adds one tiny file.
const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_FILES: u64 = 4_096;
const DEFAULT_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const ABANDONED_SESSION_AGE: Duration = Duration::from_hours(168);
const STORE_DIRECTORY: &str = "hyperion-materialized-sources-v1";
const SESSION_PREFIX: &str = "session-";
const LEASE_FILE: &str = ".lease";
const FILES_DIRECTORY: &str = "files";
const ARCHIVES_DIRECTORY: &str = "archives";

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub(super) enum AttachmentLanguage {
    Java,
}

impl AttachmentLanguage {
    const fn extension(self) -> &'static str {
        match self {
            Self::Java => "java",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MaterializedSourceLimits {
    total_bytes: u64,
    file_count: u64,
    per_file_bytes: u64,
}

impl Default for MaterializedSourceLimits {
    fn default() -> Self {
        Self {
            total_bytes: DEFAULT_MAX_BYTES,
            file_count: DEFAULT_MAX_FILES,
            per_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct MaterializedSourceKey {
    digest: [u8; 32],
    language: AttachmentLanguage,
}

#[derive(Default)]
struct StoreState {
    published: HashSet<MaterializedSourceKey>,
    accounted_bytes: u64,
    accounted_files: u64,
    next_staging_file: u64,
}

impl StoreState {
    fn reserve(
        &mut self,
        bytes: u64,
        files: u64,
        limits: MaterializedSourceLimits,
    ) -> Result<(), MaterializationError> {
        let accounted_bytes = self
            .accounted_bytes
            .checked_add(bytes)
            .ok_or(MaterializationError::QuotaExceeded { limits })?;
        let accounted_files = self
            .accounted_files
            .checked_add(files)
            .ok_or(MaterializationError::QuotaExceeded { limits })?;
        if accounted_bytes > limits.total_bytes || accounted_files > limits.file_count {
            return Err(MaterializationError::QuotaExceeded { limits });
        }
        self.accounted_bytes = accounted_bytes;
        self.accounted_files = accounted_files;
        Ok(())
    }

    const fn release(&mut self, bytes: u64, files: u64) {
        self.accounted_bytes = self
            .accounted_bytes
            .checked_sub(bytes)
            .expect("released bytes were reserved");
        self.accounted_files = self
            .accounted_files
            .checked_sub(files)
            .expect("released files were reserved");
    }
}

#[derive(Debug)]
enum MaterializationError {
    StoreUnavailable(Arc<str>),
    FileTooLarge {
        actual: u64,
        maximum: u64,
    },
    QuotaExceeded {
        limits: MaterializedSourceLimits,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for MaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreUnavailable(error) => {
                write!(
                    formatter,
                    "materialized source store is unavailable: {error}"
                )
            }
            Self::FileTooLarge { actual, maximum } => write!(
                formatter,
                "source attachment is {actual} bytes (maximum {maximum})"
            ),
            Self::QuotaExceeded { limits } => write!(
                formatter,
                "materialized source quota exhausted ({} bytes, {} files)",
                limits.total_bytes, limits.file_count
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} materialized source {}: {source}",
                path.display()
            ),
        }
    }
}

/// Owns every file URI returned by archive source providers in one LSP
/// session.
///
/// Providers may be replaced without invalidating prior responses;
/// the complete tree is removed only when the last session owner is dropped.
pub struct MaterializedSourceStore {
    root: Option<PathBuf>,
    lease: Option<File>,
    initialization_error: Option<Arc<str>>,
    limits: MaterializedSourceLimits,
    state: Mutex<StoreState>,
}

impl MaterializedSourceStore {
    pub(super) fn new() -> Self {
        match Self::create_in(
            &std::env::temp_dir().join(STORE_DIRECTORY),
            MaterializedSourceLimits::default(),
            SystemTime::now(),
            ABANDONED_SESSION_AGE,
        ) {
            Ok(store) => store,
            Err(error) => {
                warn!("Failed to initialize materialized source store: {error}");
                Self {
                    root: None,
                    lease: None,
                    initialization_error: Some(Arc::from(error.to_string())),
                    limits: MaterializedSourceLimits::default(),
                    state: Mutex::new(StoreState::default()),
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn new_for_test(base: &Path) -> Self {
        Self::create_in(
            base,
            MaterializedSourceLimits::default(),
            UNIX_EPOCH + Duration::from_secs(10_000),
            ABANDONED_SESSION_AGE,
        )
        .expect("test materialized source store")
    }

    fn create_in(
        base: &Path,
        limits: MaterializedSourceLimits,
        now: SystemTime,
        abandoned_age: Duration,
    ) -> io::Result<Self> {
        create_restricted_directory(base)?;
        scavenge_abandoned_sessions(base, now, abandoned_age)?;

        let root = create_session_directory(base)?;
        let mut construction = SessionConstruction::new(root.clone());
        let lease_path = root.join(LEASE_FILE);
        let mut lease = create_restricted_file(&lease_path)?;
        FileExt::lock_exclusive(&lease)?;
        let created_at = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        writeln!(lease, "{created_at}")?;
        lease.sync_all()?;
        create_restricted_directory(&root.join(FILES_DIRECTORY))?;
        create_restricted_directory(&root.join(ARCHIVES_DIRECTORY))?;
        construction.keep();

        Ok(Self {
            root: Some(root),
            lease: Some(lease),
            initialization_error: None,
            limits,
            state: Mutex::new(StoreState::default()),
        })
    }

    pub(super) fn materialize(
        &self,
        content: &[u8],
        language: AttachmentLanguage,
    ) -> Result<PathBuf, String> {
        self.materialize_inner(content, language)
            .map_err(|error| error.to_string())
    }

    /// Private directory for closeable archive snapshots retained by source
    /// providers. Files are removed on normal provider drop, while the session
    /// lease/scavenger also makes crash leftovers recoverable.
    pub(super) fn archive_snapshot_directory(&self) -> Result<PathBuf, String> {
        self.root
            .as_ref()
            .map(|root| root.join(ARCHIVES_DIRECTORY))
            .ok_or_else(|| {
                self.initialization_error
                    .as_deref()
                    .unwrap_or("unknown initialization failure")
                    .to_string()
            })
    }

    fn materialize_inner(
        &self,
        content: &[u8],
        language: AttachmentLanguage,
    ) -> Result<PathBuf, MaterializationError> {
        let root = self.root.as_ref().ok_or_else(|| {
            MaterializationError::StoreUnavailable(
                self.initialization_error
                    .clone()
                    .unwrap_or_else(|| Arc::from("unknown initialization failure")),
            )
        })?;
        let content_bytes =
            u64::try_from(content.len()).map_err(|_| MaterializationError::FileTooLarge {
                actual: u64::MAX,
                maximum: self.limits.per_file_bytes,
            })?;
        if content_bytes > self.limits.per_file_bytes {
            return Err(MaterializationError::FileTooLarge {
                actual: content_bytes,
                maximum: self.limits.per_file_bytes,
            });
        }

        let key = MaterializedSourceKey {
            digest: Sha256::digest(content).into(),
            language,
        };
        let destination = materialized_path(root, key);
        // Source lookups are synchronous today. One store mutex deliberately
        // serializes verification, reservation, and atomic publication so no
        // caller can observe a path whose bytes or accounting are provisional.
        let mut state = self.state.lock();
        if state.published.contains(&key) {
            if file_matches(&destination, content).map_err(|source| MaterializationError::Io {
                operation: "verify",
                path: destination.clone(),
                source,
            })? {
                return Ok(destination);
            }

            // Repair needs one staging file in addition to the already
            // accounted destination. If the store is exactly full, fail
            // closed instead of returning a URI whose bytes disagree with the
            // parsed SourceFile.
            state.reserve(content_bytes, 1, self.limits)?;
            let publication =
                publish_atomically(root, &destination, content, &mut state.next_staging_file);
            state.release(content_bytes, 1);
            publication.map_err(|source| MaterializationError::Io {
                operation: "repair",
                path: destination.clone(),
                source,
            })?;
            return Ok(destination);
        }

        state.reserve(content_bytes, 1, self.limits)?;
        if let Err(source) =
            publish_atomically(root, &destination, content, &mut state.next_staging_file)
        {
            state.release(content_bytes, 1);
            return Err(MaterializationError::Io {
                operation: "publish",
                path: destination,
                source,
            });
        }
        let inserted = state.published.insert(key);
        debug_assert!(inserted, "source key was absent before publication");
        Ok(destination)
    }

    #[cfg(test)]
    pub(super) fn root(&self) -> &Path {
        self.root.as_deref().expect("test store initialized")
    }

    #[cfg(test)]
    fn usage(&self) -> (u64, u64) {
        let state = self.state.lock();
        (state.accounted_bytes, state.accounted_files)
    }
}

impl Drop for MaterializedSourceStore {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let _ = FileExt::unlock(&lease);
            drop(lease);
        }
        let Some(root) = self.root.take() else {
            return;
        };
        if let Err(error) = fs::remove_dir_all(&root) {
            if error.kind() != io::ErrorKind::NotFound {
                warn!(
                    "Failed to clean materialized source session {}: {error}",
                    root.display()
                );
            }
        }
    }
}

fn materialized_path(root: &Path, key: MaterializedSourceKey) -> PathBuf {
    root.join(FILES_DIRECTORY).join(format!(
        "{}.{}",
        hex::encode(key.digest),
        key.language.extension()
    ))
}

fn file_matches(path: &Path, expected: &[u8]) -> io::Result<bool> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if metadata.len() != expected.len() as u64 || !metadata.is_file() {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut actual = Vec::with_capacity(expected.len());
    file.read_to_end(&mut actual)?;
    Ok(actual == expected)
}

fn publish_atomically(
    root: &Path,
    destination: &Path,
    content: &[u8],
    next_staging_file: &mut u64,
) -> io::Result<()> {
    if file_matches(destination, content)? {
        return Ok(());
    }

    let mut staged = loop {
        let sequence = *next_staging_file;
        *next_staging_file = next_staging_file
            .checked_add(1)
            .ok_or_else(|| io::Error::other("materialized source staging sequence exhausted"))?;
        let path = root.join(format!(".stage-{sequence:016x}"));
        match StagedFile::create(path) {
            Ok(staged) => break staged,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let staged_file = staged
        .file
        .as_mut()
        .expect("new staged source retains its file handle");
    staged_file.write_all(content)?;
    staged_file.sync_all()?;
    drop(staged.file.take());
    rename_replacing(&staged.path, destination)?;
    staged.published = true;
    Ok(())
}

#[cfg(unix)]
fn rename_replacing(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(not(unix))]
fn rename_replacing(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error)
            if destination.is_file()
                && matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
                ) =>
        {
            fs::remove_file(destination)?;
            fs::rename(source, destination)
        }
        Err(error) => Err(error),
    }
}

struct StagedFile {
    file: Option<File>,
    path: PathBuf,
    published: bool,
}

impl StagedFile {
    fn create(path: PathBuf) -> io::Result<Self> {
        let file = create_restricted_file(&path)?;
        Ok(Self {
            file: Some(file),
            path,
            published: false,
        })
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct SessionConstruction {
    root: PathBuf,
    keep: bool,
}

impl SessionConstruction {
    const fn new(root: PathBuf) -> Self {
        Self { root, keep: false }
    }

    const fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for SessionConstruction {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn create_session_directory(base: &Path) -> io::Result<PathBuf> {
    for _ in 0..32 {
        let sequence = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!(
            "{SESSION_PREFIX}{}-{sequence:016x}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        match create_restricted_leaf_directory(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique materialized source session",
    ))
}

fn create_restricted_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    restrict_directory(path)
}

fn create_restricted_leaf_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)?;
    restrict_directory(path)
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn create_restricted_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

fn scavenge_abandoned_sessions(
    base: &Path,
    now: SystemTime,
    abandoned_age: Duration,
) -> io::Result<()> {
    for entry in fs::read_dir(base)? {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir()
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with(SESSION_PREFIX)
        {
            continue;
        }
        let session = entry.path();
        let lease_path = session.join(LEASE_FILE);
        let Ok(mut lease) = OpenOptions::new().read(true).write(true).open(&lease_path) else {
            continue;
        };
        let Some(created_at) = read_lease_timestamp(&mut lease) else {
            continue;
        };
        let Some(age) = now
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|now| now.checked_sub(Duration::from_secs(created_at)))
        else {
            continue;
        };
        if age < abandoned_age || FileExt::try_lock_exclusive(&lease).is_err() {
            continue;
        }

        // The locked lease proves that no live store owns this unique session
        // directory. Release the handle before removal for Windows; no process
        // can legitimately reacquire an abandoned, randomly named session.
        let _ = FileExt::unlock(&lease);
        drop(lease);
        if let Err(error) = fs::remove_dir_all(&session) {
            if error.kind() != io::ErrorKind::NotFound {
                warn!(
                    "Failed to scavenge abandoned materialized sources {}: {error}",
                    session.display()
                );
            }
        }
    }
    Ok(())
}

fn read_lease_timestamp(lease: &mut File) -> Option<u64> {
    lease.seek(SeekFrom::Start(0)).ok()?;
    let mut timestamp = String::new();
    lease.take(64).read_to_string(&mut timestamp).ok()?;
    timestamp.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store(
        base: &Path,
        max_bytes: u64,
        max_files: u64,
        max_file_bytes: u64,
    ) -> MaterializedSourceStore {
        MaterializedSourceStore::create_in(
            base,
            MaterializedSourceLimits {
                total_bytes: max_bytes,
                file_count: max_files,
                per_file_bytes: max_file_bytes,
            },
            UNIX_EPOCH + Duration::from_secs(10_000),
            ABANDONED_SESSION_AGE,
        )
        .expect("test materialized source store")
    }

    #[test]
    fn quota_refusal_does_not_grow_files_or_accounting() {
        let temp = tempfile::tempdir().unwrap();
        let store = test_store(temp.path(), 3, 1, 3);
        let first = store.materialize(b"abc", AttachmentLanguage::Java).unwrap();
        let entries_before = fs::read_dir(first.parent().unwrap()).unwrap().count();

        let error = store
            .materialize(b"d", AttachmentLanguage::Java)
            .unwrap_err();
        assert!(error.contains("quota exhausted"));
        assert_eq!(store.usage(), (3, 1));
        assert_eq!(
            fs::read_dir(first.parent().unwrap()).unwrap().count(),
            entries_before
        );
    }

    #[test]
    fn per_file_limit_refuses_source_without_reserving_space() {
        let temp = tempfile::tempdir().unwrap();
        let store = test_store(temp.path(), 100, 10, 3);

        let error = store
            .materialize(b"four", AttachmentLanguage::Java)
            .unwrap_err();

        assert!(error.contains("maximum 3"));
        assert_eq!(store.usage(), (0, 0));
        assert_eq!(
            fs::read_dir(store.root().join(FILES_DIRECTORY))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn failed_publication_rolls_back_reservation_and_staging_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = test_store(temp.path(), 100, 10, 100);
        fs::remove_dir(store.root().join(FILES_DIRECTORY)).unwrap();

        assert!(store
            .materialize(b"class MissingParent {}", AttachmentLanguage::Java)
            .is_err());
        assert_eq!(store.usage(), (0, 0));
        assert!(fs::read_dir(store.root()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".stage-")));
    }

    #[test]
    fn quota_arithmetic_refuses_overflow_without_mutation() {
        let mut state = StoreState {
            accounted_bytes: u64::MAX,
            ..StoreState::default()
        };
        let limits = MaterializedSourceLimits {
            total_bytes: u64::MAX,
            file_count: u64::MAX,
            per_file_bytes: u64::MAX,
        };

        assert!(state.reserve(1, 1, limits).is_err());
        assert_eq!(state.accounted_bytes, u64::MAX);
        assert_eq!(state.accounted_files, 0);
    }

    #[test]
    fn identical_content_and_language_share_one_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = test_store(temp.path(), 100, 10, 100);
        let first = store
            .materialize(b"class Same {}", AttachmentLanguage::Java)
            .unwrap();
        let second = store
            .materialize(b"class Same {}", AttachmentLanguage::Java)
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(store.usage(), (13, 1));
    }

    #[test]
    fn externally_modified_file_is_repaired_before_return() {
        let temp = tempfile::tempdir().unwrap();
        let store = test_store(temp.path(), 100, 3, 100);
        let expected = b"class Original {}";
        let path = store
            .materialize(expected, AttachmentLanguage::Java)
            .unwrap();
        fs::write(&path, b"class Replaced {} with extra bytes").unwrap();

        assert_eq!(
            store
                .materialize(expected, AttachmentLanguage::Java)
                .unwrap(),
            path
        );
        assert_eq!(fs::read(path).unwrap(), expected);
        assert_eq!(store.usage(), (expected.len() as u64, 1));
    }

    #[test]
    fn dropped_store_removes_the_complete_session_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = {
            let store = test_store(temp.path(), 100, 10, 100);
            let root = store.root().to_path_buf();
            store
                .materialize(b"class Gone {}", AttachmentLanguage::Java)
                .unwrap();
            assert!(root.is_dir());
            root
        };
        assert!(!root.exists());
    }

    #[test]
    fn scavenging_removes_only_old_unlocked_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let abandoned = create_test_session(base, "session-abandoned", 1);
        let recent = create_test_session(base, "session-recent", 9_999);
        let live = create_test_session(base, "session-live", 1);
        let live_lease = OpenOptions::new()
            .read(true)
            .write(true)
            .open(live.join(LEASE_FILE))
            .unwrap();
        FileExt::lock_exclusive(&live_lease).unwrap();

        scavenge_abandoned_sessions(
            base,
            UNIX_EPOCH + Duration::from_secs(10_000),
            Duration::from_secs(100),
        )
        .unwrap();

        assert!(!abandoned.exists());
        assert!(recent.exists());
        assert!(live.exists());
        FileExt::unlock(&live_lease).unwrap();
    }

    #[test]
    fn materialized_store_representation_and_limits_stay_compact_and_bounded() {
        assert_eq!(std::mem::size_of::<AttachmentLanguage>(), 1);
        assert!(std::mem::size_of::<MaterializedSourceKey>() <= 33);
        assert!(std::mem::size_of::<MaterializedSourceLimits>() <= 24);
        assert_eq!(DEFAULT_MAX_BYTES, 256 * 1024 * 1024);
        assert_eq!(DEFAULT_MAX_FILES, 4_096);
        assert_eq!(DEFAULT_MAX_FILE_BYTES, 4 * 1024 * 1024);
        const { assert!(DEFAULT_MAX_FILE_BYTES <= DEFAULT_MAX_BYTES) };
    }

    fn create_test_session(base: &Path, name: &str, created_at: u64) -> PathBuf {
        let session = base.join(name);
        fs::create_dir(&session).unwrap();
        fs::write(session.join(LEASE_FILE), format!("{created_at}\n")).unwrap();
        session
    }
}
