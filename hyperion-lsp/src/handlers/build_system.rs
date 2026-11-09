// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Build system utilities: Gradle root detection, build file identification,
//! and build system refresh scheduling.
//!
//! Extracted from `server.rs` for maintainability.

mod coordinator;
mod refresh;
mod startup;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::time::{sleep, Duration};
use tower_lsp::Client;
use tracing::{debug, info};

pub use coordinator::BuildRefreshCoordinator;
pub(crate) use coordinator::{run_build_refresh, BuildReadinessCompletion, BuildRefreshTicket};
pub(crate) use refresh::{
    refresh_workspace_build_systems, retire_build_system, BuildSystemRefreshOutcome,
};
pub(crate) use startup::{spawn_initial_build_refresh, BuildProgressInputs};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BuildManifestFamily {
    Gradle,
    Maven,
}

impl BuildManifestFamily {
    const fn owner(self) -> &'static str {
        match self {
            Self::Gradle => "build:gradle",
            Self::Maven => "build:maven",
        }
    }

    const fn display_name(self) -> &'static str {
        match self {
            Self::Gradle => "Gradle",
            Self::Maven => "Maven",
        }
    }
}

/// Check for nested build roots when the workspace root itself is not a build.
///
/// Hyperion currently publishes one classpath owner per workspace root. It is
/// therefore safer to reject a nested independent build than to report an
/// empty, supposedly complete model. Traversal is bounded and runs on the
/// blocking pool; an I/O failure or safety-bound hit is treated as unknown.
pub(crate) async fn has_nested_build_manifest_or_unknown(
    root: &Path,
    family: BuildManifestFamily,
) -> bool {
    const MAX_BUILD_DISCOVERY_ENTRIES: usize = 100_000;

    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut stack = vec![root.clone()];
        let mut entries_seen = 0usize;
        while let Some(directory) = stack.pop() {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    debug!(
                        "Cannot prove build-model absence below {}: {}",
                        directory.display(),
                        error
                    );
                    return true;
                }
            };
            for entry in entries {
                entries_seen += 1;
                if entries_seen > MAX_BUILD_DISCOVERY_ENTRIES {
                    debug!(
                        "Cannot prove build-model absence below {} within {} entries",
                        root.display(),
                        MAX_BUILD_DISCOVERY_ENTRIES
                    );
                    return true;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        debug!(
                            "Cannot inspect a build-discovery entry below {}: {}",
                            directory.display(),
                            error
                        );
                        return true;
                    }
                };
                let path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        debug!(
                            "Cannot inspect build-discovery path {}: {}",
                            path.display(),
                            error
                        );
                        return true;
                    }
                };
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    // A nested build may legitimately use any ordinary
                    // directory name, including conventional output names.
                    // Only tool metadata which cannot own a source build is
                    // excluded from this proof-of-absence traversal.
                    if !matches!(name, ".git" | ".gradle" | ".idea" | ".m2") {
                        stack.push(path);
                    }
                    continue;
                }
                if !file_type.is_file() || path.parent() == Some(root.as_path()) {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                let is_manifest = match family {
                    BuildManifestFamily::Gradle => matches!(
                        name,
                        "build.gradle"
                            | "build.gradle.kts"
                            | "settings.gradle"
                            | "settings.gradle.kts"
                    ),
                    BuildManifestFamily::Maven => name == "pom.xml",
                };
                if is_manifest {
                    return true;
                }
            }
        }
        false
    })
    .await
    .unwrap_or(true)
}

/// Find the root of a Gradle multi-module project by looking for settings.gradle.
///
/// For multi-module Gradle projects, the settings.gradle file at the root defines
/// which subprojects exist. If we're given a path inside a subproject, we should
/// walk up the tree to find the actual project root.
///
/// This enables proper resolution of cross-module dependencies (e.g., a service
/// depending on a shared library in the same monorepo).
pub fn find_gradle_root(path: &Path) -> PathBuf {
    match hyperion_layout::find_settings_root(path, None) {
        Some(root) => {
            if root != path {
                info!(
                    "Found Gradle multi-module root: {:?} (was {:?})",
                    root, path
                );
            }
            root
        }
        None => path.to_path_buf(),
    }
}

/// Schedule a debounced build system refresh (Gradle + Maven).
///
/// Prevents repeated indexing when build files change rapidly.
pub(crate) fn schedule_build_system_refresh(
    coordinator: BuildRefreshCoordinator,
    root: PathBuf,
    client: Client,
    revision_publisher: crate::server::diagnostics::InvalidationRuntime,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
    reason: &str,
) {
    let reason = reason.to_string();
    let readiness = Arc::clone(&revision_publisher.destructive_refactoring);
    coordinator.spawn_with(
        move || BuildReadinessCompletion::reserve(readiness),
        move |refresh_ticket, mut readiness| async move {
            let work = async {
                sleep(Duration::from_millis(500)).await;
                debug!("Refreshing build system after {}", reason);

                let bulk_source_index =
                    crate::server::source_indexing::BulkSourceIndexContext::from(
                        &revision_publisher,
                    );
                refresh_workspace_build_systems(
                    &root,
                    &revision_publisher.host,
                    &revision_publisher.workspace_ready,
                    &source_providers,
                    bulk_source_index,
                    &revision_publisher.project_classpaths,
                    &client,
                )
                .await
            };
            let finalize_publisher = revision_publisher.clone();
            let finalize = |outcome: BuildSystemRefreshOutcome| async move {
                finalize_publisher
                    .publish_workspace_revision("build-system refresh")
                    .await;
                readiness.finish(outcome.gradle_complete, outcome.maven_complete);
                let () = finalize_publisher
                    .client
                    .log_message(
                        if outcome.dependency_issues.is_empty() {
                            tower_lsp::lsp_types::MessageType::INFO
                        } else {
                            tower_lsp::lsp_types::MessageType::WARNING
                        },
                        format!(
                            "Build system refreshed: {} dependency JARs indexed, {} dependency problems",
                            outcome.dependency_jars_indexed,
                            outcome.dependency_issues.len()
                        ),
                    )
                    .await;
            };
            Box::pin(run_build_refresh(refresh_ticket, work, finalize)).await;
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{find_gradle_root, has_nested_build_manifest_or_unknown, BuildManifestFamily};

    #[test]
    fn gradle_root_is_the_nearest_settings_directory_however_far_up() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("settings.gradle.kts"), "").unwrap();
        let deep = root.join("a/b/c/d/e/f/g/h/i/j");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_gradle_root(&deep), root);
        // No settings file anywhere: the given directory is kept, even when an
        // ancestor has a build.gradle.
        let lone = tempfile::tempdir().unwrap();
        std::fs::write(lone.path().join("build.gradle"), "").unwrap();
        let sub = lone.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(find_gradle_root(&sub), sub);
    }

    #[tokio::test]
    async fn nested_independent_builds_are_not_mistaken_for_absence() {
        let workspace = tempfile::tempdir().unwrap();
        let nested = workspace.path().join("services/api");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("pom.xml"), "<project/>").unwrap();

        assert!(
            has_nested_build_manifest_or_unknown(workspace.path(), BuildManifestFamily::Maven)
                .await
        );
        assert!(
            !has_nested_build_manifest_or_unknown(workspace.path(), BuildManifestFamily::Gradle)
                .await
        );
    }

    #[tokio::test]
    async fn nested_builds_under_output_named_directories_are_not_skipped() {
        let workspace = tempfile::tempdir().unwrap();
        let nested = workspace.path().join("target/generated/build");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("pom.xml"), "<project/>").unwrap();

        assert!(
            has_nested_build_manifest_or_unknown(workspace.path(), BuildManifestFamily::Maven)
                .await
        );
    }
}
