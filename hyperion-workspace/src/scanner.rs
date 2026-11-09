// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Workspace scanner: finds all supported JVM source files in a workspace
//
// Uses the `ignore` crate to respect .gitignore files and skip build artifacts.

use crate::Language;
use anyhow::{bail, Result};
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Clone, Copy, Debug)]
struct ScannerConfig {
    respect_gitignore: bool,
    inherit_parent_gitignore: bool,
    ignore_hidden: bool,
    apply_build_output_exclusions: bool,
    follow_links: bool,
    max_depth: Option<usize>,
}

impl ScannerConfig {
    const fn workspace_default() -> Self {
        Self {
            respect_gitignore: true,
            inherit_parent_gitignore: true,
            ignore_hidden: true,
            apply_build_output_exclusions: true,
            follow_links: false,
            max_depth: None,
        }
    }

    const fn builder_default() -> Self {
        Self {
            // ScannerBuilder historically included hidden source paths. Keep
            // that behavior distinct from the workspace startup manifest.
            ignore_hidden: false,
            ..Self::workspace_default()
        }
    }
}

/// Scans a workspace directory for supported JVM source files.
pub struct WorkspaceScanner {
    root: PathBuf,
    config: ScannerConfig,
}

impl WorkspaceScanner {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            config: ScannerConfig::workspace_default(),
        }
    }

    /// Scan the workspace and return all supported JVM source files.
    ///
    /// This respects .gitignore files and skips common build directories.
    /// Uses parallel traversal for improved performance on multi-core systems.
    pub fn scan(&self) -> Result<Vec<PathBuf>> {
        Self::scan_dir_with_config(&self.root, self.config)
    }

    pub fn scan_dir(root: &Path) -> Result<Vec<PathBuf>> {
        Self::scan_dir_with_config(root, ScannerConfig::workspace_default())
    }

    /// Scan a directory for source files, optionally ignoring .gitignore
    ///
    /// When `respect_gitignore` is false, this will scan ALL source files in the
    /// directory, even if they're listed in .gitignore. This is useful for
    /// explicitly specified source directories (e.g., from Gradle module discovery)
    /// where we KNOW the files are source code and should be indexed.
    pub fn scan_dir_ignore_gitignore(root: &Path) -> Result<Vec<PathBuf>> {
        Self::scan_dir_with_config(
            root,
            ScannerConfig {
                respect_gitignore: false,
                apply_build_output_exclusions: false,
                ..ScannerConfig::workspace_default()
            },
        )
    }

    /// Scan a directory while respecting `.gitignore`, but without inheriting ignore rules
    /// from parent directories above `root`.
    ///
    /// This is the behavior you want when `root` is (or should be treated like) a Git
    /// repository root discovered from a nested checkout/submodule inside a larger workspace.
    /// In that case, `.gitignore` files *above* the repo root must not exclude the nested repo.
    pub fn scan_dir_scoped_gitignore(root: &Path) -> Result<Vec<PathBuf>> {
        Self::scan_dir_with_config(
            root,
            ScannerConfig {
                inherit_parent_gitignore: false,
                ..ScannerConfig::workspace_default()
            },
        )
    }

    fn scan_dir_with_config(root: &Path, config: ScannerConfig) -> Result<Vec<PathBuf>> {
        use std::time::Instant;

        let start = Instant::now();
        info!(
            "Scanning directory at: {} (respect_gitignore: {}, inherit_parent_gitignore: {}, follow_links: {}, max_depth: {:?})",
            root.display(),
            config.respect_gitignore,
            config.inherit_parent_gitignore,
            config.follow_links,
            config.max_depth,
        );

        // Thread-safe set for duplicate path suppression across the parallel
        // walker. Filesystem aliases remain distinct source paths.
        // DashMap uses sharded locking - different threads write to different shards
        let source_files: Arc<DashMap<PathBuf, ()>> = Arc::new(DashMap::new());

        // Use ignore crate's parallel walker for multi-core scanning
        let mut builder = ignore::WalkBuilder::new(root);
        builder
            // Keep the manifest aligned with the live file watcher, which
            // deliberately ignores hidden path components. Indexing a hidden
            // source only at startup would leave a permanently stale revision.
            .hidden(config.ignore_hidden)
            .git_ignore(config.respect_gitignore) // Respect .gitignore only when asked
            .git_global(config.respect_gitignore) // Respect global gitignore only when asked
            .git_exclude(config.respect_gitignore) // Respect .git/info/exclude only when asked
            .require_git(false) // Work even without git
            .follow_links(config.follow_links);

        // Do not inherit ignore rules from parent directories above `root` when asked.
        // Important: only set this when needed so we preserve the ignore crate's defaults.
        if !config.inherit_parent_gitignore {
            builder.parents(false);
        }
        if let Some(depth) = config.max_depth {
            builder.max_depth(Some(depth));
        }

        let walker = builder.build_parallel(); // Parallel traversal using thread pool

        // Clone Arc for closure (cheap - just incrementing refcount)
        let files_clone = Arc::clone(&source_files);

        // Track duplicate paths defensively if the walker yields one twice.
        let duplicates = Arc::new(AtomicUsize::new(0));
        let traversal_errors = Arc::new(AtomicUsize::new(0));

        walker.run(|| {
            // This closure is executed concurrently across multiple threads
            // Each thread gets its own entry stream from the walker
            let files = Arc::clone(&files_clone);
            let dup_counter = Arc::clone(&duplicates);
            let error_counter = Arc::clone(&traversal_errors);

            Box::new(move |entry| {
                use ignore::WalkState;

                let entry = match entry {
                    Ok(e) => e,
                    Err(err) => {
                        warn!("Error walking directory: {}", err);
                        error_counter.fetch_add(1, Ordering::Relaxed);
                        return WalkState::Continue;
                    }
                };

                let path = entry.path();

                // Filter: only files (not directories)
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    return WalkState::Continue;
                }

                if !Self::is_supported_source_file(path) {
                    return WalkState::Continue;
                }

                // Filter: skip build directories
                if config.apply_build_output_exclusions && Self::should_skip(path) {
                    debug!("Skipping file: {}", path.display());
                    return WalkState::Continue;
                }

                // DashMap::insert returns Some(old_value) if the key already existed
                // (for example, if a traversal race yields the same path twice).
                if let Some(_old_value) = files.insert(path.to_path_buf(), ()) {
                    // Duplicate found - log and increment counter
                    debug!(
                        "Duplicate source path detected during traversal: {}",
                        path.display()
                    );
                    dup_counter.fetch_add(1, Ordering::Relaxed);
                }

                WalkState::Continue
            })
        });

        let traversal_error_count = traversal_errors.load(Ordering::Relaxed);
        if traversal_error_count > 0 {
            bail!(
                "workspace scan of {} encountered {} traversal error(s)",
                root.display(),
                traversal_error_count
            );
        }

        // Convert DashMap keys to Vec (sequential, but fast - just copying pointers)
        let result: Vec<PathBuf> = source_files
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let elapsed = start.elapsed();
        let duplicate_count = duplicates.load(Ordering::Relaxed);

        if duplicate_count > 0 {
            warn!(
                "Found {} source files in workspace in {:.2}s ({:.0} files/sec) - {} duplicates skipped",
                result.len(),
                elapsed.as_secs_f64(),
                result.len() as f64 / elapsed.as_secs_f64().max(0.001),
                duplicate_count
            );
        } else {
            info!(
                "Found {} source files in workspace in {:.2}s ({:.0} files/sec)",
                result.len(),
                elapsed.as_secs_f64(),
                result.len() as f64 / elapsed.as_secs_f64().max(0.001)
            );
        }

        Ok(result)
    }

    fn is_supported_source_file(path: &Path) -> bool {
        Language::from_path(path).is_some()
    }

    /// Check if a file should be skipped even if not in .gitignore
    ///
    /// This catches common build artifact and tool-state directories that
    /// might not be in `.gitignore`.
    pub fn should_skip(path: &Path) -> bool {
        // Compare path components, not rendered separators, so the exclusion
        // remains correct on Windows as well as Unix.
        const BUILD_DIRS: &[&str] = &[
            "target",
            "build",
            "out",
            "bin",
            "dist",
            ".gradle",
            ".idea",
            ".vscode",
            ".settings",
            "node_modules",
        ];
        path.components().any(|component| {
            let std::path::Component::Normal(name) = component else {
                return false;
            };
            BUILD_DIRS.iter().any(|excluded| name == *excluded)
        })
    }

    /// Get the workspace root
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Conventional generated-source roots produced by JVM build tools.
///
/// Callers scan these roots explicitly without `.gitignore` or parent build
/// directory exclusions. Scanning the common parents intentionally covers
/// annotation processors, KAPT, KSP, protobuf, Thrift, and custom Gradle
/// generators without maintaining a plugin-by-plugin path table.
pub fn generated_source_roots(project_root: &Path) -> Vec<PathBuf> {
    std::iter::once(project_root.join("build/generated"))
        .chain(maven_generated_source_roots(project_root, false))
        .chain(maven_generated_source_roots(project_root, true))
        .filter(|path| path.is_dir())
        .collect()
}

/// Conventional Maven generated roots, classified by source set for both
/// discovery and compiler visibility. Test outputs must not leak into main.
pub fn maven_generated_source_roots(project_root: &Path, test: bool) -> Vec<PathBuf> {
    let relative: &[&str] = if test {
        &["target/generated-test-sources"]
    } else {
        &["target/generated-sources", "apt_generated"]
    };
    relative
        .iter()
        .map(|path| project_root.join(path))
        .filter(|path| path.is_dir())
        .collect()
}

/// Whether a path is inside a conventional generated-source subtree.
pub fn is_generated_source_path(path: &Path) -> bool {
    let components: Vec<_> = path.components().map(|part| part.as_os_str()).collect();
    components.windows(2).any(|pair| {
        pair == ["build", "generated"]
            || pair == ["target", "generated-sources"]
            || pair == ["target", "generated-test-sources"]
    }) || components.iter().any(|part| *part == "apt_generated")
}

/// Builder for workspace scanner with custom configuration
pub struct ScannerBuilder {
    root: PathBuf,
    config: ScannerConfig,
}

impl ScannerBuilder {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            config: ScannerConfig::builder_default(),
        }
    }

    pub fn respect_gitignore(mut self, respect: bool) -> Self {
        self.config.respect_gitignore = respect;
        self
    }

    pub fn follow_links(mut self, follow: bool) -> Self {
        self.config.follow_links = follow;
        self
    }

    pub fn max_depth(mut self, depth: usize) -> Self {
        self.config.max_depth = Some(depth);
        self
    }

    pub fn build(self) -> WorkspaceScanner {
        WorkspaceScanner {
            root: self.root,
            config: self.config,
        }
    }

    /// Scan with custom configuration
    pub fn scan(self) -> Result<Vec<PathBuf>> {
        self.build().scan()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn sorted(mut files: Vec<PathBuf>) -> Vec<PathBuf> {
        files.sort_unstable();
        files
    }

    #[test]
    fn test_scanner_finds_java_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create test structure
        fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
        fs::write(
            root.join("src/main/java/com/example/Foo.java"),
            "class Foo {}",
        )
        .unwrap();
        fs::write(
            root.join("src/main/java/com/example/Bar.java"),
            "class Bar {}",
        )
        .unwrap();

        let scanner = WorkspaceScanner::new(root.to_path_buf());
        let files = scanner.scan().unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|p| p.ends_with("Foo.java")));
        assert!(files.iter().any(|p| p.ends_with("Bar.java")));
    }

    #[test]
    fn test_scanner_finds_every_supported_language_extension() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let expected = [
            "Java.java",
            "Kotlin.kt",
            "KotlinScript.kts",
            "Groovy.groovy",
            "Gradle.gradle",
            "Scala.scala",
        ];

        for file_name in expected {
            fs::write(root.join(file_name), "").unwrap();
        }
        fs::write(root.join("NotSource.class"), "").unwrap();
        fs::write(root.join("NotSource.py"), "").unwrap();

        let files = WorkspaceScanner::scan_dir(root).unwrap();

        assert_eq!(files.len(), expected.len());
        for file_name in expected {
            assert!(
                files.iter().any(|path| path.ends_with(file_name)),
                "scanner omitted {file_name}"
            );
        }
    }

    #[test]
    fn test_scan_dir_reports_walker_errors() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("missing");

        let error = WorkspaceScanner::scan_dir(&missing).unwrap_err();

        assert!(
            error.to_string().contains("traversal error"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_scanner_skips_build_directories() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create source file
        fs::create_dir_all(root.join("src/main/java")).unwrap();
        fs::write(root.join("src/main/java/Foo.java"), "class Foo {}").unwrap();

        // Create build artifact (should be skipped)
        fs::create_dir_all(root.join("target/classes")).unwrap();
        fs::write(root.join("target/classes/Foo.class"), "fake bytecode").unwrap();
        fs::write(
            root.join("target/classes/Generated.java"),
            "class Generated {}",
        )
        .unwrap();

        let scanner = WorkspaceScanner::new(root.to_path_buf());
        let files = scanner.scan().unwrap();

        // Should only find the source file, not the one in target/
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("src/main/java/Foo.java"));
    }

    #[test]
    fn test_scanner_includes_generated_code_outside_build_output() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Regular source
        fs::create_dir_all(root.join("src/main/java")).unwrap();
        fs::write(root.join("src/main/java/Foo.java"), "class Foo {}").unwrap();

        // Generated source roots outside build output are source, not artifacts.
        fs::create_dir_all(root.join("src/generated/java")).unwrap();
        fs::write(
            root.join("src/generated/java/AutoGen.java"),
            "class AutoGen {}",
        )
        .unwrap();

        let scanner = WorkspaceScanner::new(root.to_path_buf());
        let files = scanner.scan().unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|path| path.ends_with("Foo.java")));
        assert!(files.iter().any(|path| path.ends_with("AutoGen.java")));
    }

    #[test]
    fn test_scanner_does_not_inherit_parent_gitignore_above_git_root() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Outer repo ignores the entire third-party directory.
        fs::write(root.join(".gitignore"), "third-party/\n").unwrap();

        // Nested repo has its own .git marker.
        let nested = root.join("third-party/nested-repo");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(nested.join(".git")).unwrap();

        fs::create_dir_all(nested.join("src/main/java/com/example")).unwrap();
        fs::write(
            nested.join("src/main/java/com/example/Foo.java"),
            "package com.example; class Foo {}",
        )
        .unwrap();

        // Regular scan from nested root (default) will inherit parent ignore rules and can miss it.
        let inherited = WorkspaceScanner::scan_dir(&nested).unwrap();

        // Scoped scan must find it regardless of the outer ignore.
        let scoped = WorkspaceScanner::scan_dir_scoped_gitignore(&nested).unwrap();

        assert!(
            inherited.len() <= scoped.len(),
            "scoped scan should be at least as complete as inherited scan"
        );
        assert_eq!(scoped.len(), 1);
        assert!(scoped[0].ends_with("Foo.java"));
    }

    #[test]
    fn test_should_skip() {
        assert!(WorkspaceScanner::should_skip(Path::new(
            "target/classes/Foo.java"
        )));
        assert!(WorkspaceScanner::should_skip(Path::new(
            "/project/target/classes/Foo.java"
        )));
        assert!(WorkspaceScanner::should_skip(Path::new(
            "/project/build/classes/Foo.java"
        )));
        assert!(!WorkspaceScanner::should_skip(Path::new(
            "/project/src/generated/Foo.java"
        )));
        assert!(WorkspaceScanner::should_skip(Path::new(
            "/project/.idea/Foo.java"
        )));

        assert!(!WorkspaceScanner::should_skip(Path::new(
            "/project/src/main/java/Foo.java"
        )));
        assert!(!WorkspaceScanner::should_skip(Path::new(
            "/project/src/test/java/FooTest.java"
        )));
    }

    #[test]
    fn scanner_builder_build_preserves_respect_gitignore() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/Foo.java"), "class Foo {}").unwrap();
        fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/Ignored.java"), "class Ignored {}").unwrap();

        let respecting = ScannerBuilder::new(root.to_path_buf())
            .build()
            .scan()
            .unwrap();
        assert_eq!(respecting.len(), 1);
        assert!(respecting[0].ends_with("Foo.java"));

        let direct = ScannerBuilder::new(root.to_path_buf())
            .respect_gitignore(false)
            .scan()
            .unwrap();
        let built = ScannerBuilder::new(root.to_path_buf())
            .respect_gitignore(false)
            .build()
            .scan()
            .unwrap();

        assert_eq!(sorted(direct), sorted(built.clone()));
        assert_eq!(built.len(), 2);
        assert!(built.iter().any(|path| path.ends_with("Foo.java")));
        assert!(built.iter().any(|path| path.ends_with("Ignored.java")));
    }

    #[cfg(unix)]
    #[test]
    fn scanner_builder_build_preserves_follow_links() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let linked = TempDir::new().unwrap();
        let root = temp.path();

        fs::write(root.join("Direct.java"), "class Direct {}").unwrap();
        fs::write(linked.path().join("Linked.java"), "class Linked {}").unwrap();
        symlink(linked.path(), root.join("linked")).unwrap();

        let not_followed = ScannerBuilder::new(root.to_path_buf())
            .build()
            .scan()
            .unwrap();
        assert_eq!(not_followed.len(), 1);
        assert!(not_followed[0].ends_with("Direct.java"));

        let direct = ScannerBuilder::new(root.to_path_buf())
            .follow_links(true)
            .scan()
            .unwrap();
        let built = ScannerBuilder::new(root.to_path_buf())
            .follow_links(true)
            .build()
            .scan()
            .unwrap();

        assert_eq!(sorted(direct), sorted(built.clone()));
        assert_eq!(built.len(), 2);
        assert!(built.iter().any(|path| path.ends_with("Direct.java")));
        assert!(built.iter().any(|path| path.ends_with("Linked.java")));
    }

    #[test]
    fn scanner_builder_build_preserves_max_depth() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("one/two")).unwrap();
        fs::write(root.join("Shallow.java"), "class Shallow {}").unwrap();
        fs::write(root.join("one/Deep.java"), "class Deep {}").unwrap();
        fs::write(root.join("one/two/Deeper.java"), "class Deeper {}").unwrap();

        let direct = ScannerBuilder::new(root.to_path_buf())
            .max_depth(1)
            .scan()
            .unwrap();
        let built = ScannerBuilder::new(root.to_path_buf())
            .max_depth(1)
            .build()
            .scan()
            .unwrap();

        assert_eq!(sorted(direct), sorted(built.clone()));
        assert_eq!(built.len(), 1);
        assert!(built[0].ends_with("Shallow.java"));

        let unlimited = ScannerBuilder::new(root.to_path_buf())
            .build()
            .scan()
            .unwrap();
        assert_eq!(unlimited.len(), 3);
    }

    #[test]
    fn scanner_builder_build_preserves_build_output_exclusions() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target/generated-sources")).unwrap();
        fs::write(root.join("src/Main.java"), "class Main {}").unwrap();
        fs::write(
            root.join("target/generated-sources/Generated.java"),
            "class Generated {}",
        )
        .unwrap();

        let direct = ScannerBuilder::new(root.to_path_buf())
            .respect_gitignore(false)
            .scan()
            .unwrap();
        let built = ScannerBuilder::new(root.to_path_buf())
            .respect_gitignore(false)
            .build()
            .scan()
            .unwrap();

        assert_eq!(sorted(direct), sorted(built.clone()));
        assert_eq!(built.len(), 1);
        assert!(built[0].ends_with("src/Main.java"));
    }
}
