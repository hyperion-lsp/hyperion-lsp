// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Source attachments from Gradle/Maven `-sources.jar` artifacts.

use super::archive_snapshot::snapshot_to_temp_path;
use super::cache::SourceCache;
use super::temp_file::AttachmentLanguage;
use super::MaterializedSourceStore;
use super::{ExternalSourceRequest, SourceContent, SourceFile, SourceProvider};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};
use zip::ZipArchive;

use hyperion_workspace::{ExternalSourceArchive, ExternalSourceIdentity};

const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SOURCE_ENTRIES: usize = 250_000;
const MAX_ARCHIVE_PATH_BYTES: usize = 4 * 1024;
const MAX_SOURCE_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TOTAL_SOURCE_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_SOURCE_ARCHIVES: usize = 512;
const MAX_TOTAL_SOURCE_ENTRIES: usize = 1_000_000;
const MAX_TOTAL_SOURCE_INDEX_BYTES: usize = 128 * 1024 * 1024;
const MAX_INSPECTED_BINARY_ARCHIVES: usize = 4_096;
const MAX_GRADLE_VERSION_DIRECTORY_ENTRIES: usize = 4_096;
const SOURCE_INDEX_ENTRY_OVERHEAD: usize = 96;

/// Provides dependency source from source JARs paired with the indexed binary JARs.
pub struct JarSourceProvider {
    /// Exact binary archive identity -> its immutable source attachment.
    archives: HashMap<ExternalSourceIdentity, Arc<SourceArchive>>,
    materialized_sources: Arc<MaterializedSourceStore>,
    /// Shared across every archive and bounded once for the whole provider
    /// generation, including immutable fallback snapshots.
    content_cache: Arc<parking_lot::Mutex<SourceCache<CachedSourceKey>>>,
}

#[derive(Eq, Hash, PartialEq)]
struct CachedSourceKey {
    source_identity: ExternalSourceIdentity,
    fqn: String,
}

/// One source attachment copied and indexed as a private immutable snapshot.
struct SourceArchive {
    path: PathBuf,
    snapshot_bytes: u64,
    index_bytes: usize,
    entries: HashMap<String, String>,
    snapshot: tempfile::TempPath,
}

impl JarSourceProvider {
    /// Legacy constructor retained for source compatibility.
    pub fn from_binary_jars(binary_jars: &[PathBuf]) -> Option<Self> {
        Self::from_binary_jars_with_store(binary_jars, Arc::new(MaterializedSourceStore::new()))
    }

    /// Discover sibling `-sources.jar` artifacts for resolved binary JARs.
    pub fn from_binary_jars_with_store(
        binary_jars: &[PathBuf],
        materialized_sources: Arc<MaterializedSourceStore>,
    ) -> Option<Self> {
        let binary_archives: Vec<_> = binary_jars
            .iter()
            .map(|path| ExternalSourceArchive::capture(path))
            .collect();
        Self::from_binary_archives_with_store(&binary_archives, materialized_sources)
    }

    /// Discover sources for archive identities captured by the owning
    /// classpath refresh.
    pub fn from_binary_archives_with_store(
        binary_archives: &[ExternalSourceArchive],
        materialized_sources: Arc<MaterializedSourceStore>,
    ) -> Option<Self> {
        Self::from_binary_archives_with_limit(
            binary_archives,
            materialized_sources,
            MAX_SOURCE_ARCHIVES,
        )
    }

    fn from_binary_archives_with_limit(
        binary_archives: &[ExternalSourceArchive],
        materialized_sources: Arc<MaterializedSourceStore>,
        maximum_archives: usize,
    ) -> Option<Self> {
        let mut archives = HashMap::new();
        let mut snapshot_bytes = 0_u64;
        let mut source_entries = 0_usize;
        let mut source_index_bytes = 0_usize;
        let mut inspected_binary_archives = 0_usize;
        for binary_archive in binary_archives {
            let binary_jar = binary_archive
                .identity()
                .archive_path()
                .expect("a captured binary archive must have an archive path");
            let binary_identity = binary_archive.identity().clone();
            if archives.contains_key(&binary_identity) {
                continue;
            }
            inspected_binary_archives += 1;
            if inspected_binary_archives > MAX_INSPECTED_BINARY_ARCHIVES {
                warn!(
                    "Dependency source discovery exceeds {} binary archives; retaining the bounded partial source index",
                    MAX_INSPECTED_BINARY_ARCHIVES
                );
                break;
            }
            let Some(source_jar) = find_sibling_sources_jar(binary_jar) else {
                continue;
            };
            if archives.len() >= maximum_archives {
                warn!(
                    "Dependency source archive count exceeds {}; retaining the bounded partial source index",
                    maximum_archives
                );
                break;
            }
            match SourceArchive::open(&source_jar, &materialized_sources) {
                Ok(archive) if !archive.entries.is_empty() => {
                    let Some(next_snapshot_bytes) =
                        snapshot_bytes.checked_add(archive.snapshot_bytes)
                    else {
                        warn!("Dependency source snapshot byte total overflowed; retaining the bounded partial source index");
                        break;
                    };
                    if next_snapshot_bytes > MAX_TOTAL_SOURCE_ARCHIVE_BYTES {
                        warn!(
                            "Dependency source snapshots exceed {} bytes; retaining the bounded partial source index",
                            MAX_TOTAL_SOURCE_ARCHIVE_BYTES
                        );
                        break;
                    }
                    let Some(next_source_entries) =
                        source_entries.checked_add(archive.entries.len())
                    else {
                        warn!("Dependency source entry total overflowed; retaining the bounded partial source index");
                        break;
                    };
                    if next_source_entries > MAX_TOTAL_SOURCE_ENTRIES {
                        warn!(
                            "Dependency source index exceeds {} entries; retaining the bounded partial source index",
                            MAX_TOTAL_SOURCE_ENTRIES
                        );
                        break;
                    }
                    let Some(next_source_index_bytes) =
                        source_index_bytes.checked_add(archive.index_bytes)
                    else {
                        warn!("Dependency source index byte total overflowed; retaining the bounded partial source index");
                        break;
                    };
                    if next_source_index_bytes > MAX_TOTAL_SOURCE_INDEX_BYTES {
                        warn!(
                            "Dependency source index exceeds {} retained bytes; retaining the bounded partial source index",
                            MAX_TOTAL_SOURCE_INDEX_BYTES
                        );
                        break;
                    }
                    snapshot_bytes = next_snapshot_bytes;
                    source_entries = next_source_entries;
                    source_index_bytes = next_source_index_bytes;
                    archives.insert(binary_identity, Arc::new(archive));
                }
                Ok(_) => {}
                Err(error) => warn!(
                    "Failed to index source JAR {} for {}: {error}",
                    source_jar.display(),
                    binary_jar.display()
                ),
            }
        }
        if archives.is_empty() {
            return None;
        }

        info!(
            "Indexed {} dependency source files from {} binary-bound source JARs",
            source_entries,
            archives.len()
        );
        Some(Self {
            archives,
            materialized_sources,
            content_cache: Arc::new(parking_lot::Mutex::new(SourceCache::new())),
        })
    }

    fn archive_for(&self, source_identity: &ExternalSourceIdentity) -> Option<&SourceArchive> {
        self.archives.get(source_identity).map(Arc::as_ref)
    }

    fn unique_source_identity_for_fqn(&self, fqn: &str) -> Option<&ExternalSourceIdentity> {
        let mut source_identities = self
            .archives
            .iter()
            .filter(|(_, archive)| archive.find_source_entry(fqn).is_some())
            .map(|(source_identity, _)| source_identity);
        let source_identity = source_identities.next()?;
        source_identities
            .next()
            .is_none()
            .then_some(source_identity)
    }

    fn extract_source(&self, request: ExternalSourceRequest<'_>) -> Result<SourceContent, String> {
        let archive = self
            .archive_for(request.source_identity())
            .ok_or_else(|| format!("No source attachment for {:?}", request.source_identity()))?;
        archive.extract_source(
            request.source_identity(),
            request.fqn(),
            &self.materialized_sources,
            &self.content_cache,
        )
    }
}

impl SourceArchive {
    fn open(path: &Path, store: &MaterializedSourceStore) -> Result<Self, String> {
        let (snapshot, snapshot_bytes) =
            snapshot_to_temp_path(path, MAX_SOURCE_ARCHIVE_BYTES, store)?;
        let file = File::open(&snapshot).map_err(|error| error.to_string())?;
        let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
        let (entries, index_bytes) = index_source_archive(&mut archive)?;
        drop(archive);
        Ok(Self {
            path: path.to_path_buf(),
            snapshot_bytes,
            index_bytes,
            entries,
            snapshot,
        })
    }

    fn find_source_entry<'a>(&'a self, fqn: &str) -> Option<(&'a str, &'a str)> {
        let mut candidate = fqn.split_once('(').map_or(fqn, |(prefix, _)| prefix);
        loop {
            if let Some((key, entry)) = self.entries.get_key_value(candidate) {
                return Some((key.as_str(), entry.as_str()));
            }
            candidate = candidate.rsplit_once('.')?.0;
        }
    }

    fn extract_source(
        &self,
        source_identity: &ExternalSourceIdentity,
        requested_fqn: &str,
        materialized_sources: &MaterializedSourceStore,
        content_cache: &parking_lot::Mutex<SourceCache<CachedSourceKey>>,
    ) -> Result<SourceContent, String> {
        let (source_fqn, entry_path) = self
            .find_source_entry(requested_fqn)
            .ok_or_else(|| format!("No source attachment for {requested_fqn}"))?;
        let cache_key = CachedSourceKey {
            source_identity: source_identity.clone(),
            fqn: source_fqn.to_string(),
        };
        let cached_source = content_cache.lock().get(&cache_key);
        if let Some(source) = cached_source {
            let temp_file = materialized_sources
                .materialize(source.content().as_bytes(), AttachmentLanguage::Java)?;
            return Ok(SourceContent::Archive {
                source,
                archive_path: self.path.clone(),
                entry_path: entry_path.to_string(),
                temp_file,
            });
        }

        let file = File::open(&self.snapshot).map_err(|error| error.to_string())?;
        let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
        let mut entry = archive
            .by_name(entry_path)
            .map_err(|error| format!("Missing {entry_path}: {error}"))?;
        let declared_size = entry.size();
        let content: Arc<str> = read_source(&mut entry, declared_size, entry_path)?.into();
        drop(entry);
        let temp_file =
            materialized_sources.materialize(content.as_bytes(), AttachmentLanguage::Java)?;
        let source = Arc::new(SourceFile::new(content));
        content_cache.lock().insert(cache_key, Arc::clone(&source));

        Ok(SourceContent::Archive {
            source,
            archive_path: self.path.clone(),
            entry_path: entry_path.to_string(),
            temp_file,
        })
    }
}

impl SourceProvider for JarSourceProvider {
    fn get_source(&self, fqn: &str) -> Option<SourceContent> {
        let source_identity = self.unique_source_identity_for_fqn(fqn)?;
        self.get_source_exact(ExternalSourceRequest::new(source_identity, fqn))
    }

    fn can_provide(&self, fqn: &str) -> bool {
        self.unique_source_identity_for_fqn(fqn).is_some()
    }

    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        self.extract_source(request)
            .inspect_err(|error| {
                debug!(
                    "Dependency source lookup failed for {} in {:?}: {error}",
                    request.fqn(),
                    request.source_identity()
                );
            })
            .ok()
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        self.archive_for(request.source_identity())
            .and_then(|archive| archive.find_source_entry(request.fqn()))
            .is_some()
    }

    fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
        self.archives.contains_key(source_identity)
    }

    fn snapshot_for_sources(
        &self,
        source_identities: &std::collections::HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        let archives: HashMap<_, _> = source_identities
            .iter()
            .filter_map(|source_identity| {
                self.archives
                    .get(source_identity)
                    .map(|archive| (source_identity.clone(), Arc::clone(archive)))
            })
            .collect();
        (!archives.is_empty()).then(|| {
            Arc::new(Self {
                archives,
                materialized_sources: Arc::clone(&self.materialized_sources),
                content_cache: Arc::clone(&self.content_cache),
            }) as Arc<dyn SourceProvider>
        })
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "dependency source JAR"
    }
}

fn find_sibling_sources_jar(binary_jar: &Path) -> Option<PathBuf> {
    if binary_jar.extension()? != "jar" {
        return None;
    }
    let stem = binary_jar.file_stem()?;
    let display_stem = stem.to_string_lossy();
    if display_stem.ends_with("-sources") || display_stem.ends_with("-javadoc") {
        return None;
    }
    let mut expected = stem.to_os_string();
    expected.push("-sources.jar");
    let binary_dir = binary_jar.parent()?;

    // Maven local repositories keep all classifiers beside the binary.
    let sibling = binary_dir.join(&expected);
    if sibling.is_file() {
        return Some(sibling);
    }

    // Gradle's module cache stores each artifact in a separate hash directory
    // below the version directory.
    let version_dir = binary_dir.parent()?;
    let entries = fs::read_dir(version_dir).ok()?;
    let mut candidates = Vec::new();
    for (inspected, hash_dir) in entries.enumerate() {
        if inspected >= MAX_GRADLE_VERSION_DIRECTORY_ENTRIES {
            warn!(
                "Gradle artifact directory {} exceeds {} entries; source attachment discovery stopped",
                version_dir.display(),
                MAX_GRADLE_VERSION_DIRECTORY_ENTRIES
            );
            return None;
        }
        let Ok(hash_dir) = hash_dir else {
            continue;
        };
        let candidate = hash_dir.path().join(&expected);
        if candidate.is_file() {
            candidates.push(candidate);
        }
    }
    candidates.sort_unstable();
    match candidates.as_slice() {
        [candidate] => Some(candidate.clone()),
        [] => None,
        _ => {
            warn!(
                "Ambiguous Gradle source attachment for {}: {:?}",
                binary_jar.display(),
                candidates
            );
            None
        }
    }
}

#[cfg(test)]
fn index_source_jar(
    source_jar: &Path,
    index: &mut HashMap<String, (PathBuf, String)>,
) -> Result<(), String> {
    let file = File::open(source_jar).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    for (fqn, entry_path) in index_source_archive(&mut archive)?.0 {
        index
            .entry(fqn)
            .or_insert_with(|| (source_jar.to_path_buf(), entry_path));
    }
    Ok(())
}

fn index_source_archive(
    archive: &mut ZipArchive<File>,
) -> Result<(HashMap<String, String>, usize), String> {
    index_source_archive_with_limit(archive, MAX_SOURCE_ENTRIES)
}

fn index_source_archive_with_limit(
    archive: &mut ZipArchive<File>,
    maximum_entries: usize,
) -> Result<(HashMap<String, String>, usize), String> {
    let mut index = HashMap::new();
    let mut inspected_source_entries = 0_usize;
    let mut index_bytes = 0_usize;
    for entry_index in 0..archive.len() {
        let entry = archive
            .by_index(entry_index)
            .map_err(|error| error.to_string())?;
        let entry_name = entry.name().map_err(|error| error.to_string())?;
        let entry_path = entry_name.as_ref();
        if entry.is_dir()
            || !Path::new(entry_path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            || is_java_metadata_source(entry_path)
            || !safe_archive_path(entry_path)
        {
            continue;
        }
        inspected_source_entries += 1;
        if inspected_source_entries > maximum_entries {
            return Err(format!("more than {maximum_entries} source entries"));
        }
        let Some(fqn) = java_source_fqn(entry_path) else {
            continue;
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = index.entry(fqn) {
            let retained = slot
                .key()
                .len()
                .checked_add(entry_path.len())
                .and_then(|bytes| bytes.checked_add(SOURCE_INDEX_ENTRY_OVERHEAD))
                .ok_or_else(|| "source index retained-byte count overflowed".to_string())?;
            index_bytes = index_bytes
                .checked_add(retained)
                .ok_or_else(|| "source index retained-byte count overflowed".to_string())?;
            if index_bytes > MAX_TOTAL_SOURCE_INDEX_BYTES {
                return Err(format!(
                    "source index exceeds {MAX_TOTAL_SOURCE_INDEX_BYTES} retained bytes"
                ));
            }
            slot.insert(entry_path.to_string());
        }
    }
    Ok((index, index_bytes))
}

fn is_java_metadata_source(entry_path: &str) -> bool {
    Path::new(entry_path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("module-info.java")
                || name.eq_ignore_ascii_case("package-info.java")
        })
}

fn java_source_fqn(entry_path: &str) -> Option<String> {
    let extension_start = entry_path.len().checked_sub(".java".len())?;
    let extension = entry_path.get(extension_start..)?;
    if !extension.eq_ignore_ascii_case(".java") {
        return None;
    }
    Some(entry_path.get(..extension_start)?.replace('/', "."))
}

fn safe_archive_path(entry: &str) -> bool {
    !entry.is_empty()
        && entry.len() <= MAX_ARCHIVE_PATH_BYTES
        && !entry.starts_with('/')
        && !entry.contains(['\\', ':'])
        && entry
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

fn read_source(reader: impl Read, declared_size: u64, entry_path: &str) -> Result<String, String> {
    if declared_size > MAX_SOURCE_BYTES as u64 {
        return Err(format!("Source entry {entry_path} exceeds size limit"));
    }
    let mut bytes = Vec::with_capacity(declared_size as usize);
    reader
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read {entry_path}: {error}"))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!("Source entry {entry_path} exceeds size limit"));
    }
    String::from_utf8(bytes).map_err(|error| format!("Failed to decode {entry_path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        find_sibling_sources_jar, index_source_archive_with_limit, index_source_jar, read_source,
        safe_archive_path, JarSourceProvider, MAX_SOURCE_BYTES,
    };
    use crate::source_provider::{
        ExternalSourceRequest, MaterializedSourceStore, SourceContent, SourceProvider,
        SourceProviderChain,
    };
    use hyperion_workspace::{ExternalSourceArchive, ExternalSourceIdentity};
    use std::io::Write;
    use std::path::Path;
    use std::sync::Arc;

    fn write_source_jar(path: &Path, entry: &str, source: &[u8]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file(entry, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(source).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn rejects_source_jar_path_traversal() {
        assert!(safe_archive_path("org/example/Thing.java"));
        assert!(!safe_archive_path("../Thing.java"));
        assert!(!safe_archive_path("/org/example/Thing.java"));
        assert!(!safe_archive_path("C:/org/example/Thing.java"));
        assert!(!safe_archive_path("org\\example\\Thing.java"));
    }

    #[test]
    fn discovers_and_extracts_sibling_gradle_source_jar() {
        let temp = tempfile::tempdir().unwrap();
        let version_dir = temp.path().join("org.slf4j/slf4j-api/1.7.36");
        let binary_dir = version_dir.join("binary-hash");
        let source_dir = version_dir.join("source-hash");
        std::fs::create_dir_all(&binary_dir).unwrap();
        std::fs::create_dir_all(&source_dir).unwrap();
        let binary = binary_dir.join("slf4j-api-1.7.36.jar");
        std::fs::write(&binary, b"unused").unwrap();

        let source_jar = source_dir.join("slf4j-api-1.7.36-sources.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&source_jar).unwrap());
        zip.start_file(
            "org/slf4j/Logger.java",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"package org.slf4j; public interface Logger { void info(String s); }")
            .unwrap();
        zip.finish().unwrap();

        let chain = SourceProviderChain::new();
        let provider = JarSourceProvider::from_binary_jars_with_store(
            std::slice::from_ref(&binary),
            chain.materialized_sources(),
        )
        .unwrap();
        let binary_id = ExternalSourceIdentity::for_archive_path(&binary);
        assert!(provider.can_provide_exact(ExternalSourceRequest::new(
            &binary_id,
            "org.slf4j.Logger.info(java.lang.String)"
        )));
        let first = provider
            .get_source_exact(ExternalSourceRequest::new(
                &binary_id,
                "org.slf4j.Logger.info(java.lang.String)",
            ))
            .unwrap();
        assert!(first.content().contains("interface Logger"));
        assert!(first.temp_file().is_file());
        let first_symbols = first.java_symbols().expect("parse cached source");
        assert!(first_symbols
            .iter()
            .any(|symbol| symbol.fqn.starts_with("org.slf4j.Logger.info")));

        let second = provider
            .get_source_exact(ExternalSourceRequest::new(&binary_id, "org.slf4j.Logger"))
            .unwrap();
        let (
            SourceContent::Archive {
                source: first_source,
                ..
            },
            SourceContent::Archive {
                source: second_source,
                ..
            },
        ) = (&first, &second)
        else {
            panic!("dependency provider must return archive source");
        };
        assert!(
            Arc::ptr_eq(first_source, second_source),
            "members from one source file must share its parsed cache entry"
        );
        assert!(std::ptr::eq(
            first.java_symbols().unwrap(),
            second.java_symbols().unwrap()
        ));
    }

    #[test]
    fn retained_archive_snapshots_live_in_the_scavenged_session_and_drop_promptly() {
        let temp = tempfile::tempdir().unwrap();
        let artifact_dir = temp.path().join("artifacts");
        let session_base = temp.path().join("sessions");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        let binary = artifact_dir.join("library.jar");
        std::fs::write(&binary, b"binary").unwrap();
        write_source_jar(
            &artifact_dir.join("library-sources.jar"),
            "sample/Library.java",
            b"package sample; public class Library {}",
        );
        let store = Arc::new(MaterializedSourceStore::new_for_test(&session_base));
        let session_root = store.root().to_path_buf();
        let archive_dir = session_root.join("archives");

        let provider = JarSourceProvider::from_binary_jars_with_store(
            std::slice::from_ref(&binary),
            Arc::clone(&store),
        )
        .unwrap();
        assert_eq!(std::fs::read_dir(&archive_dir).unwrap().count(), 1);

        drop(provider);
        assert_eq!(std::fs::read_dir(&archive_dir).unwrap().count(), 0);
        drop(store);
        assert!(!session_root.exists());
    }

    #[test]
    fn discovers_maven_sources_beside_the_binary() {
        let temp = tempfile::tempdir().unwrap();
        let version_dir = temp.path().join("org/example/library/1.2.3");
        std::fs::create_dir_all(&version_dir).unwrap();
        let binary = version_dir.join("library-1.2.3.jar");
        let sources = version_dir.join("library-1.2.3-sources.jar");
        std::fs::write(&binary, b"unused").unwrap();
        std::fs::write(&sources, b"unused").unwrap();

        assert_eq!(find_sibling_sources_jar(&binary), Some(sources));
    }

    #[cfg(unix)]
    #[test]
    fn captured_binary_identity_ignores_unrelated_alias_local_sources() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real_dir = temp.path().join("real");
        let alias_dir = temp.path().join("alias");
        std::fs::create_dir_all(&real_dir).unwrap();
        std::fs::create_dir_all(&alias_dir).unwrap();
        let binary = real_dir.join("library.jar");
        let alias = alias_dir.join("library.jar");
        std::fs::write(&binary, b"binary identity").unwrap();
        symlink(&binary, &alias).unwrap();
        write_source_jar(
            &real_dir.join("library-sources.jar"),
            "owner/Thing.java",
            b"package owner; public class Thing { String owner = \"real\"; }",
        );
        write_source_jar(
            &alias_dir.join("library-sources.jar"),
            "owner/Thing.java",
            b"package owner; public class Thing { String owner = \"alias\"; }",
        );

        let captured = hyperion_workspace::ExternalSourceArchive::capture(&alias);
        let chain = SourceProviderChain::new();
        let provider = JarSourceProvider::from_binary_archives_with_store(
            std::slice::from_ref(&captured),
            chain.materialized_sources(),
        )
        .expect("canonical source attachment");
        let source = provider
            .get_source_exact(ExternalSourceRequest::new(
                captured.identity(),
                "owner.Thing",
            ))
            .expect("source bound to captured binary");

        assert!(source.content().contains("owner = \"real\""));
        assert!(!source.content().contains("owner = \"alias\""));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn sibling_source_discovery_preserves_non_utf8_binary_stems() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().unwrap();
        let binary = temp
            .path()
            .join(OsString::from_vec(b"library-\x80.jar".to_vec()));
        let sources = temp
            .path()
            .join(OsString::from_vec(b"library-\x80-sources.jar".to_vec()));
        std::fs::write(&binary, b"binary").unwrap();
        std::fs::write(&sources, b"sources").unwrap();

        assert_eq!(find_sibling_sources_jar(&binary), Some(sources));
    }

    #[test]
    fn ambiguous_gradle_source_attachments_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let version_dir = temp.path().join("org/example/library/1.2.3");
        let binary_dir = version_dir.join("binary-hash");
        std::fs::create_dir_all(&binary_dir).unwrap();
        let binary = binary_dir.join("library-1.2.3.jar");
        std::fs::write(&binary, b"binary").unwrap();
        for hash in ["source-b", "source-a"] {
            let source_dir = version_dir.join(hash);
            std::fs::create_dir_all(&source_dir).unwrap();
            std::fs::write(
                source_dir.join("library-1.2.3-sources.jar"),
                hash.as_bytes(),
            )
            .unwrap();
        }

        assert_eq!(find_sibling_sources_jar(&binary), None);
    }

    #[test]
    fn uppercase_java_extension_indexes_without_panicking() {
        let temp = tempfile::tempdir().unwrap();
        let source_jar = temp.path().join("uppercase-sources.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&source_jar).unwrap());
        zip.start_file(
            "org/example/Upper.JAVA",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"package org.example; public class Upper {}")
            .unwrap();
        zip.finish().unwrap();

        let mut index = std::collections::HashMap::new();
        index_source_jar(&source_jar, &mut index).unwrap();
        assert!(index.contains_key("org.example.Upper"));
    }

    #[test]
    fn source_reads_enforce_the_limit_even_if_the_declared_size_is_wrong() {
        let error = read_source(std::io::repeat(b'a'), 0, "Oversized.java").unwrap_err();
        assert!(error.contains("exceeds size limit"));

        let declared_error = read_source(
            std::io::empty(),
            MAX_SOURCE_BYTES as u64 + 1,
            "DeclaredOversized.java",
        )
        .unwrap_err();
        assert!(declared_error.contains("exceeds size limit"));
    }

    #[test]
    fn source_entry_budget_charges_inspected_entries_before_retention() {
        let temp = tempfile::tempdir().unwrap();
        let source_jar = temp.path().join("duplicates-sources.jar");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&source_jar).unwrap());
        for (entry, generation) in [
            ("duplicate/First.java", "first"),
            ("duplicate/Second.java", "second"),
        ] {
            writer
                .start_file(entry, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(generation.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        let file = std::fs::File::open(source_jar).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();

        let error = index_source_archive_with_limit(&mut archive, 1).unwrap_err();
        assert!(error.contains("more than 1 source entries"));
    }

    #[test]
    fn duplicate_fqns_are_bound_to_the_selected_binary_archive() {
        let temp = tempfile::tempdir().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        let first_binary = first_dir.join("library.jar");
        let second_binary = second_dir.join("library.jar");
        std::fs::write(&first_binary, b"first binary identity").unwrap();
        std::fs::write(&second_binary, b"second binary identity").unwrap();
        write_source_jar(
            &first_dir.join("library-sources.jar"),
            "duplicate/Thing.java",
            b"package duplicate; public class Thing { String owner = \"first\"; }",
        );
        write_source_jar(
            &second_dir.join("library-sources.jar"),
            "duplicate/Thing.java",
            b"package duplicate; public class Thing { String owner = \"second\"; }",
        );

        let chain = SourceProviderChain::new();
        let provider = JarSourceProvider::from_binary_jars_with_store(
            &[first_binary.clone(), second_binary.clone()],
            chain.materialized_sources(),
        )
        .unwrap();
        let first_id = ExternalSourceIdentity::for_archive_path(&first_binary);
        let second_id = ExternalSourceIdentity::for_archive_path(&second_binary);

        let first = provider
            .get_source_exact(ExternalSourceRequest::new(&first_id, "duplicate.Thing"))
            .unwrap();
        let second = provider
            .get_source_exact(ExternalSourceRequest::new(&second_id, "duplicate.Thing"))
            .unwrap();
        assert!(first.content().contains("owner = \"first\""));
        assert!(second.content().contains("owner = \"second\""));
    }

    #[test]
    fn archive_budget_retains_the_already_bounded_partial_provider() {
        let temp = tempfile::tempdir().unwrap();
        let mut binaries = Vec::new();
        for (directory, owner) in [("first", "First"), ("second", "Second")] {
            let directory = temp.path().join(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let binary = directory.join("library.jar");
            std::fs::write(&binary, owner.as_bytes()).unwrap();
            write_source_jar(
                &directory.join("library-sources.jar"),
                &format!("budget/{owner}.java"),
                format!("package budget; public class {owner} {{}}").as_bytes(),
            );
            binaries.push(ExternalSourceArchive::capture(&binary));
        }

        let chain = SourceProviderChain::new();
        let provider = JarSourceProvider::from_binary_archives_with_limit(
            &binaries,
            chain.materialized_sources(),
            1,
        )
        .expect("the accepted prefix remains usable");

        assert!(provider.owns_source_identity(binaries[0].identity()));
        assert!(!provider.owns_source_identity(binaries[1].identity()));
        assert!(provider
            .get_source_exact(ExternalSourceRequest::new(
                binaries[0].identity(),
                "budget.First",
            ))
            .is_some());
    }

    #[test]
    fn same_path_replacement_cannot_change_an_open_archive_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("library.jar");
        let sources = temp.path().join("library-sources.jar");
        std::fs::write(&binary, b"binary identity").unwrap();
        write_source_jar(
            &sources,
            "snapshot/Thing.java",
            b"package snapshot; public class Thing { String generation = \"R1\"; }",
        );

        let chain = SourceProviderChain::new();
        let provider = JarSourceProvider::from_binary_jars_with_store(
            std::slice::from_ref(&binary),
            chain.materialized_sources(),
        )
        .unwrap();

        let replacement = temp.path().join("replacement.jar");
        write_source_jar(
            &replacement,
            "snapshot/Thing.java",
            b"package snapshot; public class Thing { String generation = \"R2\"; }",
        );
        std::fs::rename(&replacement, &sources).expect("atomic same-path replacement");

        let binary_id = ExternalSourceIdentity::for_archive_path(&binary);
        let source = provider
            .get_source_exact(ExternalSourceRequest::new(&binary_id, "snapshot.Thing"))
            .expect("R1 archive handle remains readable");
        assert!(source.content().contains("generation = \"R1\""));
        assert!(!source.content().contains("generation = \"R2\""));
    }

    #[test]
    fn same_inode_mutation_cannot_change_a_private_archive_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("library.jar");
        let sources = temp.path().join("library-sources.jar");
        std::fs::write(&binary, b"binary identity").unwrap();
        write_source_jar(
            &sources,
            "snapshot/Thing.java",
            b"package snapshot; public class Thing { String generation = \"R1\"; }",
        );

        let chain = SourceProviderChain::new();
        let provider = JarSourceProvider::from_binary_jars_with_store(
            std::slice::from_ref(&binary),
            chain.materialized_sources(),
        )
        .unwrap();

        // File::create truncates and rewrites the same directory entry/inode;
        // retaining only an open descriptor to `sources` would observe R2.
        write_source_jar(
            &sources,
            "snapshot/Thing.java",
            b"package snapshot; public class Thing { String generation = \"R2\"; }",
        );

        let binary_id = ExternalSourceIdentity::for_archive_path(&binary);
        let source = provider
            .get_source_exact(ExternalSourceRequest::new(&binary_id, "snapshot.Thing"))
            .expect("private R1 snapshot remains readable");
        assert!(source.content().contains("generation = \"R1\""));
        assert!(!source.content().contains("generation = \"R2\""));
    }
}
