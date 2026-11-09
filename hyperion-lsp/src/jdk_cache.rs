// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK Symbol Cache
//!
//! Caches JDK symbols per JDK version in ~/.cache/hyperion/jdk-{version}/
//! to avoid re-parsing bytecode on every startup.

use fs2::FileExt;
use hyperion_workspace::{
    ArchiveFileVersion, CachedSymbol, ExternalSourceArchive, Symbol, TypeGenericSupertypes,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::{
    fs::File,
    io::{self, Read},
};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Current JDK cache format version - bump when format changes.
///
/// Version 2: Added modifiers field to CachedSymbol
/// Version 3: Chunked format for parallel deserialization
/// Version 4: Generic return types from Signature attribute
/// Version 5: Keep method-level generic return types (e.g., Optional.of → Optional<T>)
/// Version 6: Preserve class/interface type-parameter metadata
/// Version 7: Store named nested classes with source-form dotted names
/// Version 8: Preserve method type parameters and generic parameter types
/// Version 9: Preserve canonical qualified method parameter and return types
/// Version 10: Cache the complete installed JMOD set instead of a selected subset
/// Version 11: Persist the archive manifest used to build the cache
/// Version 12: Add archive content digests and authenticate the serialized symbol payload
/// Version 13: Derive the manifest from the immutable archive generation used for parsing
/// Version 14: Bind content digests to strong file versions for bounded warm validation
/// Version 15: Partition symbols by their exact JDK archive for source navigation
/// Version 16: Preserve generic supertype edges used for inherited type projection
/// Version 18: Preserve method and constructor `throws` clauses
/// Version 19: Record a thrown type variable by name instead of its erasure
/// Version 20: Bytecode declarations carry their `@Deprecated` flag
pub const JDK_CACHE_VERSION: u32 = 21;

/// Number of symbols per chunk for parallel deserialization
/// Tuned to balance parallelism vs overhead: ~500-1000 symbols/chunk
/// With ~50K JDK symbols, this creates ~50-100 chunks for good CPU utilization
const SYMBOLS_PER_CHUNK: usize = 1000;
const MAX_JDK_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_JDK_METADATA_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_JDK_ARCHIVES: usize = 1_024;
pub(crate) const MAX_JDK_ARCHIVE_BYTES: u64 = 500 * 1024 * 1024;
pub(crate) const MAX_TOTAL_JDK_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_JDK_ARCHIVE_PATH_BYTES: usize = 4 * 1024;

#[derive(Debug, Error)]
pub enum JdkCacheError {
    #[error("Failed to create JDK cache directory {path}: {source}", path = .path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Failed to open JDK cache lock file {path}: {source}", path = .path.display())]
    OpenLock {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Failed to lock JDK cache {path}: {source}", path = .path.display())]
    AcquireLock {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Failed to serialize JDK cache: {0}")]
    SerializeCache(#[source] bincode::Error),
    #[error("JDK cache is too large: {actual} bytes (maximum {maximum})")]
    CacheTooLarge { actual: usize, maximum: usize },
    #[error("Failed to write JDK cache {path}: {source}", path = .path.display())]
    WriteCache {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "Failed to publish JDK cache from {temporary} to {destination}: {source}",
        temporary = .temporary.display(),
        destination = .destination.display()
    )]
    PublishCache {
        temporary: PathBuf,
        destination: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Failed to serialize JDK cache metadata: {0}")]
    SerializeMetadata(#[source] serde_json::Error),
    #[error("Failed to write JDK cache metadata {path}: {source}", path = .path.display())]
    WriteMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Failed to read JDK cache metadata {path}: {source}", path = .path.display())]
    ReadMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("JDK cache metadata {path} is not valid UTF-8: {source}", path = .path.display())]
    MetadataUtf8 {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("Failed to parse JDK cache metadata {path}: {source}", path = .path.display())]
    ParseMetadata {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Failed to remove JDK cache {path}: {source}", path = .path.display())]
    Remove {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Failed to inspect JDK archive {path}: {source}", path = .path.display())]
    ArchiveMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Invalid JDK archive manifest: {reason}")]
    ArchiveManifest { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct JdkArchiveFingerprint {
    relative_path: PathBuf,
    file_version: ArchiveFileVersion,
    length: u64,
    content_sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JdkArchiveMetadataFingerprint {
    relative_path: PathBuf,
    file_version: ArchiveFileVersion,
}

/// Cheaply staged identity of the archive set named by one JDK refresh.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct JdkArchiveMetadataManifest(Vec<JdkArchiveMetadataFingerprint>);

impl JdkArchiveMetadataManifest {
    pub(crate) fn capture(
        java_home: &Path,
        archives: &[ExternalSourceArchive],
    ) -> Result<Self, JdkCacheError> {
        if archives.len() > MAX_JDK_ARCHIVES {
            return Err(JdkCacheError::ArchiveManifest {
                reason: format!(
                    "{} archives exceeds the {MAX_JDK_ARCHIVES}-archive limit",
                    archives.len()
                ),
            });
        }

        let mut fingerprints = Vec::with_capacity(archives.len());
        let mut seen_paths = HashSet::with_capacity(archives.len());
        let mut total_bytes = 0_u64;
        for archive in archives {
            let relative_path = bounded_relative_path(java_home, archive.path())?;
            if !seen_paths.insert(relative_path.clone()) {
                return Err(JdkCacheError::ArchiveManifest {
                    reason: format!(
                        "archive {} is listed more than once",
                        archive.path().display()
                    ),
                });
            }
            let file_version =
                archive
                    .expected_file_version()
                    .ok_or_else(|| JdkCacheError::ArchiveManifest {
                        reason: format!(
                            "archive {} was not staged with a strong file version",
                            archive.path().display()
                        ),
                    })?;
            validate_archive_size(archive.path(), file_version.bytes(), &mut total_bytes)?;
            fingerprints.push(JdkArchiveMetadataFingerprint {
                relative_path,
                file_version,
            });
        }
        fingerprints.sort_unstable_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(Self(fingerprints))
    }

    #[cfg(test)]
    pub(crate) fn still_matches(
        &self,
        java_home: &Path,
        archives: &[ExternalSourceArchive],
    ) -> bool {
        archives
            .iter()
            .all(ExternalSourceArchive::staged_version_is_current)
            && Self::capture(java_home, archives).is_ok_and(|current| current == *self)
    }

    fn matches_persisted(&self, persisted: &[JdkArchiveFingerprint]) -> bool {
        self.0.len() == persisted.len()
            && self.0.iter().zip(persisted).all(|(staged, cached)| {
                staged.relative_path == cached.relative_path
                    && staged.file_version == cached.file_version
            })
    }

    pub(crate) fn with_content(
        &self,
        java_home: &Path,
        archives: &[ExternalSourceArchive],
    ) -> Result<JdkArchiveManifest, JdkCacheError> {
        let mut fingerprints = Vec::with_capacity(archives.len());
        for archive in archives {
            let relative_path = bounded_relative_path(java_home, archive.path())?;
            let metadata = self
                .0
                .binary_search_by(|entry| entry.relative_path.cmp(&relative_path))
                .ok()
                .map(|index| &self.0[index])
                .ok_or_else(|| JdkCacheError::ArchiveManifest {
                    reason: format!(
                        "archive {} is absent from the staged JDK generation",
                        archive.path().display()
                    ),
                })?;
            let content =
                archive
                    .expected_content()
                    .ok_or_else(|| JdkCacheError::ArchiveManifest {
                        reason: format!(
                            "archive {} has no captured content identity",
                            archive.path().display()
                        ),
                    })?;
            if content.bytes() != metadata.file_version.bytes() {
                return Err(JdkCacheError::ArchiveManifest {
                    reason: format!(
                        "archive {} snapshot length differs from its staged file version",
                        archive.path().display()
                    ),
                });
            }
            fingerprints.push(JdkArchiveFingerprint {
                relative_path,
                file_version: metadata.file_version,
                length: content.bytes(),
                content_sha256: content.sha256(),
            });
        }
        fingerprints.sort_unstable_by(|left, right| left.relative_path.cmp(&right.relative_path));
        if fingerprints.len() != self.0.len()
            || fingerprints
                .iter()
                .zip(&self.0)
                .any(|(captured, staged)| captured.relative_path != staged.relative_path)
        {
            return Err(JdkCacheError::ArchiveManifest {
                reason: "captured archive set differs from its staged JDK generation".to_string(),
            });
        }
        Ok(JdkArchiveManifest(fingerprints))
    }
}

fn bounded_relative_path(java_home: &Path, archive: &Path) -> Result<PathBuf, JdkCacheError> {
    let relative_path =
        archive
            .strip_prefix(java_home)
            .map_err(|_| JdkCacheError::ArchiveManifest {
                reason: format!(
                    "archive {} is outside JAVA_HOME {}",
                    archive.display(),
                    java_home.display()
                ),
            })?;
    if !is_bounded_cached_relative_path(relative_path) {
        return Err(JdkCacheError::ArchiveManifest {
            reason: format!(
                "archive path {} is not a bounded relative path",
                archive.display()
            ),
        });
    }
    Ok(relative_path.to_path_buf())
}

fn is_bounded_cached_relative_path(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().len() <= MAX_JDK_ARCHIVE_PATH_BYTES
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn validate_archive_size(
    archive: &Path,
    bytes: u64,
    total_bytes: &mut u64,
) -> Result<(), JdkCacheError> {
    if bytes > MAX_JDK_ARCHIVE_BYTES {
        return Err(JdkCacheError::ArchiveManifest {
            reason: format!(
                "archive {} is {bytes} bytes (maximum {MAX_JDK_ARCHIVE_BYTES})",
                archive.display()
            ),
        });
    }
    *total_bytes =
        total_bytes
            .checked_add(bytes)
            .ok_or_else(|| JdkCacheError::ArchiveManifest {
                reason: "archive byte total overflowed u64".to_string(),
            })?;
    if *total_bytes > MAX_TOTAL_JDK_ARCHIVE_BYTES {
        return Err(JdkCacheError::ArchiveManifest {
            reason: format!(
                "archives total {total_bytes} bytes (maximum {MAX_TOTAL_JDK_ARCHIVE_BYTES})"
            ),
        });
    }
    Ok(())
}

/// Content identity for one complete, immutable JDK archive generation.
///
/// The cache accepts this value only from the archive plan that also owns the
/// snapshots used by the bytecode indexer. It never authenticates symbols by
/// reopening mutable installation paths independently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct JdkArchiveManifest(Vec<JdkArchiveFingerprint>);

impl JdkArchiveManifest {
    #[cfg(test)]
    pub(crate) fn from_archives(
        java_home: &Path,
        archives: &[ExternalSourceArchive],
    ) -> Result<Self, JdkCacheError> {
        let metadata = JdkArchiveMetadataManifest::capture(java_home, archives)?;
        metadata.with_content(java_home, archives)
    }

    fn fingerprints(&self) -> &[JdkArchiveFingerprint] {
        &self.0
    }
}

/// Complete index data cached for one exact archive in the persisted JDK manifest.
///
/// The relative path is deliberately part of the authenticated symbol payload,
/// rather than inferred from a symbol package or module name. Split packages
/// and duplicate class names make those inferences lossy.
#[derive(Debug)]
pub(crate) struct JdkCachedArchiveIndex {
    relative_path: PathBuf,
    symbols: Vec<Symbol>,
    generic_supertypes: Vec<TypeGenericSupertypes>,
}

impl JdkCachedArchiveIndex {
    pub(crate) const fn new(
        relative_path: PathBuf,
        symbols: Vec<Symbol>,
        generic_supertypes: Vec<TypeGenericSupertypes>,
    ) -> Self {
        Self {
            relative_path,
            symbols,
            generic_supertypes,
        }
    }

    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub(crate) fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub(crate) fn generic_supertypes(&self) -> &[TypeGenericSupertypes] {
        &self.generic_supertypes
    }

    pub(crate) fn into_symbols(self) -> Vec<Symbol> {
        self.symbols
    }

    pub(crate) fn into_parts(self) -> (PathBuf, Vec<Symbol>, Vec<TypeGenericSupertypes>) {
        (self.relative_path, self.symbols, self.generic_supertypes)
    }
}

/// JDK cache metadata
#[derive(Debug, Serialize, Deserialize)]
struct JdkCacheMetadata {
    /// Cache format version
    version: u32,
    /// JDK version string (e.g., "21", "17", "8")
    jdk_version: String,
    /// Original JAVA_HOME path (to detect JDK relocation)
    java_home: PathBuf,
    /// Number of symbols cached
    symbol_count: usize,
    /// Number of chunks in symbols.bin (for parallel loading)
    #[serde(default)]
    chunk_count: usize,
    /// Sorted metadata and content fingerprint of every represented archive.
    #[serde(default)]
    archive_manifest: Vec<JdkArchiveFingerprint>,
    /// Digest of the exact serialized payload. A cache file can otherwise be
    /// valid bincode after truncation or replacement while containing symbols
    /// unrelated to this metadata and archive manifest.
    #[serde(default)]
    symbols_sha256: [u8; 32],
}

/// Cached JDK index data (symbols are chunked for parallel conversion).
#[derive(Serialize, Deserialize)]
struct JdkCacheData {
    archives: Vec<JdkCachedArchiveData>,
}

#[derive(Serialize, Deserialize)]
struct JdkCachedArchiveData {
    /// Exact path from the cached JAVA_HOME to the owning JMOD or rt.jar.
    relative_path: PathBuf,
    /// Symbol chunks (each chunk is independently convertible in parallel).
    chunks: Vec<Vec<CachedSymbol>>,
    /// Generic arguments written on direct class/interface edges.
    generic_supertypes: Vec<TypeGenericSupertypes>,
}

/// JDK cache manager
pub struct JdkCache {
    pub(crate) cache_dir: PathBuf,
}

impl JdkCache {
    /// Create a new JDK cache for the given JDK version
    pub fn new(jdk_version: &str) -> Self {
        let cache_base = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from(".cache"))
            .join("hyperion")
            .join(format!("jdk-{jdk_version}"));

        Self {
            cache_dir: cache_base,
        }
    }

    /// Check if cache exists and is valid for the given JAVA_HOME
    pub fn is_valid(&self, java_home: &Path) -> bool {
        self.is_valid_for_manifest(java_home, &JdkArchiveManifest::default())
    }

    /// Validate both the JDK root and the exact immutable archive generation
    /// used to produce the cached symbols.
    pub(crate) fn is_valid_for_manifest(
        &self,
        java_home: &Path,
        manifest: &JdkArchiveManifest,
    ) -> bool {
        let _lock = match self.lock_shared() {
            Ok(lock) => lock,
            Err(e) => {
                warn!("JDK cache validation failed to acquire lock: {}", e);
                return false;
            }
        };
        self.valid_metadata_for_manifest(java_home, manifest)
            .is_some()
    }

    /// Validate and load under one shared cache lock. The manifest belongs to
    /// the immutable snapshots retained by the caller, so a live pathname
    /// replacement cannot change the generation being authenticated while the
    /// cache payload is loaded or later published.
    #[cfg(test)]
    pub(crate) fn load_valid_for_manifest(
        &self,
        java_home: &Path,
        manifest: &JdkArchiveManifest,
    ) -> Option<Vec<Symbol>> {
        let _lock = match self.lock_shared() {
            Ok(lock) => lock,
            Err(error) => {
                warn!("JDK cache load failed to acquire lock: {}", error);
                return None;
            }
        };
        let metadata = self.valid_metadata_for_manifest(java_home, manifest)?;
        Some(
            self.load_partitioned_locked(&metadata)?
                .into_iter()
                .flat_map(JdkCachedArchiveIndex::into_symbols)
                .collect(),
        )
    }

    /// Warm-cache validation using only bounded filesystem metadata. The
    /// caller must revalidate this exact staged set while holding the JDK
    /// publication gate before making the returned symbols visible.
    pub(crate) fn load_valid_for_staged_manifest(
        &self,
        java_home: &Path,
        manifest: &JdkArchiveMetadataManifest,
    ) -> Option<Vec<JdkCachedArchiveIndex>> {
        let _lock = match self.lock_shared() {
            Ok(lock) => lock,
            Err(error) => {
                warn!("JDK cache load failed to acquire lock: {}", error);
                return None;
            }
        };
        let metadata = self.valid_metadata_header(java_home)?;
        if !manifest.matches_persisted(&metadata.archive_manifest) {
            info!("JDK cache MISS - staged archive file versions changed");
            return None;
        }
        self.load_partitioned_locked(&metadata)
    }

    fn valid_metadata_for_manifest(
        &self,
        java_home: &Path,
        manifest: &JdkArchiveManifest,
    ) -> Option<JdkCacheMetadata> {
        let meta = self.valid_metadata_header(java_home)?;

        if meta.archive_manifest != manifest.fingerprints() {
            info!("JDK cache MISS - captured archive generation changed");
            return None;
        }

        info!(
            "JDK cache content-manifest validation complete - cache is valid ({} symbols)",
            meta.symbol_count
        );
        Some(meta)
    }

    fn valid_metadata_header(&self, java_home: &Path) -> Option<JdkCacheMetadata> {
        let metadata_path = self.cache_dir.join("metadata.json");
        let symbols_path = self.cache_dir.join("symbols.bin");
        if !metadata_path.exists() || !symbols_path.exists() {
            info!("JDK cache MISS - cache not found at {:?}", self.cache_dir);
            return None;
        }

        let meta = match self.load_metadata() {
            Ok(meta) => meta,
            Err(error) => {
                warn!("JDK cache MISS - failed to load metadata: {}", error);
                return None;
            }
        };
        if meta.version != JDK_CACHE_VERSION {
            info!(
                "JDK cache MISS - version mismatch (expected {}, got {}), invalidating stale cache",
                JDK_CACHE_VERSION, meta.version
            );
            return None;
        }
        if meta.java_home != java_home {
            info!(
                "JDK cache MISS - java_home mismatch ({:?} vs {:?}), invalidating",
                meta.java_home, java_home
            );
            return None;
        }
        Some(meta)
    }

    /// Load cached JDK symbols with parallel deserialization.
    ///
    /// This compatibility API intentionally returns symbols only. Production
    /// restoration uses the partitioned API so generic hierarchy metadata and
    /// archive ownership are retained.
    pub fn load(&self) -> Option<Vec<Symbol>> {
        let _lock = match self.lock_shared() {
            Ok(lock) => lock,
            Err(e) => {
                warn!("Failed to acquire JDK cache lock: {}", e);
                return None;
            }
        };
        let metadata = match self.load_metadata() {
            Ok(metadata) if metadata.version == JDK_CACHE_VERSION => metadata,
            Ok(metadata) => {
                warn!(
                    "Failed to load JDK cache: version mismatch (expected {}, got {})",
                    JDK_CACHE_VERSION, metadata.version
                );
                return None;
            }
            Err(error) => {
                warn!("Failed to load JDK cache metadata: {}", error);
                return None;
            }
        };
        Some(
            self.load_partitioned_locked(&metadata)?
                .into_iter()
                .flat_map(JdkCachedArchiveIndex::into_symbols)
                .collect(),
        )
    }

    fn load_partitioned_locked(
        &self,
        metadata: &JdkCacheMetadata,
    ) -> Option<Vec<JdkCachedArchiveIndex>> {
        let start = std::time::Instant::now();
        let symbols_path = self.cache_dir.join("symbols.bin");

        let read_start = std::time::Instant::now();
        let bytes = match read_bounded_file(&symbols_path, MAX_JDK_CACHE_BYTES) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to read JDK cache: {}", e);
                return None;
            }
        };
        let read_time = read_start.elapsed();
        let payload_digest: [u8; 32] = Sha256::digest(&bytes).into();
        if payload_digest != metadata.symbols_sha256 {
            warn!("Failed to load JDK cache: symbol payload digest mismatch");
            return None;
        }

        let deserialize_start = std::time::Instant::now();
        let data: JdkCacheData = match deserialize_cache_data(&bytes) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to deserialize JDK cache: {}", e);
                return None;
            }
        };
        let deserialize_time = deserialize_start.elapsed();
        let expected_paths: Vec<&Path> = if metadata.archive_manifest.is_empty() {
            vec![Path::new("")]
        } else {
            metadata
                .archive_manifest
                .iter()
                .map(|entry| entry.relative_path.as_path())
                .collect()
        };
        if data.archives.len() != expected_paths.len()
            || data
                .archives
                .iter()
                .zip(&expected_paths)
                .any(|(archive, expected)| {
                    archive.relative_path != *expected
                        || !is_bounded_cached_relative_path(&archive.relative_path)
                })
        {
            warn!("Failed to load JDK cache: archive partitions do not match metadata");
            return None;
        }
        let (chunk_count, symbol_count) = data.archives.iter().try_fold(
            (0_usize, 0_usize),
            |(chunk_count, symbol_count), archive| {
                let chunk_count = chunk_count.checked_add(archive.chunks.len())?;
                let symbol_count = archive
                    .chunks
                    .iter()
                    .try_fold(symbol_count, |count, chunk| count.checked_add(chunk.len()))?;
                Some((chunk_count, symbol_count))
            },
        )?;
        if chunk_count != metadata.chunk_count || symbol_count != metadata.symbol_count {
            warn!(
                "Failed to load JDK cache: payload shape does not match metadata ({} chunks/{} symbols, expected {}/{})",
                chunk_count,
                symbol_count,
                metadata.chunk_count,
                metadata.symbol_count
            );
            return None;
        }

        let convert_start = std::time::Instant::now();
        // Convert archive partitions independently while preserving both
        // manifest order and the owner assignment authenticated by the payload.
        use rayon::prelude::*;
        let archives: Vec<JdkCachedArchiveIndex> = data
            .archives
            .into_par_iter()
            .map(|archive| {
                let symbol_chunks: Vec<Vec<Symbol>> = archive
                    .chunks
                    .into_par_iter()
                    .map(|chunk| {
                        chunk
                            .into_iter()
                            .map(|symbol| hyperion_workspace::cache::cached_to_symbol(&symbol))
                            .collect()
                    })
                    .collect();
                let symbols = symbol_chunks.into_iter().flatten().collect();
                JdkCachedArchiveIndex::new(
                    archive.relative_path,
                    symbols,
                    archive.generic_supertypes,
                )
            })
            .collect();
        let convert_time = convert_start.elapsed();

        let total_time = start.elapsed();
        let cache_size_mb = bytes.len() as f64 / (1024.0 * 1024.0);
        let total_ms = total_time.as_secs_f64() * 1000.0;

        info!(
            "JDK cache HIT - Loaded {} symbols from cache in {:.2}ms (read: {:.2}ms, deserialize: {:.2}ms, convert: {:.2}ms [parallel, {} chunks]) - cache size: {:.2}MB",
            symbol_count,
            total_ms,
            read_time.as_secs_f64() * 1000.0,
            deserialize_time.as_secs_f64() * 1000.0,
            convert_time.as_secs_f64() * 1000.0,
            chunk_count,
            cache_size_mb
        );

        // Celebrate if we hit the <100ms target!
        if total_ms < 100.0 {
            info!("JDK cache load achieved <100ms target! ({:.2}ms)", total_ms);
        } else if total_ms > 200.0 {
            // Warn if still slower than acceptable threshold
            debug!(
                "JDK cache load slower than target: {:.2}ms > 100ms (deserialize: {:.2}ms, cache size: {:.2}MB)",
                total_ms,
                deserialize_time.as_secs_f64() * 1000.0,
                cache_size_mb
            );
        }

        Some(archives)
    }

    /// Save JDK symbols to cache (with chunking for parallel deserialization)
    pub fn save(
        &self,
        symbols: &[Symbol],
        jdk_version: &str,
        java_home: &Path,
    ) -> Result<(), JdkCacheError> {
        self.save_for_manifest(
            symbols,
            jdk_version,
            java_home,
            &JdkArchiveManifest::default(),
        )
    }

    pub(crate) fn save_for_manifest(
        &self,
        symbols: &[Symbol],
        jdk_version: &str,
        java_home: &Path,
        manifest: &JdkArchiveManifest,
    ) -> Result<(), JdkCacheError> {
        let partitions = match manifest.fingerprints() {
            [] => vec![JdkCachedArchiveIndex::new(
                PathBuf::new(),
                symbols.to_vec(),
                Vec::new(),
            )],
            [archive] => vec![JdkCachedArchiveIndex::new(
                archive.relative_path.clone(),
                symbols.to_vec(),
                Vec::new(),
            )],
            archives if symbols.is_empty() => archives
                .iter()
                .map(|archive| {
                    JdkCachedArchiveIndex::new(
                        archive.relative_path.clone(),
                        Vec::new(),
                        Vec::new(),
                    )
                })
                .collect(),
            _ => {
                return Err(JdkCacheError::ArchiveManifest {
                    reason: "symbols from multiple JDK archives must be saved with exact archive ownership"
                        .to_string(),
                });
            }
        };
        self.save_archive_partitions_for_manifest(&partitions, jdk_version, java_home, manifest)
    }

    pub(crate) fn save_archive_partitions_for_manifest(
        &self,
        partitions: &[JdkCachedArchiveIndex],
        jdk_version: &str,
        java_home: &Path,
        manifest: &JdkArchiveManifest,
    ) -> Result<(), JdkCacheError> {
        let start = std::time::Instant::now();

        let expected_paths: Vec<&Path> = if manifest.fingerprints().is_empty() {
            vec![Path::new("")]
        } else {
            manifest
                .fingerprints()
                .iter()
                .map(|archive| archive.relative_path.as_path())
                .collect()
        };
        if partitions.len() != expected_paths.len()
            || partitions
                .iter()
                .zip(&expected_paths)
                .any(|(partition, expected)| {
                    partition.relative_path() != *expected
                        || !is_bounded_cached_relative_path(partition.relative_path())
                })
        {
            return Err(JdkCacheError::ArchiveManifest {
                reason: "symbol archive partitions do not exactly match the JDK manifest"
                    .to_string(),
            });
        }
        let symbol_count = partitions.iter().try_fold(0_usize, |count, partition| {
            count.checked_add(partition.symbols().len())
        });
        let Some(symbol_count) = symbol_count else {
            return Err(JdkCacheError::ArchiveManifest {
                reason: "JDK symbol count overflowed usize".to_string(),
            });
        };

        // Ensure cache directory exists
        std::fs::create_dir_all(&self.cache_dir).map_err(|source| {
            JdkCacheError::CreateDirectory {
                path: self.cache_dir.clone(),
                source,
            }
        })?;

        let _lock = self.lock_exclusive()?;

        let convert_start = std::time::Instant::now();
        let archives: Vec<JdkCachedArchiveData> = partitions
            .iter()
            .map(|partition| {
                let cached_symbols: Vec<CachedSymbol> = partition
                    .symbols()
                    .iter()
                    .map(|symbol| {
                        hyperion_workspace::cache::symbol_to_cached(symbol, symbol.range.start)
                    })
                    .collect();
                let chunks = cached_symbols
                    .chunks(SYMBOLS_PER_CHUNK)
                    .map(<[CachedSymbol]>::to_vec)
                    .collect();
                JdkCachedArchiveData {
                    relative_path: partition.relative_path().to_path_buf(),
                    chunks,
                    generic_supertypes: partition.generic_supertypes().to_vec(),
                }
            })
            .collect();
        let chunk_count = archives.iter().try_fold(0_usize, |count, archive| {
            count.checked_add(archive.chunks.len())
        });
        let Some(chunk_count) = chunk_count else {
            return Err(JdkCacheError::ArchiveManifest {
                reason: "JDK cache chunk count overflowed usize".to_string(),
            });
        };
        let data = JdkCacheData { archives };
        let convert_time = convert_start.elapsed();

        let serialize_start = std::time::Instant::now();
        // Save symbols (atomic write)
        let symbols_path = self.cache_dir.join("symbols.bin");
        let temp_path = symbols_path.with_extension("tmp");

        let bytes = bincode::serialize(&data).map_err(JdkCacheError::SerializeCache)?;
        if bytes.len() > MAX_JDK_CACHE_BYTES {
            return Err(JdkCacheError::CacheTooLarge {
                actual: bytes.len(),
                maximum: MAX_JDK_CACHE_BYTES,
            });
        }
        let serialize_time = serialize_start.elapsed();

        let write_start = std::time::Instant::now();
        std::fs::write(&temp_path, &bytes).map_err(|source| JdkCacheError::WriteCache {
            path: temp_path.clone(),
            source,
        })?;

        std::fs::rename(&temp_path, &symbols_path).map_err(|source| {
            JdkCacheError::PublishCache {
                temporary: temp_path.clone(),
                destination: symbols_path.clone(),
                source,
            }
        })?;
        let write_time = write_start.elapsed();

        // Save metadata
        let metadata = JdkCacheMetadata {
            version: JDK_CACHE_VERSION,
            jdk_version: jdk_version.to_string(),
            java_home: java_home.to_path_buf(),
            symbol_count,
            chunk_count,
            archive_manifest: manifest.fingerprints().to_vec(),
            symbols_sha256: Sha256::digest(&bytes).into(),
        };

        let metadata_path = self.cache_dir.join("metadata.json");
        let metadata_json =
            serde_json::to_string_pretty(&metadata).map_err(JdkCacheError::SerializeMetadata)?;

        std::fs::write(&metadata_path, metadata_json).map_err(|source| {
            JdkCacheError::WriteMetadata {
                path: metadata_path.clone(),
                source,
            }
        })?;

        let total_time = start.elapsed();
        let cache_size_mb = bytes.len() as f64 / (1024.0 * 1024.0);

        info!(
            "JDK cache SAVE - Saved {} symbols to cache in {:.2}ms (convert: {:.2}ms, serialize: {:.2}ms, write: {:.2}ms) - {} chunks, cache size: {:.2}MB at {:?}",
            symbol_count,
            total_time.as_secs_f64() * 1000.0,
            convert_time.as_secs_f64() * 1000.0,
            serialize_time.as_secs_f64() * 1000.0,
            write_time.as_secs_f64() * 1000.0,
            chunk_count,
            cache_size_mb,
            self.cache_dir
        );

        Ok(())
    }

    /// Load metadata from cache
    fn load_metadata(&self) -> Result<JdkCacheMetadata, JdkCacheError> {
        let metadata_path = self.cache_dir.join("metadata.json");
        let bytes =
            read_bounded_file(&metadata_path, MAX_JDK_METADATA_BYTES).map_err(|source| {
                JdkCacheError::ReadMetadata {
                    path: metadata_path.clone(),
                    source,
                }
            })?;
        let json = std::str::from_utf8(&bytes).map_err(|source| JdkCacheError::MetadataUtf8 {
            path: metadata_path.clone(),
            source,
        })?;

        serde_json::from_str(json).map_err(|source| JdkCacheError::ParseMetadata {
            path: metadata_path,
            source,
        })
    }

    fn lock_shared(&self) -> Result<File, JdkCacheError> {
        self.lock_with(|file| file.lock_shared())
    }

    fn lock_exclusive(&self) -> Result<File, JdkCacheError> {
        self.lock_with(|file| file.lock_exclusive())
    }

    fn lock_with<F>(&self, lock_fn: F) -> Result<File, JdkCacheError>
    where
        F: FnOnce(&File) -> io::Result<()>,
    {
        std::fs::create_dir_all(&self.cache_dir).map_err(|source| {
            JdkCacheError::CreateDirectory {
                path: self.cache_dir.clone(),
                source,
            }
        })?;
        let lock_path = self.cache_dir.join(".lock");
        let file = File::create(&lock_path).map_err(|source| JdkCacheError::OpenLock {
            path: lock_path.clone(),
            source,
        })?;
        lock_fn(&file).map_err(|source| JdkCacheError::AcquireLock {
            path: lock_path,
            source,
        })?;
        Ok(file)
    }

    /// Invalidate the cache by removing the cache directory
    pub fn invalidate(&self) -> Result<(), JdkCacheError> {
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir).map_err(|source| JdkCacheError::Remove {
                path: self.cache_dir.clone(),
                source,
            })?;
            info!("JDK cache invalidated");
        }
        Ok(())
    }
}

fn read_bounded_file(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let declared_size = file.metadata()?.len();
    if declared_size > maximum as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is {declared_size} bytes (maximum {maximum})",
                path.display()
            ),
        ));
    }

    let mut bytes = Vec::with_capacity(declared_size as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} grew beyond the {maximum}-byte limit", path.display()),
        ));
    }
    Ok(bytes)
}

fn deserialize_cache_data(bytes: &[u8]) -> bincode::Result<JdkCacheData> {
    use bincode::Options;

    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(MAX_JDK_CACHE_BYTES as u64)
        .deserialize(bytes)
}

/// Get version string for cache key
pub fn version_string(version: &super::jdk_indexer::JavaVersion) -> String {
    use super::jdk_indexer::JavaVersion;
    match version {
        JavaVersion::Java8 => "8".to_string(),
        JavaVersion::Java9Plus(v) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_workspace::{Modifiers, TextRange};
    use lsp_types::SymbolKind;
    use std::sync::Arc;
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn bounded_jdk_cache_reader_rejects_oversized_files() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("symbols.bin");
        std::fs::write(&path, b"12345").unwrap();

        let error = read_bounded_file(&path, 4).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn cache_directory_failure_preserves_operation_and_path() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache-is-a-file");
        std::fs::write(&cache_path, b"not a directory").unwrap();
        let cache = JdkCache {
            cache_dir: cache_path.clone(),
        };

        let error = cache.save(&[], "21", temp_dir.path()).unwrap_err();

        assert!(matches!(
            error,
            JdkCacheError::CreateDirectory { path, source }
                if path == cache_path && source.kind() == io::ErrorKind::AlreadyExists
        ));
    }

    #[test]
    fn malformed_metadata_preserves_parse_error_and_path() {
        let temp_dir = TempDir::new().unwrap();
        let metadata_path = temp_dir.path().join("metadata.json");
        std::fs::write(&metadata_path, b"{not json").unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().to_path_buf(),
        };

        let error = cache.load_metadata().unwrap_err();

        assert!(matches!(
            error,
            JdkCacheError::ParseMetadata { path, source }
                if path == metadata_path && source.is_syntax()
        ));
    }

    fn make_test_symbol(fqn: &str, name: &str) -> Symbol {
        Symbol {
            fqn: fqn.into(),
            simple_name: name.into(),
            range: TextRange::new(0, name.len() as u32),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces: vec![],
            documentation: None,
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None, // Java doesn't have extension functions
            kotlin: None,
            throws: None,
        }
    }

    fn capture_manifest(java_home: &Path, paths: &[PathBuf]) -> JdkArchiveManifest {
        let archives: Vec<_> = paths
            .iter()
            .map(|path| {
                let staged =
                    ExternalSourceArchive::capture_staged(path, MAX_JDK_ARCHIVE_BYTES).unwrap();
                let (archive, snapshot) = staged
                    .into_versioned_snapshot(MAX_JDK_ARCHIVE_BYTES)
                    .unwrap();
                assert_eq!(
                    archive.expected_content(),
                    Some(snapshot.content_identity())
                );
                archive
            })
            .collect();
        JdkArchiveManifest::from_archives(java_home, &archives).unwrap()
    }

    #[test]
    fn test_jdk_cache_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();

        // Override cache dir for test
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };

        let symbols = vec![
            make_test_symbol("java.lang.String", "String"),
            make_test_symbol("java.lang.Object", "Object"),
        ];

        // Save
        cache.save(&symbols, "21", &java_home).unwrap();

        // Validate
        assert!(cache.is_valid(&java_home));

        // Load
        let loaded = cache
            .load_valid_for_manifest(&java_home, &JdkArchiveManifest::default())
            .expect("Should validate and load cache");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].fqn.as_ref(), "java.lang.String");
        assert_eq!(loaded[1].fqn.as_ref(), "java.lang.Object");
    }

    #[test]
    fn cache_round_trip_preserves_exact_archive_symbol_partitions() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let base_path = jmods.join("java.base.jmod");
        let sql_path = jmods.join("java.sql.jmod");
        std::fs::write(&base_path, b"base generation").unwrap();
        std::fs::write(&sql_path, b"sql generation").unwrap();
        let archives: Vec<_> = [&base_path, &sql_path]
            .into_iter()
            .map(|path| {
                ExternalSourceArchive::capture_staged(path, 1024)
                    .unwrap()
                    .into_versioned(1024)
                    .unwrap()
            })
            .collect();
        let staged = JdkArchiveMetadataManifest::capture(&java_home, &archives).unwrap();
        let manifest = staged.with_content(&java_home, &archives).unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        let partitions = vec![
            JdkCachedArchiveIndex::new(
                PathBuf::from("jmods/java.base.jmod"),
                vec![make_test_symbol("split.Duplicate", "Duplicate")],
                vec![TypeGenericSupertypes {
                    type_fqn: "java.util.List".to_string(),
                    edges: vec![hyperion_workspace::GenericSupertype {
                        written_target: "java.util.Collection".to_string(),
                        arguments: vec!["E".to_string()],
                    }],
                }],
            ),
            JdkCachedArchiveIndex::new(
                PathBuf::from("jmods/java.sql.jmod"),
                vec![make_test_symbol("split.Duplicate", "Duplicate")],
                Vec::new(),
            ),
        ];

        cache
            .save_archive_partitions_for_manifest(&partitions, "21", &java_home, &manifest)
            .unwrap();
        let loaded = cache
            .load_valid_for_staged_manifest(&java_home, &staged)
            .expect("unchanged archive generation must load");

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].relative_path(), Path::new("jmods/java.base.jmod"));
        assert_eq!(loaded[1].relative_path(), Path::new("jmods/java.sql.jmod"));
        assert_eq!(loaded[0].symbols()[0].fqn.as_ref(), "split.Duplicate");
        assert_eq!(loaded[1].symbols()[0].fqn.as_ref(), "split.Duplicate");
        assert_eq!(
            loaded[0].generic_supertypes(),
            &[TypeGenericSupertypes {
                type_fqn: "java.util.List".to_string(),
                edges: vec![hyperion_workspace::GenericSupertype {
                    written_target: "java.util.Collection".to_string(),
                    arguments: vec!["E".to_string()],
                }],
            }]
        );
    }

    #[test]
    fn cache_save_rejects_archive_partitions_in_the_wrong_manifest_order() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let base_path = jmods.join("java.base.jmod");
        let sql_path = jmods.join("java.sql.jmod");
        std::fs::write(&base_path, b"base generation").unwrap();
        std::fs::write(&sql_path, b"sql generation").unwrap();
        let archives: Vec<_> = [&base_path, &sql_path]
            .into_iter()
            .map(|path| {
                ExternalSourceArchive::capture_staged(path, 1024)
                    .unwrap()
                    .into_versioned(1024)
                    .unwrap()
            })
            .collect();
        let manifest = JdkArchiveManifest::from_archives(&java_home, &archives).unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        let reversed = vec![
            JdkCachedArchiveIndex::new(
                PathBuf::from("jmods/java.sql.jmod"),
                Vec::new(),
                Vec::new(),
            ),
            JdkCachedArchiveIndex::new(
                PathBuf::from("jmods/java.base.jmod"),
                Vec::new(),
                Vec::new(),
            ),
        ];

        let error = cache
            .save_archive_partitions_for_manifest(&reversed, "21", &java_home, &manifest)
            .unwrap_err();

        assert!(matches!(error, JdkCacheError::ArchiveManifest { .. }));
    }

    #[test]
    fn archive_manifest_invalidates_cache_when_installed_jmods_change() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let first = jmods.join("java.base.jmod");
        std::fs::write(&first, b"first").unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        let first_manifest = capture_manifest(&java_home, std::slice::from_ref(&first));

        cache
            .save_for_manifest(&[], "21", &java_home, &first_manifest)
            .unwrap();
        assert!(cache.is_valid_for_manifest(&java_home, &first_manifest));

        let second = jmods.join("java.compiler.jmod");
        std::fs::write(&second, b"second").unwrap();
        let changed_manifest = capture_manifest(&java_home, &[first, second]);
        assert!(!cache.is_valid_for_manifest(&java_home, &changed_manifest));
    }

    #[test]
    fn archive_manifest_rejects_changed_content_even_when_length_is_unchanged() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let archive = jmods.join("java.base.jmod");
        std::fs::write(&archive, b"first").unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        let symbols = vec![make_test_symbol("java.lang.Object", "Object")];
        let first_manifest = capture_manifest(&java_home, std::slice::from_ref(&archive));

        cache
            .save_for_manifest(&symbols, "21", &java_home, &first_manifest)
            .unwrap();
        std::fs::write(&archive, b"other").unwrap();
        let changed_manifest = capture_manifest(&java_home, std::slice::from_ref(&archive));

        assert_eq!(std::fs::metadata(&archive).unwrap().len(), 5);
        assert!(!cache.is_valid_for_manifest(&java_home, &changed_manifest));
    }

    #[test]
    fn warm_cache_validation_cannot_cross_a_same_path_generation_change() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let path = jmods.join("java.base.jmod");
        std::fs::write(&path, b"generation-one").unwrap();

        let staged_archive =
            ExternalSourceArchive::capture_staged(&path, MAX_JDK_ARCHIVE_BYTES).unwrap();
        let staged =
            JdkArchiveMetadataManifest::capture(&java_home, std::slice::from_ref(&staged_archive))
                .unwrap();
        let (versioned_archive, snapshot) = staged_archive
            .into_versioned_snapshot(MAX_JDK_ARCHIVE_BYTES)
            .unwrap();
        assert_eq!(
            versioned_archive.expected_content(),
            Some(snapshot.content_identity())
        );
        assert!(staged.still_matches(&java_home, std::slice::from_ref(&versioned_archive)));
        let parsed_manifest = staged
            .with_content(&java_home, std::slice::from_ref(&versioned_archive))
            .unwrap();
        drop(snapshot);

        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        cache
            .save_for_manifest(
                &[make_test_symbol("jdk.GenerationOne", "GenerationOne")],
                "21",
                &java_home,
                &parsed_manifest,
            )
            .unwrap();

        let warm_archive =
            ExternalSourceArchive::capture_staged(&path, MAX_JDK_ARCHIVE_BYTES).unwrap();
        let warm_stage =
            JdkArchiveMetadataManifest::capture(&java_home, std::slice::from_ref(&warm_archive))
                .unwrap();
        let loaded = cache
            .load_valid_for_staged_manifest(&java_home, &warm_stage)
            .expect("the unchanged staged generation is a warm hit");

        // This models replacement after validation but before WorkspaceIndex
        // publication. The indexer rechecks this staged set while holding the
        // owner gate, so these symbols can no longer become visible.
        std::fs::write(&path, b"generation-two").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 14);
        assert!(!warm_stage.still_matches(&java_home, std::slice::from_ref(&warm_archive)));
        assert_eq!(loaded[0].symbols()[0].fqn.as_ref(), "jdk.GenerationOne");

        let next_archive =
            ExternalSourceArchive::capture_staged(&path, MAX_JDK_ARCHIVE_BYTES).unwrap();
        let next_stage =
            JdkArchiveMetadataManifest::capture(&java_home, std::slice::from_ref(&next_archive))
                .unwrap();
        assert!(cache
            .load_valid_for_staged_manifest(&java_home, &next_stage)
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn warm_metadata_revalidation_rejects_a_replacement_leaf_symlink() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let archive_path = jmods.join("java.base.jmod");
        let replacement = jmods.join("replacement.jmod");
        std::fs::write(&archive_path, b"generation-one").unwrap();
        std::fs::write(&replacement, b"generation-two").unwrap();

        let archive =
            ExternalSourceArchive::capture_staged(&archive_path, MAX_JDK_ARCHIVE_BYTES).unwrap();
        let staged =
            JdkArchiveMetadataManifest::capture(&java_home, std::slice::from_ref(&archive))
                .unwrap();
        std::fs::remove_file(&archive_path).unwrap();
        symlink(&replacement, &archive_path).unwrap();

        assert!(!staged.still_matches(&java_home, &[archive]));
    }

    #[test]
    fn archive_manifest_fails_warm_validation_after_a_timestamp_touch() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        let jmods = java_home.join("jmods");
        std::fs::create_dir_all(&jmods).unwrap();
        let archive = jmods.join("java.base.jmod");
        std::fs::write(&archive, b"base").unwrap();
        let cached_mtime = std::fs::metadata(&archive).unwrap().modified().unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        let first_manifest = capture_manifest(&java_home, std::slice::from_ref(&archive));
        cache
            .save_for_manifest(
                &[make_test_symbol("java.lang.Object", "Object")],
                "21",
                &java_home,
                &first_manifest,
            )
            .unwrap();

        File::options()
            .write(true)
            .open(&archive)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(cached_mtime + std::time::Duration::from_secs(2)),
            )
            .unwrap();

        let touched_manifest = capture_manifest(&java_home, std::slice::from_ref(&archive));
        assert_ne!(first_manifest, touched_manifest);
        assert!(!cache.is_valid_for_manifest(&java_home, &touched_manifest));
    }

    #[test]
    fn archive_manifest_rejects_missing_duplicate_external_and_oversized_inputs() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();
        let archive = java_home.join("java.base.jmod");
        std::fs::write(&archive, b"base").unwrap();

        let captured = ExternalSourceArchive::capture_staged(&archive, 1024)
            .unwrap()
            .into_versioned(1024)
            .unwrap();
        let too_many = vec![captured.clone(); MAX_JDK_ARCHIVES + 1];
        assert!(matches!(
            JdkArchiveManifest::from_archives(&java_home, &too_many),
            Err(JdkCacheError::ArchiveManifest { .. })
        ));
        assert!(matches!(
            JdkArchiveManifest::from_archives(&java_home, &[captured.clone(), captured]),
            Err(JdkCacheError::ArchiveManifest { .. })
        ));
        let outside_path = temp_dir.path().join("outside.jmod");
        std::fs::write(&outside_path, b"outside").unwrap();
        let outside = ExternalSourceArchive::capture_staged(&outside_path, 1024)
            .unwrap()
            .into_versioned(1024)
            .unwrap();
        assert!(matches!(
            JdkArchiveManifest::from_archives(&java_home, &[outside]),
            Err(JdkCacheError::ArchiveManifest { .. })
        ));
        assert!(
            ExternalSourceArchive::capture_staged(&java_home.join("missing.jmod"), 1024).is_err()
        );

        File::create(&archive)
            .unwrap()
            .set_len(MAX_JDK_ARCHIVE_BYTES + 1)
            .unwrap();
        assert!(ExternalSourceArchive::capture_staged(&archive, MAX_JDK_ARCHIVE_BYTES).is_err());
    }

    #[test]
    fn cache_load_rejects_a_valid_bincode_payload_not_named_by_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();
        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };
        cache
            .save(
                &[make_test_symbol("java.lang.Object", "Object")],
                "21",
                &java_home,
            )
            .unwrap();

        let unrelated_payload = bincode::serialize(&JdkCacheData {
            archives: Vec::new(),
        })
        .unwrap();
        std::fs::write(cache.cache_dir.join("symbols.bin"), unrelated_payload).unwrap();

        assert!(cache.load().is_none());
        assert!(cache
            .load_valid_for_manifest(&java_home, &JdkArchiveManifest::default())
            .is_none());
    }

    #[test]
    fn test_jdk_cache_concurrent_load_save() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();

        let cache = Arc::new(JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        });

        let symbols = vec![
            make_test_symbol("java.lang.String", "String"),
            make_test_symbol("java.lang.Object", "Object"),
        ];

        cache.save(&symbols, "21", &java_home).unwrap();

        let writer_cache = Arc::clone(&cache);
        let writer_home = java_home.clone();
        let writer_symbols = symbols.clone();
        let writer = thread::spawn(move || {
            for _ in 0..20 {
                writer_cache
                    .save(&writer_symbols, "21", &writer_home)
                    .unwrap();
            }
        });

        let mut readers = Vec::new();
        for _ in 0..4 {
            let reader_cache = Arc::clone(&cache);
            readers.push(thread::spawn(move || {
                for _ in 0..50 {
                    let loaded = reader_cache.load().expect("Expected cache load");
                    assert_eq!(loaded.len(), 2);
                }
            }));
        }

        writer.join().unwrap();
        for reader in readers {
            reader.join().unwrap();
        }
    }

    #[test]
    fn test_jdk_cache_invalid_java_home() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();

        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };

        let symbols = vec![make_test_symbol("java.lang.String", "String")];
        cache.save(&symbols, "21", &java_home).unwrap();

        // Different java_home should invalidate cache
        let different_home = temp_dir.path().join("other-jdk");
        assert!(!cache.is_valid(&different_home));
    }

    #[test]
    fn test_jdk_cache_chunking() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();

        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };

        // Create a large set of symbols to test chunking
        // SYMBOLS_PER_CHUNK = 1000, so create 2500 symbols = 3 chunks
        let symbols: Vec<Symbol> = (0..2500)
            .map(|i| make_test_symbol(&format!("java.test.Class{}", i), &format!("Class{}", i)))
            .collect();

        // Save
        cache.save(&symbols, "21", &java_home).unwrap();

        // Load metadata to verify chunks were created
        let metadata = cache.load_metadata().unwrap();
        assert_eq!(
            metadata.chunk_count, 3,
            "Should have 3 chunks for 2500 symbols"
        );
        assert_eq!(metadata.symbol_count, 2500);

        // Load and verify all symbols are present
        let loaded = cache.load().expect("Should load cache");
        assert_eq!(loaded.len(), 2500, "Should load all 2500 symbols");

        // Verify symbols are correct (spot check first and last)
        assert_eq!(loaded[0].fqn.as_ref(), "java.test.Class0");
        assert_eq!(loaded[2499].fqn.as_ref(), "java.test.Class2499");
    }

    #[test]
    fn test_jdk_cache_performance_large() {
        let temp_dir = TempDir::new().unwrap();
        let java_home = temp_dir.path().join("jdk");
        std::fs::create_dir_all(&java_home).unwrap();

        let cache = JdkCache {
            cache_dir: temp_dir.path().join("cache"),
        };

        // Simulate realistic JDK symbol count (~50K symbols)
        let symbols: Vec<Symbol> = (0..50_000)
            .map(|i| make_test_symbol(&format!("java.lang.Class{}", i), &format!("Class{}", i)))
            .collect();

        // Save
        let save_start = std::time::Instant::now();
        cache.save(&symbols, "21", &java_home).unwrap();
        let save_time = save_start.elapsed();
        println!("Save time: {:.2}ms", save_time.as_secs_f64() * 1000.0);

        // Load and measure performance
        let load_start = std::time::Instant::now();
        let loaded = cache.load().expect("Should load cache");
        let load_time = load_start.elapsed();

        let load_time_ms = load_time.as_secs_f64() * 1000.0;
        println!("Load time: {:.2}ms", load_time_ms);

        assert_eq!(loaded.len(), 50_000);

        // Performance target: <100ms (ideal), <200ms (acceptable)
        if load_time_ms < 100.0 {
            println!("✓ EXCELLENT: Cache load achieved <100ms target!");
        } else if load_time_ms < 200.0 {
            println!("✓ GOOD: Cache load under 200ms");
        } else {
            println!(
                "⚠ WARNING: Cache load time {:.2}ms exceeds 200ms threshold",
                load_time_ms
            );
        }
    }
}
