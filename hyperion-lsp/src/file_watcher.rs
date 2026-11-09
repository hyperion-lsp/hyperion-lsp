// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! File system watcher for detecting external file changes
//!
//! This module provides file system watching capabilities using the `notify` crate
//! to detect when files are modified outside of the LSP editor (e.g., via git operations,
//! external editors, or build systems).
//!
//! # Architecture
//!
//! The file watcher uses a debounced event system to prevent excessive notifications
//! from rapid file changes. Events are filtered to only include source files with
//! relevant extensions (.java, .kt, .scala, .groovy).
//!
//! # Usage
//!
//! ```no_run
//! use hyperion_lsp::file_watcher::FileWatcher;
//! use std::path::PathBuf;
//!
//! let workspace_root = PathBuf::from("/path/to/workspace");
//! let mut watcher = FileWatcher::new(workspace_root).expect("Failed to create watcher");
//!
//! tokio::spawn(async move {
//!     while let Some(event) = watcher.next_event().await {
//!         println!("File changed: {:?}", event);
//!     }
//! });
//! ```

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Maximum consecutive channel send failures before disabling forwarding.
const MAX_CONSECUTIVE_FAILURES: usize = 3;

/// Capacity of file watcher event channel.
///
/// Bounded to prevent unbounded RAM growth if event production outpaces
/// the consumer (e.g., massive git checkout or build output churn).
const FILE_WATCHER_CHANNEL_CAPACITY: usize = 2048;

/// File system change event
#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    /// File was created
    Created(PathBuf),
    /// File was modified
    Modified(PathBuf),
    /// File was deleted
    Deleted(PathBuf),
    /// File was renamed (old_path, new_path)
    Renamed(PathBuf, PathBuf),
    /// Directory was renamed (old_path, new_path)
    ///
    /// When a directory is renamed (e.g., com/example -> com/company),
    /// all files within need to be re-indexed with their new paths.
    /// This is critical for package refactoring operations.
    DirectoryRenamed { from: PathBuf, to: PathBuf },
    /// A directory appeared. Some watcher backends report only the directory,
    /// so the handler performs a scoped source-tree reconciliation.
    DirectoryCreated(PathBuf),
    /// A directory disappeared. Its indexed descendants must be retired even
    /// when no per-file removal notifications are emitted.
    DirectoryDeleted(PathBuf),
}

/// File system watcher that monitors workspace files for external changes
///
/// This watcher uses the `notify` crate with debouncing to efficiently detect
/// file system changes without overwhelming the LSP with notifications.
pub struct FileWatcher {
    /// Debouncer wrapping the file system watcher
    /// Must be kept alive for watching to continue
    _debouncer: Debouncer<RecommendedWatcher, FileIdMap>,

    /// Channel receiver for file change events
    event_rx: mpsc::Receiver<FileChangeEvent>,
}

fn send_file_change(
    event_tx: &mpsc::Sender<FileChangeEvent>,
    change: FileChangeEvent,
) -> Result<(), mpsc::error::SendError<FileChangeEvent>> {
    event_tx.blocking_send(change)
}

impl FileWatcher {
    /// Create a new file watcher for the given workspace root
    ///
    /// # Arguments
    ///
    /// * `workspace_root` - The root directory to watch recursively
    ///
    /// # Returns
    ///
    /// Returns `Ok(FileWatcher)` on success, or an error if the watcher cannot be initialized.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The workspace root path does not exist
    /// - The file system watcher cannot be initialized
    /// - Permissions prevent watching the directory
    pub fn new(workspace_root: PathBuf) -> Result<Self, notify::Error> {
        Self::new_inner(workspace_root, None)
    }

    pub(crate) fn new_with_readiness(
        workspace_root: PathBuf,
        readiness: std::sync::Arc<crate::server::DestructiveRefactoringReadiness>,
    ) -> Result<Self, notify::Error> {
        Self::new_inner(workspace_root, Some(readiness))
    }

    fn new_inner(
        workspace_root: PathBuf,
        readiness: Option<std::sync::Arc<crate::server::DestructiveRefactoringReadiness>>,
    ) -> Result<Self, notify::Error> {
        debug!("Creating file watcher for: {}", workspace_root.display());

        let (event_tx, event_rx) = mpsc::channel(FILE_WATCHER_CHANNEL_CAPACITY);

        // M2 FIX: Per-instance failure counter (moved from static)
        // This prevents cascading failures when multiple FileWatcher instances exist
        // (e.g., multi-workspace LSP, parallel tests)
        let consecutive_failures = std::sync::Arc::new(AtomicUsize::new(0));
        let failures_clone = std::sync::Arc::clone(&consecutive_failures);
        let forwarding_disabled = std::sync::Arc::new(AtomicBool::new(false));
        let forwarding_disabled_clone = std::sync::Arc::clone(&forwarding_disabled);
        let failure_readiness = readiness.clone();
        let event_root = workspace_root.clone();

        // Track event sequence numbers to detect lost events
        static EVENT_RECEIVED_COUNTER: AtomicU64 = AtomicU64::new(0);
        static EVENT_SENT_COUNTER: AtomicU64 = AtomicU64::new(0);

        // Create debounced watcher with 500ms delay
        // This prevents excessive notifications during rapid changes (e.g., git operations)
        //
        // IMPORTANT: The debouncer batches events but does NOT drop them. When multiple
        // file changes occur within 500ms, they are all delivered together in one batch
        // after the 500ms window expires. This is correct behavior for git operations.
        let mut debouncer = new_debouncer(
            Duration::from_millis(500),
            None, // No custom tick rate
            move |result: DebounceEventResult| {
                match result {
                    Ok(events) => {
                        if forwarding_disabled_clone.load(Ordering::Acquire) {
                            debug!(
                                "File watcher forwarding disabled; dropping {} debounced events",
                                events.len()
                            );
                            return;
                        }

                        // Track how many events we received from the file system
                        let received_count = events.len() as u64;
                        let total_received = EVENT_RECEIVED_COUNTER
                            .fetch_add(received_count, Ordering::SeqCst)
                            + received_count;

                        debug!(
                            "Debouncer batch: {} events (total received: {})",
                            events.len(),
                            total_received
                        );

                        let mut sent_in_batch = 0u64;
                        for event in events {
                            // Convert notify events to our FileChangeEvent
                            for change in Self::convert_event_in(event.event, &event_root) {
                                // notify-debouncer-full invokes this callback on its own
                                // dedicated thread, after releasing the mutex that receives
                                // raw watcher events. Applying bounded backpressure here does
                                // not block Tokio or the OS watcher callback: new raw events
                                // continue to accumulate and coalesce inside the debouncer
                                // until the LSP consumer catches up. Dropping at this boundary
                                // left WorkspaceIndex and Salsa permanently stale because no
                                // recovery scan actually existed.
                                match send_file_change(&event_tx, change) {
                                    Ok(()) => {
                                        sent_in_batch += 1;
                                        // Successful send - reset failure counter
                                        failures_clone.store(0, Ordering::SeqCst);
                                    }
                                    Err(_change) => {
                                        if let Some(readiness) = &failure_readiness {
                                            readiness.fail_sources_permanently();
                                        }
                                        let failure_count =
                                            failures_clone.fetch_add(1, Ordering::SeqCst) + 1;
                                        error!(
                                            "File watcher channel send failed (failure #{} of {}): receiver closed",
                                            failure_count, MAX_CONSECUTIVE_FAILURES
                                        );
                                        error!(
                                            "This indicates the file event handler task has died. \
                                             The LSP can no longer process external changes."
                                        );

                                        if failure_count >= MAX_CONSECUTIVE_FAILURES {
                                            if !forwarding_disabled_clone
                                                .swap(true, Ordering::AcqRel)
                                            {
                                                error!(
                                                    "Disabling file watcher forwarding after {} consecutive send failures; \
                                                     external file updates will be ignored until restart",
                                                    failure_count
                                                );
                                            }
                                            return;
                                        }
                                    }
                                }
                            }
                        }

                        // Track how many relevant events we actually sent
                        if sent_in_batch > 0 {
                            let total_sent = EVENT_SENT_COUNTER
                                .fetch_add(sent_in_batch, Ordering::SeqCst)
                                + sent_in_batch;
                            info!(
                                "File watcher batch complete: sent {} relevant events (total sent: {}, total received: {})",
                                sent_in_batch, total_sent, total_received
                            );
                        }
                    }
                    Err(errors) => {
                        if let Some(readiness) = &failure_readiness {
                            readiness.fail_sources_permanently();
                        }
                        for err in errors {
                            error!("File watch error: {:?}", err);
                        }
                    }
                }
            },
        )?;

        // Watch workspace recursively
        debug!("Starting recursive watch on: {}", workspace_root.display());
        debouncer
            .watcher()
            .watch(&workspace_root, RecursiveMode::Recursive)?;

        // Add root to debouncer cache for event processing
        debouncer
            .cache()
            .add_root(&workspace_root, RecursiveMode::Recursive);

        debug!("File watcher initialized successfully");

        Ok(Self {
            _debouncer: debouncer,
            event_rx,
        })
    }

    /// Receive the next file change event
    ///
    /// This is an async function that will wait until a file change event is available.
    /// Returns `None` if the watcher has been dropped or the channel is closed.
    pub async fn next_event(&mut self) -> Option<FileChangeEvent> {
        self.event_rx.recv().await
    }

    /// Convert a notify Event to our FileChangeEvent enum
    ///
    /// This function filters events to only include relevant source files
    /// and converts them to our simplified event types.
    ///
    /// # Returns
    ///
    /// Returns every relevant path represented by the notify event. Backends
    /// are allowed to batch multiple paths into one event; silently taking only
    /// the first path leaves the index permanently stale.
    /// Whether a client-reported file change is one the server tracks
    /// (sources and build files, outside hidden/build-output directories).
    pub(crate) fn is_relevant_source_path(root: &Path, path: &Path) -> bool {
        let relative = path.strip_prefix(root).unwrap_or(path);
        if relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            return false;
        }
        let is_build_output = relative.components().any(|c| {
            matches!(
                c.as_os_str().to_string_lossy().as_ref(),
                "target" | "build" | "out"
            )
        });
        if is_build_output && !hyperion_workspace::scanner::is_generated_source_path(path) {
            return false;
        }
        hyperion_layout::is_build_input(path)
            || path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(ext, "java" | "kt" | "kts" | "scala" | "groovy" | "gradle")
                })
    }

    #[cfg(test)]
    fn convert_event(event: Event) -> Vec<FileChangeEvent> {
        Self::convert_event_in(event, Path::new(""))
    }

    /// Classify one notify event. Ignore rules (hidden and build-output
    /// directories) apply to the path *below the watched root*: a workspace
    /// that itself lives under a hidden or `build`/`out`/`target` directory
    /// (`~/.local/...`, `/tmp/.tmpXXXX`, `~/build/project`) must not have
    /// every event discarded.
    fn convert_event_in(event: Event, root: &Path) -> Vec<FileChangeEvent> {
        use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
        use notify::EventKind;

        fn path_is_ignored(root: &Path, path: &Path) -> bool {
            // M24 FIX: Ignore symlinks to prevent duplicate indexing
            // Symlinks can cause the same file to be indexed multiple times or
            // create infinite loops if they point to parent directories
            if let Ok(metadata) = std::fs::symlink_metadata(path) {
                if metadata.is_symlink() {
                    debug!("Ignoring symlink: {}", path.display());
                    return true;
                }
            }

            // Ignore hidden files and directories (e.g., .git, .idea)
            let relative = path.strip_prefix(root).unwrap_or(path);
            if relative
                .components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            {
                return true;
            }

            // Ignore build output directories except generated-source trees.
            // Annotation processors commonly write below `build/` or
            // `target/`; source events there need the same incremental update
            // path as hand-written source, while classes/resources remain noise.
            let is_build_output = relative.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                matches!(s.as_ref(), "target" | "build" | "out" | ".gradle" | ".m2")
            });
            if is_build_output && !hyperion_workspace::scanner::is_generated_source_path(path) {
                return true;
            }

            false
        }

        fn is_relevant_file(root: &Path, path: &Path) -> bool {
            if path_is_ignored(root, path) {
                return false;
            }

            if hyperion_layout::is_build_input(path) {
                return true;
            }

            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(ext, "java" | "kt" | "kts" | "scala" | "groovy" | "gradle")
                })
        }

        fn is_relevant_directory(root: &Path, path: &Path) -> bool {
            if let Ok(metadata) = std::fs::symlink_metadata(path) {
                if metadata.is_symlink() {
                    return false;
                }
            }
            // Do not suppress build-output directories here. They may contain
            // indexed generated sources, and a directory-only removal event
            // must retire those descendants. The reconciliation scan itself
            // filters non-source files and has a hard entry bound.
            !path
                .strip_prefix(root)
                .unwrap_or(path)
                .components()
                .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
        }

        fn directory_like(path: &Path) -> bool {
            path.is_dir()
                || (path.extension().is_none()
                    && !path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains('.')))
        }

        fn renamed_pair(root: &Path, old: PathBuf, new: PathBuf) -> Option<FileChangeEvent> {
            if new.is_dir() || directory_like(&old) && directory_like(&new) {
                if is_relevant_directory(root, &old) || is_relevant_directory(root, &new) {
                    debug!("Directory renamed: {} -> {}", old.display(), new.display());
                    return Some(FileChangeEvent::DirectoryRenamed { from: old, to: new });
                }
            } else if is_relevant_file(root, &new) || is_relevant_file(root, &old) {
                debug!("File renamed: {} -> {}", old.display(), new.display());
                return Some(FileChangeEvent::Renamed(old, new));
            }
            None
        }

        fn created_path(root: &Path, path: PathBuf) -> Option<FileChangeEvent> {
            if path.is_dir() && is_relevant_directory(root, &path) {
                Some(FileChangeEvent::DirectoryCreated(path))
            } else if is_relevant_file(root, &path) {
                Some(FileChangeEvent::Created(path))
            } else {
                None
            }
        }

        fn deleted_path(root: &Path, path: PathBuf) -> Option<FileChangeEvent> {
            // RenameMode::From and RemoveKind::Any arrive after the path has
            // disappeared, so metadata and filename heuristics cannot prove
            // whether it was a file or directory. Directory reconciliation is
            // safe for both: an exact file path selects that one indexed file,
            // while a directory selects its descendants.
            if is_relevant_directory(root, &path) {
                Some(FileChangeEvent::DirectoryDeleted(path))
            } else {
                None
            }
        }

        match event.kind {
            EventKind::Create(kind) => {
                let is_folder = matches!(kind, CreateKind::Folder);
                let is_ambiguous = matches!(kind, CreateKind::Any);
                event
                    .paths
                    .into_iter()
                    .filter_map(|path| {
                        if is_folder || is_ambiguous && directory_like(&path) {
                            if is_relevant_directory(root, &path) {
                                debug!("Directory created: {}", path.display());
                                return Some(FileChangeEvent::DirectoryCreated(path));
                            }
                        } else if is_relevant_file(root, &path) {
                            debug!("File created: {}", path.display());
                            return Some(FileChangeEvent::Created(path));
                        }
                        None
                    })
                    .collect()
            }
            EventKind::Modify(ModifyKind::Name(mode)) => {
                // Rename events - must come BEFORE general Modify pattern
                match mode {
                    RenameMode::From => {
                        return event
                            .paths
                            .into_iter()
                            .filter_map(|path| deleted_path(root, path))
                            .collect();
                    }
                    RenameMode::To => {
                        return event
                            .paths
                            .into_iter()
                            .filter_map(|path| created_path(root, path))
                            .collect();
                    }
                    _ => {}
                }
                let mut changes = Vec::new();
                let mut paths = event.paths.into_iter();
                while let Some(old) = paths.next() {
                    if let Some(new) = paths.next() {
                        if let Some(change) = renamed_pair(root, old, new) {
                            changes.push(change);
                        }
                    } else {
                        // RenameMode::Any is backend-specific. Reconcile the
                        // observed side according to its current existence;
                        // explicit From/To modes were handled above.
                        let change = if old.exists() {
                            created_path(root, old)
                        } else {
                            deleted_path(root, old)
                        };
                        if let Some(change) = change {
                            changes.push(change);
                        }
                    }
                }
                changes
            }
            EventKind::Modify(_) => event
                .paths
                .into_iter()
                .filter(|path| is_relevant_file(root, path))
                .map(|path| {
                    // General modification events (content changes, etc.)
                    debug!("File modified: {}", path.display());
                    FileChangeEvent::Modified(path)
                })
                .collect(),
            EventKind::Remove(kind) => {
                let is_folder = matches!(kind, RemoveKind::Folder);
                let is_ambiguous = matches!(kind, RemoveKind::Any);
                event
                    .paths
                    .into_iter()
                    .filter_map(|path| {
                        if is_ambiguous {
                            return deleted_path(root, path);
                        }
                        if is_folder {
                            if is_relevant_directory(root, &path) {
                                debug!("Directory deleted: {}", path.display());
                                return Some(FileChangeEvent::DirectoryDeleted(path));
                            }
                        } else if is_relevant_file(root, &path) {
                            debug!("File deleted: {}", path.display());
                            return Some(FileChangeEvent::Deleted(path));
                        }
                        None
                    })
                    .collect()
            }
            _ => {
                // Ignore other event types (access, metadata changes, etc.)
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{event::CreateKind, EventKind};
    use std::fs;
    use std::sync::mpsc as std_mpsc;
    use tempfile::TempDir;

    #[test]
    fn saturated_delivery_waits_without_losing_the_next_event() {
        let (event_tx, mut event_rx) = mpsc::channel(1);
        let first_path = PathBuf::from("First.java");
        let second_path = PathBuf::from("Second.java");
        send_file_change(&event_tx, FileChangeEvent::Modified(first_path.clone())).unwrap();

        let (finished_tx, finished_rx) = std_mpsc::channel();
        let worker = std::thread::spawn(move || {
            let sent = send_file_change(&event_tx, FileChangeEvent::Created(second_path)).is_ok();
            finished_tx.send(sent).unwrap();
        });

        assert!(matches!(
            finished_rx.recv_timeout(Duration::from_millis(25)),
            Err(std_mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(
            event_rx.blocking_recv(),
            Some(FileChangeEvent::Modified(path)) if path == first_path
        ));
        assert!(finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("sender should resume when capacity is available"));
        assert!(matches!(
            event_rx.blocking_recv(),
            Some(FileChangeEvent::Created(path)) if path == Path::new("Second.java")
        ));
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn test_file_watcher_detects_new_file() {
        if std::env::var_os("HYPERION_FS_WATCH_TEST").is_none() {
            eprintln!("Skipping file watcher integration test.");
            eprintln!("Set HYPERION_FS_WATCH_TEST=1 to enable it.");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();

        let mut watcher = FileWatcher::new(workspace_root.clone()).unwrap();

        // Create a new Java file
        let file_path = workspace_root.join("Test.java");
        tokio::spawn(async move {
            // Wait a bit to ensure watcher is fully initialized
            tokio::time::sleep(Duration::from_millis(200)).await;
            fs::write(&file_path, "public class Test {}").unwrap();
        });

        // Wait for event (with generous timeout due to debouncing)
        let event = tokio::time::timeout(Duration::from_secs(3), watcher.next_event())
            .await
            .expect("Timeout waiting for event")
            .expect("No event received");

        // Accept either Created or Modified, as different file systems may report differently
        match event {
            FileChangeEvent::Created(path) | FileChangeEvent::Modified(path) => {
                assert!(path.ends_with("Test.java"));
            }
            _ => panic!("Expected Created or Modified event, got: {:?}", event),
        }
    }

    #[tokio::test]
    async fn test_file_watcher_ignores_non_source_files() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();

        let mut watcher = FileWatcher::new(workspace_root.clone()).unwrap();

        // Create a non-source file
        let file_path = workspace_root.join("README.txt");
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            fs::write(&file_path, "Hello").unwrap();
        });

        // Should timeout because no event is sent
        let result = tokio::time::timeout(Duration::from_millis(700), watcher.next_event()).await;

        assert!(
            result.is_err(),
            "Should not receive event for non-source file"
        );
    }

    #[tokio::test]
    async fn test_file_watcher_ignores_build_directories() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();

        let mut watcher = FileWatcher::new(workspace_root.clone()).unwrap();

        // Create a Java file in target directory
        let target_dir = workspace_root.join("target");
        fs::create_dir(&target_dir).unwrap();
        let file_path = target_dir.join("Generated.java");

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            fs::write(&file_path, "public class Generated {}").unwrap();
        });

        // The directory itself is reported (a removal must retire indexed
        // generated sources), but no file event may arrive for build output.
        let deadline = tokio::time::Instant::now() + Duration::from_millis(1200);
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, watcher.next_event()).await {
            assert!(
                matches!(
                    event,
                    FileChangeEvent::DirectoryCreated(_) | FileChangeEvent::DirectoryDeleted(_)
                ),
                "Should not receive file events for build directories, got {event:?}"
            );
        }
    }

    #[test]
    fn generated_source_events_pass_the_build_directory_filter() {
        let generated = Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from(
            "/workspace/build/generated/ksp/main/java/com/example/Generated.java",
        ));
        assert!(matches!(
            FileWatcher::convert_event(generated).as_slice(),
            [FileChangeEvent::Created(path)] if path.ends_with("Generated.java")
        ));

        let compiled = Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from(
            "/workspace/build/classes/java/main/com/example/Generated.java",
        ));
        assert!(FileWatcher::convert_event(compiled).is_empty());

        let misleading_parent = Event::new(EventKind::Create(CreateKind::File)).add_path(
            PathBuf::from("/workspace/generated/project/build/classes/LooksGenerated.java"),
        );
        assert!(FileWatcher::convert_event(misleading_parent).is_empty());
    }

    #[test]
    fn one_notify_event_preserves_every_relevant_path() {
        let event = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/workspace/src/A.java"))
            .add_path(PathBuf::from("/workspace/src/B.kt"))
            .add_path(PathBuf::from("/workspace/README.md"));
        let changes = FileWatcher::convert_event(event);
        assert_eq!(changes.len(), 2);
        assert!(matches!(&changes[0], FileChangeEvent::Created(path) if path.ends_with("A.java")));
        assert!(matches!(&changes[1], FileChangeEvent::Created(path) if path.ends_with("B.kt")));
    }

    #[test]
    fn directory_and_version_catalog_events_are_forwarded() {
        use notify::event::{ModifyKind, RemoveKind, RenameMode};

        let created = Event::new(EventKind::Create(CreateKind::Folder))
            .add_path(PathBuf::from("/workspace/new-module"));
        assert!(matches!(
            FileWatcher::convert_event(created).as_slice(),
            [FileChangeEvent::DirectoryCreated(path)] if path.ends_with("new-module")
        ));

        let removed = Event::new(EventKind::Remove(RemoveKind::Folder))
            .add_path(PathBuf::from("/workspace/old-module"));
        assert!(matches!(
            FileWatcher::convert_event(removed).as_slice(),
            [FileChangeEvent::DirectoryDeleted(path)] if path.ends_with("old-module")
        ));

        let generated_container = Event::new(EventKind::Remove(RemoveKind::Folder))
            .add_path(PathBuf::from("/workspace/target"));
        assert!(matches!(
            FileWatcher::convert_event(generated_container).as_slice(),
            [FileChangeEvent::DirectoryDeleted(path)] if path.ends_with("target")
        ));

        let catalog = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/workspace/gradle/libs.versions.toml"));
        assert!(matches!(
            FileWatcher::convert_event(catalog).as_slice(),
            [FileChangeEvent::Created(path)] if path.ends_with("libs.versions.toml")
        ));

        let rename_from = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path(PathBuf::from("/workspace/removed/Old.java"));
        assert!(matches!(
            FileWatcher::convert_event(rename_from).as_slice(),
            [FileChangeEvent::DirectoryDeleted(path)] if path.ends_with("Old.java")
        ));

        let ambiguous_dotted_directory = Event::new(EventKind::Remove(RemoveKind::Any))
            .add_path(PathBuf::from("/workspace/modules/com.example"));
        assert!(matches!(
            FileWatcher::convert_event(ambiguous_dotted_directory).as_slice(),
            [FileChangeEvent::DirectoryDeleted(path)] if path.ends_with("com.example")
        ));
    }

    #[test]
    fn a_workspace_below_a_hidden_or_build_named_directory_is_still_watched() {
        let root = Path::new("/tmp/.tmpAbC/build/project");
        let event = Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(root.join("src/main/java/demo/App.java"));
        assert!(matches!(
            FileWatcher::convert_event_in(event, root).as_slice(),
            [FileChangeEvent::Modified(_)]
        ));
        // Ignore rules still apply below the root.
        let hidden = Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(root.join(".git/hooks/App.java"));
        assert!(FileWatcher::convert_event_in(hidden, root).is_empty());
    }
}
