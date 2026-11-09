// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Persistent Index Cache
//!
//! Saves workspace index to disk for faster cold starts.
//! Cache entries are restored only when their source bytes still match.
//!
//! Cache files are stored in the system temp directory with a hash of the
//! workspace path to avoid cluttering the workspace.

use crate::{KotlinSymbolMetadata, Symbol};
use lsp_types::SymbolKind;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};
use std::time::SystemTime;
use tracing::{debug, info, warn};

/// Current cache format version - bump when format changes
const CACHE_VERSION: u32 = 40; // Kotlin: source type arguments keep qualified names and variance

/// Magic header to identify Hyperion cache files
/// "HYPE" in ASCII = [0x48, 0x59, 0x50, 0x45]
const CACHE_MAGIC: &[u8; 4] = b"HYPE";

/// Encoding byte following the magic header.
const CACHE_ENCODING_LZ4_FRAME: u8 = 1;

/// Cache data is disposable. Refuse implausibly large or corrupt files rather
/// than letting startup consume unbounded memory.
const MAX_CACHE_BYTES: usize = 512 * 1024 * 1024;

/// Result type for cache operations
pub type Result<T> = std::result::Result<T, CacheError>;

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Cache version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u32, actual: u32 },

    #[error("Cache file not found")]
    NotFound,

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Invalid cache file: missing or incorrect magic header")]
    InvalidMagic,

    #[error("Unsupported cache payload encoding: {0}")]
    UnsupportedEncoding(u8),

    #[error("Cache data is too large: {size} bytes (maximum {maximum})")]
    TooLarge { size: u64, maximum: usize },
}

/// Cached index data
#[derive(Serialize, Deserialize)]
pub struct CachedIndex {
    /// Cache format version
    version: u32,

    /// Cached file entries
    entries: HashMap<PathBuf, CachedFile>,

    /// Persisted user-interaction counts (FQN -> usage count). Source-reference
    /// counts are derived from current source bytes and are never persisted.
    #[serde(default)]
    usage_stats: HashMap<String, u32>,
}

/// Cached data for a single file
#[derive(Serialize, Deserialize, Clone)]
pub struct CachedFile {
    /// File modification time
    pub mtime: SystemTime,

    /// Hash of the exact source revision from which `symbols` were extracted.
    /// Modification times are only a fast prefilter; they are not a sufficient
    /// revision identity on their own.
    pub content_hash: u64,

    /// Symbols in this file
    pub symbols: Vec<CachedSymbol>,

    /// Explicit generic arguments on nominal supertype edges.
    #[serde(default)]
    pub generic_supertypes: Vec<crate::TypeGenericSupertypes>,

    /// Complete import context for this file, including package, wildcard, and
    /// static imports. Resolution after a warm restore must match fresh indexing.
    pub imports: crate::ImportMap,
}

/// Cached symbol data
#[derive(Serialize, Deserialize, Clone)]
pub struct CachedSymbol {
    /// Fully qualified name
    pub fqn: String,

    /// Simple name
    pub name: String,

    /// Symbol kind name (e.g., "class", "method", "field")
    pub kind: String,

    /// Byte offset in file
    pub offset: u32,

    /// Exclusive end byte offset in file.
    pub end_offset: u32,

    /// Container FQN (for methods/fields)
    pub container: Option<String>,

    /// Return type for methods
    pub return_type: Option<String>,

    /// Parameter types for methods
    pub parameters: Vec<String>,

    /// Written source parent type or exact external bytecode FQN.
    pub parent_class: Option<String>,

    /// Canonical declaration modifiers retained without language-adapter loss.
    pub modifiers: crate::Modifiers,

    /// JavaDoc or external documentation attached to the declaration.
    #[serde(default)]
    pub documentation: Option<String>,

    /// Whether the declaration is deprecated.
    #[serde(default)]
    pub is_deprecated: bool,

    /// Written source interface types or exact external bytecode FQNs.
    #[serde(default)]
    pub interfaces: Vec<String>,

    /// Receiver type for extension functions (Kotlin)
    /// e.g., "String" for `fun String.hello() = ...`
    #[serde(default)]
    pub receiver_type: Option<String>,

    /// Generic type parameters declared by this symbol.
    #[serde(default)]
    pub type_parameters: Vec<crate::TypeParameterInfo>,

    /// Kotlin declaration facts recovered from bytecode metadata.
    #[serde(default)]
    pub kotlin: Option<Box<KotlinSymbolMetadata>>,

    /// Qualified exception types of a method's or constructor's `throws` clause.
    #[serde(default)]
    pub throws: Vec<String>,
}

/// Index cache manager
pub struct IndexCache {
    cache_path: PathBuf,
    /// Serialize saves issued by every `IndexCache` handle for this path.
    coordinator: Arc<SaveCoordinator>,
}

#[derive(Default)]
struct SaveState {
    next_ticket: u64,
    serving_ticket: u64,
    pending: usize,
}

#[derive(Default)]
struct SaveCoordinator {
    state: parking_lot::Mutex<SaveState>,
    changed: parking_lot::Condvar,
}

impl SaveCoordinator {
    fn reserve(&self) -> u64 {
        let mut state = self.state.lock();
        let ticket = state.next_ticket;
        state.next_ticket = state
            .next_ticket
            .checked_add(1)
            .expect("cache save ticket space exhausted");
        state.pending = state
            .pending
            .checked_add(1)
            .expect("pending cache save count overflowed");
        ticket
    }

    fn turn(&self, ticket: u64) -> SaveTurn<'_> {
        let mut state = self.state.lock();
        while state.serving_ticket != ticket {
            self.changed.wait(&mut state);
        }
        drop(state);
        SaveTurn {
            coordinator: self,
            ticket,
        }
    }

    fn finish(&self, ticket: u64) {
        let mut state = self.state.lock();
        debug_assert_eq!(state.serving_ticket, ticket);
        state.serving_ticket = state
            .serving_ticket
            .checked_add(1)
            .expect("cache save ticket space exhausted");
        state.pending = state
            .pending
            .checked_sub(1)
            .expect("cache save completed without a reservation");
        self.changed.notify_all();
    }
}

struct SaveTurn<'a> {
    coordinator: &'a SaveCoordinator,
    ticket: u64,
}

impl Drop for SaveTurn<'_> {
    fn drop(&mut self) {
        self.coordinator.finish(self.ticket);
    }
}

struct TemporaryCacheFile {
    path: Option<PathBuf>,
}

impl TemporaryCacheFile {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn persisted(mut self) {
        self.path = None;
    }
}

impl Drop for TemporaryCacheFile {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn save_coordinator(cache_path: &Path) -> Arc<SaveCoordinator> {
    static COORDINATORS: OnceLock<parking_lot::Mutex<HashMap<PathBuf, Weak<SaveCoordinator>>>> =
        OnceLock::new();

    let coordinators = COORDINATORS.get_or_init(Default::default);
    let mut coordinators = coordinators.lock();
    if let Some(coordinator) = coordinators.get(cache_path).and_then(Weak::upgrade) {
        return coordinator;
    }

    // Weak entries let one-shot workspaces disappear from the registry while
    // an in-flight background save remains discoverable by a later handle.
    coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
    let coordinator = Arc::new(SaveCoordinator::default());
    coordinators.insert(cache_path.to_path_buf(), Arc::downgrade(&coordinator));
    coordinator
}

impl IndexCache {
    /// Create a new cache for the given workspace root
    ///
    /// Cache file is stored in system temp directory as:
    /// `/tmp/{sha256-hash}-hyperion.bin`
    ///
    /// This avoids cluttering the workspace with cache files.
    pub fn new(workspace_root: &Path) -> Self {
        let hash = Self::hash_path(workspace_root);
        let cache_name = format!("{}-hyperion.bin", hash);
        let cache_path = std::env::temp_dir().join(cache_name);

        debug!("Cache path for {:?}: {:?}", workspace_root, cache_path);

        let coordinator = save_coordinator(&cache_path);
        Self {
            cache_path,
            coordinator,
        }
    }

    /// Compute SHA256 hash of a path (first 16 hex chars)
    fn hash_path(path: &Path) -> String {
        let mut hasher = Sha256::new();
        let identity = crate::path_lookup_key(path);
        hasher.update(identity.as_os_str().as_encoded_bytes());
        let result = hasher.finalize();
        // Use first 8 bytes (16 hex chars) for a shorter filename
        hex::encode(&result[..8])
    }

    /// Load cached index from disk
    ///
    /// Returns None if cache doesn't exist or is invalid
    pub fn load(&self) -> Option<CachedIndex> {
        let started = std::time::Instant::now();
        let file = match open_bounded_file(&self.cache_path, MAX_CACHE_BYTES) {
            Ok(file) => file,
            Err(CacheError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                debug!("Cache file not found: {:?}", self.cache_path);
                return None;
            }
            Err(e) => {
                warn!("Failed to read cache file: {}", e);
                return None;
            }
        };

        match self.deserialize_from(file) {
            Ok(cached) => {
                if cached.version != CACHE_VERSION {
                    info!(
                        "Cache version mismatch (expected {}, got {}), will rebuild",
                        CACHE_VERSION, cached.version
                    );
                    return None;
                }
                info!("Loaded cache with {} file entries", cached.entries.len());
                info!(
                    "Cache load completed in {:.2}s (read + decompress + deserialize)",
                    started.elapsed().as_secs_f64()
                );
                Some(cached)
            }
            Err(e) => {
                // M21 FIX: Provide actionable error messages for cache corruption
                use CacheError::*;
                match &e {
                    InvalidMagic => {
                        warn!(
                            "Cache file is corrupted (invalid magic header). \
                             This usually means the cache file was truncated or overwritten. \
                             Cache will be rebuilt. File: {:?}",
                            self.cache_path
                        );
                    }
                    Decompression(msg) => {
                        warn!(
                            "Cache file decompression failed: {}. \
                             This may indicate disk corruption or incomplete write. \
                             Cache will be rebuilt. File: {:?}",
                            msg, self.cache_path
                        );
                    }
                    Serialization(msg) => {
                        warn!(
                            "Cache file deserialization failed: {}. \
                             This usually means the cache format changed or file is corrupted. \
                             Cache will be rebuilt. File: {:?}",
                            msg, self.cache_path
                        );
                    }
                    _ => {
                        warn!(
                            "Failed to load cache: {}. \
                             Cache will be rebuilt. File: {:?}",
                            e, self.cache_path
                        );
                    }
                }
                None
            }
        }
    }

    /// Save index to cache file
    ///
    /// Uses atomic write (temp file + rename) to prevent corruption
    pub fn save(
        &self,
        entries: HashMap<PathBuf, CachedFile>,
        usage_stats: HashMap<String, u32>,
    ) -> Result<()> {
        let ticket = self.coordinator.reserve();
        let _turn = self.coordinator.turn(ticket);
        self.save_reserved(entries, usage_stats)
    }

    /// Serialize a live index in cache-ticket order. Capturing after taking
    /// the turn keeps a shutdown snapshot newer than every earlier background
    /// save, even when an earlier worker had not started extracting yet.
    pub fn save_index(&self, index: &crate::WorkspaceIndex) -> Result<()> {
        let ticket = self.coordinator.reserve();
        let _turn = self.coordinator.turn(ticket);
        self.save_reserved(index.extract_cache_data(), index.extract_usage_stats())
    }

    fn save_reserved(
        &self,
        entries: HashMap<PathBuf, CachedFile>,
        usage_stats: HashMap<String, u32>,
    ) -> Result<()> {
        let cached = CachedIndex {
            version: CACHE_VERSION,
            entries,
            usage_stats,
        };

        // Atomic write: temp file + rename
        static TEMP_FILE_SEQUENCE: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_path = self
            .cache_path
            .with_extension(format!("tmp-{}-{sequence}", std::process::id()));
        let temp_guard = TemporaryCacheFile::new(temp_path.clone());

        let serialize_started = std::time::Instant::now();
        let file = std::fs::File::create(&temp_path)?;
        let file = self.serialize_into(file, &cached)?;
        file.sync_all()?;
        let cache_bytes = file.metadata()?.len();
        drop(file);

        std::fs::rename(&temp_path, &self.cache_path)?;
        temp_guard.persisted();

        info!(
            "Saved cache with {} entries ({} bytes) to {:?} in {:?}",
            cached.entries.len(),
            cache_bytes,
            self.cache_path,
            serialize_started.elapsed(),
        );
        Ok(())
    }

    /// Save index to cache file in a background thread
    ///
    /// This method clones the data and spawns a background thread to perform the save,
    /// returning immediately without blocking. This is the recommended method for saving
    /// cache during normal LSP operation, as it has zero perceived overhead.
    ///
    /// For graceful shutdown, call `wait_for_pending_saves()` to ensure all background
    /// saves complete before exiting.
    ///
    /// # Performance
    ///
    /// - Cloning HashMaps: ~1-2ms (cheap with small entry counts)
    /// - Serialization + compression + disk I/O: 10-50ms (runs in background)
    /// - Perceived overhead: ~1-2ms (just the clone)
    ///
    /// # Errors
    ///
    /// Errors are logged but not returned, since the save happens asynchronously.
    /// Failed saves do not affect LSP operation.
    pub fn save_async(
        &self,
        entries: HashMap<PathBuf, CachedFile>,
        usage_stats: HashMap<String, u32>,
    ) {
        self.spawn_save(move || (entries, usage_stats));
    }

    /// Reserve a cache-save ticket before another startup phase can publish
    /// more sources, then capture the index on the cache worker. A later
    /// shutdown save receives a later ticket and cannot be overwritten by an
    /// older snapshot whose worker started late.
    pub fn save_index_async(&self, index: Arc<crate::WorkspaceIndex>) {
        self.spawn_save(move || (index.extract_cache_data(), index.extract_usage_stats()));
    }

    fn spawn_save(
        &self,
        prepare: impl FnOnce() -> (HashMap<PathBuf, CachedFile>, HashMap<String, u32>) + Send + 'static,
    ) {
        let cache_path = self.cache_path.clone();
        let coordinator = Arc::clone(&self.coordinator);
        let worker_coordinator = Arc::clone(&coordinator);
        let ticket = coordinator.reserve();
        let pending_prepare = Arc::new(parking_lot::Mutex::new(Some(prepare)));
        let worker_prepare = Arc::clone(&pending_prepare);

        // Spawn background thread to perform save
        let worker = std::thread::Builder::new()
            .name("hyperion-cache-save".to_string())
            .spawn(move || {
                use std::time::Instant;
                let start = Instant::now();

                // Create temporary IndexCache just for serialization
                // (We can't move `self` into the thread, so we reconstruct the minimal needed state)
                let temp_cache = IndexCache {
                    cache_path: cache_path.clone(),
                    coordinator: Arc::clone(&worker_coordinator),
                };

                let result = {
                    let _turn = worker_coordinator.turn(ticket);
                    let prepare = worker_prepare
                        .lock()
                        .take()
                        .expect("cache save input exists");
                    let (entries, usage_stats) = prepare();
                    temp_cache.save_reserved(entries, usage_stats)
                };
                let elapsed = start.elapsed();

                match result {
                    Ok(()) => {
                        info!(
                            "Background cache save completed in {:?} (cache: {:?})",
                            elapsed, cache_path
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Background cache save failed after {:?}: {} (cache: {:?})",
                            elapsed, e, cache_path
                        );
                    }
                }
            });

        match worker {
            Ok(_) => debug!("Spawned background cache save task"),
            Err(error) => {
                warn!("Failed to spawn background cache save: {error}; saving inline");
                let _turn = coordinator.turn(ticket);
                let prepare = pending_prepare
                    .lock()
                    .take()
                    .expect("cache save input exists");
                let (entries, usage_stats) = prepare();
                if let Err(error) = self.save_reserved(entries, usage_stats) {
                    warn!("Inline cache save failed: {error}");
                }
            }
        }
    }

    /// Wait for all pending background saves to complete
    ///
    /// This method blocks until all background save tasks spawned via `save_async()`
    /// have completed. Should be called during LSP shutdown to ensure cache is
    /// persisted before exit.
    ///
    /// # Timeout
    ///
    /// Waits up to 5 seconds for pending saves. If saves don't complete within
    /// this timeout, logs a warning and returns.
    pub fn wait_for_pending_saves(&self) {
        use std::time::{Duration, Instant};

        let mut state = self.coordinator.state.lock();
        let pending = state.pending;
        if pending == 0 {
            return;
        }

        info!(
            "Waiting for {} pending cache save(s) to complete...",
            pending
        );

        let start = Instant::now();
        let timeout = Duration::from_secs(5);

        loop {
            let pending = state.pending;
            if pending == 0 {
                info!("All pending cache saves completed in {:?}", start.elapsed());
                return;
            }

            if start.elapsed() > timeout {
                warn!(
                    "Timeout waiting for {} pending cache save(s) after {:?}",
                    pending,
                    start.elapsed()
                );
                return;
            }

            let remaining = timeout.saturating_sub(start.elapsed());
            self.coordinator.changed.wait_for(&mut state, remaining);
        }
    }

    /// Check which files have changed since cache was created (parallel version)
    ///
    /// Uses rayon to parallelize metadata checks across multiple threads.
    /// This is critical for warm starts where we have 10K+ cached entries.
    ///
    /// Each `std::fs::metadata()` call is a blocking syscall (~50-100μs on SSD).
    /// For 10K files:
    /// - Sequential: 10,000 × 100μs = 1000ms
    /// - Parallel (4 threads): 2,500 × 100μs per thread = 250ms total
    /// - Expected speedup: **4x on quad-core**
    ///
    /// ## TOCTOU Safety
    ///
    /// Uses a single `metadata()` syscall instead of `exists() + metadata()` to
    /// avoid races. Missing or unreadable files are stale and must never have
    /// their cached symbols restored.
    pub fn check_stale_files(&self, cached: &CachedIndex) -> Vec<PathBuf> {
        use std::time::Instant;

        let start = Instant::now();
        let total_entries = cached.entries.len();

        // Convert HashMap to Vec for parallel iteration
        // This is a one-time O(n) cost but enables O(n/cores) parallelism
        let entries: Vec<_> = cached.entries.iter().collect();

        let stale: Vec<PathBuf> = entries
            .par_iter() // Parallel iterator - work-stealing across thread pool
            .filter_map(|(path, entry)| {
                // This closure runs concurrently across rayon's thread pool

                // Single syscall - atomic check for existence AND mtime
                // This prevents TOCTOU race where file is deleted between exists() and metadata()
                match std::fs::metadata(path) {
                    Ok(meta) => {
                        match meta.modified() {
                            Ok(mtime) => {
                                // Only mark stale if file exists AND mtime differs
                                if mtime != entry.mtime {
                                    Some(path.to_path_buf())
                                } else {
                                    None // Fresh file
                                }
                            }
                            Err(_) => {
                                // Can't get mtime → treat as stale to re-validate
                                Some(path.to_path_buf())
                            }
                        }
                    }
                    Err(_) => Some(path.to_path_buf()),
                }
            })
            .collect(); // Rayon's collect() runs in parallel

        let elapsed = start.elapsed();
        info!(
            "Cache validation: {} stale files out of {} in {:.2}s ({:.0} files/sec)",
            stale.len(),
            total_entries,
            elapsed.as_secs_f64(),
            total_entries as f64 / elapsed.as_secs_f64()
        );

        stale
    }

    /// Delete the cache file
    pub fn invalidate(&self) -> Result<()> {
        if self.cache_path.exists() {
            std::fs::remove_file(&self.cache_path)?;
            info!("Cache invalidated");
        }
        Ok(())
    }

    /// Get the cache file path (for testing/debugging)
    pub fn path(&self) -> &Path {
        &self.cache_path
    }

    fn serialize_into<W: Write>(&self, mut writer: W, cached: &CachedIndex) -> Result<W> {
        use bincode::Options;

        writer.write_all(CACHE_MAGIC)?;
        writer.write_all(&[CACHE_ENCODING_LZ4_FRAME])?;

        let mut encoder = lz4_flex::frame::FrameEncoder::new(writer);
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_CACHE_BYTES as u64)
            .serialize_into(&mut encoder, cached)
            .map_err(|error| CacheError::Serialization(error.to_string()))?;
        encoder
            .finish()
            .map_err(|error| CacheError::Compression(error.to_string()))
    }

    #[cfg(test)]
    fn serialize(&self, cached: &CachedIndex) -> Result<Vec<u8>> {
        self.serialize_into(Vec::new(), cached)
    }

    #[cfg(test)]
    fn deserialize(&self, bytes: &[u8]) -> Result<CachedIndex> {
        self.deserialize_from(bytes)
    }

    fn deserialize_from<R: Read>(&self, mut reader: R) -> Result<CachedIndex> {
        use std::time::Instant;

        let mut header = [0_u8; CACHE_MAGIC.len() + 1];
        if let Err(error) = reader.read_exact(&mut header) {
            return if error.kind() == std::io::ErrorKind::UnexpectedEof {
                Err(CacheError::InvalidMagic)
            } else {
                Err(CacheError::Io(error))
            };
        }
        if &header[..CACHE_MAGIC.len()] != CACHE_MAGIC {
            return Err(CacheError::InvalidMagic);
        }
        let encoding = header[CACHE_MAGIC.len()];
        if encoding != CACHE_ENCODING_LZ4_FRAME {
            return Err(CacheError::UnsupportedEncoding(encoding));
        }

        let deserialize_start = Instant::now();
        let mut decoder = lz4_flex::frame::FrameDecoder::new(reader);
        let cached = deserialize_cached_index_from(&mut decoder, MAX_CACHE_BYTES as u64)
            .map_err(|error| CacheError::Serialization(error.to_string()))?;

        // Force the decoder through the frame terminator/checksum and reject
        // hidden payload after the one expected CachedIndex value.
        let mut trailing = [0_u8; 1];
        match decoder.read(&mut trailing) {
            Ok(0) => {}
            Ok(_) => {
                return Err(CacheError::Serialization(
                    "trailing data after cache payload".to_string(),
                ));
            }
            Err(error) => return Err(CacheError::Decompression(error.to_string())),
        }
        let mut reader = decoder.into_inner();
        match reader.read(&mut trailing) {
            Ok(0) => {}
            Ok(_) => {
                return Err(CacheError::Serialization(
                    "trailing data after cache frame".to_string(),
                ));
            }
            Err(error) => return Err(CacheError::Io(error)),
        }
        let deserialize_time = deserialize_start.elapsed();

        debug!(
            "Cache deserialized {} entries in {:?}",
            cached.entries.len(),
            deserialize_time
        );

        Ok(cached)
    }
}

fn open_bounded_file(path: &Path, maximum: usize) -> Result<std::fs::File> {
    let file = std::fs::File::open(path)?;
    let declared_size = file.metadata()?.len();
    if declared_size > maximum as u64 {
        return Err(CacheError::TooLarge {
            size: declared_size,
            maximum,
        });
    }
    Ok(file)
}

#[cfg(test)]
fn read_bounded_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = open_bounded_file(path, maximum)?;
    let mut bytes = Vec::with_capacity(file.metadata()?.len() as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(CacheError::TooLarge {
            size: bytes.len() as u64,
            maximum,
        });
    }
    Ok(bytes)
}

fn deserialize_cached_index_from<R: Read>(reader: R, maximum: u64) -> bincode::Result<CachedIndex> {
    use bincode::Options;

    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(maximum)
        .deserialize_from(reader)
}

/// Convert SymbolKind to string for serialization
fn symbol_kind_to_string(kind: SymbolKind) -> String {
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "module",
        SymbolKind::NAMESPACE => "namespace",
        SymbolKind::PACKAGE => "package",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY => "property",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "constructor",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "interface",
        SymbolKind::FUNCTION => "function",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::CONSTANT => "constant",
        SymbolKind::STRING => "string",
        SymbolKind::NUMBER => "number",
        SymbolKind::BOOLEAN => "boolean",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "object",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "enum_member",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "operator",
        SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "unknown",
    }
    .to_string()
}

/// Convert a Symbol to CachedSymbol
pub fn symbol_to_cached(symbol: &Symbol, offset: u32) -> CachedSymbol {
    CachedSymbol {
        fqn: symbol.fqn.to_string(),
        name: symbol.simple_name.to_string(),
        kind: symbol_kind_to_string(symbol.kind),
        offset,
        end_offset: symbol.range.end,
        container: symbol.container.as_ref().map(|s| s.to_string()),
        return_type: symbol.return_type.as_ref().map(|s| s.to_string()),
        parameters: symbol.parameters.iter().map(|s| s.to_string()).collect(),
        parent_class: symbol.parent_class.as_ref().map(|s| s.to_string()),
        modifiers: symbol.modifiers,
        documentation: symbol.documentation.as_ref().map(|s| s.to_string()),
        is_deprecated: symbol.is_deprecated,
        interfaces: symbol.interfaces.iter().map(|s| s.to_string()).collect(),
        receiver_type: symbol.receiver_type.as_ref().map(|s| s.to_string()),
        type_parameters: symbol.type_parameters.clone(),
        kotlin: symbol.kotlin.clone(),
        throws: symbol.throws().iter().map(|s| s.to_string()).collect(),
    }
}

/// Convert string kind back to SymbolKind
fn string_to_symbol_kind(kind: &str) -> Option<SymbolKind> {
    Some(match kind {
        "file" => SymbolKind::FILE,
        "module" => SymbolKind::MODULE,
        "namespace" => SymbolKind::NAMESPACE,
        "package" => SymbolKind::PACKAGE,
        "class" => SymbolKind::CLASS,
        "method" => SymbolKind::METHOD,
        "property" => SymbolKind::PROPERTY,
        "field" => SymbolKind::FIELD,
        "constructor" => SymbolKind::CONSTRUCTOR,
        "enum" => SymbolKind::ENUM,
        "interface" => SymbolKind::INTERFACE,
        "function" => SymbolKind::FUNCTION,
        "variable" => SymbolKind::VARIABLE,
        "constant" => SymbolKind::CONSTANT,
        "string" => SymbolKind::STRING,
        "number" => SymbolKind::NUMBER,
        "boolean" => SymbolKind::BOOLEAN,
        "array" => SymbolKind::ARRAY,
        "object" => SymbolKind::OBJECT,
        "key" => SymbolKind::KEY,
        "null" => SymbolKind::NULL,
        "enum_member" => SymbolKind::ENUM_MEMBER,
        "struct" => SymbolKind::STRUCT,
        "event" => SymbolKind::EVENT,
        "operator" => SymbolKind::OPERATOR,
        "type_parameter" => SymbolKind::TYPE_PARAMETER,
        _ => return None,
    })
}

/// Reject a semantically malformed payload before it can enter the live index.
///
/// The LZ4 checksum protects against most accidental corruption, but cache data
/// is deliberately treated as untrusted at the publication boundary as well.
/// In particular, invalid byte ranges would otherwise escape into navigation
/// responses and an unknown kind used to silently become a variable.
pub(crate) fn cached_file_payload_is_valid(cached: &CachedFile, source: &str) -> bool {
    let symbols_are_valid = cached.symbols.iter().all(|symbol| {
        let Ok(start) = usize::try_from(symbol.offset) else {
            return false;
        };
        let Ok(end) = usize::try_from(symbol.end_offset) else {
            return false;
        };
        !symbol.fqn.is_empty()
            && !symbol.name.is_empty()
            && !symbol.fqn.contains('\0')
            && !symbol.name.contains('\0')
            && string_to_symbol_kind(&symbol.kind).is_some()
            && start <= end
            && end <= source.len()
            && source.is_char_boundary(start)
            && source.is_char_boundary(end)
    });
    if !symbols_are_valid {
        return false;
    }

    let valid_text = |value: &str| !value.is_empty() && !value.contains('\0');
    let imports = &cached.imports;
    let wildcard_count = imports
        .wildcards
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let static_wildcard_count = imports
        .static_wildcards
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let static_import_count = imports
        .static_imports
        .iter()
        .map(|import| (import.fqn.as_ref(), import.alias.as_deref()))
        .collect::<std::collections::HashSet<_>>()
        .len();
    if imports
        .current_package
        .as_deref()
        .is_some_and(|package| !valid_text(package))
        || imports
            .explicit
            .iter()
            .any(|(name, fqn)| !valid_text(name) || !valid_text(fqn))
        || imports.wildcards.iter().any(|package| !valid_text(package))
        || imports.static_imports.iter().any(|import| {
            !valid_text(&import.fqn)
                || import
                    .alias
                    .as_deref()
                    .is_some_and(|alias| !valid_text(alias))
        })
        || imports
            .static_wildcards
            .iter()
            .any(|owner| !valid_text(owner))
        || wildcard_count != imports.wildcards.len()
        || static_wildcard_count != imports.static_wildcards.len()
        || static_import_count != imports.static_imports.len()
    {
        return false;
    }

    let mut generic_owners = std::collections::HashSet::new();
    cached.generic_supertypes.iter().all(|metadata| {
        !metadata.type_fqn.is_empty()
            && generic_owners.insert(metadata.type_fqn.as_str())
            && !metadata.edges.is_empty()
            && metadata.edges.iter().all(|edge| {
                !edge.written_target.is_empty()
                    && !edge.arguments.is_empty()
                    && !edge.written_target.contains('\0')
                    && edge.arguments.iter().all(|argument| valid_text(argument))
            })
    })
}

/// Convert a CachedSymbol back to Symbol
fn cached_to_symbol_with_kind(cached: &CachedSymbol, kind: SymbolKind) -> Symbol {
    use crate::TextRange;
    use std::sync::Arc;

    Symbol {
        fqn: cached.fqn.clone().into(),
        simple_name: cached.name.clone().into(),
        range: TextRange::new(cached.offset, cached.end_offset),
        kind,
        modifiers: cached.modifiers,
        container: cached.container.clone().map(Into::into),
        return_type: cached.return_type.clone().map(Into::into),
        parameters: cached.parameters.iter().map(|s| s.clone().into()).collect(),
        parent_class: cached.parent_class.clone().map(Into::into),
        interfaces: cached
            .interfaces
            .iter()
            .map(|s| Arc::from(s.as_str()))
            .collect(),
        documentation: cached.documentation.clone().map(Into::into),
        type_parameters: cached.type_parameters.clone(),
        is_deprecated: cached.is_deprecated,
        receiver_type: cached.receiver_type.clone().map(Into::into),
        kotlin: cached.kotlin.clone(),
        throws: (!cached.throws.is_empty()).then(|| {
            Arc::new(
                cached
                    .throws
                    .iter()
                    .map(|s| Arc::from(s.as_str()))
                    .collect(),
            )
        }),
    }
}

/// Convert a symbol whose cache envelope has already authenticated its shape.
///
/// Workspace cache restoration uses the fallible owned conversion below. The
/// JDK cache has its own authenticated/versioned envelope, but its older caller
/// still expects an infallible conversion; retain the historical variable
/// fallback there rather than in the workspace publication path.
pub fn cached_to_symbol(cached: &CachedSymbol) -> Symbol {
    let kind = string_to_symbol_kind(&cached.kind).unwrap_or(SymbolKind::VARIABLE);
    cached_to_symbol_with_kind(cached, kind)
}

pub(crate) fn validated_cached_to_symbol_owned(cached: CachedSymbol) -> Option<Symbol> {
    let kind = string_to_symbol_kind(&cached.kind)?;
    Some(Symbol {
        fqn: cached.fqn.into(),
        simple_name: cached.name.into(),
        range: crate::TextRange::new(cached.offset, cached.end_offset),
        kind,
        modifiers: cached.modifiers,
        container: cached.container.map(Into::into),
        return_type: cached.return_type.map(Into::into),
        parameters: cached.parameters.into_iter().map(Into::into).collect(),
        parent_class: cached.parent_class.map(Into::into),
        interfaces: cached.interfaces.into_iter().map(Into::into).collect(),
        documentation: cached.documentation.map(Into::into),
        type_parameters: cached.type_parameters,
        is_deprecated: cached.is_deprecated,
        receiver_type: cached.receiver_type.map(Into::into),
        kotlin: cached.kotlin,
        throws: (!cached.throws.is_empty())
            .then(|| Arc::new(cached.throws.into_iter().map(Into::into).collect())),
    })
}

impl CachedIndex {
    /// Get the cached entries
    pub fn entries(&self) -> &HashMap<PathBuf, CachedFile> {
        &self.entries
    }

    /// Get the usage statistics
    pub fn usage_stats(&self) -> &HashMap<String, u32> {
        &self.usage_stats
    }

    /// Consume the decoded snapshot so restoration can move its strings and
    /// vectors directly into the live index rather than retaining one complete
    /// serialized object graph alongside a second converted graph.
    pub fn into_parts(self) -> (HashMap<PathBuf, CachedFile>, HashMap<String, u32>) {
        (self.entries, self.usage_stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KotlinParameterMetadata, KotlinPropertyMetadata, KotlinSymbolMetadata, TextRange};
    use tempfile::TempDir;

    #[cfg(unix)]
    #[test]
    fn workspace_aliases_share_one_cache_identity() {
        let directory = TempDir::new().unwrap();
        let real = directory.path().join("real-workspace");
        let alias = directory.path().join("workspace-alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();

        assert_eq!(
            IndexCache::new(&real).path(),
            IndexCache::new(&alias).path()
        );
    }

    #[test]
    fn bounded_decoder_rejects_oversized_payloads() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());
        let cached = CachedIndex {
            version: CACHE_VERSION,
            entries: HashMap::new(),
            usage_stats: HashMap::new(),
        };
        let bytes = cache.serialize(&cached).unwrap();
        let payload = &bytes[CACHE_MAGIC.len() + 1..];
        let mut decoder = lz4_flex::frame::FrameDecoder::new(payload);
        let error = match deserialize_cached_index_from(&mut decoder, 1) {
            Ok(_) => panic!("oversized cache payload was accepted"),
            Err(error) => error,
        };

        assert!(matches!(*error, bincode::ErrorKind::SizeLimit));
    }

    #[test]
    fn bounded_cache_reader_rejects_oversized_files() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("cache.bin");
        std::fs::write(&path, b"12345").unwrap();

        let error = read_bounded_file(&path, 4).unwrap_err();

        assert!(matches!(error, CacheError::TooLarge { .. }));
    }

    #[test]
    fn test_cache_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        let mut entries = HashMap::new();
        entries.insert(
            PathBuf::from("/test/Foo.java"),
            CachedFile {
                mtime: SystemTime::now(),
                content_hash: 0,
                symbols: vec![CachedSymbol {
                    fqn: "com.test.Foo".into(),
                    name: "Foo".into(),
                    kind: "class".to_string(),
                    offset: 0,
                    end_offset: 3,
                    container: None,
                    return_type: None,
                    parameters: vec![],
                    parent_class: None,
                    modifiers: crate::Modifiers {
                        is_public: true,
                        ..crate::Modifiers::default()
                    },
                    documentation: Some("Foo documentation".to_string()),
                    is_deprecated: true,
                    interfaces: vec![],
                    receiver_type: None,
                    type_parameters: vec![crate::TypeParameterInfo::with_bounds(
                        "Value",
                        0,
                        ["java.lang.Number"],
                    )],
                    kotlin: None,
                    throws: Vec::new(),
                }],
                generic_supertypes: vec![crate::TypeGenericSupertypes {
                    type_fqn: "com.test.Foo".to_string(),
                    edges: vec![crate::GenericSupertype {
                        written_target: "Base".to_string(),
                        arguments: vec!["String".to_string()],
                    }],
                }],
                imports: crate::ImportMap {
                    explicit: HashMap::from([("List".into(), "java.util.List".into())]),
                    ..crate::ImportMap::default()
                },
            },
        );

        cache.save(entries.clone(), HashMap::new()).unwrap();

        let loaded = cache.load().expect("Cache should load");
        assert_eq!(loaded.version, CACHE_VERSION);
        assert_eq!(loaded.entries.len(), 1);
        let loaded_symbol = &loaded.entries[&PathBuf::from("/test/Foo.java")].symbols[0];
        assert_eq!(
            loaded_symbol.type_parameters,
            entries[&PathBuf::from("/test/Foo.java")].symbols[0].type_parameters
        );
        assert_eq!(
            loaded_symbol.documentation.as_deref(),
            Some("Foo documentation")
        );
        assert_eq!(
            loaded.entries[&PathBuf::from("/test/Foo.java")].generic_supertypes,
            entries[&PathBuf::from("/test/Foo.java")].generic_supertypes
        );
        assert!(loaded_symbol.is_deprecated);
    }

    #[test]
    fn kotlin_symbol_metadata_roundtrips_through_cache() {
        let symbol = Symbol {
            fqn: "sample.Record.value".into(),
            simple_name: "value".into(),
            range: TextRange::new(4, 9),
            kind: SymbolKind::PROPERTY,
            modifiers: crate::Modifiers {
                is_internal: true,
                is_data: true,
                is_sealed: true,
                is_operator: true,
                ..crate::Modifiers::default()
            },
            container: Some("sample.Record".into()),
            return_type: Some("kotlin.String?".into()),
            parameters: vec!["kotlin.Int count".into()],
            parent_class: None,
            interfaces: vec![],
            documentation: Some("Property documentation".into()),
            type_parameters: vec![crate::TypeParameterInfo::with_bounds(
                "Value",
                0,
                ["kotlin.Comparable<Value>"],
            )],
            is_deprecated: true,
            receiver_type: Some("kotlin.String".into()),
            throws: None,
            kotlin: Some(Box::new(KotlinSymbolMetadata {
                return_type_nullable: true,
                receiver_type_nullable: false,
                parameters: vec![KotlinParameterMetadata {
                    nullable: false,
                    has_default: true,
                }],
                is_suspend: true,
                is_operator: true,
                is_inline: true,
                property: Some(KotlinPropertyMetadata {
                    is_val: true,
                    has_getter: true,
                    has_setter: false,
                }),
                jvm_origin: None,
                type_aliases: vec![crate::KotlinTypeAlias {
                    fqn: "app.ByName".into(),
                    expanded_class: "java.util.HashMap".into(),
                    type_parameters: vec!["T".into()],
                    expanded_type_arguments: vec!["kotlin.String".into(), "T".into()],
                }],
            })),
        };

        let restored = cached_to_symbol(&symbol_to_cached(&symbol, 4));
        assert_eq!(restored.modifiers, symbol.modifiers);
        assert!(restored.modifiers.is_internal);
        assert!(restored.modifiers.is_data);
        assert!(restored.modifiers.is_sealed);
        assert!(restored.modifiers.is_operator);
        assert_eq!(restored.receiver_type.as_deref(), Some("kotlin.String"));
        assert_eq!(restored.type_parameters, symbol.type_parameters);
        assert_eq!(restored.kotlin, symbol.kotlin);
        assert_eq!(restored.documentation, symbol.documentation);
        assert_eq!(restored.is_deprecated, symbol.is_deprecated);
    }

    #[test]
    fn test_cache_version_mismatch() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Save with current version
        let entries = HashMap::new();
        cache.save(entries, HashMap::new()).unwrap();

        // Manually corrupt the version while preserving the cache envelope.
        let cache_path = cache.path();
        let bytes = std::fs::read(cache_path).unwrap();
        let mut cached: CachedIndex = cache.deserialize(&bytes).unwrap();
        cached.version = 999;
        std::fs::write(cache_path, cache.serialize(&cached).unwrap()).unwrap();

        // Load should fail due to version mismatch
        assert!(cache.load().is_none());
    }

    #[test]
    fn test_stale_file_detection() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Create a test file
        let test_file = temp_dir.path().join("Test.java");
        std::fs::write(&test_file, "class Test {}").unwrap();

        let mtime = std::fs::metadata(&test_file).unwrap().modified().unwrap();

        let mut entries = HashMap::new();
        entries.insert(
            test_file.clone(),
            CachedFile {
                mtime,
                content_hash: 0,
                symbols: vec![],
                generic_supertypes: vec![],
                imports: crate::ImportMap::default(),
            },
        );

        let cached = CachedIndex {
            version: CACHE_VERSION,
            entries,
            usage_stats: HashMap::new(),
        };

        // No stale files initially
        assert!(cache.check_stale_files(&cached).is_empty());

        // Modify the file
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&test_file, "class Test { int x; }").unwrap();

        // Now file should be stale
        let stale = cache.check_stale_files(&cached);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], test_file);
    }

    #[test]
    fn test_deleted_file_is_marked_stale() {
        // A cached declaration whose source disappeared must not be restored.
        // The single metadata call also avoids splitting existence and mtime
        // checks across a deletion race: every error fails closed as stale.

        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Create test file
        let test_file = temp_dir.path().join("Deleted.java");
        std::fs::write(&test_file, "class Deleted {}").unwrap();
        let mtime = std::fs::metadata(&test_file).unwrap().modified().unwrap();

        // Create cached entry for file
        let mut entries = HashMap::new();
        entries.insert(
            test_file.clone(),
            CachedFile {
                mtime,
                content_hash: 0,
                symbols: vec![],
                generic_supertypes: vec![],
                imports: crate::ImportMap::default(),
            },
        );

        let cached = CachedIndex {
            version: CACHE_VERSION,
            entries,
            usage_stats: HashMap::new(),
        };

        // Verify file is fresh
        assert!(cache.check_stale_files(&cached).is_empty());

        // Delete the file
        std::fs::remove_file(&test_file).unwrap();

        // Deleted files are stale so callers exclude them from restoration.
        let stale = cache.check_stale_files(&cached);
        assert_eq!(stale, vec![test_file]);
    }

    #[test]
    fn test_index_cache_roundtrip() {
        use crate::WorkspaceIndex;

        let temp_dir = TempDir::new().unwrap();

        // Create test Java file
        let java_file = temp_dir.path().join("Test.java");
        std::fs::write(
            &java_file,
            r#"
            package com.test;
            public class Test {
                private String name;
                public String getName() { return name; }
            }
            "#,
        )
        .unwrap();

        // Index the file
        let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
        index.index_file_sync_for_test(&java_file).unwrap();

        // Verify symbols are indexed
        assert!(index.has_class("com.test.Test"));

        // Extract cache data and save
        let cache = IndexCache::new(temp_dir.path());
        let cache_data = index.extract_cache_data();
        assert!(!cache_data.is_empty());
        let usage_stats = HashMap::new(); // TODO: Extract from index
        cache.save(cache_data, usage_stats).unwrap();

        // Create a fresh index and restore from cache
        let index2 = WorkspaceIndex::new(temp_dir.path().to_path_buf());
        let loaded = cache.load().expect("Cache should load");

        for (path, cached_file) in loaded.entries() {
            index2.restore_from_cache(path, cached_file);
        }

        // Verify symbols are restored
        assert!(index2.has_class("com.test.Test"));
        assert!(index2.find_symbol("com.test.Test.getName()").is_some());
    }

    #[test]
    fn bench_parallel_cache_validation() {
        use std::time::Instant;

        let temp_dir = TempDir::new().unwrap();

        // Create 2K cached entries (reduced from 10K for faster test iteration)
        // This is still enough to demonstrate parallel speedup
        let file_count = 2000;
        let mut entries = HashMap::new();

        println!("Creating {} test files...", file_count);
        for i in 0..file_count {
            let path = temp_dir.path().join(format!("Test{}.java", i));
            std::fs::write(&path, format!("class Test{} {{}}", i)).unwrap();
            let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

            entries.insert(
                path,
                CachedFile {
                    mtime,
                    content_hash: 0,
                    symbols: vec![],
                    generic_supertypes: vec![],
                    imports: crate::ImportMap::default(),
                },
            );
        }

        let cached = CachedIndex {
            version: CACHE_VERSION,
            entries,
            usage_stats: HashMap::new(),
        };

        let cache = IndexCache::new(temp_dir.path());

        // Warm up filesystem cache
        println!("Warming up filesystem cache...");
        let _ = cache.check_stale_files(&cached);

        // Actual benchmark
        println!("Running benchmark...");
        let start = Instant::now();
        let stale = cache.check_stale_files(&cached);
        let elapsed = start.elapsed();

        println!("\nResults:");
        println!("  Files validated: {}", cached.entries.len());
        println!("  Stale files: {}", stale.len());
        println!("  Time: {:.2}s", elapsed.as_secs_f64());
        println!(
            "  Throughput: {:.0} files/sec",
            cached.entries.len() as f64 / elapsed.as_secs_f64()
        );

        // Performance expectations:
        // - Sequential (single-threaded): ~200ms for 2K files (100μs per file)
        // - Parallel (4 threads): ~60ms for 2K files (4x speedup)
        // - Parallel (8 threads): ~35ms for 2K files (6x speedup)
        //
        // We set a generous upper bound to avoid flakiness on slow CI systems
        let max_time_ms = 300; // 300ms should be achievable even on single-core
        assert!(
            elapsed.as_millis() < max_time_ms,
            "Cache validation took {}ms, expected < {}ms",
            elapsed.as_millis(),
            max_time_ms
        );

        // Verify correctness - no files should be stale
        assert_eq!(
            stale.len(),
            0,
            "Expected no stale files, but found {}",
            stale.len()
        );
    }

    #[test]
    fn test_cache_compression_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Create a larger dataset to ensure compression is beneficial
        let mut entries = HashMap::new();
        for i in 0..100 {
            let path = PathBuf::from(format!("/test/File{}.java", i));
            let mut symbols = vec![];

            // Add multiple symbols per file to increase data size
            for j in 0..10 {
                symbols.push(CachedSymbol {
                    fqn: format!("com.test.File{}.method{}", i, j),
                    name: format!("method{}", j),
                    kind: "method".to_string(),
                    offset: j * 100,
                    end_offset: j * 100 + 8,
                    container: Some(format!("com.test.File{}", i)),
                    return_type: Some("void".into()),
                    parameters: vec!["String".to_string(), "int".to_string()],
                    parent_class: None,
                    modifiers: crate::Modifiers {
                        is_public: true,
                        ..crate::Modifiers::default()
                    },
                    documentation: None,
                    is_deprecated: false,
                    interfaces: vec![],
                    receiver_type: None,
                    type_parameters: vec![],
                    kotlin: None,
                    throws: Vec::new(),
                });
            }

            entries.insert(
                path,
                CachedFile {
                    mtime: SystemTime::now(),
                    content_hash: 0,
                    symbols,
                    generic_supertypes: vec![],
                    imports: crate::ImportMap {
                        explicit: HashMap::from([
                            ("List".into(), "java.util.List".into()),
                            ("Map".into(), "java.util.Map".into()),
                            ("Set".into(), "java.util.Set".into()),
                        ]),
                        ..crate::ImportMap::default()
                    },
                },
            );
        }

        // Save with compression
        cache.save(entries.clone(), HashMap::new()).unwrap();

        // Load and verify
        let loaded = cache.load().expect("Cache should load");
        assert_eq!(loaded.version, CACHE_VERSION);
        assert_eq!(loaded.entries.len(), 100);

        // Verify symbols are preserved correctly
        for i in 0..100 {
            let path = PathBuf::from(format!("/test/File{}.java", i));
            let cached_file = loaded.entries.get(&path).unwrap();
            assert_eq!(cached_file.symbols.len(), 10);

            for j in 0..10 {
                let symbol = &cached_file.symbols[j];
                assert_eq!(symbol.fqn, format!("com.test.File{}.method{}", i, j));
                assert_eq!(&*symbol.name, format!("method{}", j));
                assert_eq!(symbol.kind, "method");
                assert_eq!(symbol.parameters.len(), 2);
            }
        }

        // Verify cache file exists and check compression effectiveness
        let cache_bytes = std::fs::read(cache.path()).unwrap();
        println!("Cache file size: {} bytes", cache_bytes.len());

        // With 100 files × 10 symbols each with meaningful data,
        // we should see compression benefit
        // Uncompressed would be ~100KB+, compressed should be <50KB
        assert!(
            cache_bytes.len() < 100_000,
            "Cache should be compressed (got {} bytes)",
            cache_bytes.len()
        );
    }

    #[test]
    fn test_parallel_cache_validation_correctness() {
        // Test that parallel validation produces correct results
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Create 100 test files
        let mut entries = HashMap::new();
        for i in 0..100 {
            let path = temp_dir.path().join(format!("Test{}.java", i));
            std::fs::write(&path, format!("class Test{} {{}}", i)).unwrap();
            let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

            entries.insert(
                path.clone(),
                CachedFile {
                    mtime,
                    content_hash: 0,
                    symbols: vec![],
                    generic_supertypes: vec![],
                    imports: crate::ImportMap::default(),
                },
            );
        }

        let cached = CachedIndex {
            version: CACHE_VERSION,
            entries,
            usage_stats: HashMap::new(),
        };

        // Initially, no stale files
        let stale = cache.check_stale_files(&cached);
        assert!(
            stale.is_empty(),
            "Expected no stale files initially, but found {}",
            stale.len()
        );

        // Modify 10 files
        for i in 0..10 {
            let path = temp_dir.path().join(format!("Test{}.java", i));
            std::thread::sleep(std::time::Duration::from_millis(10));
            std::fs::write(&path, format!("class Test{} {{ int x; }}", i)).unwrap();
        }

        // Should detect exactly 10 stale files
        let stale = cache.check_stale_files(&cached);
        assert_eq!(
            stale.len(),
            10,
            "Expected 10 stale files, but found {}",
            stale.len()
        );

        // Verify all stale files are from the modified set
        for stale_path in &stale {
            let filename = stale_path.file_name().unwrap().to_str().unwrap();
            let num: usize = filename
                .strip_prefix("Test")
                .unwrap()
                .strip_suffix(".java")
                .unwrap()
                .parse()
                .unwrap();
            assert!(num < 10, "Stale file {} should be in 0..10 range", filename);
        }
    }

    #[test]
    fn test_async_save_completes() {
        use std::time::{Duration, Instant};

        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        let mut entries = HashMap::new();
        entries.insert(
            PathBuf::from("/test/AsyncTest.java"),
            CachedFile {
                mtime: SystemTime::now(),
                content_hash: 0,
                symbols: vec![CachedSymbol {
                    fqn: "com.test.AsyncTest".into(),
                    name: "AsyncTest".into(),
                    kind: "class".to_string(),
                    offset: 0,
                    end_offset: 9,
                    container: None,
                    return_type: None,
                    parameters: vec![],
                    parent_class: None,
                    modifiers: crate::Modifiers {
                        is_public: true,
                        ..crate::Modifiers::default()
                    },
                    documentation: None,
                    is_deprecated: false,
                    interfaces: vec![],
                    receiver_type: None,
                    type_parameters: vec![],
                    kotlin: None,
                    throws: Vec::new(),
                }],
                generic_supertypes: vec![],
                imports: crate::ImportMap::default(),
            },
        );

        // Start async save
        let start = Instant::now();
        cache.save_async(entries.clone(), HashMap::new());
        let call_duration = start.elapsed();

        // save_async should return quickly (<10ms just for cloning)
        assert!(
            call_duration < Duration::from_millis(20),
            "save_async took {:?}, expected <20ms",
            call_duration
        );

        // Wait for background save to complete
        cache.wait_for_pending_saves();

        // Verify cache was saved correctly
        let loaded = cache.load().expect("Cache should load after async save");
        assert_eq!(loaded.entries.len(), 1);
        assert!(loaded
            .entries
            .contains_key(&PathBuf::from("/test/AsyncTest.java")));
    }

    #[test]
    fn test_multiple_async_saves() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Spawn multiple async saves
        for i in 0..5 {
            let mut entries = HashMap::new();
            entries.insert(
                PathBuf::from(format!("/test/Test{}.java", i)),
                CachedFile {
                    mtime: SystemTime::now(),
                    content_hash: 0,
                    symbols: vec![],
                    generic_supertypes: vec![],
                    imports: crate::ImportMap::default(),
                },
            );
            cache.save_async(entries, HashMap::new());
        }

        // Wait for all saves to complete
        cache.wait_for_pending_saves();

        // Last save should have succeeded (overwrites previous ones)
        let loaded = cache.load().expect("Cache should load");
        assert_eq!(loaded.entries.len(), 1);
        assert!(loaded
            .entries
            .contains_key(&PathBuf::from("/test/Test4.java")));
    }

    #[test]
    fn cache_handles_share_save_order_and_pending_work() {
        let temp_dir = TempDir::new().unwrap();
        let background_cache = IndexCache::new(temp_dir.path());
        let shutdown_cache = IndexCache::new(temp_dir.path());

        // Make the older background snapshot expensive enough that an
        // uncoordinated newer save would normally finish first.
        let old_stats = (0..25_000)
            .map(|index| (format!("old.symbol.{index}"), index))
            .collect();
        background_cache.save_async(HashMap::new(), old_stats);

        shutdown_cache
            .save(
                HashMap::new(),
                HashMap::from([("latest.symbol".to_string(), 7)]),
            )
            .unwrap();
        shutdown_cache.wait_for_pending_saves();

        let loaded = shutdown_cache.load().expect("latest cache should load");
        assert_eq!(
            loaded.usage_stats(),
            &HashMap::from([("latest.symbol".to_string(), 7)])
        );
    }

    #[test]
    fn index_snapshot_save_reserves_before_a_later_shutdown_save() {
        let workspace = TempDir::new().unwrap();
        let path = workspace.path().join("Generated.java");
        std::fs::write(&path, "class Generated {}").unwrap();
        let index = Arc::new(crate::WorkspaceIndex::new(workspace.path().to_path_buf()));
        index.index_file_sync_for_test(&path).unwrap();
        let background_cache = IndexCache::new(workspace.path());
        let shutdown_cache = IndexCache::new(workspace.path());

        background_cache.save_index_async(Arc::clone(&index));
        let empty_index = crate::WorkspaceIndex::new(workspace.path().to_path_buf());
        shutdown_cache.save_index(&empty_index).unwrap();
        shutdown_cache.wait_for_pending_saves();

        assert!(shutdown_cache.load().unwrap().entries().is_empty());
    }

    #[test]
    fn test_wait_for_pending_saves_with_no_saves() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Should return immediately if no pending saves
        let start = std::time::Instant::now();
        cache.wait_for_pending_saves();
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(10),
            "wait_for_pending_saves with no saves took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_async_save_error_handling() {
        // Create cache with invalid path to trigger error
        let cache = IndexCache {
            cache_path: PathBuf::from("/this/path/definitely/does/not/exist/cache.bin"),
            coordinator: Arc::new(SaveCoordinator::default()),
        };

        let mut entries = HashMap::new();
        entries.insert(
            PathBuf::from("/test/ErrorTest.java"),
            CachedFile {
                mtime: SystemTime::now(),
                content_hash: 0,
                symbols: vec![],
                generic_supertypes: vec![],
                imports: crate::ImportMap::default(),
            },
        );

        // Should not panic, just log error
        cache.save_async(entries, HashMap::new());

        // Wait for background task to complete (and fail)
        cache.wait_for_pending_saves();

        // Counter should be back to 0 even though save failed
        assert_eq!(cache.coordinator.state.lock().pending, 0);
    }

    #[test]
    fn test_magic_header_validation() {
        // M6: Test that cache files must have valid magic header
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Test 1: Valid cache file should have magic header
        let mut entries = HashMap::new();
        entries.insert(
            PathBuf::from("/test/Magic.java"),
            CachedFile {
                mtime: SystemTime::now(),
                content_hash: 0,
                symbols: vec![],
                generic_supertypes: vec![],
                imports: crate::ImportMap::default(),
            },
        );

        cache.save(entries.clone(), HashMap::new()).unwrap();

        // Read raw bytes and verify magic header
        let bytes = std::fs::read(cache.path()).unwrap();
        assert_eq!(
            &bytes[..4],
            b"HYPE",
            "Cache file should start with HYPE magic"
        );
        assert_eq!(bytes[4], CACHE_ENCODING_LZ4_FRAME);

        // Test 2: Loading should succeed with valid magic
        let loaded = cache.load().expect("Cache with valid magic should load");
        assert_eq!(loaded.entries.len(), 1);

        // Test 3: Corrupted file without magic should fail
        let corrupted = b"NOPE this is not a valid cache file";
        std::fs::write(cache.path(), corrupted).unwrap();
        assert!(
            cache.load().is_none(),
            "Cache with invalid magic should not load"
        );

        // Test 4: File too short (less than 4 bytes)
        std::fs::write(cache.path(), b"HY").unwrap();
        assert!(
            cache.load().is_none(),
            "Cache with truncated magic should not load"
        );

        // Test 5: Random binary data should fail
        let random_data = vec![0xFF, 0xFE, 0xFD, 0xFC, 0x00, 0x01, 0x02, 0x03];
        std::fs::write(cache.path(), random_data).unwrap();
        assert!(
            cache.load().is_none(),
            "Cache with random data should not load"
        );

        let unsupported = [CACHE_MAGIC.as_slice(), &[0xff]].concat();
        assert!(matches!(
            cache.deserialize(&unsupported),
            Err(CacheError::UnsupportedEncoding(0xff))
        ));

        let mut trailing = cache
            .serialize(&CachedIndex {
                version: CACHE_VERSION,
                entries: HashMap::new(),
                usage_stats: HashMap::new(),
            })
            .unwrap();
        trailing.push(0xff);
        assert!(
            cache.deserialize(&trailing).is_err(),
            "trailing bytes outside the cache frame must be rejected"
        );
    }

    #[test]
    fn test_cache_version_mismatch_with_magic() {
        // M6: Version mismatch should still be detected after magic header validation
        let temp_dir = TempDir::new().unwrap();
        let cache = IndexCache::new(temp_dir.path());

        // Save valid cache
        let entries = HashMap::new();
        cache.save(entries, HashMap::new()).unwrap();

        // Load, modify version, save back
        let bytes = std::fs::read(cache.path()).unwrap();
        let mut cached = cache.deserialize(&bytes).unwrap();
        cached.version = 999;

        std::fs::write(cache.path(), cache.serialize(&cached).unwrap()).unwrap();

        // Load should fail due to version mismatch (not magic - that passed)
        assert!(cache.load().is_none());
    }
}
