// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK Source Provider
//!
//! Provides Java source code from $JAVA_HOME/lib/src.zip

use super::cache::SourceCache;
use super::temp_file::AttachmentLanguage;
use super::MaterializedSourceStore;
use super::{ExternalSourceRequest, SourceContent, SourceFile, SourceProvider};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tracing::{debug, info, warn};
use zip::ZipArchive;

use hyperion_workspace::{ExternalSourceArchive, ExternalSourceIdentity};

const MAX_JDK_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_JDK_SOURCE_ENTRIES: usize = 250_000;
const MAX_JDK_ARCHIVE_ENTRIES: usize = 500_000;
const MAX_JDK_SOURCE_INDEX_BYTES: usize = 128 * 1024 * 1024;
const MAX_JDK_BINARY_ARCHIVES: usize = 1_024;
const SOURCE_INDEX_ENTRY_OVERHEAD: usize = 96;
const MAX_ARCHIVE_PATH_BYTES: usize = 4 * 1024;
const MAX_JDK_SOURCE_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
pub const JDK_SOURCE_ID: &str = "jdk";

/// Provides JDK source from $JAVA_HOME/lib/src.zip
pub struct JdkSourceProvider {
    shared: Arc<JdkSourceProviderShared>,
    /// Exact WorkspaceIndex source identity -> optional JDK module constraint.
    binary_sources: HashMap<ExternalSourceIdentity, Option<String>>,
}

struct JdkSourceProviderShared {
    src_zip_path: PathBuf,
    src_zip: ExternalSourceArchive,
    source_archive: OnceLock<Result<JdkSourceArchive, String>>,
    /// Extracted content cache (LRU)
    content_cache: Mutex<SourceCache>,
    /// Shared session owner for every URI already returned to the client.
    materialized_sources: Arc<MaterializedSourceStore>,
}

struct JdkSourceArchive {
    /// FQN -> candidate (module, path within archive) entries.
    index: HashMap<String, Vec<(String, String)>>,
    /// The archive indexed by `index`, retained for this provider generation.
    archive: Mutex<ZipArchive<File>>,
}

impl JdkSourceProvider {
    /// Legacy constructor retained for source compatibility.
    pub fn new(java_home: &Path) -> Option<Self> {
        Self::new_with_store(java_home, Arc::new(MaterializedSourceStore::new()))
    }

    /// Create a new JDK source provider from JAVA_HOME
    pub fn new_with_store(
        java_home: &Path,
        materialized_sources: Arc<MaterializedSourceStore>,
    ) -> Option<Self> {
        Self::new_with_binary_sources(
            java_home,
            jdk_binary_source_identities(java_home),
            materialized_sources,
        )
    }

    /// Build a provider for the exact binary archive identities retained by a
    /// `jdk-classpath` refresh. No live JMOD enumeration occurs here, so source
    /// routing cannot drift to a newer same-path JDK generation.
    pub(crate) fn from_binary_archives_with_store(
        java_home: &Path,
        binary_archives: &[ExternalSourceArchive],
        materialized_sources: Arc<MaterializedSourceStore>,
    ) -> Option<Self> {
        let mut binary_sources = HashMap::from([(jdk_virtual_identity().clone(), None)]);
        for archive in binary_archives {
            let module =
                if archive.path().extension().and_then(|value| value.to_str()) == Some("jmod") {
                    archive
                        .path()
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .map(str::to_string)
                } else {
                    Some(String::new())
                };
            binary_sources.insert(archive.identity().clone(), module);
        }
        Self::new_with_binary_sources(java_home, binary_sources, materialized_sources)
    }

    fn new_with_binary_sources(
        java_home: &Path,
        binary_sources: HashMap<ExternalSourceIdentity, Option<String>>,
        materialized_sources: Arc<MaterializedSourceStore>,
    ) -> Option<Self> {
        let (src_zip, src_zip_archive) = [java_home.join("lib/src.zip"), java_home.join("src.zip")]
            .into_iter()
            .find_map(|path| {
                match ExternalSourceArchive::capture_staged(&path, MAX_JDK_SOURCE_ARCHIVE_BYTES) {
                    Ok(archive) => Some((path, archive)),
                    Err(error) => {
                        debug!(
                            "JDK src.zip candidate is unavailable at {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            })?;
        info!("JDK src.zip staged at: {}", src_zip.display());

        Some(Self {
            shared: Arc::new(JdkSourceProviderShared {
                src_zip_path: src_zip,
                src_zip: src_zip_archive,
                source_archive: OnceLock::new(),
                content_cache: Mutex::new(SourceCache::new()),
                materialized_sources,
            }),
            binary_sources,
        })
    }

    fn source_archive(&self) -> Result<&JdkSourceArchive, &str> {
        self.shared
            .source_archive
            .get_or_init(|| self.open_source_archive())
            .as_ref()
            .map_err(String::as_str)
    }

    fn open_source_archive(&self) -> Result<JdkSourceArchive, String> {
        let snapshot = self
            .shared
            .src_zip
            .snapshot_contents(MAX_JDK_SOURCE_ARCHIVE_BYTES)?;
        let mut archive = ZipArchive::new(snapshot.into_file())
            .map_err(|error| format!("Failed to read JDK src.zip: {error}"))?;
        let index = Self::index_src_zip(&mut archive)?;
        info!("Indexed {} JDK source files from src.zip", index.len());
        Ok(JdkSourceArchive {
            index,
            archive: Mutex::new(archive),
        })
    }

    /// Legacy cleanup hook retained for source compatibility.
    ///
    /// Materialized source URIs are now owned by the surrounding session store
    /// and intentionally remain readable after this provider is replaced. The
    /// store removes them when its final owner is dropped, so eagerly deleting
    /// them here would violate that lifetime contract.
    pub const fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }

    /// Index src.zip to build FQN → (module, entry_path) mapping
    fn index_src_zip(
        archive: &mut ZipArchive<File>,
    ) -> Result<HashMap<String, Vec<(String, String)>>, String> {
        Self::index_src_zip_with_limit(archive, MAX_JDK_SOURCE_ENTRIES)
    }

    fn index_src_zip_with_limit(
        archive: &mut ZipArchive<File>,
        maximum_entries: usize,
    ) -> Result<HashMap<String, Vec<(String, String)>>, String> {
        let mut index = HashMap::new();
        let mut inspected_source_entries = 0_usize;
        let mut retained_bytes = 0_usize;
        if archive.len() > MAX_JDK_ARCHIVE_ENTRIES {
            return Err(format!(
                "src.zip contains {} entries (maximum {MAX_JDK_ARCHIVE_ENTRIES})",
                archive.len()
            ));
        }

        for i in 0..archive.len() {
            let entry = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry: {e}"))?;

            let entry_path = entry
                .name()
                .map_err(|e| format!("Invalid zip entry name: {e}"))?
                .into_owned();

            // Skip directories and non-Java files (Java source files always use lowercase .java extension)
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            if entry.is_dir()
                || !entry_path.ends_with(".java")
                || !is_safe_archive_path(&entry_path)
                || entry_path == "module-info.java"
                || entry_path.ends_with("/module-info.java")
                || entry_path == "package-info.java"
                || entry_path.ends_with("/package-info.java")
            {
                continue;
            }

            let (module, class_path) = source_path_parts(&entry_path);

            // Convert file path to FQN: "java/lang/String.java" -> "java.lang.String"
            let Some(fqn) = source_fqn(&class_path) else {
                continue;
            };

            inspected_source_entries += 1;
            if inspected_source_entries > maximum_entries {
                return Err(format!(
                    "src.zip contains more than {maximum_entries} Java source entries"
                ));
            }
            let entry_bytes = fqn
                .len()
                .checked_add(module.len())
                .and_then(|bytes| bytes.checked_add(entry_path.len()))
                .and_then(|bytes| bytes.checked_add(SOURCE_INDEX_ENTRY_OVERHEAD))
                .ok_or_else(|| "src.zip index retained-byte count overflowed".to_string())?;
            retained_bytes = retained_bytes
                .checked_add(entry_bytes)
                .ok_or_else(|| "src.zip index retained-byte count overflowed".to_string())?;
            if retained_bytes > MAX_JDK_SOURCE_INDEX_BYTES {
                return Err(format!(
                    "src.zip index exceeds {MAX_JDK_SOURCE_INDEX_BYTES} retained bytes"
                ));
            }
            index
                .entry(fqn)
                .or_insert_with(Vec::new)
                .push((module, entry_path));
        }

        Ok(index)
    }

    /// Find the top-level source file that owns a class or member FQN.
    ///
    /// `src.zip` contains `java/util/Map.java`, not separate files for
    /// `java.util.Map.Entry` or its members. Walk outward until the nearest
    /// indexed top-level type is found.
    fn find_source_entry<'a>(
        &'a self,
        request: ExternalSourceRequest<'_>,
    ) -> Option<(&'a String, &'a (String, String))> {
        let expected_module = self.binary_sources.get(request.source_identity())?;
        let source_archive = self.source_archive().ok()?;
        let mut candidate = request
            .fqn()
            .split_once('(')
            .map_or(request.fqn(), |(prefix, _)| prefix);
        loop {
            if let Some((source_fqn, entries)) = source_archive.index.get_key_value(candidate) {
                if let Some(entry) = entries.iter().find(|(module, _)| {
                    expected_module
                        .as_ref()
                        .is_none_or(|expected| expected == module)
                }) {
                    return Some((source_fqn, entry));
                }
            }
            candidate = candidate.rsplit_once('.')?.0;
        }
    }

    /// Extract source file from src.zip to temp directory
    fn extract_source(
        &self,
        request: ExternalSourceRequest<'_>,
    ) -> Result<(Arc<SourceFile>, PathBuf), String> {
        let (source_fqn, (_module, entry_path)) =
            self.find_source_entry(request).ok_or_else(|| {
                format!(
                    "No source entry for {} in {:?}",
                    request.fqn(),
                    request.source_identity()
                )
            })?;

        let cached_source = self.shared.content_cache.lock().get(entry_path);
        if let Some(cached_source) = cached_source {
            debug!("Cache hit for JDK source: {}", source_fqn);
            let temp_file = self
                .shared
                .materialized_sources
                .materialize(cached_source.content().as_bytes(), AttachmentLanguage::Java)?;

            return Ok((cached_source, temp_file));
        }

        let mut archive = self
            .source_archive()
            .map_err(str::to_string)?
            .archive
            .lock();
        let mut entry = archive
            .by_name(entry_path)
            .map_err(|e| format!("Failed to find entry '{entry_path}': {e}"))?;

        // Read content
        let declared_size = entry.size();
        let content: Arc<str> = read_source(&mut entry, declared_size)?.into();
        drop(entry);
        drop(archive);

        let temp_file = self
            .shared
            .materialized_sources
            .materialize(content.as_bytes(), AttachmentLanguage::Java)?;
        let source = Arc::new(SourceFile::new(content));

        debug!(
            "Extracted JDK source: {} -> {}",
            request.fqn(),
            temp_file.display()
        );

        // Cache the content
        {
            let mut cache = self.shared.content_cache.lock();
            cache.insert(entry_path.clone(), Arc::clone(&source));
        }

        Ok((source, temp_file))
    }
}

fn jdk_binary_source_identities(
    java_home: &Path,
) -> HashMap<ExternalSourceIdentity, Option<String>> {
    let mut identities = HashMap::from([(jdk_virtual_identity().clone(), None)]);
    let jmods = java_home.join("jmods");
    if let Ok(entries) = std::fs::read_dir(jmods) {
        for (inspected, entry) in entries.enumerate() {
            if inspected >= MAX_JDK_BINARY_ARCHIVES {
                warn!(
                    "JDK jmods directory exceeds {} entries; source identity discovery stopped",
                    MAX_JDK_BINARY_ARCHIVES
                );
                break;
            }
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("jmod")
            {
                continue;
            }
            let Some(module) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            identities.insert(
                ExternalSourceIdentity::for_archive_path(&path),
                Some(module.to_string()),
            );
        }
    }
    for rt_jar in [
        java_home.join("jre/lib/rt.jar"),
        java_home.join("lib/rt.jar"),
    ] {
        if rt_jar.is_file() {
            identities.insert(
                ExternalSourceIdentity::for_archive_path(&rt_jar),
                Some(String::new()),
            );
        }
    }
    identities
}

fn jdk_virtual_identity() -> &'static ExternalSourceIdentity {
    static IDENTITY: OnceLock<ExternalSourceIdentity> = OnceLock::new();
    IDENTITY.get_or_init(|| ExternalSourceIdentity::for_virtual(JDK_SOURCE_ID))
}

fn is_safe_archive_path(path: &str) -> bool {
    path.len() <= MAX_ARCHIVE_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

fn source_fqn(class_path: &str) -> Option<String> {
    let path = class_path.strip_suffix(".java")?;
    if path
        .split('/')
        .any(|component| component.is_empty() || component.contains('.'))
    {
        return None;
    }
    Some(path.replace('/', "."))
}

fn source_path_parts(entry_path: &str) -> (String, String) {
    let Some((first, rest)) = entry_path.split_once('/') else {
        return (String::new(), entry_path.to_string());
    };
    // Standard Java 9+ module names contain a dot (`java.base`,
    // `jdk.compiler`). Plain package roots such as `java/` and `com/` are
    // Java 8-style source paths and must remain part of the class name.
    if first.contains('.') && rest.contains('/') {
        (first.to_string(), rest.to_string())
    } else {
        (String::new(), entry_path.to_string())
    }
}

fn read_source(reader: impl Read, declared_size: u64) -> Result<String, String> {
    if declared_size > MAX_JDK_SOURCE_BYTES as u64 {
        return Err(format!(
            "source entry is {declared_size} bytes (maximum {MAX_JDK_SOURCE_BYTES})"
        ));
    }
    let mut bytes = Vec::with_capacity(declared_size as usize);
    reader
        .take(MAX_JDK_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read entry content: {error}"))?;
    if bytes.len() > MAX_JDK_SOURCE_BYTES {
        return Err(format!(
            "source entry expanded beyond {MAX_JDK_SOURCE_BYTES} bytes"
        ));
    }
    String::from_utf8(bytes).map_err(|error| format!("Source is not valid UTF-8: {error}"))
}

impl SourceProvider for JdkSourceProvider {
    fn get_source(&self, fqn: &str) -> Option<SourceContent> {
        self.get_source_exact(ExternalSourceRequest::new(jdk_virtual_identity(), fqn))
    }

    fn can_provide(&self, fqn: &str) -> bool {
        self.can_provide_exact(ExternalSourceRequest::new(jdk_virtual_identity(), fqn))
    }

    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        match self.extract_source(request) {
            Ok((source, temp_file)) => {
                let (_source_fqn, (_module, entry_path)) = self.find_source_entry(request)?;
                Some(SourceContent::Archive {
                    source,
                    archive_path: self.shared.src_zip_path.clone(),
                    entry_path: entry_path.clone(),
                    temp_file,
                })
            }
            Err(e) => {
                debug!(
                    "Failed to extract JDK source for {} in {:?}: {}",
                    request.fqn(),
                    request.source_identity(),
                    e
                );
                None
            }
        }
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        self.find_source_entry(request).is_some()
    }

    fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
        self.binary_sources.contains_key(source_identity)
    }

    fn snapshot_for_sources(
        &self,
        source_identities: &std::collections::HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        let binary_sources = self
            .binary_sources
            .iter()
            .filter(|(identity, _)| source_identities.contains(*identity))
            .map(|(identity, module)| (identity.clone(), module.clone()))
            .collect();
        Some(Arc::new(Self {
            shared: Arc::clone(&self.shared),
            binary_sources,
        }))
    }

    fn priority(&self) -> u32 {
        10 // High priority - prefer real sources
    }

    fn name(&self) -> &'static str {
        "jdk"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_provider::{ExternalSourceRequest, SourceProviderChain};
    use std::io::Write;

    fn write_src_zip(path: &Path, source: &[u8]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file(
            "java.base/java/util/Map.java",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(source).unwrap();
        zip.finish().unwrap();
    }

    fn provider_with_map_source() -> (JdkSourceProvider, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("temporary source directory");
        std::fs::create_dir_all(temp.path().join("lib")).unwrap();
        std::fs::create_dir_all(temp.path().join("jmods")).unwrap();
        std::fs::write(temp.path().join("jmods/java.base.jmod"), b"identity").unwrap();
        write_src_zip(
            &temp.path().join("lib/src.zip"),
            b"package java.util; public interface Map { interface Entry {} }",
        );
        let source_chain = SourceProviderChain::new();
        (
            JdkSourceProvider::new_with_store(temp.path(), source_chain.materialized_sources())
                .unwrap(),
            temp,
        )
    }

    #[test]
    fn java_eight_top_level_src_zip_serves_the_exact_rt_jar_identity() {
        let temp = tempfile::tempdir().expect("temporary Java 8 home");
        std::fs::create_dir_all(temp.path().join("jre/lib")).unwrap();
        let rt_jar = temp.path().join("jre/lib/rt.jar");
        std::fs::write(&rt_jar, b"binary identity").unwrap();
        let mut zip =
            zip::ZipWriter::new(std::fs::File::create(temp.path().join("src.zip")).unwrap());
        zip.start_file(
            "java/util/Map.java",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"package java.util; public interface Map {}")
            .unwrap();
        zip.finish().unwrap();
        let archive = ExternalSourceArchive::capture_staged(&rt_jar, 1024).unwrap();
        let source_chain = SourceProviderChain::new();

        let provider = JdkSourceProvider::from_binary_archives_with_store(
            temp.path(),
            std::slice::from_ref(&archive),
            source_chain.materialized_sources(),
        )
        .expect("the Java 8 top-level src.zip should be discovered");
        let source = provider
            .get_source_exact(ExternalSourceRequest::new(
                archive.identity(),
                "java.util.Map",
            ))
            .expect("the exact rt.jar identity should route to Java 8 sources");

        assert!(source.content().contains("interface Map"));
        assert_eq!(provider.shared.src_zip_path, temp.path().join("src.zip"));
    }

    #[test]
    fn construction_stages_src_zip_without_reading_or_indexing_its_contents() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("lib")).unwrap();
        std::fs::write(temp.path().join("lib/src.zip"), b"not a zip archive").unwrap();
        let source_chain = SourceProviderChain::new();

        let provider =
            JdkSourceProvider::new_with_store(temp.path(), source_chain.materialized_sources())
                .expect("metadata-only construction accepts a bounded regular file");
        assert!(
            provider.shared.source_archive.get().is_none(),
            "construction must not initialize, hash, or index src.zip"
        );
        assert!(!provider.can_provide("java.lang.String"));
        assert!(
            provider.shared.source_archive.get().unwrap().is_err(),
            "the invalid contents are observed only by the first source lookup"
        );
    }

    #[test]
    fn nested_class_and_member_fqns_resolve_to_enclosing_source_file() {
        let (provider, _temp) = provider_with_map_source();
        let (source_fqn, (_, entry_path)) = provider
            .find_source_entry(ExternalSourceRequest::new(
                jdk_virtual_identity(),
                "java.util.Map.Entry.getKey()",
            ))
            .expect("nested member should resolve to Map.java");
        assert_eq!(source_fqn, "java.util.Map");
        assert_eq!(entry_path, "java.base/java/util/Map.java");
        assert!(provider.can_provide_exact(ExternalSourceRequest::new(
            jdk_virtual_identity(),
            "java.util.Map.Entry"
        )));
        assert!(!provider.can_provide_exact(ExternalSourceRequest::new(
            jdk_virtual_identity(),
            "java.util.Missing.Entry"
        )));
    }

    #[test]
    fn jmod_identity_must_match_the_source_module() {
        let (provider, temp) = provider_with_map_source();
        let java_base = temp.path().join("jmods/java.base.jmod");
        let java_base_id = ExternalSourceIdentity::for_archive_path(&java_base);
        assert!(
            provider.can_provide_exact(ExternalSourceRequest::new(&java_base_id, "java.util.Map"))
        );
        let different = ExternalSourceIdentity::for_archive_path(Path::new(
            "/different/jdk/jmods/java.base.jmod",
        ));
        assert!(
            !provider.can_provide_exact(ExternalSourceRequest::new(&different, "java.util.Map"))
        );
    }

    #[test]
    fn refresh_snapshots_retain_only_the_requested_jdk_identities() {
        let (provider, temp) = provider_with_map_source();
        let java_base =
            ExternalSourceIdentity::for_archive_path(&temp.path().join("jmods/java.base.jmod"));

        let virtual_snapshot = provider
            .snapshot_for_sources(&std::collections::HashSet::from([
                jdk_virtual_identity().clone()
            ]))
            .expect("JDK provider supports bounded snapshots");
        assert!(virtual_snapshot.owns_source_identity(jdk_virtual_identity()));
        assert!(!virtual_snapshot.owns_source_identity(&java_base));

        let archive_snapshot = provider
            .snapshot_for_sources(&std::collections::HashSet::from([java_base.clone()]))
            .expect("JDK provider supports bounded snapshots");
        assert!(archive_snapshot.owns_source_identity(&java_base));
        assert!(!archive_snapshot.owns_source_identity(jdk_virtual_identity()));
    }

    #[test]
    fn source_paths_distinguish_java_8_packages_from_java_9_modules() {
        assert_eq!(
            source_path_parts("java/lang/String.java"),
            (String::new(), "java/lang/String.java".to_string())
        );
        assert_eq!(
            source_path_parts("java.base/java/lang/String.java"),
            ("java.base".to_string(), "java/lang/String.java".to_string())
        );
        assert_eq!(
            source_fqn("java/lang/String.java").as_deref(),
            Some("java.lang.String")
        );
    }

    #[test]
    fn unsafe_archive_paths_cannot_escape_the_extraction_directory() {
        assert!(!is_safe_archive_path("../../outside.java"));
        assert!(!is_safe_archive_path("C:/outside.java"));
        assert!(!is_safe_archive_path("java\\lang\\String.java"));
        assert!(source_fqn("pkg/..outside.java").is_none());
    }

    #[test]
    fn oversized_source_is_rejected_before_reading() {
        let error = read_source(std::io::empty(), MAX_JDK_SOURCE_BYTES as u64 + 1).unwrap_err();
        assert!(error.contains("maximum"));
    }

    #[test]
    fn duplicate_source_names_cannot_bypass_the_entry_budget() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("duplicate-src.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for generation in 0..3 {
            zip.start_file(
                format!("module.{generation}/java/util/Map.java"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            write!(zip, "// generation {generation}").unwrap();
        }
        zip.finish().unwrap();

        let file = std::fs::File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let error = JdkSourceProvider::index_src_zip_with_limit(&mut archive, 2).unwrap_err();

        assert!(error.contains("more than 2 Java source entries"));
    }

    #[test]
    fn same_path_src_zip_replacement_does_not_change_the_provider_snapshot() {
        let (provider, temp) = provider_with_map_source();
        assert!(provider.can_provide("java.util.Map"));
        let replacement = temp.path().join("replacement.zip");
        write_src_zip(
            &replacement,
            b"package java.util; public interface Map { String generation = \"R2\"; }",
        );
        std::fs::rename(&replacement, temp.path().join("lib/src.zip"))
            .expect("atomic same-path replacement");

        let source = provider
            .get_source_exact(ExternalSourceRequest::new(
                jdk_virtual_identity(),
                "java.util.Map",
            ))
            .expect("open R1 src.zip remains readable");
        assert!(source.content().contains("interface Entry"));
        assert!(!source.content().contains("generation = \"R2\""));
    }

    #[test]
    fn same_inode_src_zip_mutation_does_not_change_the_private_snapshot() {
        let (provider, temp) = provider_with_map_source();
        assert!(provider.can_provide("java.util.Map"));
        write_src_zip(
            &temp.path().join("lib/src.zip"),
            b"package java.util; public interface Map { String generation = \"R2\"; }",
        );

        let source = provider
            .get_source_exact(ExternalSourceRequest::new(
                jdk_virtual_identity(),
                "java.util.Map",
            ))
            .expect("private R1 src.zip remains readable");
        assert!(source.content().contains("interface Entry"));
        assert!(!source.content().contains("generation = \"R2\""));
    }

    #[test]
    fn src_zip_change_before_first_lookup_fails_closed() {
        let (provider, temp) = provider_with_map_source();
        write_src_zip(
            &temp.path().join("lib/src.zip"),
            b"package java.util; public interface Map { String generation = \"R2\"; }",
        );

        assert!(!provider.can_provide("java.util.Map"));
        assert!(provider
            .get_source_exact(ExternalSourceRequest::new(
                jdk_virtual_identity(),
                "java.util.Map",
            ))
            .is_none());
    }
}
