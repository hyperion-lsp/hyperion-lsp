// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK Indexing Support
//!
//! This module automatically detects and indexes the JDK's stdlib classes (java.lang, java.util, etc.)
//! to enable fast navigation to standard library symbols.
//!
//! ## How it works:
//!
//! 1. Detect JDK installation (JAVA_HOME, system default, or bundled JRE)
//! 2. For Java 8: Index rt.jar
//! 3. For Java 9+: Index jmods (java.base, java.logging, etc.)
//! 4. Extract symbols and add to workspace index
//!
//! This makes `String`, `List`, `Map`, etc. navigation instant.

use anyhow::{Context, Result};
use hyperion_workspace::{ExternalArchiveSnapshot, ExternalSourceArchive, WorkspaceIndex};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JdkIndexOutcome {
    Indexed,
    Unavailable {
        reason: String,
    },
    Partial {
        indexed_archives: usize,
        failed_archives: usize,
    },
}

impl JdkIndexOutcome {
    const fn is_complete(&self) -> bool {
        matches!(self, Self::Indexed)
    }
}

const fn archive_outcome(total_archives: usize, failed_archives: usize) -> JdkIndexOutcome {
    if failed_archives == 0 {
        JdkIndexOutcome::Indexed
    } else {
        JdkIndexOutcome::Partial {
            indexed_archives: total_archives.saturating_sub(failed_archives),
            failed_archives,
        }
    }
}

/// JDK cache performance metrics
struct JdkCacheMetrics {
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    cache_load_time_ms: AtomicU64,
    bytecode_index_time_ms: AtomicU64,
}

impl JdkCacheMetrics {
    const fn new() -> Self {
        Self {
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            cache_load_time_ms: AtomicU64::new(0),
            bytecode_index_time_ms: AtomicU64::new(0),
        }
    }

    fn record_cache_hit(&self, load_time_ms: u64) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
        self.cache_load_time_ms
            .store(load_time_ms, Ordering::Relaxed);
    }

    fn record_cache_miss(&self, index_time_ms: u64) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        self.bytecode_index_time_ms
            .store(index_time_ms, Ordering::Relaxed);
    }

    fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;

        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }

    fn speedup_factor(&self) -> Option<f64> {
        let bytecode_time = self.bytecode_index_time_ms.load(Ordering::Relaxed);
        let cache_time = self.cache_load_time_ms.load(Ordering::Relaxed);

        if bytecode_time > 0 && cache_time > 0 {
            Some(bytecode_time as f64 / cache_time as f64)
        } else {
            None
        }
    }
}

static METRICS: JdkCacheMetrics = JdkCacheMetrics::new();

/// JDK version info
#[derive(Debug)]
pub struct JdkInfo {
    pub java_home: PathBuf,
    pub version: JavaVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaVersion {
    Java8,
    Java9Plus(u8), // Java 9, 11, 17, 21, etc.
}

/// Detect JDK installation on the system
pub fn detect_jdk() -> Result<JdkInfo> {
    // Try 1: JAVA_HOME environment variable
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let path = PathBuf::from(java_home);
        if path.exists() {
            info!("Found JDK via JAVA_HOME: {}", path.display());
            return jdk_info_from_home(&path);
        }
    }

    // Try 2: inspect PATH directly. Spawning `which`/`where` can hang or retain unbounded output
    // when PATH resolves to a wrapper rather than the system utility.
    if let Some(java_path) = find_java_on_path() {
        // Resolve symlinks and find JAVA_HOME
        let java_home = resolve_java_home(&java_path)?;
        info!("Found JDK via PATH: {}", java_home.display());
        return jdk_info_from_home(&java_home);
    }

    // Try 3: Common installation paths
    let common_paths = if cfg!(target_os = "linux") {
        vec![
            "/usr/lib/jvm/default",
            "/usr/lib/jvm/default-java",
            "/usr/lib/jvm/java-11-openjdk",
            "/usr/lib/jvm/java-17-openjdk",
            "/usr/lib/jvm/java-21-openjdk",
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            "/Library/Java/JavaVirtualMachines/jdk-11.jdk/Contents/Home",
            "/Library/Java/JavaVirtualMachines/jdk-17.jdk/Contents/Home",
            "/Library/Java/JavaVirtualMachines/jdk-21.jdk/Contents/Home",
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            "C:\\Program Files\\Java\\jdk-11",
            "C:\\Program Files\\Java\\jdk-17",
        ]
    } else {
        vec![]
    };

    for path in common_paths {
        let path = PathBuf::from(path);
        if path.exists() {
            info!("Found JDK at common path: {}", path.display());
            return jdk_info_from_home(&path);
        }
    }

    anyhow::bail!("Could not detect JDK installation. Please set JAVA_HOME environment variable.")
}

fn find_java_on_path() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "java.exe" } else { "java" };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        let candidate = directory.join(executable);
        candidate.is_file().then_some(candidate)
    })
}

fn canonicalize_java_home(java_home: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(java_home)
        .with_context(|| format!("Could not resolve JAVA_HOME {}", java_home.display()))?;
    if !canonical.is_dir() {
        anyhow::bail!("JAVA_HOME is not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn jdk_info_from_home(java_home: &Path) -> Result<JdkInfo> {
    let java_home = canonicalize_java_home(java_home)?;
    let version = detect_java_version(&java_home)?;
    Ok(JdkInfo { java_home, version })
}

/// Resolve symlinks and find actual JAVA_HOME from a java binary path
fn resolve_java_home(java_bin: &Path) -> Result<PathBuf> {
    // java_bin might be: /usr/bin/java (symlink) → /usr/lib/jvm/java-11-openjdk/bin/java
    // `canonicalize` resolves the full chain and anchors relative symlink
    // targets against their containing directory.
    let real_path = std::fs::canonicalize(java_bin)
        .with_context(|| format!("Could not resolve Java executable {}", java_bin.display()))?;

    // Real path: /usr/lib/jvm/java-11-openjdk/bin/java
    // JAVA_HOME: /usr/lib/jvm/java-11-openjdk

    let java_home = real_path
        .parent() // /usr/lib/jvm/java-11-openjdk/bin
        .and_then(|p| p.parent()) // /usr/lib/jvm/java-11-openjdk
        .context("Could not resolve JAVA_HOME from java binary path")?;

    canonicalize_java_home(java_home)
}

/// Detect Java version from JAVA_HOME
fn detect_java_version(java_home: &Path) -> Result<JavaVersion> {
    // Every standard JDK distribution includes a small `release` properties file. Reading it is
    // faster and avoids executing a potentially user-controlled JAVA_HOME/bin/java.
    let release_path = java_home.join("release");
    if let Ok(release) = hyperion_workspace::read_utf8_file_bounded(&release_path, 1024 * 1024) {
        if let Some(version) = parse_release_version(&release) {
            return Ok(version);
        }
    }

    // Fallback: Check for rt.jar (Java 8) vs jmods (Java 9+)
    let rt_jar = java_home.join("jre/lib/rt.jar");
    let jmods = java_home.join("jmods");

    if rt_jar.exists() {
        Ok(JavaVersion::Java8)
    } else if jmods.exists() {
        Ok(JavaVersion::Java9Plus(11)) // Default to 11 if we can't detect
    } else {
        anyhow::bail!(
            "Could not determine Java version from {}",
            java_home.display()
        )
    }
}

fn parse_release_version(release: &str) -> Option<JavaVersion> {
    let raw_version = release.lines().find_map(|line| {
        line.strip_prefix("JAVA_VERSION=")
            .map(|value| value.trim().trim_matches('"'))
    })?;
    if raw_version.starts_with("1.8") || raw_version == "8" || raw_version.starts_with("8.") {
        return Some(JavaVersion::Java8);
    }
    let major = raw_version.split('.').next()?.parse::<u8>().ok()?;
    (major >= 9).then_some(JavaVersion::Java9Plus(major))
}

fn add_jdk_archive(
    archives: &mut Vec<PathBuf>,
    total_bytes: &mut u64,
    archive: PathBuf,
) -> Result<()> {
    use crate::jdk_cache::{MAX_JDK_ARCHIVES, MAX_JDK_ARCHIVE_BYTES, MAX_TOTAL_JDK_ARCHIVE_BYTES};

    if archives.len() == MAX_JDK_ARCHIVES {
        anyhow::bail!("JDK contains more than {MAX_JDK_ARCHIVES} archives");
    }
    let metadata = std::fs::metadata(&archive)
        .with_context(|| format!("failed to inspect JDK archive {}", archive.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("JDK archive is not a regular file: {}", archive.display());
    }
    if metadata.len() > MAX_JDK_ARCHIVE_BYTES {
        anyhow::bail!(
            "JDK archive {} is {} bytes (maximum {MAX_JDK_ARCHIVE_BYTES})",
            archive.display(),
            metadata.len()
        );
    }
    *total_bytes = total_bytes
        .checked_add(metadata.len())
        .context("JDK archive byte total overflowed u64")?;
    if *total_bytes > MAX_TOTAL_JDK_ARCHIVE_BYTES {
        anyhow::bail!(
            "JDK archives total {} bytes (maximum {MAX_TOTAL_JDK_ARCHIVE_BYTES})",
            *total_bytes
        );
    }
    archives.push(archive);
    Ok(())
}

/// Find JDK JAR files to index
fn find_jdk_jars(jdk_info: &JdkInfo) -> Result<Vec<PathBuf>> {
    let mut jars = Vec::new();
    let mut total_bytes = 0_u64;

    match jdk_info.version {
        JavaVersion::Java8 => {
            // Java 8: rt.jar contains all stdlib classes
            let rt_jar = jdk_info.java_home.join("jre/lib/rt.jar");
            if rt_jar.is_file() {
                add_jdk_archive(&mut jars, &mut total_bytes, rt_jar)?;
            } else {
                // Sometimes rt.jar is at $JAVA_HOME/lib/rt.jar
                let alt_rt_jar = jdk_info.java_home.join("lib/rt.jar");
                if alt_rt_jar.is_file() {
                    add_jdk_archive(&mut jars, &mut total_bytes, alt_rt_jar)?;
                } else {
                    warn!("rt.jar not found at expected locations");
                }
            }
        }
        JavaVersion::Java9Plus(_) => {
            use crate::jdk_cache::MAX_JDK_ARCHIVES;

            // Java 9+: jmods directory contains modular JARs
            let jmods_dir = jdk_info.java_home.join("jmods");
            if !jmods_dir.is_dir() {
                anyhow::bail!("jmods directory not found at {}", jmods_dir.display());
            }

            // Destructive-refactoring readiness requires the installed JDK, not
            // a hand-picked subset. Enumerate every regular JMOD so APIs such as
            // java.net.http and jdk.compiler cannot be silently absent while the
            // JDK component reports complete.
            for (inspected, entry) in std::fs::read_dir(&jmods_dir)
                .with_context(|| {
                    format!("failed to enumerate JMOD directory {}", jmods_dir.display())
                })?
                .enumerate()
            {
                if inspected >= MAX_JDK_ARCHIVES {
                    anyhow::bail!(
                        "JDK jmods directory contains more than {MAX_JDK_ARCHIVES} entries"
                    );
                }
                let entry = entry.with_context(|| {
                    format!("failed to read an entry in {}", jmods_dir.display())
                })?;
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .with_context(|| format!("failed to inspect {}", path.display()))?;
                if path.extension().and_then(|ext| ext.to_str()) == Some("jmod")
                    && (file_type.is_file() || file_type.is_symlink())
                {
                    add_jdk_archive(&mut jars, &mut total_bytes, path)?;
                }
            }
            jars.sort_unstable();
        }
    }

    if jars.is_empty() {
        anyhow::bail!(
            "no JDK archives found under {}",
            jdk_info.java_home.display()
        );
    }
    Ok(jars)
}

/// Cheaply staged archive identities for one JDK refresh.
///
/// The warm path reads metadata only. A cache miss upgrades this plan to
/// retained immutable snapshots before any bytecode is parsed.
struct JdkArchivePlan {
    archives: Vec<ExternalSourceArchive>,
    metadata: crate::jdk_cache::JdkArchiveMetadataManifest,
}

struct CapturedJdkArchivePlan {
    entries: Vec<CapturedJdkArchive>,
    manifest: crate::jdk_cache::JdkArchiveManifest,
    metadata: crate::jdk_cache::JdkArchiveMetadataManifest,
}

struct CapturedJdkArchive {
    archive: ExternalSourceArchive,
    snapshot: ExternalArchiveSnapshot,
}

struct CachedJdkArchive {
    archive: ExternalSourceArchive,
    symbols: Vec<hyperion_workspace::Symbol>,
    generic_supertypes: Vec<hyperion_workspace::TypeGenericSupertypes>,
}

impl JdkArchivePlan {
    fn stage(jdk_info: &JdkInfo) -> Result<Self> {
        use crate::jdk_cache::MAX_JDK_ARCHIVE_BYTES;

        let archives = find_jdk_jars(jdk_info)?
            .into_iter()
            .map(|path| {
                ExternalSourceArchive::capture_staged(&path, MAX_JDK_ARCHIVE_BYTES)
                    .map_err(anyhow::Error::msg)
                    .with_context(|| format!("failed to stage JDK archive {}", path.display()))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut seen_identities = std::collections::HashSet::with_capacity(archives.len());
        for archive in &archives {
            if !seen_identities.insert(archive.identity().clone()) {
                anyhow::bail!(
                    "JDK archive {} aliases another archive in the same installation",
                    archive.path().display()
                );
            }
        }
        let metadata =
            crate::jdk_cache::JdkArchiveMetadataManifest::capture(&jdk_info.java_home, &archives)?;
        Ok(Self { archives, metadata })
    }

    fn still_matches(&self, jdk_info: &JdkInfo) -> bool {
        // Re-enumerate the complete bounded archive set as part of the
        // publication fence. Checking only the originally staged paths would
        // miss an archive added after cache validation and could publish an
        // authenticated-but-incomplete JDK generation.
        Self::stage(jdk_info).is_ok_and(|current| current.metadata == self.metadata)
    }

    fn bind_cached_index_data(
        &self,
        java_home: &Path,
        partitions: Vec<crate::jdk_cache::JdkCachedArchiveIndex>,
    ) -> Result<Vec<CachedJdkArchive>> {
        let mut data_by_path = std::collections::HashMap::with_capacity(partitions.len());
        for partition in partitions {
            let (relative_path, symbols, generic_supertypes) = partition.into_parts();
            if data_by_path
                .insert(relative_path.clone(), (symbols, generic_supertypes))
                .is_some()
            {
                anyhow::bail!(
                    "JDK cache contains duplicate archive partition {}",
                    relative_path.display()
                );
            }
        }
        if data_by_path.len() != self.archives.len() {
            anyhow::bail!(
                "JDK cache contains {} archive partitions for {} staged archives",
                data_by_path.len(),
                self.archives.len()
            );
        }

        let mut bound = Vec::with_capacity(self.archives.len());
        for archive in &self.archives {
            let relative_path = archive.path().strip_prefix(java_home).with_context(|| {
                format!(
                    "staged JDK archive {} is outside JAVA_HOME {}",
                    archive.path().display(),
                    java_home.display()
                )
            })?;
            let (symbols, generic_supertypes) =
                data_by_path.remove(relative_path).with_context(|| {
                    format!(
                        "JDK cache has no index-data partition for {}",
                        relative_path.display()
                    )
                })?;
            bound.push(CachedJdkArchive {
                archive: archive.clone(),
                symbols,
                generic_supertypes,
            });
        }
        if !data_by_path.is_empty() {
            anyhow::bail!("JDK cache contains partitions absent from the staged archive set");
        }
        Ok(bound)
    }

    fn capture(self, jdk_info: &JdkInfo) -> Result<CapturedJdkArchivePlan> {
        use crate::jdk_cache::MAX_JDK_ARCHIVE_BYTES;
        use rayon::prelude::*;

        if !self.still_matches(jdk_info) {
            anyhow::bail!("JDK archive set changed before immutable capture");
        }
        let Self { archives, metadata } = self;
        let entries: Vec<_> = archives
            .into_par_iter()
            .map(|archive| {
                let display_path = archive.path().to_path_buf();
                archive
                    .into_versioned_snapshot(MAX_JDK_ARCHIVE_BYTES)
                    .map(|(archive, snapshot)| CapturedJdkArchive { archive, snapshot })
                    .map_err(anyhow::Error::msg)
                    .with_context(|| {
                        format!("failed to capture JDK archive {}", display_path.display())
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let archives: Vec<_> = entries.iter().map(|entry| entry.archive.clone()).collect();
        if !Self::stage(jdk_info).is_ok_and(|current| current.metadata == metadata) {
            anyhow::bail!("JDK archive set changed during immutable capture");
        }
        let manifest = metadata.with_content(&jdk_info.java_home, &archives)?;
        Ok(CapturedJdkArchivePlan {
            entries,
            manifest,
            metadata,
        })
    }
}

impl CapturedJdkArchivePlan {
    fn still_matches(&self, jdk_info: &JdkInfo) -> bool {
        JdkArchivePlan::stage(jdk_info).is_ok_and(|current| current.metadata == self.metadata)
    }
}

fn source_provider_for_plan(
    jdk_info: &JdkInfo,
    archive_plan: &JdkArchivePlan,
    source_providers: &crate::source_provider::SourceProviderChain,
) -> Option<Arc<dyn crate::source_provider::SourceProvider>> {
    crate::source_provider::JdkSourceProvider::from_binary_archives_with_store(
        &jdk_info.java_home,
        &archive_plan.archives,
        source_providers.materialized_sources(),
    )
    .map(|provider| Arc::new(provider) as Arc<dyn crate::source_provider::SourceProvider>)
}

fn record_jdk_provider_archive_success(
    source_providers: &crate::source_provider::SourceProviderChain,
    publication: &crate::source_provider::SourceProviderPublication,
    refresh: &hyperion_workspace::ExternalSourceRefresh,
    source_identity: &hyperion_workspace::ExternalSourceIdentity,
) -> bool {
    refresh
        .revision_for_identity(source_identity)
        .is_some_and(|revision| {
            source_providers.record_group_publication_success_for_revision(
                publication,
                source_identity,
                revision,
            )
        })
}

/// Index JDK stdlib classes into the workspace
///
/// Uses a persistent cache at ~/.cache/hyperion/jdk-{version}/ to avoid
/// re-parsing bytecode on every startup.
/// Index the JDK synchronously.
///
/// This performs filesystem access, cache decompression/serialization, and
/// CPU-heavy JAR/JMOD parsing. Async callers must run it on a blocking thread.
pub fn index_jdk_stdlib(workspace_index: Arc<WorkspaceIndex>) -> Result<JdkIndexOutcome> {
    index_jdk_stdlib_with_sources(
        crate::semantic_host::SemanticHost::new(workspace_index),
        Arc::new(crate::source_provider::SourceProviderChain::new()),
    )
}

/// Index JDK symbols and publish matching source navigation through one exact
/// `jdk-classpath` refresh transaction.
///
/// Runs on a blocking thread that holds no transaction: detection, cache I/O,
/// bytecode parsing and archive extraction happen outside the gate, and every
/// index publication step (reserving the refresh, installing one archive, the
/// final completion) is its own short write transaction so completion, hover
/// and editor notifications interleave with a JDK load instead of waiting for it.
pub fn index_jdk_stdlib_with_sources(
    host: Arc<crate::semantic_host::SemanticHost>,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
) -> Result<JdkIndexOutcome> {
    use crate::jdk_cache::{version_string, JdkCache};

    let workspace_index = Arc::clone(host.index());
    let total_start = std::time::Instant::now();
    info!("Starting JDK stdlib indexing...");
    let refresh_ticket =
        host.publish_blocking(|txn| txn.index().begin_external_source_refresh("jdk-classpath"));
    let source_refresh = source_providers
        .observe_group_refresh_guard(&refresh_ticket)
        .context("JDK source-provider refresh was superseded before detection")?;

    // Detect JDK
    let jdk_info = match detect_jdk() {
        Ok(info) => info,
        Err(e) => {
            warn!(
                "Failed to detect JDK: {}. Stdlib navigation will not work.",
                e
            );
            return Ok(JdkIndexOutcome::Unavailable {
                reason: e.to_string(),
            });
        }
    };

    let version_str = version_string(&jdk_info.version);
    info!(
        "Detected JDK {} at {}",
        version_str,
        jdk_info.java_home.display()
    );

    // Check JDK cache
    let jdk_cache = JdkCache::new(&version_str);
    // Stage a bounded, strong file-version identity before consulting the
    // cache. The warm path remains metadata-only; the cold path upgrades this
    // same plan to immutable private snapshots.
    let mut archive_plan = JdkArchivePlan::stage(&jdk_info)?;
    let mut source_provider = source_provider_for_plan(&jdk_info, &archive_plan, &source_providers);

    if let Some(partitions) =
        jdk_cache.load_valid_for_staged_manifest(&jdk_info.java_home, &archive_plan.metadata)
    {
        let cached_archives = archive_plan.bind_cached_index_data(&jdk_info.java_home, partitions);
        if let Err(error) = &cached_archives {
            warn!("JDK cache archive partitions are invalid: {error:#}");
        }
        if let Ok(cached_archives) = cached_archives {
            let archives: Vec<_> = cached_archives
                .iter()
                .map(|cached| cached.archive.clone())
                .collect();
            let source_identities: Vec<_> = archives
                .iter()
                .map(|archive| archive.identity().clone())
                .collect();
            // Provider callbacks run off-gate. If an older JDK publication
            // advances during preparation, coordinated publication refreshes
            // the fallback snapshot before committing the membership.
            // Cache hit - restore from cache
            let symbol_count = cached_archives
                .iter()
                .map(|cached| cached.symbols.len())
                .sum::<usize>();
            info!("Restoring {} JDK symbols from cache", symbol_count);
            let (provider_publication, refresh) = host.publish_blocking(|txn| {
                let (workspace_publication, provider_publication) =
                    crate::source_provider::begin_coordinated_external_publication(
                        txn.index(),
                        &refresh_ticket,
                        Some(&source_providers),
                        &source_identities,
                        source_provider.clone(),
                    )
                    .map_err(anyhow::Error::msg)
                    .context("JDK cache publication could not begin")?;
                let provider_publication = provider_publication.expect(
                    "a coordinated JDK publication with a provider chain returns a capability",
                );
                // Reserve one publication revision per staged JDK archive. Warm
                // symbols must retain the same provenance as cold bytecode
                // symbols so navigation can route split packages and duplicate
                // FQNs to the matching JMOD rather than an installation-wide
                // virtual source.
                let Some(refresh) = txn
                    .index()
                    .replace_external_archives_with_priority_for_refresh(
                        &refresh_ticket,
                        0,
                        &archives,
                    )
                else {
                    let retirement =
                        source_providers.abort_group_publication_deferred(&provider_publication);
                    drop(workspace_publication);
                    drop(retirement);
                    anyhow::bail!("JDK cache refresh was superseded before restoration");
                };
                // Restoration is deliberately outside the owner gate. The exact
                // revision token rejects a successor, while the pending provider
                // publication keeps source routing request-invisible.
                drop(workspace_publication);
                anyhow::Ok((provider_publication, refresh))
            })?;

            // Cache restoration interleaves with interactive requests: one write
            // transaction per archive (each publishes bounded chunks), so
            // completion is never stuck behind the entire JDK.
            for cached in cached_archives {
                let revision = refresh
                    .revision_for_identity(cached.archive.identity())
                    .expect("cached JDK archive must belong to its reserved refresh");
                if !host.publish_blocking(|txn| {
                    txn.index().add_external_index_data_responsive_for_revision(
                        cached.symbols,
                        cached.generic_supertypes,
                        revision,
                    )
                }) {
                    let retirement =
                        source_providers.abort_group_publication_deferred(&provider_publication);
                    drop(retirement);
                    anyhow::bail!("JDK cache restoration was superseded by a newer refresh");
                }
            }

            // Match the cold path: the authenticated generation remains a
            // coherent, routable snapshot even if the mutable installation
            // changes while it is being published, but readiness is withheld.
            let outcome = if archive_plan.still_matches(&jdk_info) {
                JdkIndexOutcome::Indexed
            } else {
                warn!("JDK archive set changed during cache restoration; retaining the authenticated navigation snapshot but withholding readiness");
                JdkIndexOutcome::Unavailable {
                    reason: "JDK archive set changed during cache restoration".to_string(),
                }
            };

            host.publish_blocking(|txn| -> Result<()> {
                let workspace_publication = txn
                    .index()
                    .begin_committed_external_source_publication(&refresh_ticket)
                    .context("JDK cache refresh was superseded before provider completion")?;
                for source_identity in &source_identities {
                    if !record_jdk_provider_archive_success(
                        &source_providers,
                        &provider_publication,
                        &refresh,
                        source_identity,
                    ) {
                        let retirement = source_providers
                            .abort_group_publication_deferred(&provider_publication);
                        drop(workspace_publication);
                        drop(retirement);
                        anyhow::bail!("JDK source-provider cache publication was superseded");
                    }
                }
                let retirement = source_providers
                    .finish_group_publication_deferred(&provider_publication)
                    .context("JDK source-provider cache publication was superseded")?;
                source_refresh.disarm();
                drop(workspace_publication);
                drop(retirement);
                // The completed JDK generation is visible to Salsa in the same
                // transaction that published it.
                txn.bump_workspace_epoch();
                Ok(())
            })?;

            let total_time = total_start.elapsed();
            let total_time_ms = (total_time.as_secs_f64() * 1000.0) as u64;

            // Record cache hit metrics
            METRICS.record_cache_hit(total_time_ms);

            // Show performance comparison if we have bytecode baseline
            if let Some(speedup) = METRICS.speedup_factor() {
                info!(
                    "JDK stdlib indexing complete via CACHE in {:.2}ms ({} symbols, {:.1}x faster than bytecode indexing)",
                    total_time.as_secs_f64() * 1000.0,
                    symbol_count,
                    speedup
                );
            } else {
                info!(
                    "JDK stdlib indexing complete via CACHE in {:.2}ms ({} symbols)",
                    total_time.as_secs_f64() * 1000.0,
                    symbol_count
                );
            }

            // Log cache statistics
            let hit_rate = METRICS.cache_hit_rate();
            if hit_rate > 0.0 {
                debug!("JDK cache hit rate: {:.1}%", hit_rate);
            }

            return Ok(outcome);
        }
        info!("JDK cache MISS - archive symbol partitions do not match the staged JDK");
        // Continue from the generation that defeated the warm publication
        // fence, not the stale pre-cache plan. Capture will fence it again
        // before, during, and immediately before bytecode publication.
        archive_plan = JdkArchivePlan::stage(&jdk_info)?;
        source_provider = source_provider_for_plan(&jdk_info, &archive_plan, &source_providers);
    }
    // Cache validation or loading failed; fall through to full indexing.

    // Cache miss or invalid - do full JDK indexing
    info!(
        "Building JDK cache from bytecode (this will take a few seconds, cache location: {:?})...",
        jdk_cache.cache_dir
    );

    let archive_plan = archive_plan.capture(&jdk_info)?;
    info!(
        "Found {} JDK archive(s) to index",
        archive_plan.entries.len()
    );
    let archives: Vec<_> = archive_plan
        .entries
        .iter()
        .map(|entry| entry.archive.clone())
        .collect();
    let source_identities: Vec<_> = archives
        .iter()
        .map(|archive| archive.identity().clone())
        .collect();
    // Directory enumeration and metadata inspection may block. Perform this
    // best-effort live-installation fence before entering the owner gate; the
    // captured bytecode generation itself is already immutable.
    if !archive_plan.still_matches(&jdk_info) {
        anyhow::bail!("JDK archive set changed before bytecode publication");
    }
    let (provider_publication, refresh) = host.publish_blocking(|txn| {
        let (workspace_publication, provider_publication) =
            crate::source_provider::begin_coordinated_external_publication(
                txn.index(),
                &refresh_ticket,
                Some(&source_providers),
                &source_identities,
                source_provider,
            )
            .map_err(anyhow::Error::msg)
            .context("JDK archive publication could not begin")?;
        let provider_publication = provider_publication
            .expect("a coordinated JDK publication with a provider chain returns a capability");
        let Some(refresh) = txn
            .index()
            .replace_external_archives_with_priority_for_refresh(&refresh_ticket, 0, &archives)
        else {
            let retirement =
                source_providers.abort_group_publication_deferred(&provider_publication);
            drop(workspace_publication);
            drop(retirement);
            anyhow::bail!("JDK archive refresh was superseded before extraction");
        };
        drop(workspace_publication);
        anyhow::Ok((provider_publication, refresh))
    })?;

    // Index each JAR
    let indexing_start = std::time::Instant::now();
    // Parallelize JAR/JMOD indexing for 40-60% speedup on multi-core systems
    // WorkspaceIndex uses RwLock internally, making it safe for concurrent access
    use rayon::prelude::*;

    let total_archives = archive_plan.entries.len();
    let captured_metadata = archive_plan.metadata.clone();
    let manifest = archive_plan.manifest;
    let results: Vec<_> = archive_plan
        .entries
        .into_par_iter()
        .map(|entry| {
            let file = entry.archive.path().to_path_buf();
            let source_identity = entry.archive.identity().clone();
            let revision = refresh
                .revision_for_identity(entry.archive.identity())
                .expect("JDK archive must belong to the reserved classpath refresh");
            // Check if it's a JMOD or JAR file
            let is_jmod = file.extension().and_then(|s| s.to_str()) == Some("jmod");

            debug!(
                "Indexing JDK {}: {}",
                if is_jmod { "JMOD" } else { "JAR" },
                file.display()
            );

            // One malformed third-party archive must not unwind the whole
            // Rayon pool and strand a half-open classpath transaction. Treat a
            // parser panic like any other per-archive failure; partial JDK
            // results are deliberately never cached.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if is_jmod {
                    workspace_index.prepare_jmod_snapshot_for_revision(entry.snapshot, revision)
                } else {
                    workspace_index.prepare_jar_snapshot_for_revision(entry.snapshot, revision)
                }
            }))
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "JDK archive parser panicked for {}",
                    file.display()
                ))
            });
            (file, source_identity, result)
        })
        .collect();

    // M19 FIX: Track failures instead of silently discarding them
    let mut total_classes = 0;
    let mut total_methods = 0;
    let mut total_fields = 0;
    let mut failed_jars = Vec::new();

    for (archive, source_identity, prepared) in results {
        let result = match prepared {
            Ok(prepared) => host.publish_blocking(|txn| {
                let publication = txn
                    .index()
                    .begin_committed_external_source_publication(&refresh_ticket)
                    .context("JDK archive refresh was superseded during extraction")?;
                let result = txn.index().publish_prepared_jar(prepared);
                if result.is_ok()
                    && !record_jdk_provider_archive_success(
                        &source_providers,
                        &provider_publication,
                        &refresh,
                        &source_identity,
                    )
                {
                    let retirement =
                        source_providers.abort_group_publication_deferred(&provider_publication);
                    drop(publication);
                    drop(retirement);
                    anyhow::bail!("JDK source-provider publication was superseded");
                }
                drop(publication);
                anyhow::Ok(result)
            })?,
            Err(error) => Err(error),
        };
        match result {
            Ok(stats) => {
                info!(
                    "Indexed {}: {} classes, {} methods, {} fields",
                    archive.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                    stats.classes_indexed,
                    stats.methods_indexed,
                    stats.fields_indexed
                );
                total_classes += stats.classes_indexed;
                total_methods += stats.methods_indexed;
                total_fields += stats.fields_indexed;
            }
            Err(e) => {
                warn!("Failed to index {}: {}", archive.display(), e);
                // Track failed JAR for reporting
                let jar_name = archive
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                failed_jars.push((jar_name.to_string(), e));
            }
        }
    }

    let indexing_time = indexing_start.elapsed();

    // M19 FIX: Report indexing failures to user
    if failed_jars.is_empty() {
        info!(
            "JDK bytecode indexing complete in {:.2}ms: {} classes, {} methods, {} fields",
            indexing_time.as_secs_f64() * 1000.0,
            total_classes,
            total_methods,
            total_fields
        );
    } else {
        warn!(
            "JDK indexing completed with {} FAILURES: {} classes, {} methods, {} fields indexed in {:.2}ms",
            failed_jars.len(),
            total_classes,
            total_methods,
            total_fields,
            indexing_time.as_secs_f64() * 1000.0
        );
        for (jar_name, error) in &failed_jars {
            warn!("  Failed to index {}: {}", jar_name, error);
        }
    }

    let live_installation_matches =
        JdkArchivePlan::stage(&jdk_info).is_ok_and(|current| current.metadata == captured_metadata);
    let outcome = if live_installation_matches {
        archive_outcome(total_archives, failed_jars.len())
    } else {
        warn!("JDK archive set changed during bytecode indexing; retaining the immutable navigation snapshot but withholding readiness and cache publication");
        JdkIndexOutcome::Unavailable {
            reason: "JDK archive set changed during bytecode indexing".to_string(),
        }
    };
    host.publish_blocking(|txn| -> Result<()> {
        let publication = txn
            .index()
            .begin_committed_external_source_publication(&refresh_ticket)
            .context("JDK archive refresh was superseded before source-provider completion")?;
        let retirement = source_providers
            .finish_group_publication_deferred(&provider_publication)
            .context("JDK source-provider completion was superseded")?;
        source_refresh.disarm();
        drop(publication);
        drop(retirement);
        // The completed JDK generation is visible to Salsa in the same
        // transaction that published it.
        txn.bump_workspace_epoch();
        Ok(())
    })?;

    let jdk_symbol_count = total_classes
        .checked_add(total_methods)
        .and_then(|count| count.checked_add(total_fields))
        .context("JDK symbol count overflowed usize")?;

    // Save only complete results. Persisting a partial archive set would make
    // the missing stdlib symbols survive subsequent healthy startups. A failed
    // retained archive deliberately keeps its prior published generation, so a
    // refresh-scoped cache snapshot must not even be requested for a partial
    // outcome: the exact publication fence would (correctly) reject it.
    if outcome.is_complete() {
        // Snapshot every symbol owned by this exact JDK refresh: a workspace
        // declaration can shadow an elected JDK symbol, and Gradle/Maven
        // indexing concurrently shares the global external-symbol index.
        let index_data_by_source = workspace_index
            .get_external_index_data_by_source_for_refresh(&refresh)
            .context("JDK archive refresh was superseded before cache snapshot")?;
        let mut index_data_by_source: std::collections::HashMap<_, _> = index_data_by_source
            .into_iter()
            .map(|(identity, symbols, generic_supertypes)| {
                (identity, (symbols, generic_supertypes))
            })
            .collect();
        let mut cached_partitions = Vec::with_capacity(archives.len());
        let mut cached_symbol_count = 0_usize;
        for archive in &archives {
            let (symbols, generic_supertypes) = index_data_by_source
                .remove(archive.identity())
                .context("JDK cache snapshot omitted a refreshed archive")?;
            cached_symbol_count = cached_symbol_count
                .checked_add(symbols.len())
                .context("JDK cache symbol count overflowed usize")?;
            let relative_path = archive
                .path()
                .strip_prefix(&jdk_info.java_home)
                .with_context(|| {
                    format!(
                        "JDK archive {} is outside JAVA_HOME {}",
                        archive.path().display(),
                        jdk_info.java_home.display()
                    )
                })?
                .to_path_buf();
            cached_partitions.push(crate::jdk_cache::JdkCachedArchiveIndex::new(
                relative_path,
                symbols,
                generic_supertypes,
            ));
        }
        if !index_data_by_source.is_empty() {
            anyhow::bail!("JDK cache snapshot contains a source outside the captured archive set");
        }
        // Extraction statistics count entries before symbol-key deduplication.
        // Cache partitions count the published entries, so equality is not an
        // invariant (even when every archive was indexed successfully).
        debug!(
            extracted_symbols = jdk_symbol_count,
            cached_symbols = cached_symbol_count,
            "JDK cache snapshot contains deduplicated published symbols"
        );
        if cached_symbol_count != 0 {
            if let Err(e) = jdk_cache.save_archive_partitions_for_manifest(
                &cached_partitions,
                &version_str,
                &jdk_info.java_home,
                &manifest,
            ) {
                warn!("Failed to save JDK cache: {}", e);
            }
        }
    }

    let total_time = total_start.elapsed();
    let total_time_ms = (total_time.as_secs_f64() * 1000.0) as u64;

    // Record cache miss metrics
    METRICS.record_cache_miss(total_time_ms);

    match &outcome {
        JdkIndexOutcome::Indexed => info!(
            "JDK stdlib indexing complete via BYTECODE in {:.2}ms (total symbols: {}, cache saved for next startup)",
            total_time.as_secs_f64() * 1000.0,
            jdk_symbol_count
        ),
        JdkIndexOutcome::Partial { .. } => warn!(
            "JDK stdlib indexing is partial after {:.2}ms (total symbols: {}, cache not saved)",
            total_time.as_secs_f64() * 1000.0,
            jdk_symbol_count
        ),
        JdkIndexOutcome::Unavailable { reason } => warn!(
            "JDK bytecode generation was retained for navigation but is not readiness-complete after {:.2}ms (total symbols: {}): {}",
            total_time.as_secs_f64() * 1000.0,
            jdk_symbol_count,
            reason
        ),
    }

    // Log cache statistics
    let hit_rate = METRICS.cache_hit_rate();
    if hit_rate > 0.0 {
        debug!("JDK cache hit rate: {:.1}%", hit_rate);
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct JdkRouteTestProvider {
        sources: std::collections::HashMap<hyperion_workspace::ExternalSourceIdentity, Arc<str>>,
    }

    impl JdkRouteTestProvider {
        fn new(
            sources: impl IntoIterator<
                Item = (hyperion_workspace::ExternalSourceIdentity, &'static str),
            >,
        ) -> Self {
            Self {
                sources: sources
                    .into_iter()
                    .map(|(identity, source)| (identity, Arc::from(source)))
                    .collect(),
            }
        }
    }

    impl crate::source_provider::SourceProvider for JdkRouteTestProvider {
        fn get_source_exact(
            &self,
            request: crate::source_provider::ExternalSourceRequest<'_>,
        ) -> Option<crate::source_provider::SourceContent> {
            let source = self.sources.get(request.source_identity())?;
            Some(crate::source_provider::SourceContent::Stub {
                source: Arc::new(crate::source_provider::SourceFile::new(Arc::clone(source))),
                temp_file: PathBuf::from(format!("/{}.java", request.fqn().replace('.', "/"))),
            })
        }

        fn can_provide_exact(
            &self,
            request: crate::source_provider::ExternalSourceRequest<'_>,
        ) -> bool {
            self.sources.contains_key(request.source_identity())
        }

        fn owns_source_identity(
            &self,
            source_identity: &hyperion_workspace::ExternalSourceIdentity,
        ) -> bool {
            self.sources.contains_key(source_identity)
        }

        fn snapshot_for_sources(
            &self,
            source_identities: &std::collections::HashSet<
                hyperion_workspace::ExternalSourceIdentity,
            >,
        ) -> Option<Arc<dyn crate::source_provider::SourceProvider>> {
            let sources = self
                .sources
                .iter()
                .filter(|(identity, _)| source_identities.contains(*identity))
                .map(|(identity, source)| (identity.clone(), Arc::clone(source)))
                .collect::<std::collections::HashMap<_, _>>();
            (!sources.is_empty()).then(|| {
                Arc::new(Self { sources }) as Arc<dyn crate::source_provider::SourceProvider>
            })
        }

        fn priority(&self) -> u32 {
            0
        }

        fn name(&self) -> &'static str {
            "JDK route test"
        }
    }

    fn cached_test_symbol(fqn: &str) -> hyperion_workspace::Symbol {
        let simple_name = fqn.rsplit('.').next().unwrap_or(fqn);
        hyperion_workspace::Symbol {
            fqn: Arc::from(fqn),
            simple_name: Arc::from(simple_name),
            range: hyperion_workspace::TextRange::new(0, simple_name.len() as u32),
            kind: lsp_types::SymbolKind::CLASS,
            modifiers: hyperion_workspace::Modifiers::default(),
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        }
    }

    #[test]
    fn partial_archive_outcome_is_not_cacheable() {
        assert!(archive_outcome(3, 0).is_complete());
        assert_eq!(
            archive_outcome(3, 1),
            JdkIndexOutcome::Partial {
                indexed_archives: 2,
                failed_archives: 1,
            }
        );
        assert!(!archive_outcome(3, 1).is_complete());
        assert!(!JdkIndexOutcome::Unavailable {
            reason: "missing".to_string(),
        }
        .is_complete());
    }

    #[test]
    fn java_nine_plus_enumerates_every_regular_jmod() {
        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(jmods.join("not-a-module.jmod")).unwrap();
        std::fs::write(jmods.join("java.base.jmod"), b"base").unwrap();
        std::fs::write(jmods.join("java.net.http.jmod"), b"http").unwrap();
        std::fs::write(jmods.join("README.txt"), b"ignored").unwrap();
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };

        let archives = find_jdk_jars(&info).unwrap();
        assert_eq!(
            archives,
            vec![
                jmods.join("java.base.jmod"),
                jmods.join("java.net.http.jmod")
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn staged_plan_rejects_two_jmod_names_for_one_canonical_archive() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        std::fs::write(jmods.join("java.base.jmod"), b"base").unwrap();
        symlink("java.base.jmod", jmods.join("java.base-alias.jmod")).unwrap();
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };

        let error = JdkArchivePlan::stage(&info)
            .err()
            .expect("aliased archives must be rejected");

        assert!(error.to_string().contains("aliases another archive"));
    }

    #[test]
    fn java_nine_plus_bounds_all_directory_entries_not_only_jmods() {
        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        std::fs::write(jmods.join("java.base.jmod"), b"base").unwrap();
        for index in 0..crate::jdk_cache::MAX_JDK_ARCHIVES {
            std::fs::write(jmods.join(format!("ignored-{index}.txt")), b"ignored").unwrap();
        }
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };

        let error = find_jdk_jars(&info).unwrap_err();

        assert!(error.to_string().contains("more than 1024 entries"));
    }

    #[test]
    fn staged_plan_rejects_a_jmod_added_before_publication() {
        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        std::fs::write(jmods.join("java.base.jmod"), b"base").unwrap();
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };
        let plan = JdkArchivePlan::stage(&info).unwrap();

        std::fs::write(jmods.join("java.net.http.jmod"), b"http").unwrap();

        assert!(!plan.still_matches(&info));
    }

    #[test]
    fn warm_archive_plan_retains_only_metadata_until_a_cache_miss() {
        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        // Deliberately not a JMOD. Staging must not parse, snapshot, or compute
        // a content identity for it; those operations belong only to capture.
        std::fs::write(jmods.join("java.base.jmod"), b"not bytecode").unwrap();
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };

        let plan = JdkArchivePlan::stage(&info).unwrap();

        assert_eq!(plan.archives.len(), 1);
        assert!(plan.archives[0].expected_file_version().is_some());
        assert!(plan.archives[0].expected_content().is_none());
    }

    #[test]
    fn warm_partitions_publish_under_their_exact_archive_revisions() {
        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let base_path = jmods.join("java.base.jmod");
        let sql_path = jmods.join("java.sql.jmod");
        std::fs::write(&base_path, b"base").unwrap();
        std::fs::write(&sql_path, b"sql").unwrap();
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };
        let plan = JdkArchivePlan::stage(&info).unwrap();
        let bound = plan
            .bind_cached_index_data(
                &info.java_home,
                vec![
                    crate::jdk_cache::JdkCachedArchiveIndex::new(
                        PathBuf::from("jmods/java.base.jmod"),
                        vec![cached_test_symbol("java.lang.CachedBase")],
                        vec![hyperion_workspace::TypeGenericSupertypes {
                            type_fqn: "java.util.List".to_string(),
                            edges: vec![hyperion_workspace::GenericSupertype {
                                written_target: "java.util.Collection".to_string(),
                                arguments: vec!["E".to_string()],
                            }],
                        }],
                    ),
                    crate::jdk_cache::JdkCachedArchiveIndex::new(
                        PathBuf::from("jmods/java.sql.jmod"),
                        vec![cached_test_symbol("java.sql.CachedSql")],
                        Vec::new(),
                    ),
                ],
            )
            .unwrap();
        let archives: Vec<_> = bound.iter().map(|cached| cached.archive.clone()).collect();
        let source_identities: Vec<_> = archives
            .iter()
            .map(|archive| archive.identity().clone())
            .collect();
        let index = WorkspaceIndex::new(temp.path().join("workspace"));
        let providers = crate::source_provider::SourceProviderChain::new();
        let ticket = index.begin_external_source_refresh("jdk-classpath");
        assert!(providers.observe_group_refresh(&ticket));
        let provider_publication = providers
            .begin_group_publication(
                &ticket,
                &source_identities,
                Some(Arc::new(JdkRouteTestProvider::new([
                    (
                        source_identities[0].clone(),
                        "package java.lang; public class CachedBase {}",
                    ),
                    (
                        source_identities[1].clone(),
                        "package java.sql; public class CachedSql {}",
                    ),
                ]))),
            )
            .unwrap();
        let refresh = index
            .replace_external_archives_with_priority_for_refresh(&ticket, 0, &archives)
            .unwrap();
        for cached in bound {
            let revision = refresh
                .revision_for_identity(cached.archive.identity())
                .unwrap();
            assert!(index.add_external_index_data_responsive_for_revision(
                cached.symbols,
                cached.generic_supertypes,
                revision,
            ));
            assert!(record_jdk_provider_archive_success(
                &providers,
                &provider_publication,
                &refresh,
                cached.archive.identity(),
            ));
        }
        assert!(providers.finish_group_publication(&provider_publication));

        let base_location = index.find_symbol("java.lang.CachedBase").unwrap();
        let sql_location = index.find_symbol("java.sql.CachedSql").unwrap();
        assert_eq!(
            index.get_external_source_identity(base_location.file_id),
            Some(archives[0].identity().clone())
        );
        assert_eq!(
            index.get_external_source_identity(sql_location.file_id),
            Some(archives[1].identity().clone())
        );
        let restored_by_source: std::collections::HashMap<_, _> = index
            .get_external_index_data_by_source_for_refresh(&refresh)
            .unwrap()
            .into_iter()
            .map(|(identity, _symbols, generic_supertypes)| (identity, generic_supertypes))
            .collect();
        assert_eq!(
            restored_by_source[archives[0].identity()][0].type_fqn,
            "java.util.List"
        );
        assert!(crate::navigation_util::symbol_location_to_lsp_location(
            &index,
            &base_location,
            Some(&providers),
        )
        .unwrap()
        .is_some());
        assert!(crate::navigation_util::symbol_location_to_lsp_location(
            &index,
            &sql_location,
            Some(&providers),
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn cold_archive_publication_routes_navigation_by_exact_revision() {
        let temp = tempfile::TempDir::new().unwrap();
        let archive_path = temp.path().join("java.base.jar");
        let archive = ExternalSourceArchive::capture(&archive_path);
        let source_identity = archive.identity().clone();
        let index = WorkspaceIndex::new(temp.path().join("workspace"));
        let providers = crate::source_provider::SourceProviderChain::new();
        let ticket = index.begin_external_source_refresh("jdk-classpath");
        assert!(providers.observe_group_refresh(&ticket));
        let provider_publication = providers
            .begin_group_publication(
                &ticket,
                std::slice::from_ref(&source_identity),
                Some(Arc::new(JdkRouteTestProvider::new([(
                    source_identity.clone(),
                    "package java.lang; public class ColdOnly {}",
                )]))),
            )
            .unwrap();
        let refresh = index
            .replace_external_archives_with_priority_for_refresh(
                &ticket,
                0,
                std::slice::from_ref(&archive),
            )
            .unwrap();
        let revision = refresh.revision_for_identity(&source_identity).unwrap();
        assert!(index.add_external_symbols_responsive_for_revision(
            vec![cached_test_symbol("java.lang.ColdOnly")],
            revision,
        ));
        assert!(record_jdk_provider_archive_success(
            &providers,
            &provider_publication,
            &refresh,
            &source_identity,
        ));
        assert!(providers.finish_group_publication(&provider_publication));

        let location = index.find_symbol("java.lang.ColdOnly").unwrap();
        assert!(crate::navigation_util::symbol_location_to_lsp_location(
            &index,
            &location,
            Some(&providers),
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn staged_plan_rejects_removed_and_renamed_jmods() {
        let temp = tempfile::TempDir::new().unwrap();
        let jmods = temp.path().join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        std::fs::write(jmods.join("java.base.jmod"), b"base").unwrap();
        let logging = jmods.join("java.logging.jmod");
        std::fs::write(&logging, b"logging").unwrap();
        let info = JdkInfo {
            java_home: temp.path().to_path_buf(),
            version: JavaVersion::Java9Plus(21),
        };

        let before_removal = JdkArchivePlan::stage(&info).unwrap();
        std::fs::remove_file(&logging).unwrap();
        assert!(!before_removal.still_matches(&info));

        std::fs::write(&logging, b"logging").unwrap();
        let before_rename = JdkArchivePlan::stage(&info).unwrap();
        std::fs::rename(&logging, jmods.join("java.sql.jmod")).unwrap();
        assert!(!before_rename.still_matches(&info));
    }

    #[test]
    fn parses_standard_jdk_release_versions() {
        assert_eq!(
            parse_release_version("JAVA_VERSION=\"1.8.0_402\"\n"),
            Some(JavaVersion::Java8)
        );
        assert_eq!(
            parse_release_version("IMPLEMENTOR=\"OpenJDK\"\nJAVA_VERSION=\"21.0.2\"\n"),
            Some(JavaVersion::Java9Plus(21))
        );
        assert_eq!(parse_release_version("JAVA_VERSION=\"broken\"\n"), None);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_java_home_follows_relative_symlink_chains() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let jdk_bin = temp.path().join("jdk/bin");
        let alternatives = temp.path().join("alternatives");
        let system_bin = temp.path().join("system-bin");
        std::fs::create_dir_all(&jdk_bin).unwrap();
        std::fs::create_dir_all(&alternatives).unwrap();
        std::fs::create_dir_all(&system_bin).unwrap();
        std::fs::write(jdk_bin.join("java"), b"fixture").unwrap();
        symlink("../jdk/bin/java", alternatives.join("java")).unwrap();
        symlink("../alternatives/java", system_bin.join("java")).unwrap();

        let java_home = resolve_java_home(&system_bin.join("java")).unwrap();

        assert_eq!(java_home, temp.path().canonicalize().unwrap().join("jdk"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_java_home_uses_one_canonical_root_for_archive_staging() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real_home = temp.path().join("jdk-21");
        let jmods = real_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        std::fs::write(real_home.join("release"), "JAVA_VERSION=\"21.0.2\"\n").unwrap();
        std::fs::write(jmods.join("java.base.jmod"), b"metadata-only fixture").unwrap();
        let alias = temp.path().join("default-java");
        symlink("jdk-21", &alias).unwrap();

        let info = jdk_info_from_home(&alias).unwrap();
        let canonical_home = real_home.canonicalize().unwrap();
        assert_eq!(info.java_home, canonical_home);

        let plan = JdkArchivePlan::stage(&info)
            .expect("canonical archive paths must remain beneath canonical JAVA_HOME");
        assert_eq!(plan.archives.len(), 1);
        assert_eq!(
            plan.archives[0].path(),
            canonical_home.join("jmods/java.base.jmod")
        );
    }

    #[test]
    fn test_jdk_detection() {
        // This test will only work if JDK is installed
        match detect_jdk() {
            Ok(info) => {
                println!("Detected JDK: {:?}", info);
                assert!(info.java_home.exists());
            }
            Err(e) => {
                println!("JDK detection failed (may be expected in CI): {}", e);
            }
        }
    }

    #[test]
    fn test_version_detection() {
        // Test version parsing logic
        if let Ok(jdk) = detect_jdk() {
            println!("JDK version: {:?}", jdk.version);

            let jars = find_jdk_jars(&jdk).expect("Should find JDK JARs");
            println!("Found {} JAR(s)", jars.len());

            for jar in &jars {
                println!("  - {}", jar.display());
                assert!(jar.exists(), "JAR should exist: {:?}", jar);
            }
        }
    }

    #[test]
    fn test_java8_vs_java9plus() {
        if let Ok(jdk) = detect_jdk() {
            match jdk.version {
                JavaVersion::Java8 => {
                    let rt_jar = jdk.java_home.join("jre/lib/rt.jar");
                    let alt_rt_jar = jdk.java_home.join("lib/rt.jar");
                    assert!(
                        rt_jar.exists() || alt_rt_jar.exists(),
                        "Java 8 should have rt.jar"
                    );
                }
                JavaVersion::Java9Plus(_) => {
                    let jmods_dir = jdk.java_home.join("jmods");
                    assert!(jmods_dir.exists(), "Java 9+ should have jmods directory");

                    // Check for java.base.jmod specifically
                    let java_base = jmods_dir.join("java.base.jmod");
                    assert!(java_base.exists(), "Should have java.base.jmod");
                }
            }
        }
    }

    #[test]
    fn test_jmod_file_extension_detection() {
        use std::path::PathBuf;

        let jmod_file = PathBuf::from("/some/path/java.base.jmod");
        let jar_file = PathBuf::from("/some/path/library.jar");

        let is_jmod = jmod_file.extension().and_then(|s| s.to_str()) == Some("jmod");
        let is_jar = jar_file.extension().and_then(|s| s.to_str()) == Some("jar");

        assert!(is_jmod, "Should detect .jmod extension");
        assert!(is_jar, "Should detect .jar extension");
    }
}
