// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Revision proof for workspace sources and name-bearing resources.
//!
//! JVM behavior is not described by source files alone: service descriptors,
//! Spring XML, JSON configuration, and native-image metadata can name classes
//! and members. Destructive refactorings therefore scan the bounded resource
//! manifest and retain its content fingerprint alongside the exact source-file
//! manifest.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::{CurrentUsageQueryError, Language, PathLookupKey, MAX_SOURCE_FILE_BYTES};
use sha2::{Digest, Sha256};

// Crossing a manifest limit fails closed; it never silently removes resources
// from the negative proof.
const MAX_RESOURCE_WALK_ENTRIES: usize = 250_000;
const MAX_RESOURCE_FILES: usize = 50_000;
const MAX_RESOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SOURCE_PROOF_FILES: usize = 100_000;
const MAX_SOURCE_PROOF_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResourceRevision {
    path: PathBuf,
    content_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFilesystemRevision {
    path: PathLookupKey,
    content_hash: [u8; 32],
}

/// One indexed source expected to be represented by the filesystem manifest.
/// Disk publications carry their indexed byte hash; editor-owned publications
/// deliberately do not require the on-disk bytes to match the open buffer.
#[derive(Clone, Debug)]
pub(crate) struct RefactoringSourceInput {
    pub(crate) path: PathBuf,
    pub(crate) indexed_disk_hash: Option<u64>,
}

struct RefactoringFilesystemSnapshot {
    resources: Vec<ResourceRevision>,
    sources: Vec<SourceFilesystemRevision>,
}

/// Exact negative proof over workspace sources and non-source files that can
/// drive JVM behavior.
#[derive(Clone, Debug)]
pub struct RefactoringResourceProof {
    root: PathBuf,
    target_names: Arc<[Arc<str>]>,
    resource_revisions: Arc<[ResourceRevision]>,
    source_revisions: Arc<[SourceFilesystemRevision]>,
}

impl RefactoringResourceProof {
    pub(crate) fn capture<F>(
        root: &Path,
        target_names: impl IntoIterator<Item = Arc<str>>,
        indexed_sources: &[RefactoringSourceInput],
        is_cancelled: &F,
    ) -> Result<Self, CurrentUsageQueryError>
    where
        F: Fn() -> bool + ?Sized,
    {
        let mut target_names = target_names.into_iter().collect::<Vec<_>>();
        target_names.sort_unstable();
        target_names.dedup();
        let snapshot =
            scan_refactoring_filesystem(root, &target_names, Some(indexed_sources), is_cancelled)?;
        Ok(Self {
            root: root.to_path_buf(),
            target_names: Arc::from(target_names),
            resource_revisions: Arc::from(snapshot.resources),
            source_revisions: Arc::from(snapshot.sources),
        })
    }

    /// Rewalk and rehash the manifest. This fences additions, removals, and
    /// byte changes rather than revalidating only files that existed before.
    pub fn is_current(&self) -> bool {
        self.is_current_cancellable(&|| false)
    }

    /// Cancellable variant for request-time proof fences. Filesystem walks and
    /// hashing belong on a blocking worker, but cooperative cancellation still
    /// keeps a superseded refactoring from finishing the rest of the manifest.
    pub fn is_current_cancellable<F>(&self, is_cancelled: &F) -> bool
    where
        F: Fn() -> bool + ?Sized,
    {
        scan_refactoring_filesystem(&self.root, &self.target_names, None, is_cancelled).is_ok_and(
            |current| {
                current.resources.as_slice() == self.resource_revisions.as_ref()
                    && current.sources.as_slice() == self.source_revisions.as_ref()
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn file_count(&self) -> usize {
        self.resource_revisions.len()
    }
}

fn scan_refactoring_filesystem<F>(
    root: &Path,
    target_names: &[Arc<str>],
    indexed_sources: Option<&[RefactoringSourceInput]>,
    is_cancelled: &F,
) -> Result<RefactoringFilesystemSnapshot, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
{
    let root_key = crate::path_lookup_key(root);
    let mut expected_sources = indexed_sources
        .map(|sources| {
            let mut expected = HashMap::with_capacity(sources.len());
            for source in sources {
                let path = crate::path_lookup_key(&source.path);
                if !path.starts_with(&root_key) {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "indexed source {} is outside refactoring proof root {}",
                        source.path.display(),
                        root.display()
                    )));
                }
                if expected.insert(path, source.indexed_disk_hash).is_some() {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "indexed source manifest contains duplicate path {}",
                        source.path.display()
                    )));
                }
            }
            Ok(expected)
        })
        .transpose()?;
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        // Build tools routinely copy ignored resources into live artifacts.
        // Editor-search ignore policy is therefore unsound for this proof.
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false);
    let filter_root = root.to_path_buf();
    builder.filter_entry(move |entry| resource_walk_entry(&filter_root, entry.path()));
    let mut resource_revisions = Vec::new();
    let mut source_revisions = Vec::new();
    let mut walked_entries = 0usize;
    let mut scanned_resource_bytes = 0u64;
    let mut scanned_source_bytes = 0u64;
    for entry in builder.build() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        let entry = entry.map_err(|error| {
            CurrentUsageQueryError::Unavailable(format!(
                "refactoring filesystem scan of {} failed: {error}",
                root.display()
            ))
        })?;
        walked_entries = walked_entries.checked_add(1).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable("resource manifest size overflow".to_string())
        })?;
        if walked_entries > MAX_RESOURCE_WALK_ENTRIES {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "resource manifest exceeds the {MAX_RESOURCE_WALK_ENTRIES} entry proof limit"
            )));
        }
        let file_type = entry.file_type();
        if file_type.is_some_and(|kind| kind.is_symlink()) {
            // `WalkBuilder` deliberately does not follow links: doing so could
            // escape the bounded workspace tree or introduce cycles. Silently
            // ignoring a link is nevertheless unsound for a destructive
            // refactoring because it may name a live source or resource. Keep
            // the proof conservative until linked roots can be canonicalized
            // and fenced explicitly.
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "refactoring resource proof cannot validate symbolic link {}",
                entry.path().display()
            )));
        }
        if !file_type.is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if Language::from_path(path).is_some() {
            if source_revisions.len() >= MAX_SOURCE_PROOF_FILES {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "source manifest exceeds the {MAX_SOURCE_PROOF_FILES} file proof limit"
                )));
            }
            let path_key = crate::path_lookup_key(path);
            let expected_disk_hash = match expected_sources.as_mut() {
                Some(expected) => expected.remove(&path_key).ok_or_else(|| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "source {} exists on disk but has no indexed revision",
                        path.display()
                    ))
                })?,
                None => None,
            };
            let source = crate::read_source_file(path).map_err(|error| {
                CurrentUsageQueryError::Unavailable(format!(
                    "cannot read refactoring source {}: {error}",
                    path.display()
                ))
            })?;
            scanned_source_bytes = scanned_source_bytes
                .checked_add(source.len() as u64)
                .ok_or_else(|| {
                    CurrentUsageQueryError::Unavailable(
                        "source proof byte count overflow".to_string(),
                    )
                })?;
            if scanned_source_bytes > MAX_SOURCE_PROOF_BYTES {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "source manifest exceeds the {MAX_SOURCE_PROOF_BYTES} byte proof limit"
                )));
            }
            if expected_disk_hash
                .is_some_and(|expected| crate::index::compute_content_hash(&source) != expected)
            {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "disk source {} changed before its indexed revision was published",
                    path.display()
                )));
            }
            source_revisions.push(SourceFilesystemRevision {
                path: path_key,
                content_hash: Sha256::digest(source.as_bytes()).into(),
            });
            continue;
        }
        if !is_name_bearing_resource(root, path) {
            continue;
        }
        if resource_revisions.len() >= MAX_RESOURCE_FILES {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "resource manifest exceeds the {MAX_RESOURCE_FILES} file proof limit"
            )));
        }
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        let relative_path = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        if let Some(name) = target_names
            .iter()
            .find(|name| contains_identifier_bytes(relative_path.as_bytes(), name.as_bytes()))
        {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains a possible runtime/configuration reference to '{}' in its resource path",
                path.display(),
                name
            )));
        }
        let metadata = entry.metadata().map_err(|error| {
            CurrentUsageQueryError::Unavailable(format!(
                "cannot inspect refactoring resource {}: {error}",
                path.display()
            ))
        })?;
        if metadata.len() > MAX_SOURCE_FILE_BYTES as u64 {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "refactoring resource {} exceeds the {} byte proof limit",
                path.display(),
                MAX_SOURCE_FILE_BYTES
            )));
        }
        let file = std::fs::File::open(path).map_err(|error| {
            CurrentUsageQueryError::Unavailable(format!(
                "cannot read refactoring resource {}: {error}",
                path.display()
            ))
        })?;
        // Metadata is only a fast preflight: the file can grow between stat
        // and open. Bound the actual read as well so a racing resource can
        // never turn a refactoring proof into an unbounded allocation.
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_SOURCE_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                CurrentUsageQueryError::Unavailable(format!(
                    "cannot read refactoring resource {}: {error}",
                    path.display()
                ))
            })?;
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if bytes.len() > MAX_SOURCE_FILE_BYTES {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "refactoring resource {} exceeds the {} byte proof limit",
                path.display(),
                MAX_SOURCE_FILE_BYTES
            )));
        }
        scanned_resource_bytes = scanned_resource_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| {
                CurrentUsageQueryError::Unavailable("resource byte count overflow".to_string())
            })?;
        if scanned_resource_bytes > MAX_RESOURCE_BYTES {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "resource manifest exceeds the {MAX_RESOURCE_BYTES} byte proof limit"
            )));
        }
        if let Some(name) = target_names
            .iter()
            .find(|name| contains_identifier_bytes(&bytes, name.as_bytes()))
        {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains a possible runtime/configuration reference to '{}'",
                path.display(),
                name
            )));
        }
        let content_hash = Sha256::digest(&bytes).into();
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        resource_revisions.push(ResourceRevision {
            path: path.to_path_buf(),
            content_hash,
        });
    }
    if is_cancelled() {
        return Err(CurrentUsageQueryError::Cancelled);
    }
    if let Some(expected) = expected_sources {
        if let Some((path, indexed_disk_hash)) = expected
            .into_iter()
            .find(|(path, indexed_disk_hash)| indexed_disk_hash.is_some() || path.exists())
        {
            let reason = if indexed_disk_hash.is_some() {
                "indexed disk source is missing from the filesystem manifest"
            } else {
                "editor-owned source exists outside the bounded filesystem manifest"
            };
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{reason}: {}",
                path.display()
            )));
        }
    }
    resource_revisions.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    source_revisions.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    if is_cancelled() {
        return Err(CurrentUsageQueryError::Cancelled);
    }
    Ok(RefactoringFilesystemSnapshot {
        resources: resource_revisions,
        sources: source_revisions,
    })
}

/// Keep source resources and resource-bearing JVM output directories, but do
/// not descend into VCS/tool state or native compilation products.
fn resource_walk_entry(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            ".git" | ".hg" | ".svn" | ".gradle" | ".idea" | ".vscode" | "node_modules"
        )
    }) {
        return false;
    }
    for (index, component) in components.iter().enumerate() {
        if components[..index].contains(&"resources") {
            continue;
        }
        let allowed_children: Option<&[&str]> = match *component {
            // Maven and Cargo both use `target`; retain only JVM directories.
            "target" => Some(&[
                "classes",
                "test-classes",
                "generated-sources",
                "generated-test-sources",
                "generated-resources",
            ]),
            // A Gradle module's `build` directory holds compiled classes and
            // copied resources (live artifacts) next to reports, logs and test
            // results that only mention names incidentally. `build` is also a
            // legitimate package name, so require a Gradle build script beside it.
            "build" if is_gradle_module_dir(root, &components[..index]) => {
                Some(&["classes", "resources", "generated", "kotlin"])
            }
            _ => None,
        };
        if let (Some(allowed), Some(child)) = (allowed_children, components.get(index + 1)) {
            return allowed.contains(child);
        }
    }
    true
}

fn is_gradle_module_dir(root: &Path, relative_components: &[&str]) -> bool {
    let module = relative_components
        .iter()
        .fold(root.to_path_buf(), |path, component| path.join(component));
    ["build.gradle", "build.gradle.kts"]
        .iter()
        .any(|script| module.join(script).is_file())
}

fn is_name_bearing_resource(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    let service_descriptor = components
        .windows(2)
        .any(|pair| pair == ["META-INF", "services"]);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let jvm_metadata = file_name.eq_ignore_ascii_case("MANIFEST.MF")
        || file_name.eq_ignore_ascii_case("spring.factories")
        || matches!(
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("factories" | "imports" | "handlers" | "schemas" | "tooling" | "mf")
        );
    let configured_text = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "cfg"
                    | "conf"
                    | "config"
                    | "csv"
                    | "graphql"
                    | "htm"
                    | "html"
                    | "ini"
                    | "json"
                    | "list"
                    | "md"
                    | "properties"
                    | "sql"
                    | "toml"
                    | "txt"
                    | "xml"
                    | "yaml"
                    | "yml"
            )
        });
    service_descriptor || jvm_metadata || configured_text
}

fn contains_identifier_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    // A non-ASCII neighbouring byte may be part of an identifier, or it may
    // be punctuation such as a curly quote or em dash. Treating every such
    // byte as an identifier part can hide an actual name; treating it as a
    // boundary can only make this destructive-operation proof more
    // conservative.
    let is_identifier_part = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$');
    haystack
        .windows(needle.len())
        .enumerate()
        .any(|(start, window)| {
            window == needle
                && (start == 0 || !is_identifier_part(haystack[start - 1]))
                && (start + needle.len() == haystack.len()
                    || !is_identifier_part(haystack[start + needle.len()]))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_proof_detects_service_and_configuration_names() {
        let root = tempfile::tempdir().unwrap();
        let services = root.path().join("src/main/resources/META-INF/services");
        std::fs::create_dir_all(&services).unwrap();
        std::fs::write(services.join("example.Service"), "example.Impl\n").unwrap();
        let error = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Impl")],
            &[],
            &|| false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Impl"));
    }

    #[test]
    fn service_interface_names_in_paths_are_refactoring_uses() {
        let root = tempfile::tempdir().unwrap();
        let services = root.path().join("src/main/resources/META-INF/services");
        std::fs::create_dir_all(&services).unwrap();
        std::fs::write(services.join("example.Plugin"), "example.Impl\n").unwrap();

        let error = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Plugin")],
            &[],
            &|| false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("resource path"));
    }

    #[test]
    fn unrelated_binary_resources_do_not_disable_refactoring() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("src/main/resources/static");
        std::fs::create_dir_all(&resources).unwrap();
        let image = resources.join("banner.png");
        let file = std::fs::File::create(&image).unwrap();
        file.set_len(MAX_SOURCE_FILE_BYTES as u64 + 1).unwrap();

        let proof = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &[],
            &|| false,
        )
        .unwrap();
        assert_eq!(proof.file_count(), 0);
    }

    #[test]
    fn resource_proof_fences_manifest_and_bytes() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("src/main/resources");
        std::fs::create_dir_all(&resources).unwrap();
        let config = resources.join("application.json");
        std::fs::write(&config, r#"{"mode":"ordinary"}"#).unwrap();
        let proof = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &[],
            &|| false,
        )
        .unwrap();
        assert_eq!(proof.file_count(), 1);
        assert!(proof.is_current());

        std::fs::write(&config, r#"{"class":"Dead"}"#).unwrap();
        assert!(!proof.is_current());
    }

    #[test]
    fn ignored_resources_and_jvm_metadata_remain_in_the_proof() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("src/main/resources/META-INF");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(root.path().join(".gitignore"), "src/main/resources/\n").unwrap();
        std::fs::write(
            resources.join("spring.factories"),
            "example.Factory=example.Dead\n",
        )
        .unwrap();

        let error = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &[],
            &|| false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("spring.factories"));
    }

    /// Gradle reports and test results under `build/` mention names
    /// incidentally; they must not veto a rename. Copied resources still count,
    /// and a package named `build` is ordinary source.
    #[test]
    fn gradle_build_reports_do_not_block_but_copied_resources_and_build_packages_do() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("build.gradle"), "plugins { id 'java' }").unwrap();
        let reports = root.path().join("build/reports/problems");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(
            reports.join("problems-report.html"),
            "<p>Dead is mentioned</p>",
        )
        .unwrap();
        let package = root.path().join("src/main/java/acme/build");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("Task.java"),
            "package acme.build; class Task {}",
        )
        .unwrap();
        let sources = [
            RefactoringSourceInput {
                path: package.join("Task.java"),
                indexed_disk_hash: None,
            },
            RefactoringSourceInput {
                path: root.path().join("build.gradle"),
                indexed_disk_hash: None,
            },
        ];

        RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &sources,
            &|| false,
        )
        .expect("build reports are not name-bearing resources");

        let resources = root.path().join("build/resources/main/META-INF");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("MANIFEST.MF"), "Main-Class: example.Dead\n").unwrap();
        let error = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &sources,
            &|| false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("MANIFEST.MF"), "{error}");
    }

    #[test]
    fn generated_build_resources_are_live_but_native_targets_are_pruned() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("build/resources/main/META-INF");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("MANIFEST.MF"), "Main-Class: example.Dead\n").unwrap();
        let native = root.path().join("target/debug/incremental");
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(native.join("ordinary.txt"), "Dead").unwrap();

        let error = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &[],
            &|| false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("MANIFEST.MF"));
    }

    #[test]
    fn resource_proof_revalidation_observes_walk_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("src/main/resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("first.json"), r#"{"mode":"ordinary"}"#).unwrap();
        std::fs::write(resources.join("second.json"), r#"{"mode":"ordinary"}"#).unwrap();
        let proof = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &[],
            &|| false,
        )
        .unwrap();
        let checks = std::cell::Cell::new(0usize);
        assert!(!proof.is_current_cancellable(&|| {
            checks.set(checks.get() + 1);
            checks.get() > 1
        }));
        assert!(checks.get() > 1);
    }

    #[test]
    fn source_manifest_fences_unpublished_disk_changes_and_new_files() {
        let root = tempfile::tempdir().unwrap();
        let sources = root.path().join("src/main/java/example");
        std::fs::create_dir_all(&sources).unwrap();
        let live = sources.join("Live.java");
        let indexed = "package example; class Live {}";
        std::fs::write(&live, indexed).unwrap();
        let indexed_sources = [RefactoringSourceInput {
            path: live.clone(),
            indexed_disk_hash: Some(crate::index::compute_content_hash(indexed)),
        }];

        let proof = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &indexed_sources,
            &|| false,
        )
        .unwrap();
        assert!(proof.is_current());

        std::fs::write(&live, "package example; class Changed {}").unwrap();
        assert!(!proof.is_current());
        let stale = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &indexed_sources,
            &|| false,
        )
        .unwrap_err();
        assert!(stale.to_string().contains("changed before"));

        std::fs::write(&live, indexed).unwrap();
        let unpublished = sources.join("Unpublished.java");
        std::fs::write(&unpublished, "package example; class Unpublished {}").unwrap();
        let unindexed = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &indexed_sources,
            &|| false,
        )
        .unwrap_err();
        assert!(unindexed.to_string().contains("no indexed revision"));

        std::fs::remove_file(&unpublished).unwrap();
        let proof = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &indexed_sources,
            &|| false,
        )
        .unwrap();
        std::fs::write(&unpublished, "package example; class Unpublished {}").unwrap();
        assert!(!proof.is_current());
    }

    #[test]
    fn maven_generated_source_roots_participate_in_the_manifest() {
        let root = tempfile::tempdir().unwrap();
        let main = root
            .path()
            .join("target/generated-sources/annotations/example/Generated.java");
        let test = root
            .path()
            .join("target/generated-test-sources/test-annotations/example/GeneratedTest.java");
        std::fs::create_dir_all(main.parent().unwrap()).unwrap();
        std::fs::create_dir_all(test.parent().unwrap()).unwrap();
        let main_text = "package example; class Generated {}";
        let test_text = "package example; class GeneratedTest {}";
        std::fs::write(&main, main_text).unwrap();
        std::fs::write(&test, test_text).unwrap();
        let indexed_sources = [
            RefactoringSourceInput {
                path: main,
                indexed_disk_hash: Some(crate::index::compute_content_hash(main_text)),
            },
            RefactoringSourceInput {
                path: test,
                indexed_disk_hash: Some(crate::index::compute_content_hash(test_text)),
            },
        ];

        let proof = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &indexed_sources,
            &|| false,
        )
        .unwrap();
        assert!(proof.is_current());
    }

    #[test]
    fn resource_names_use_identifier_boundaries() {
        assert!(!contains_identifier_bytes(b"target budget", b"get"));
        assert!(contains_identifier_bytes(b"target \"get\"", b"get"));
        assert!(contains_identifier_bytes(
            "Dead—implementation".as_bytes(),
            b"Dead"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_resources_fail_closed() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let external = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(external.path(), "implementation=example.Dead\n").unwrap();
        let resources = root.path().join("src/main/resources");
        std::fs::create_dir_all(&resources).unwrap();
        let linked = resources.join("application.properties");
        symlink(external.path(), &linked).unwrap();

        let error = RefactoringResourceProof::capture(
            root.path(),
            [Arc::<str>::from("Dead")],
            &[],
            &|| false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("symbolic link"));
        assert!(error.to_string().contains("application.properties"));
    }
}
