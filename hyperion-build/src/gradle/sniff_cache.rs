// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_bytecode::gradle::GradleBuildConfig;
use hyperion_bytecode::gradle_plugin_executor::LoadedGradlePluginClasspath;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Filesystem identity for one cache-key path.
///
/// Existing ancestors are canonicalized so relative, `..`, and symbolic-link
/// spellings converge. `PathBuf` equality and hashing retain the operating
/// system's native path representation, including non-UTF-8 bytes on Unix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct FilesystemPathIdentity(PathBuf);

impl FilesystemPathIdentity {
    fn new(path: &Path) -> Self {
        Self(resolve_from_deepest_existing_ancestor(&absolute_path(path)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternalPluginCacheKey {
    root_dir: FilesystemPathIdentity,
    included_build_root: FilesystemPathIdentity,
    plugin_id: String,
    implementation_class: String,
}

impl InternalPluginCacheKey {
    pub fn new(
        root_dir: &Path,
        included_build_root: &Path,
        plugin_id: String,
        implementation_class: String,
    ) -> Self {
        Self {
            root_dir: FilesystemPathIdentity::new(root_dir),
            included_build_root: FilesystemPathIdentity::new(included_build_root),
            plugin_id,
            implementation_class,
        }
    }
}

/// Strong identity for the exact owned class snapshot and VM context.
///
/// The executor consumes the same `LoadedGradlePluginClasspath` whose bytes are
/// hashed here. Cache lookup and miss execution therefore cannot observe
/// different filesystem revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InternalPluginFingerprint([u8; 32]);

pub fn fingerprint_loaded_inputs(
    loaded: &LoadedGradlePluginClasspath,
    project_dir: &Path,
    subprojects: &[String],
    version_catalog_versions: &HashMap<String, String>,
) -> InternalPluginFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(b"hyperion-internal-plugin-snapshot-v2\0");

    hasher.update(b"classes\0");
    hasher.update((loaded.classes().len() as u64).to_le_bytes());
    for (name, bytes) in loaded.classes() {
        update_bytes(&mut hasher, name.as_bytes());
        update_bytes(&mut hasher, bytes);
    }

    // `execute_apply_loaded` stores this exact spelling in the resulting
    // GradleBuildConfig. Filesystem-equivalent aliases share a cache key, but
    // must not return a configuration containing another caller's path.
    hasher.update(b"project-dir\0");
    update_bytes(&mut hasher, project_dir.as_os_str().as_encoded_bytes());

    // Gradle's subproject set and version catalog are lookup context, not
    // order-sensitive input. The parser passes this same normalized subproject
    // vector to execution.
    let normalized_subprojects = subprojects
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    hasher.update(b"subprojects\0");
    hasher.update((normalized_subprojects.len() as u64).to_le_bytes());
    for subproject in normalized_subprojects {
        update_bytes(&mut hasher, subproject.as_bytes());
    }

    let normalized_versions = version_catalog_versions
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<BTreeMap<_, _>>();
    hasher.update(b"version-catalog\0");
    hasher.update((normalized_versions.len() as u64).to_le_bytes());
    for (key, value) in normalized_versions {
        update_bytes(&mut hasher, key.as_bytes());
        update_bytes(&mut hasher, value.as_bytes());
    }

    InternalPluginFingerprint(hasher.finalize().into())
}

fn update_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[derive(Debug, Clone)]
struct InternalPluginCacheEntry {
    fingerprint: InternalPluginFingerprint,
    config: Arc<GradleBuildConfig>,
}

#[derive(Debug, Default)]
struct InternalPluginCache {
    entries: HashMap<InternalPluginCacheKey, InternalPluginCacheEntry>,
}

/// Configurations can contain many dependencies and source paths, so keep a
/// strict process-wide entry bound. A single mutex makes the bound atomic;
/// plugin execution and snapshot loading happen outside it.
const MAX_CACHE_ENTRIES: usize = 256;

static CACHE: OnceLock<Mutex<InternalPluginCache>> = OnceLock::new();

fn cache() -> &'static Mutex<InternalPluginCache> {
    CACHE.get_or_init(|| Mutex::new(InternalPluginCache::default()))
}

pub fn get(
    key: &InternalPluginCacheKey,
    fingerprint: InternalPluginFingerprint,
) -> Option<Arc<GradleBuildConfig>> {
    let cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entries
        .get(key)
        .filter(|entry| entry.fingerprint == fingerprint)
        .map(|entry| entry.config.clone())
}

pub fn put(
    key: InternalPluginCacheKey,
    fingerprint: InternalPluginFingerprint,
    config: GradleBuildConfig,
) {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !cache.entries.contains_key(&key) && cache.entries.len() == MAX_CACHE_ENTRIES {
        // Cold misses are cheap relative to retaining another potentially large
        // configuration. Clearing is deterministic in capacity and avoids an
        // approximate concurrent eviction protocol.
        cache.entries.clear();
    }
    cache.entries.insert(
        key,
        InternalPluginCacheEntry {
            fingerprint,
            config: Arc::new(config),
        },
    );
    debug_assert!(cache.entries.len() <= MAX_CACHE_ENTRIES);
}

/// Explicitly release all cached entries (e.g. on LSP shutdown).
pub fn clear() {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entries
        .clear();
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

fn lexically_normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::ParentDir) | None if !path.has_root() => normalized.push(".."),
                Some(Component::Prefix(_)) if !path.has_root() => normalized.push(".."),
                _ => {}
            },
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

fn resolve_from_deepest_existing_ancestor(path: &Path) -> PathBuf {
    let mut probe = path.to_path_buf();
    let mut unresolved = Vec::<OsString>::new();

    loop {
        let candidate = if probe.as_os_str().is_empty() {
            Path::new(".")
        } else {
            probe.as_path()
        };
        if let Ok(existing) = fs::canonicalize(candidate) {
            let mut resolved = existing;
            for component in unresolved.iter().rev() {
                resolved.push(component);
            }
            return lexically_normalized(&resolved);
        }

        let Some(component) = probe.components().next_back() else {
            return lexically_normalized(path);
        };
        match component {
            Component::Normal(value) => unresolved.push(value.to_owned()),
            Component::ParentDir => unresolved.push(OsString::from("..")),
            Component::CurDir => unresolved.push(OsString::from(".")),
            Component::Prefix(_) | Component::RootDir => return lexically_normalized(path),
        };
        if !probe.pop() {
            return lexically_normalized(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn cache_keys_converge_for_real_and_symlinked_roots() {
        let directory = tempfile::tempdir().unwrap();
        let real_root = directory.path().join("real");
        let alias_root = directory.path().join("alias");
        std::fs::create_dir(&real_root).unwrap();
        std::os::unix::fs::symlink(&real_root, &alias_root).unwrap();

        let real = InternalPluginCacheKey::new(
            &real_root,
            &real_root.join(".").join("nested").join(".."),
            "example.plugin".to_string(),
            "example.Plugin".to_string(),
        );
        let alias = InternalPluginCacheKey::new(
            &alias_root,
            &alias_root,
            "example.plugin".to_string(),
            "example.Plugin".to_string(),
        );

        assert_eq!(real, alias);
    }

    #[cfg(unix)]
    #[test]
    fn parent_traversal_resolves_an_existing_symlink_prefix_first() {
        let directory = tempfile::tempdir().unwrap();
        let lexical_parent = directory.path().join("lexical");
        let target_parent = directory.path().join("target");
        let target_child = target_parent.join("child");
        std::fs::create_dir(&lexical_parent).unwrap();
        std::fs::create_dir_all(&target_child).unwrap();
        let alias = lexical_parent.join("alias");
        std::os::unix::fs::symlink(&target_child, &alias).unwrap();

        let through_alias =
            FilesystemPathIdentity::new(&alias.join("..").join("included-build").join("missing"));
        let filesystem_destination =
            FilesystemPathIdentity::new(&target_parent.join("included-build").join("missing"));
        let lexical_destination =
            FilesystemPathIdentity::new(&lexical_parent.join("included-build").join("missing"));

        assert_eq!(through_alias, filesystem_destination);
        assert_ne!(through_alias, lexical_destination);
    }

    #[test]
    fn inherited_execution_context_changes_the_fingerprint() {
        let loaded = LoadedGradlePluginClasspath::load(&[]).unwrap();
        let subprojects = vec!["api".to_string(), "core".to_string()];
        let old_versions = HashMap::from([("kotlin".to_string(), "1.9.0".to_string())]);
        let new_versions = HashMap::from([("kotlin".to_string(), "2.0.0".to_string())]);

        let project_dir = Path::new("/workspace");
        let old = fingerprint_loaded_inputs(&loaded, project_dir, &subprojects, &old_versions);
        let new = fingerprint_loaded_inputs(&loaded, project_dir, &subprojects, &new_versions);
        assert_ne!(old, new);

        let reordered = vec!["core".to_string(), "api".to_string(), "api".to_string()];
        assert_eq!(
            old,
            fingerprint_loaded_inputs(&loaded, project_dir, &reordered, &old_versions),
            "normalized subproject context must be order and duplicate insensitive"
        );
    }

    #[test]
    fn concurrent_puts_never_exceed_the_entry_bound() {
        clear();
        let directory = tempfile::tempdir().unwrap();
        let root = Arc::new(directory.path().to_path_buf());
        let mut workers = Vec::new();
        for worker in 0..8_u32 {
            let root = Arc::clone(&root);
            workers.push(std::thread::spawn(move || {
                for item in 0..(MAX_CACHE_ENTRIES as u32) {
                    let discriminator = worker * MAX_CACHE_ENTRIES as u32 + item;
                    let key = InternalPluginCacheKey::new(
                        &root,
                        &root,
                        format!("example.plugin.{discriminator}"),
                        "example.Plugin".to_string(),
                    );
                    let mut fingerprint = [0_u8; 32];
                    fingerprint[..4].copy_from_slice(&discriminator.to_le_bytes());
                    put(
                        key,
                        InternalPluginFingerprint(fingerprint),
                        GradleBuildConfig::new(),
                    );
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }

        let len = cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .len();
        assert!(len <= MAX_CACHE_ENTRIES, "cache retained {len} entries");
        clear();
    }
}
