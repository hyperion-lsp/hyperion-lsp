// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Workspace Index - Cross-File Symbol Storage and Query Engine
//!
//! This module provides the core indexing infrastructure for Hyperion LSP, storing and querying
//! symbol information across an entire workspace (10K-1M+ files). It's designed for **low memory
//! usage** (<500MB for 1M lines) and **fast queries** (<50ms for goto-definition).
//!
//! # Architecture
//!
//! ```text
//! WorkspaceIndex (Arc<RwLock<IndexInner>>)
//! ├── Symbol Storage
//! │   ├── symbol_locations: FQN → SymbolLocation (definitions)
//! │   ├── symbol_usages: FQN → Vec<SymbolLocation> (references)
//! │   ├── symbols: FQN → Symbol (full metadata)
//! │   └── name_index: SimpleName → Vec<FQN> (fast lookup)
//! ├── Reverse Dependencies
//! │   ├── reverse_dependencies: FQN → Vec<FileId> (who imports this?)
//! │   ├── file_symbols: FileId → Vec<Arc<Symbol>> (exact source declarations)
//! │   └── symbol_definition_files: FQN → SmallVec<SourceSymbolCandidate>
//! ├── File Tracking
//! │   ├── file_map: Path → FileId (bidirectional path↔id mapping)
//! │   ├── file_content_cache: LRU cache of parsed files (100 entries)
//! │   └── parse_tree_cache: Size-aware cache (50MB limit)
//! └── Caches (LRU, invalidated on file change)
//!     ├── method_resolution_cache: (FQN, method) → Option<ReturnType> (2000 entries)
//!     └── inheritance_cache: FQN → Vec<ParentFQNs> (1000 entries)
//! ```
//!
//! # Invariants
//!
//! The index maintains strict consistency guarantees:
//!
//! **1. Symbol Consistency:**
//! - Every FQN in `symbol_locations` MUST exist in `symbols`
//! - Every FQN in `symbol_usages` MUST exist in `symbols` (or be a valid external reference)
//! - Every FQN in `name_index` values MUST exist in `symbols`
//!
//! **2. Reverse Dependency Consistency:**
//! - Reverse-dependency keys describe source imports and MAY be unresolved.
//!   Their lifetime follows the importing file, not the target declaration.
//! - Every FileId in `reverse_dependencies` values MUST exist in `file_map`
//! - Every symbol in `file_symbols` values MUST be declared by that file/source.
//!   The active `symbol_locations` owner may differ while a workspace or
//!   earlier classpath declaration shadows it.
//! - `file_symbols` and `symbol_definition_files` MUST be exact inverses,
//!   including declarations with the same textual FQN but different kinds.
//!
//! **3. Cache Invalidation Contract:**
//! - When a file changes, ALL dependent files MUST be re-indexed (transitive invalidation)
//! - When a file changes, ALL caches (method_resolution_cache, inheritance_cache) MUST be cleared
//! - Parse tree cache MUST evict entries when memory limit is exceeded (50MB limit)
//!
//! **4. File ID Consistency:**
//! - FileIds are assigned incrementally and NEVER reused (monotonic counter)
//! - Path → FileId mapping uses one platform-aware lookup key everywhere
//! - FileId → Path mapping is always 1:1 (no duplicates)
//!
//! **5. Threading Guarantees:**
//! - `WorkspaceIndex` is `Clone + Send + Sync` (backed by `Arc<RwLock<IndexInner>>`)
//! - Salsa handles concurrent access to indexed data (read-write locks)
//! - Per-file locks prevent concurrent indexing of the same file (prevents stale overwrites)
//!
//! # Cache Invalidation
//!
//! External file changes (git checkout, other editors) trigger cache invalidation:
//!
//! ```text
//! External Change (e.g., git checkout)
//!     ↓
//! File Watcher (notify crate, 500ms debounce)
//!     ↓
//! Event Processing (Created/Modified/Deleted/Renamed)
//!     ↓
//! Update Changed File (Workspace Index + Salsa DB)
//!     ↓
//! Query Reverse Dependencies ("Who imports this file's symbols?")
//!     ↓
//! Transitive Invalidation (Re-index all dependent files)
//! ```
//!
//! **Key APIs:**
//! - `get_dependent_files(fqn: &str) -> Vec<FileId>` - Get all files that import a symbol
//! - `get_file_symbols(file_id: FileId) -> Vec<String>` - Get all symbols defined in a file
//!
//! # Performance Characteristics
//!
//! - **Initial Index:** 1-3 seconds for 100K lines (Rayon parallel indexing)
//! - **Incremental Update:** <10ms for single file change
//! - **Query Time:** <5ms for goto-definition, <50ms for find-references
//! - **Memory Usage:** ~300MB for 1M lines
//!
//! # Example
//!
//! ```rust,ignore
//! use hyperion_workspace::WorkspaceIndex;
//! use std::path::PathBuf;
//!
//! # fn example() -> anyhow::Result<()> {
//! // Create index
//! let index = WorkspaceIndex::new(PathBuf::from("/workspace"));
//!
//! // Initial scan
//! index.scan_workspace()?;
//!
//! // Query symbols
//! let location = index.get_symbol_location("com.example.User");
//! let members = index.get_class_members("com.example.User");
//!
//! // Update file (invalidates caches)
//! index.update_file(file_id, source_code)?;
//!
//! // Reverse dependencies
//! let dependents = index.get_dependent_files("com.example.User");
//! # Ok(())
//! # }
//! ```
//!
//! # Safety Notes
//!
//! - Per-file locks use `Arc<Mutex<()>>` to prevent concurrent indexing of the same file
//! - This prevents race conditions where stale data overwrites fresh data
//! - The lock is held only during indexing, not during queries
//! - Salsa provides concurrent read access during indexing of other files

mod cache;
mod core;
mod current_usage;
mod effective_fields;
mod effective_methods;
mod external_source;
mod functional_interface;
#[cfg(test)]
mod functional_interface_tests;
mod generic_supertypes;
mod inheritance;
mod path_registry;
mod query;
mod refactoring;
mod removal;

pub mod types;
pub use types::*;

use external_source::RegisteredExternalSource;
pub use external_source::{ExternalArchiveSnapshot, ExternalSourceArchive, ExternalSourceIdentity};
pub use functional_interface::{
    java_parameter_parts_from_declaration, java_parameter_type_from_declaration,
    FunctionalInterfaceQuery, FunctionalInterfaceSignature,
};
pub(crate) use generic_supertypes::project_type_from_receiver_in;
pub(crate) use inheritance::{kotlin_type_alias_expansion_from, KotlinTypeAliasExpansion};
use path_registry::PathRegistry;
pub use path_registry::RegisteredFilePath;
pub(crate) use query::OwnerElection;
pub use query::SourceScope;
pub(crate) use query::{
    find_symbol_definition_coherent_excluding_in, find_value_definition_in_file_at_range_in,
    selected_owner_member_definitions_excluding_in, selected_owner_member_definitions_in,
};

use crate::{
    path_lookup_key, scanner::WorkspaceScanner, symbol_extractor::SymbolExtractor, FileId,
    ImportMap, Language, PathLookupKey, SymbolLocation, ValidatedContent,
};
use anyhow::{Context, Result};
use dashmap::DashMap;
use lru::LruCache;
use lsp_types::SymbolKind;
use parking_lot::{Condvar, Mutex, RwLock};
use rayon::prelude::*;
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Arc,
};
use tracing::{debug, info, warn};

// Cache size constants - using const ensures compile-time safety
// without requiring unsafe or panicking unwrap() at runtime
pub(super) const FILE_CONTENT_CACHE_SIZE: NonZeroUsize = match NonZeroUsize::new(100) {
    Some(n) => n,
    None => panic!("100 is not zero"),
};
pub(super) const METHOD_RESOLUTION_CACHE_SIZE: NonZeroUsize = match NonZeroUsize::new(2000) {
    Some(n) => n,
    None => panic!("2000 is not zero"),
};
pub(super) const INHERITANCE_CACHE_SIZE: NonZeroUsize = match NonZeroUsize::new(1000) {
    Some(n) => n,
    None => panic!("1000 is not zero"),
};
const CURRENT_USAGE_CACHE_ENTRIES: NonZeroUsize = match NonZeroUsize::new(8192) {
    Some(n) => n,
    None => panic!("8192 is not zero"),
};
const CURRENT_USAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Cache for method resolution results: (receiver_fqn, method_name) -> Option<return_type>
pub(super) type MethodResolutionCache = Arc<Mutex<LruCache<(String, String), Option<String>>>>;

type CurrentUsageCacheKey = (WorkspaceRevision, FileRevisionToken);

struct CurrentUsageCache {
    entries: LruCache<CurrentUsageCacheKey, (Arc<[crate::usage_extractor::Usage]>, usize)>,
    total_bytes: usize,
}

impl CurrentUsageCache {
    fn new() -> Self {
        Self {
            entries: LruCache::new(CURRENT_USAGE_CACHE_ENTRIES),
            total_bytes: 0,
        }
    }

    fn get(&mut self, key: &CurrentUsageCacheKey) -> Option<Arc<[crate::usage_extractor::Usage]>> {
        self.entries.get(key).map(|(usages, _)| Arc::clone(usages))
    }

    fn put(&mut self, key: CurrentUsageCacheKey, usages: Arc<[crate::usage_extractor::Usage]>) {
        let entry_bytes =
            usages
                .iter()
                .fold(std::mem::size_of_val(usages.as_ref()), |bytes, usage| {
                    bytes
                        .saturating_add(usage.fqn.len())
                        .saturating_add(usage.location.name.len())
                        .saturating_add(usage.location.fqn.len())
                        .saturating_add(
                            usage
                                .location
                                .container_name
                                .as_ref()
                                .map_or(0, |container| container.len()),
                        )
                });
        if entry_bytes > CURRENT_USAGE_CACHE_BYTES {
            return;
        }
        if let Some((_, old_bytes)) = self.entries.pop(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old_bytes);
        }
        while self.total_bytes.saturating_add(entry_bytes) > CURRENT_USAGE_CACHE_BYTES {
            let Some((_, (_, evicted_bytes))) = self.entries.pop_lru() else {
                break;
            };
            self.total_bytes = self.total_bytes.saturating_sub(evicted_bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(entry_bytes);
        if let Some((_, replaced_bytes)) = self.entries.put(key, (usages, entry_bytes)) {
            self.total_bytes = self.total_bytes.saturating_sub(replaced_bytes);
        }
    }
}

const PROFILE_LANGUAGE_COUNT: usize = 4;

/// Aggregate worker CPU time for the file-indexing pipeline.
///
/// These counters intentionally sum time across parallel workers, so they
/// describe where indexing work is spent rather than wall-clock duration.
#[derive(Default)]
pub(super) struct IndexPhaseProfile {
    files: [AtomicU64; PROFILE_LANGUAGE_COUNT],
    read_ns: AtomicU64,
    hash_ns: AtomicU64,
    setup_lock_ns: AtomicU64,
    symbol_extract_ns: [AtomicU64; PROFILE_LANGUAGE_COUNT],
    usage_extract_ns: [AtomicU64; PROFILE_LANGUAGE_COUNT],
    parse_cache_ns: AtomicU64,
    store_lock_ns: AtomicU64,
    post_process_ns: AtomicU64,
}

impl IndexPhaseProfile {
    fn language_index(language: Option<Language>) -> usize {
        match language {
            Some(Language::Kotlin) => 1,
            Some(Language::Groovy) => 2,
            Some(Language::Scala) => 3,
            Some(Language::Java) | None => 0,
        }
    }

    fn add_duration(counter: &AtomicU64, elapsed: std::time::Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        counter.fetch_add(nanos, Ordering::Relaxed);
    }

    fn snapshot(&self) -> IndexPhaseSnapshot {
        IndexPhaseSnapshot {
            files: std::array::from_fn(|index| self.files[index].load(Ordering::Relaxed)),
            read_ns: self.read_ns.load(Ordering::Relaxed),
            hash_ns: self.hash_ns.load(Ordering::Relaxed),
            setup_lock_ns: self.setup_lock_ns.load(Ordering::Relaxed),
            symbol_extract_ns: std::array::from_fn(|index| {
                self.symbol_extract_ns[index].load(Ordering::Relaxed)
            }),
            usage_extract_ns: std::array::from_fn(|index| {
                self.usage_extract_ns[index].load(Ordering::Relaxed)
            }),
            parse_cache_ns: self.parse_cache_ns.load(Ordering::Relaxed),
            store_lock_ns: self.store_lock_ns.load(Ordering::Relaxed),
            post_process_ns: self.post_process_ns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PublishedResolvedMethodUsages {
    pub(super) source_revision_id: NonZeroU64,
    pub(super) workspace_revision: WorkspaceRevision,
    pub(super) usages: ResolvedMethodUsages,
}

/// Monotonic ownership of one logical classpath refresh.
///
/// Reserve this before asynchronous dependency resolution. A superseded task
/// can then be rejected before it changes classpath membership or reserves
/// per-archive publication generations.
#[derive(Debug)]
pub(super) struct ExternalSourceRefreshAuthority {
    owner: Arc<str>,
    latest_generation: AtomicU64,
    committed_generation: AtomicU64,
}

#[derive(Default)]
struct ExternalSourcePublicationGate {
    locked: Mutex<bool>,
    unlocked: Condvar,
}

/// Provenance and byte identity of the currently published source revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PublishedRevision {
    pub id: NonZeroU64,
    pub content_hash: u64,
    pub origin: SourceRevisionOrigin,
}

impl PublishedRevision {
    fn token(self, file_id: FileId) -> FileRevisionToken {
        FileRevisionToken {
            file_id,
            revision_id: self.id,
        }
    }
}

/// Usage signals for one symbol, co-located under one shard lock.
///
/// Indexed references are revision-derived and replaceable; interactions are
/// durable user history. Keeping them in one entry avoids duplicate keys and
/// makes ranking incapable of acquiring two usage maps in opposite orders.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct UsageCounts {
    pub(super) indexed: u32,
    pub(super) interactions: u32,
}

impl UsageCounts {
    pub(super) fn total(self) -> u32 {
        self.indexed.saturating_add(self.interactions)
    }
}

/// Names captured under one workspace publication.
type RevisionedJavaLangNames = (WorkspaceRevision, Arc<HashSet<String>>);

#[derive(Clone)]
struct SourceScopePublication {
    ticket: ExternalSourceRefreshTicket,
    entries: Arc<[(PathLookupKey, SourceScope)]>,
}

/// One source set's ordered compile visibility from a build publication.
#[derive(Clone, Debug)]
pub struct SourceScopeInput {
    pub requester_roots: Vec<PathBuf>,
    pub visible_source_roots: Vec<PathBuf>,
    pub visible_external_archives: Vec<PathBuf>,
}

/// Main workspace index
#[derive(Clone)]
pub struct WorkspaceIndex {
    pub(super) inner: Arc<RwLock<IndexInner>>,
    /// Immutable request-local owner ordering. Clones share every index map;
    /// only this small selection context differs between source sets.
    pub(super) source_scope: Option<SourceScope>,
    source_scope_ticket: Option<ExternalSourceRefreshTicket>,
    source_scope_ambiguous: bool,
    source_scope_publication: Arc<RwLock<Option<SourceScopePublication>>>,
    /// Per-path locks serialize indexing before a FileId is chosen.
    ///
    /// A path can temporarily have two candidate IDs when startup indexing races
    /// an editor `didOpen`. FileId-keyed locks alone cannot serialize that case.
    pub(super) path_locks: Arc<DashMap<PathLookupKey, Arc<std::sync::Mutex<()>>>>,
    /// Per-file locks to prevent concurrent indexing of the same file
    /// This prevents race conditions where stale data overwrites fresh data
    pub(super) file_locks: Arc<DashMap<FileId, Arc<std::sync::Mutex<()>>>>,
    /// Per-owner commit gates spanning WorkspaceIndex and its side registries.
    /// There are only a handful of logical owners (JDK, Maven, Gradle) per
    /// session, so retaining these gates avoids an unsafe remove/reinsert race.
    external_source_publication_gates: Arc<DashMap<Arc<str>, Arc<ExternalSourcePublicationGate>>>,
    /// Cache for method resolution results (receiver_fqn, method_name) -> Option<return_type>
    /// Caches both positive (found method) and negative (method not found) results
    /// Uses LRU eviction to bound memory usage (max 2000 entries)
    /// Cleared on file changes to maintain consistency
    pub(super) method_resolution_cache: MethodResolutionCache,
    /// Monotonic publication generation used to reject cache fills computed
    /// against an older index revision.
    pub(super) resolution_cache_generation: Arc<AtomicU64>,
    /// Cache for inheritance chains: FQN -> Vec<parent FQNs> (most specific first)
    /// Caches the complete parent chain for each class to avoid repeated traversal
    /// Uses LRU eviction to bound memory usage (max 1000 entries)
    /// Cleared on file changes to maintain consistency
    pub(super) inheritance_cache: Arc<Mutex<LruCache<String, Vec<String>>>>,
    /// Complete usage extractions keyed by the exact source and workspace
    /// revisions that governed name resolution. This amortizes reference
    /// queries across symbols without allowing reuse after any rebinding.
    current_usage_cache: Arc<Mutex<CurrentUsageCache>>,
    /// Co-located user-interaction and revision-derived usage counts.
    /// Replacing a source revision updates only `indexed`; cache persistence
    /// reads and writes only `interactions`.
    pub(super) usage_stats: Arc<DashMap<Arc<str>, UsageCounts>>,
    /// Low-overhead aggregate timings for cold-index profiling.
    pub(super) index_profile: Arc<IndexPhaseProfile>,
    /// Serializes complete workspace builds. Per-file publishers remain
    /// concurrent with the build through their narrower path/file locks.
    full_build_lock: Arc<std::sync::Mutex<()>>,
    /// The active two-pass cold-build phase. LSP source publication uses this
    /// to avoid scheduling work that the not-yet-started usage pass is already
    /// guaranteed to perform.
    cold_build_phase: Arc<AtomicU8>,
    /// `java.lang` simple names, memoized against the workspace revision that
    /// produced them.
    ///
    /// `ImportResolver` is constructed per name resolution, so its own
    /// `OnceLock` never amortized anything: every resolution rebuilt and
    /// sorted a `Vec<String>` of the whole package and re-extended a HashSet.
    /// The memo must stay revision-keyed rather than one-shot — the set is
    /// legitimately empty until JDK indexing lands, and caching that emptiness
    /// forever would make `java.lang` permanently unresolvable.
    java_lang_names: Arc<RwLock<Option<RevisionedJavaLangNames>>>,
    /// Debug-build check that every mutation runs inside the host's write
    /// transaction. Disabled until a host calls [`Self::require_exclusive_writes`].
    exclusive_writes: Arc<ExclusiveWriteGate>,
}

/// State behind [`WorkspaceIndex::require_exclusive_writes`].
#[derive(Default)]
struct ExclusiveWriteGate {
    required: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicUsize,
}

/// Held by the semantic host's write transaction for its whole lifetime.
#[must_use = "index writes are only allowed while this guard is alive"]
pub struct ExclusiveIndexWrites(Arc<ExclusiveWriteGate>);

impl Drop for ExclusiveIndexWrites {
    fn drop(&mut self) {
        self.0
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// H3 FIX: Size-aware parse tree cache to prevent unbounded memory growth
///
/// The previous LRU cache counted entries, not bytes. Large files (100KB+) could
/// cause memory spikes even with entry limits. This wrapper tracks total bytes
/// and evicts entries when memory threshold is exceeded.
///
/// Memory limit: 50MB (enough for ~500 average files or ~50 large files)
pub(super) struct ParseTreeCache {
    cache: LruCache<FileId, (Arc<rowan::GreenNode>, Arc<String>)>,
    total_bytes: usize,
    max_bytes: usize,
}

impl ParseTreeCache {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(1000).unwrap()), // High entry limit, byte limit is primary
            total_bytes: 0,
            max_bytes,
        }
    }

    /// Insert a parse tree into the cache, evicting old entries if size limit exceeded
    pub(super) fn put(
        &mut self,
        file_id: FileId,
        tree: Arc<rowan::GreenNode>,
        source: Arc<String>,
    ) {
        // If this file already has a cached entry, remove it first so total_bytes
        // reflects replacement, not additive growth.
        if let Some((old_tree, old_source)) = self.cache.pop(&file_id) {
            let old_size = Self::estimate_entry_size(&old_tree, &old_source);
            self.total_bytes = self.total_bytes.saturating_sub(old_size);
        }

        let entry_size = Self::estimate_entry_size(&tree, &source);

        // Evict LRU entries until we have space for the new entry
        while self.total_bytes + entry_size > self.max_bytes {
            if let Some((_, (old_tree, old_source))) = self.cache.pop_lru() {
                let old_size = Self::estimate_entry_size(&old_tree, &old_source);
                self.total_bytes = self.total_bytes.saturating_sub(old_size);
            } else {
                // Cache is empty but entry still too large → insert anyway (better than crash)
                break;
            }
        }

        self.cache.put(file_id, (tree, source));
        self.total_bytes += entry_size;
    }

    pub(super) fn peek(&self, file_id: &FileId) -> Option<&(Arc<rowan::GreenNode>, Arc<String>)> {
        self.cache.peek(file_id)
    }

    pub(super) fn remove(
        &mut self,
        file_id: &FileId,
    ) -> Option<(Arc<rowan::GreenNode>, Arc<String>)> {
        let (tree, source) = self.cache.pop(file_id)?;
        let entry_size = Self::estimate_entry_size(&tree, &source);
        self.total_bytes = self.total_bytes.saturating_sub(entry_size);
        Some((tree, source))
    }

    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(super) fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Estimate memory usage of a cache entry.
    ///
    /// PERF: Uses a source-length heuristic instead of recursively walking the
    /// entire green tree. The old approach traversed every node on every cache
    /// insert, which was O(n) in tree size. Empirically, green tree memory is
    /// ~4x the source text size (each character becomes a token with pointer +
    /// kind + text slice overhead, plus interior nodes).
    fn estimate_entry_size(_tree: &Arc<rowan::GreenNode>, source: &Arc<String>) -> usize {
        let source_size = source.len();
        // Green tree overhead: ~4x source size (conservative).
        // This avoids the O(n) recursive tree walk per cache insert.
        let tree_size = source_size.saturating_mul(4);
        source_size + tree_size
    }
}

#[derive(Debug, Clone)]
pub(super) struct ExternalSourceGroup {
    pub(super) priority: u32,
    pub(super) sources: Vec<RegisteredExternalSource>,
}

/// One exact declaration in the reverse FQN index.
///
/// The ordinal points into the source-owned `file_symbols` vector. This
/// upgrades the existing reverse map into an O(1)-by-FQN candidate index
/// without cloning one `Arc<Symbol>` per declaration. Whole-source removal and
/// relocation rebuild these ordinals, so they are never observed across a
/// vector compaction. The inline-one `SmallVec` keeps the common unambiguous
/// case allocation-free.
#[derive(Debug, Clone)]
pub(super) struct SourceSymbolCandidate {
    pub(super) file_id: FileId,
    pub(super) ordinal: u32,
}

/// Members of one owner, elected once per published source revision.
pub(super) struct MemberDefinitionsEntry {
    pub(super) file_id: FileId,
    pub(super) revision_id: NonZeroU64,
    pub(super) workspace_revision: WorkspaceRevision,
    pub(super) definitions: Arc<Vec<ResolvedDefinition>>,
}

pub(super) struct IndexInner {
    /// Owner FQN -> its elected member definitions (see
    /// `selected_owner_member_definitions_shared_from_owner`). Resolving every
    /// call of a 600-method class rebuilt this list per call (O(n^2)).
    pub(super) member_definitions_cache:
        parking_lot::Mutex<HashMap<String, MemberDefinitionsEntry>>,

    /// Workspace root path
    pub(super) root: PathBuf,

    /// Filesystem-aware workspace root used for containment decisions.
    pub(super) root_key: PathLookupKey,

    /// FQN → SymbolLocation (where symbols are DEFINED)
    /// BTreeMap enables O(log N) prefix scans via .range() for parameterized FQN matching
    pub(super) symbol_locations: BTreeMap<Arc<str>, SymbolLocation>,

    /// FQN → Vec<SymbolLocation> (where symbols are USED/REFERENCED)
    /// This enables cross-file find-references functionality
    /// BTreeMap enables O(log N) prefix scans via .range() for parameterized FQN matching
    pub(super) symbol_usages: BTreeMap<Arc<str>, Vec<SymbolLocation>>,

    /// FQN → Full Symbol data (includes return_type, modifiers, etc.)
    /// Wrapped in Arc to enable efficient cloning during completion
    pub(super) symbols: HashMap<Arc<str>, Arc<crate::Symbol>>,

    /// Simple name → Set<FQN>
    pub(super) simple_name_index: HashMap<Arc<str>, HashSet<Arc<str>>>,
    /// Lowercased simple name → Set<FQN> (prefix lookup, case-insensitive)
    pub(super) simple_name_lower_index: BTreeMap<String, HashSet<Arc<str>>>,

    /// Class FQN → Set<Constructor FQN>
    pub(super) constructor_index: HashMap<Arc<str>, HashSet<String>>,

    /// FileId → declarations contributed by this file or external source.
    ///
    /// `symbols`/`symbol_locations` contain only the elected declaration. This
    /// map is the canonical per-source store, including shadowed workspace and
    /// external declarations, so promotion and cache extraction retain exact
    /// metadata and ranges. Active entries share the same `Arc<Symbol>` with
    /// `symbols`; the common single-owner case therefore replaces the former
    /// fat `Arc<str>` file-list pointer with one thin `Arc<Symbol>` pointer.
    pub(super) file_symbols: HashMap<FileId, Vec<Arc<crate::Symbol>>>,

    /// Exact-publication generic arguments for nominal supertype edges.
    ///
    /// Keeping this beside `file_symbols`, rather than on every `Symbol`,
    /// avoids charging methods and fields for metadata owned only by the small
    /// subset of types with explicitly parameterized supertypes.
    pub(super) file_generic_supertypes:
        HashMap<FileId, HashMap<Arc<str>, Arc<[crate::GenericSupertype]>>>,

    /// FQN → every exact declaration, including shadowed definitions.
    ///
    /// `symbol_locations` intentionally selects one active declaration. This
    /// reverse candidate index serves both election and operations that must
    /// remain conservative across duplicate workspace definitions. Its
    /// candidates address the sole owning Arc in `file_symbols` by ordinal.
    pub(super) symbol_definition_files: HashMap<Arc<str>, SmallVec<[SourceSymbolCandidate; 1]>>,

    /// FileId → ImportMap
    pub(super) file_imports: HashMap<FileId, ImportMap>,

    /// M8 FIX: Track which FQNs each file depends on (for efficient reverse_dependencies cleanup)
    /// Maps FileId → Set of imported/referenced FQNs
    /// This enables O(m) cleanup instead of O(n) when removing a file
    /// Example: If File A imports "com.example.User", then:
    ///   file_imported_fqns[FileId(A)] = {"com.example.User"}
    /// When File A is removed, we only clean these specific entries in reverse_dependencies
    pub(super) file_imported_fqns: HashMap<FileId, HashSet<String>>,

    /// Track how many usages each file contributed to `symbol_usages`.
    ///
    /// Without this map, re-indexing the same file would append fresh usages without removing
    /// stale ones from previous revisions, causing unbounded memory growth.
    pub(super) file_usage_counts: HashMap<FileId, HashMap<Arc<str>, u32>>,

    /// A bounded Bloom filter of ASCII identifier spellings in each exact
    /// source publication. False positives only admit extra parsing; absent
    /// bits prove that Java source cannot mention a target. Files containing
    /// Java Unicode escapes are omitted and take the conservative scan path.
    pub(super) file_lexical_identifiers: HashMap<FileId, Box<core::LexicalIdentifierBloom>>,

    /// FileId → resolved method-name ranges and their declaration FQNs.
    ///
    /// The reverse usage index is optimized for find-references. This compact
    /// forward view lets semantic analysis consume the indexer's exact
    /// overload/inheritance decision without scanning every workspace use of
    /// a common method.
    pub(super) file_method_usages: HashMap<FileId, PublishedResolvedMethodUsages>,

    /// FileId → display spelling and immutable filesystem identity.
    pub(super) paths: PathRegistry,

    /// Next FileId to assign
    pub(super) next_file_id: FileId,

    /// File content cache (FileId → file contents)
    /// LRU cache with automatic eviction to avoid unbounded memory growth
    /// M14 FIX: Use Arc<String> to avoid cloning 100KB+ strings while holding write lock
    /// Arc::clone() is just a refcount increment (cheap, lock-free)
    pub(super) file_content_cache: LruCache<FileId, Arc<String>>,

    /// H3: Parse tree cache for incremental re-parsing with size-based eviction
    /// Maps FileId → (GreenNode, source_text)
    /// Size-aware eviction with max 50MB to prevent memory spikes
    /// Enables incremental re-parsing: only re-parse changed regions instead of full file
    /// Expected memory usage: Bounded at 50MB regardless of file sizes
    pub(super) parse_tree_cache: ParseTreeCache,

    /// Reverse dependency tracking: import key → HashSet<FileId>
    /// Maps each explicit/static/wildcard import to the files that declare it.
    /// Keys deliberately survive while their target declaration is absent;
    /// they are removed when the importing file changes or disappears.
    /// This enables transitive cache invalidation when a file changes
    /// Example: If File A imports "com.example.User", then:
    ///   reverse_dependencies["com.example.User"] = {FileId(A)}
    /// Uses HashSet for O(1) deduplication instead of O(n) Vec::contains
    pub(super) reverse_dependencies: HashMap<Arc<str>, HashSet<FileId>>,

    /// Package index for O(1) wildcard import matching
    /// Maps package names to files that have wildcard imports for that package
    /// Example: If File A has "import com.example.*", then:
    ///   package_index["com.example"] = {FileId(A)}
    /// This converts O(depth) string splitting per lookup to O(1) HashMap access
    pub(super) package_index: HashMap<Arc<str>, HashSet<FileId>>,

    /// Track which FileIds correspond to external files (JARs, JMODs, JDK)
    /// This enables get_external_symbols() to identify external symbols for caching
    /// without using FileId(0) for all external files (which causes HashMap collisions)
    pub(super) external_file_ids: HashSet<FileId>,

    /// Exact external source identities (archives and virtual sources) to FileIds.
    /// Prevents reusing the same FileId for distinct filesystem paths whose
    /// user-facing strings happen to be equal.
    pub(super) external_source_ids: HashMap<ExternalSourceIdentity, FileId>,

    /// Logical classpath owner → external source identifiers.
    ///
    /// Build-system refreshes replace one owner at a time. A source is removed
    /// only when no owner retains it, so a JAR shared by multiple classpaths
    /// remains available until its final owner releases it.
    pub(super) external_source_groups: HashMap<Arc<str>, ExternalSourceGroup>,

    /// Shared latest/committed authority for each logical classpath owner.
    pub(super) external_source_owner_authorities:
        HashMap<Arc<str>, Arc<ExternalSourceRefreshAuthority>>,

    /// Monotonic owner-refresh generation, shared across all owners.
    pub(super) next_external_source_owner_generation: NonZeroU64,

    /// External FileId → effective `(group priority, classpath position,
    /// owner)` rank. Declaration authority uses this rank rather than FileId
    /// allocation order, which remains stable across classpath refreshes.
    pub(super) external_source_precedence: HashMap<FileId, (u32, usize, Arc<str>)>,

    /// Reverse mapping: FileId → exact identity plus its selected display spelling.
    /// Enables hover tooltips to show specific source locations without making
    /// their necessarily lossy display strings part of archive identity.
    pub(super) external_file_id_to_source: HashMap<FileId, RegisteredExternalSource>,

    /// Most recently reserved extraction generation for each live external
    /// source. Publication requires an exact match.
    pub(super) external_source_generations: HashMap<FileId, NonZeroU64>,

    /// Generation which owns the declarations currently visible for each
    /// external file. This advances only when symbols are published, not when
    /// replacement extraction is reserved, so a failed replacement keeps its
    /// last-good declarations paired with their last-good source provider.
    pub(super) published_external_source_generations: HashMap<FileId, NonZeroU64>,

    /// Exact generation of a first publication that has exposed one or more
    /// owner-complete chunks but has not completed. A newer reservation uses
    /// this marker to retract that partial candidate set before invalidating
    /// the old publisher. Completed publications are deliberately unmarked so
    /// they remain visible while replacement extraction runs.
    pub(super) external_source_publications_in_progress: HashMap<FileId, NonZeroU64>,

    /// Monotonic external extraction generation. This is independent of
    /// workspace/file revisions because reservation itself publishes no data.
    pub(super) next_external_source_generation: NonZeroU64,

    /// Counter for external FileIds (counts down from u32::MAX)
    /// Workspace files use IDs starting from 1 and counting up
    /// External files use IDs starting from u32::MAX and counting down
    /// This prevents collisions between workspace and external files
    pub(super) next_external_id: u32,

    /// Exact identity, bytes, and authority of every source publication.
    pub(super) published_revisions: HashMap<FileId, PublishedRevision>,

    /// Next publication ID. IDs are monotonic and never reused by this index.
    pub(super) next_revision_id: NonZeroU64,

    /// Coherent generation of all state used by contextual type resolution.
    pub(super) workspace_revision: WorkspaceRevision,

    /// Most recent workspace revision produced by a *rebinding* advance.
    ///
    /// Most publications can retract or replace declarations, so a reader that
    /// observed an older generation may have combined facts that never coexisted.
    /// Appending owner-complete external declarations to a source that is being
    /// published for the first time cannot do that: every intermediate state is
    /// itself a state some legal chunk ordering produces, and no previously
    /// visible declaration disappears. Those advances leave this marker behind,
    /// so an interactive reader holding revision `r` remains coherent while a
    /// 350k-symbol JDK restore publishes hundreds of chunks underneath it.
    pub(super) last_rebinding_revision: WorkspaceRevision,

    /// Count of elections that displaced an already visible declaration.
    ///
    /// Append-only classification is a *claim* about a publication's inputs;
    /// only election knows the outcome. A classpath can rank a later external
    /// source above an earlier one (two JARs shipping `javax.annotation.*`, or
    /// a JAR outranking the JDK), and then registering "new" candidates
    /// replaces a declaration readers can already see. Every such displacement
    /// passes through `remove_symbol_for_replacement`, which bumps this
    /// counter, so an append-only pass can compare it across its own election
    /// and promote itself to rebinding before releasing the write lock.
    pub(super) displaced_elections: u64,

    /// Reverse inheritance graph over raw source spellings and exact bytecode
    /// FQNs. Queries validate contextual source edges through the canonical
    /// resolver before exposing them.
    ///
    /// Ordinary subclasses, interface implementations, and sealed subclasses
    /// are query-time views over this one graph, so indexing and removal paths
    /// cannot leave those views out of sync.
    pub(in crate::index) inheritance: inheritance::InheritanceIndex,

    /// H9: Nested class index: outer class FQN → Set<nested class FQN>
    /// Maps each class to its nested classes for O(1) lookup
    /// Example: "com.example.User" → {"com.example.User.Builder", "com.example.User.Address"}
    /// This converts O(n) iteration over all symbols to O(1) HashMap access
    /// Improves reverse dependency tracking from O(n²) to O(n)
    pub(super) nested_class_index: HashMap<Arc<str>, HashSet<String>>,

    /// Package → top-level class FQNs index for O(1) wildcard import nested class tracking
    /// Maps package names to all top-level classes defined in that package
    /// Example: "com.example" → {"com.example.User", "com.example.Order"}
    /// Used to find nested classes within wildcard-imported packages without O(n) scan
    pub(super) package_to_classes: HashMap<Arc<str>, HashSet<String>>,

    /// Kotlin typealias FQN → the active facade symbols that declare it.
    ///
    /// The aliases themselves live on the facade's
    /// [`crate::KotlinSymbolMetadata::type_aliases`], so they are published,
    /// shadowed, cached and removed with that symbol. This index only finds the
    /// facade for an alias name; see `IndexInner::expand_kotlin_type_alias`.
    pub(super) kotlin_type_alias_owners: HashMap<Arc<str>, SmallVec<[Arc<str>; 1]>>,

    /// Declaration FQN → directly owned declaration FQNs.
    ///
    /// Nominal types own ordinary members and nested types; JVM-language
    /// callables and properties can also own local/anonymous type declarations.
    /// Keeping the relation language-neutral supports coherent source election
    /// while retaining O(1) member lookup for completion and navigation.
    pub(super) owned_declarations_index: HashMap<Arc<str>, HashSet<Arc<str>>>,

    /// Pre-lowercased FQN index: lowercase(FQN) → original FQN
    /// Avoids per-call to_lowercase() in workspace symbol search (search()).
    /// BTreeMap enables O(log N) prefix scan for case-insensitive prefix matching.
    pub(super) fqn_lower_index: BTreeMap<String, SmallVec<[Arc<str>; 1]>>,
}

/// Compute a fast hash of file content for stale detection
///
/// Uses Rust's default hasher (SipHash-1-3) which is fast enough for our needs.
/// We don't need cryptographic security, just collision resistance for detecting changes.
pub(super) fn compute_content_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

impl WorkspaceIndex {
    /// Clone the index with one immutable owner scope; all index storage stays
    /// shared. Request-local queries must not fill FQN-only resolution caches.
    #[must_use]
    pub fn with_source_scope(&self, scope: SourceScope) -> Self {
        let mut view = self.clone();
        view.source_scope = Some(scope);
        view.source_scope_ticket = None;
        view.source_scope_ambiguous = false;
        view
    }

    #[must_use]
    pub fn has_source_scope(&self) -> bool {
        self.source_scope.is_some()
    }

    /// Publication proof to retain with a result derived from this view.
    #[must_use]
    pub fn source_scope_refresh_ticket(&self) -> Option<ExternalSourceRefreshTicket> {
        self.source_scope_ticket.clone()
    }

    /// Reject results when a Gradle refresh superseded the view mid-request.
    #[must_use]
    pub fn source_scope_is_current(&self) -> bool {
        if self.source_scope_ambiguous {
            return false;
        }
        let Some(ticket) = &self.source_scope_ticket else {
            return true;
        };
        let publication = self.source_scope_publication.read();
        publication
            .as_ref()
            .is_some_and(|published| published.ticket == *ticket && ticket.is_committed())
    }

    /// Publish the Gradle source ordering with the exact external-source
    /// generation that supplied its classpath and indexed dependencies.
    pub fn publish_source_scopes_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        scopes: Vec<SourceScopeInput>,
    ) -> bool {
        if !ticket.is_latest() || !ticket.is_committed() {
            return false;
        }
        let entries = scopes
            .into_iter()
            .flat_map(|input| {
                let scope = SourceScope::with_external_archives(
                    input.visible_source_roots,
                    input.visible_external_archives,
                );
                input
                    .requester_roots
                    .into_iter()
                    .map(move |root| (path_lookup_key(&root), scope.clone()))
            })
            .collect::<Vec<_>>();
        let mut inner = self.write_inner();
        self.assert_exclusive_write();
        let mut publication = self.source_scope_publication.write();
        if !ticket.is_latest()
            || !ticket.is_committed()
            || !inner
                .external_source_owner_authorities
                .get(ticket.owner())
                .is_some_and(|authority| Arc::ptr_eq(authority, &ticket.authority))
        {
            return false;
        }
        *publication = Some(SourceScopePublication {
            ticket: ticket.clone(),
            entries: entries.into(),
        });
        inner.advance_workspace_revision();
        true
    }

    /// Retire only scopes published by this build owner after its empty
    /// external-source generation commits. Other build owners keep their maps.
    pub fn retire_source_scopes_for_refresh(&self, ticket: &ExternalSourceRefreshTicket) -> bool {
        if !ticket.is_latest() || !ticket.is_committed() {
            return false;
        }
        let mut inner = self.write_inner();
        self.assert_exclusive_write();
        let mut publication = self.source_scope_publication.write();
        if !ticket.is_latest()
            || !ticket.is_committed()
            || !inner
                .external_source_owner_authorities
                .get(ticket.owner())
                .is_some_and(|authority| Arc::ptr_eq(authority, &ticket.authority))
        {
            return false;
        }
        if publication
            .as_ref()
            .is_some_and(|published| published.ticket.has_same_authority(ticket))
        {
            *publication = None;
            inner.advance_workspace_revision();
        }
        true
    }

    /// Build a request view from the deepest published source root containing
    /// this file. A superseded Gradle generation cannot supply a scope.
    pub fn scoped_view_for_file(&self, file_id: FileId) -> Option<Self> {
        let path = self.inner.read().file_path_key(file_id)?.clone();
        let publication = self.source_scope_publication.read();
        let published = publication.as_ref()?;
        let mut matches = published
            .entries
            .iter()
            .filter(|(root, _)| path.starts_with(root.as_path()))
            .map(|(root, scope)| (root.components().count(), scope));
        let (mut depth, mut selected) = matches.next()?;
        let mut ambiguous = false;
        for (candidate_depth, candidate) in matches {
            if candidate_depth > depth {
                depth = candidate_depth;
                selected = candidate;
                ambiguous = false;
            } else if candidate_depth == depth && candidate != selected {
                ambiguous = true;
            }
        }
        let mut view = self.with_source_scope(selected.clone());
        view.source_scope_ticket = Some(published.ticket.clone());
        view.source_scope_ambiguous = ambiguous;
        Some(view)
    }

    /// Create a new index with capacity hints
    ///
    /// The `estimated_files` parameter is used to pre-allocate HashMaps, eliminating
    /// 14 reallocations per HashMap during indexing (0→1→2→4→8→...→8192 for 10K files).
    ///
    /// Capacity multipliers are based on empirical analysis of typical Java/Kotlin codebases:
    /// - 1 file ≈ 3 top-level declarations (class + nested classes/interfaces)
    /// - 1 class ≈ 10 symbols (methods + fields + nested types)
    /// - 1 file ≈ 5 imports (direct dependencies)
    ///
    /// All capacities include 1.33x safety margin to avoid rehashing at 75% load factor.
    ///
    /// If `estimated_files` is 0, this behaves like the old `HashMap::new()` behavior.
    pub fn new_with_capacity(root: PathBuf, estimated_files: usize) -> Self {
        let root_key = path_lookup_key(&root);
        // Capacity calculations (all use saturating_mul to prevent overflow)
        // Each multiplier is empirically tuned and includes ~33% overhead for load factor

        // Symbol storage: ~10 symbols per file (class + methods + fields)
        // Multiplied by 1.33 to account for 75% load factor threshold
        let estimated_symbols = match estimated_files.checked_mul(13) {
            Some(n) => n,
            None => {
                tracing::error!(
                    "L7: HashMap capacity overflow: estimated_files={} exceeds usize::MAX/13. \
                     Using saturating multiplication - HashMap will still work but may reallocate frequently.",
                    estimated_files
                );
                estimated_files.saturating_mul(13)
            }
        };

        // Top-level declarations: ~3 per file (class + nested classes)
        let estimated_locations = match estimated_files.checked_mul(4) {
            Some(n) => n,
            None => {
                tracing::error!(
                    "L7: HashMap capacity overflow: estimated_files={} exceeds usize::MAX/4. \
                     Using saturating multiplication.",
                    estimated_files
                );
                estimated_files.saturating_mul(4)
            }
        };

        // Import tracking: ~5 imports per file (dependencies)
        let estimated_imports = match estimated_files.checked_mul(7) {
            Some(n) => n,
            None => {
                tracing::error!(
                    "L7: HashMap capacity overflow: estimated_files={} exceeds usize::MAX/7. \
                     Using saturating multiplication.",
                    estimated_files
                );
                estimated_files.saturating_mul(7)
            }
        };

        // Constructors: ~50% of classes have explicit constructors
        let estimated_constructors = estimated_locations / 2;

        // External symbols: JDK + common libraries (constant, not file-dependent)
        let estimated_external = 100;

        Self {
            inner: Arc::new(RwLock::new(IndexInner {
                member_definitions_cache: parking_lot::Mutex::new(HashMap::new()),
                root,
                root_key,

                // Core symbol storage (FQN-based lookups)
                // BTreeMap: no with_capacity needed — ordered for O(log N) prefix scans
                symbol_locations: BTreeMap::new(),
                symbol_usages: BTreeMap::new(),
                symbols: HashMap::with_capacity(estimated_symbols),

                // Name-based indices
                simple_name_index: HashMap::with_capacity(estimated_locations),
                simple_name_lower_index: BTreeMap::new(), // BTreeMap doesn't support with_capacity
                constructor_index: HashMap::with_capacity(estimated_constructors),

                // Per-file tracking (1:1 with file count)
                file_symbols: HashMap::with_capacity(estimated_files),
                file_generic_supertypes: HashMap::new(),
                symbol_definition_files: HashMap::with_capacity(estimated_symbols),
                file_imports: HashMap::with_capacity(estimated_files),
                file_imported_fqns: HashMap::with_capacity(estimated_files),
                file_usage_counts: HashMap::with_capacity(estimated_files),
                file_lexical_identifiers: HashMap::with_capacity(estimated_files),
                file_method_usages: HashMap::with_capacity(estimated_files),
                paths: PathRegistry::with_capacity(estimated_files),
                published_revisions: HashMap::with_capacity(estimated_files),

                // Dependency tracking
                reverse_dependencies: HashMap::with_capacity(estimated_imports),
                package_index: HashMap::with_capacity(estimated_imports / 10), // ~10 files per package

                // External source tracking (JARs, JDK)
                external_file_ids: HashSet::with_capacity(estimated_external),
                external_source_ids: HashMap::with_capacity(10), // Few external sources
                external_source_groups: HashMap::with_capacity(4),
                external_source_owner_authorities: HashMap::with_capacity(4),
                external_source_precedence: HashMap::with_capacity(estimated_external),
                external_file_id_to_source: HashMap::with_capacity(10),
                external_source_generations: HashMap::with_capacity(estimated_external),
                published_external_source_generations: HashMap::with_capacity(estimated_external),
                // Exceptional/transient state: most indexes never have a
                // chunked first publication paused between writes.
                external_source_publications_in_progress: HashMap::new(),
                // Reverse inheritance graph. Most source files declare at most
                // one type with a superclass and a small number of interfaces.
                inheritance: inheritance::InheritanceIndex::with_capacity(estimated_files / 2),

                // H9: Nested class index for O(1) lookup
                // Estimate: ~10% of classes have nested classes
                nested_class_index: HashMap::with_capacity(estimated_files / 10),

                // Package → top-level classes for O(1) wildcard nested class lookup
                // Estimate: ~3 files per package (typical package size)
                package_to_classes: HashMap::with_capacity(estimated_files / 3),

                // Only file facades that declare typealiases contribute.
                kotlin_type_alias_owners: HashMap::new(),

                // Class → member FQNs for O(1) member lookup
                // Estimate: ~5 members per class
                owned_declarations_index: HashMap::with_capacity(estimated_files),

                // Pre-lowercased FQN index for workspace symbol search
                // BTreeMap: no with_capacity needed
                fqn_lower_index: BTreeMap::new(),

                // Non-HashMap fields
                next_file_id: FileId::new(1),
                next_revision_id: NonZeroU64::MIN,
                workspace_revision: WorkspaceRevision(NonZeroU64::MIN),
                last_rebinding_revision: WorkspaceRevision(NonZeroU64::MIN),
                displaced_elections: 0,
                file_content_cache: LruCache::new(FILE_CONTENT_CACHE_SIZE),
                parse_tree_cache: ParseTreeCache::new(50 * 1024 * 1024), // H3: 50MB limit
                next_external_id: u32::MAX,
                next_external_source_generation: NonZeroU64::MIN,
                next_external_source_owner_generation: NonZeroU64::MIN,
            })),
            source_scope: None,
            source_scope_ticket: None,
            source_scope_ambiguous: false,
            source_scope_publication: Arc::new(RwLock::new(None)),
            path_locks: Arc::new(DashMap::with_capacity(estimated_files)),
            file_locks: Arc::new(DashMap::with_capacity(estimated_files)),
            external_source_publication_gates: Arc::new(DashMap::with_capacity(4)),
            method_resolution_cache: Arc::new(Mutex::new(LruCache::new(
                METHOD_RESOLUTION_CACHE_SIZE,
            ))),
            resolution_cache_generation: Arc::new(AtomicU64::new(0)),
            inheritance_cache: Arc::new(Mutex::new(LruCache::new(INHERITANCE_CACHE_SIZE))),
            current_usage_cache: Arc::new(Mutex::new(CurrentUsageCache::new())),
            usage_stats: Arc::new(DashMap::new()),
            index_profile: Arc::new(IndexPhaseProfile::default()),
            full_build_lock: Arc::new(std::sync::Mutex::new(())),
            cold_build_phase: Arc::new(AtomicU8::new(0)),
            java_lang_names: Arc::new(RwLock::new(None)),
            exclusive_writes: Arc::new(ExclusiveWriteGate::default()),
        }
    }

    /// From now on (in debug builds) panic when authoritative index state is mutated while no
    /// [`ExclusiveIndexWrites`] guard is alive. Called once by the semantic host,
    /// whose write transaction is the single publisher of this index. Exact
    /// byte-cache refills do not publish state and remain available to readers.
    pub fn require_exclusive_writes(&self) {
        self.exclusive_writes
            .required
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Mark the calling write transaction as live. See [`Self::require_exclusive_writes`].
    pub fn begin_exclusive_writes(&self) -> ExclusiveIndexWrites {
        self.exclusive_writes
            .active
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        ExclusiveIndexWrites(Arc::clone(&self.exclusive_writes))
    }

    /// Gate authoritative index mutations through the semantic writer.
    /// Exact byte-cache refills in query.rs take the lock directly: they
    /// revalidate the publication and do not change any semantic state.
    #[inline]
    pub(super) fn write_inner(&self) -> parking_lot::RwLockWriteGuard<'_, IndexInner> {
        self.assert_exclusive_write();
        self.inner.write()
    }

    #[inline]
    fn assert_exclusive_write(&self) {
        debug_assert!(
            !self
                .exclusive_writes
                .required
                .load(std::sync::atomic::Ordering::Acquire)
                || self
                    .exclusive_writes
                    .active
                    .load(std::sync::atomic::Ordering::Acquire)
                    != 0,
            "lock order violation: the workspace index was mutated outside a semantic write transaction"
        );
    }

    /// Create a new empty index without capacity hints
    ///
    /// This is a convenience wrapper around `new_with_capacity(root, 0)`.
    /// Prefer `new_with_capacity()` when the file count is known for better performance.
    pub fn new(root: PathBuf) -> Self {
        Self::new_with_capacity(root, 0)
    }

    pub fn index_phase_snapshot(&self) -> IndexPhaseSnapshot {
        self.index_profile.snapshot()
    }

    /// Root used by workspace discovery and incremental subtree scans.
    pub fn root(&self) -> PathBuf {
        self.inner.read().root.clone()
    }

    /// Update the workspace root path
    pub fn set_root(&self, root: PathBuf) {
        let root_key = path_lookup_key(&root);
        let mut inner = self.write_inner();
        inner.root = root;
        inner.root_key = root_key;
    }

    /// Get index statistics for monitoring
    pub fn stats(&self) -> IndexStats {
        let inner = self.inner.read();
        IndexStats {
            files_scanned: inner.paths.len(),
            files_indexed: inner.file_symbols.len(),
            files_failed: 0,
            symbols_indexed: inner.symbol_locations.len(),
        }
    }

    /// Get total number of symbols in the index
    pub fn symbol_count(&self) -> usize {
        let inner = self.inner.read();
        inner.symbols.len()
    }

    /// Check if index has core JDK classes loaded
    pub fn has_core_jdk_classes(&self) -> bool {
        let inner = self.inner.read();
        inner.symbols.contains_key(&Arc::from("java.lang.Object"))
            && inner.symbols.contains_key(&Arc::from("java.lang.String"))
    }

    /// Get diagnostic information about index state
    pub fn get_index_diagnostic(&self) -> String {
        let inner = self.inner.read();
        format!(
            "Index: {} symbols, {} files, {} imports, {} cached files",
            inner.symbols.len(),
            inner.paths.len(),
            inner.file_imports.len(),
            inner.file_content_cache.len()
        )
    }
}

// ===== Additional trait implementations =====

impl hyperion_types::SymbolLookup for WorkspaceIndex {
    fn symbol_exists(&self, fqn: &str) -> bool {
        self.find_symbol(fqn).is_some()
    }
}

// ===== IndexInner helper methods =====

impl IndexInner {
    fn advance_workspace_revision(&mut self) -> WorkspaceRevision {
        let revision = self.advance_workspace_revision_additively();
        self.last_rebinding_revision = revision;
        revision
    }

    /// Advance the coherent generation for a publication that only *adds*
    /// declarations to a source being published for the first time.
    ///
    /// Cache keys and equality proofs still see a new generation, but readers
    /// that captured an earlier one are not invalidated: see
    /// [`IndexInner::last_rebinding_revision`].
    fn advance_workspace_revision_additively(&mut self) -> WorkspaceRevision {
        self.workspace_revision = WorkspaceRevision(
            NonZeroU64::new(
                self.workspace_revision
                    .0
                    .get()
                    .checked_add(1)
                    .expect("workspace hierarchy revision space exhausted"),
            )
            .expect("incrementing a non-zero workspace revision cannot produce zero"),
        );
        self.workspace_revision
    }

    /// Retire every generation older than `revision` after an election that was
    /// classified as append-only turned out to displace a visible declaration.
    ///
    /// The caller still holds the index write lock, so no reader can observe
    /// the optimistic classification.
    fn promote_revision_to_rebinding(&mut self, revision: WorkspaceRevision) {
        if self.last_rebinding_revision.0 < revision.0 {
            self.last_rebinding_revision = revision;
        }
    }

    /// Whether every workspace-wide binding decision an answer could have
    /// observed at `revision` is still valid.
    ///
    /// Exact equality is the strong form; a captured generation also stays
    /// valid while the only advances since have been additive external
    /// publication.
    pub(super) fn workspace_bindings_are_current(&self, revision: WorkspaceRevision) -> bool {
        revision.0 >= self.last_rebinding_revision.0 && revision.0 <= self.workspace_revision.0
    }

    fn allocate_revision(
        &mut self,
        file_id: FileId,
        content_hash: u64,
        origin: SourceRevisionOrigin,
    ) -> FileRevisionToken {
        let id = self.next_revision_id;
        self.next_revision_id = NonZeroU64::new(
            id.get()
                .checked_add(1)
                .expect("workspace publication revision space exhausted"),
        )
        .expect("incrementing a non-zero revision ID cannot produce zero");
        let revision = PublishedRevision {
            id,
            content_hash,
            origin,
        };
        self.published_revisions.insert(file_id, revision);
        revision.token(file_id)
    }

    fn revision_token(&self, file_id: FileId) -> Option<FileRevisionToken> {
        self.published_revisions
            .get(&file_id)
            .copied()
            .map(|revision| revision.token(file_id))
    }

    fn replace_file_method_usages<'a>(
        &mut self,
        file_id: FileId,
        usages: impl IntoIterator<Item = &'a SymbolLocation>,
    ) {
        let method_usages: Vec<_> = usages
            .into_iter()
            .filter(|usage| matches!(usage.kind, SymbolKind::METHOD | SymbolKind::FUNCTION))
            .filter(|usage| {
                self.symbols.get(usage.fqn.as_ref()).is_some_and(|symbol| {
                    matches!(symbol.kind, SymbolKind::METHOD | SymbolKind::FUNCTION)
                })
            })
            .map(|usage| (usage.range, Arc::clone(&usage.fqn)))
            .collect();
        if method_usages.is_empty() {
            self.file_method_usages.remove(&file_id);
        } else {
            let source_revision_id = self
                .revision_token(file_id)
                .expect("method usage publication requires a source revision")
                .revision_id();
            self.file_method_usages.insert(
                file_id,
                PublishedResolvedMethodUsages {
                    source_revision_id,
                    workspace_revision: self.workspace_revision,
                    usages: Arc::from(method_usages),
                },
            );
        }
    }

    fn replace_file_symbols(&mut self, file_id: FileId, symbols: Vec<Arc<crate::Symbol>>) {
        self.take_file_symbols(file_id);
        self.record_file_symbols(file_id, symbols);
    }

    fn replace_file_generic_supertypes(
        &mut self,
        file_id: FileId,
        metadata: Vec<crate::TypeGenericSupertypes>,
    ) {
        let by_type = metadata
            .into_iter()
            .filter(|metadata| !metadata.edges.is_empty())
            .map(|metadata| {
                (
                    Arc::<str>::from(metadata.type_fqn),
                    Arc::<[crate::GenericSupertype]>::from(metadata.edges),
                )
            })
            .collect::<HashMap<_, _>>();
        if by_type.is_empty() {
            self.file_generic_supertypes.remove(&file_id);
        } else {
            self.file_generic_supertypes.insert(file_id, by_type);
        }
    }

    fn take_file_generic_supertypes(
        &mut self,
        file_id: FileId,
    ) -> HashMap<Arc<str>, Arc<[crate::GenericSupertype]>> {
        self.file_generic_supertypes
            .remove(&file_id)
            .unwrap_or_default()
    }

    /// Append a source's exact declarations to both directions of the
    /// candidate registry without electing an active shape.
    ///
    /// This deliberately does not collapse equal textual FQNs: Java permits,
    /// for example, a nested type and a field with the same name. Election has
    /// a deterministic representative policy, but cache extraction and future
    /// typed-identity work must retain both declarations.
    fn record_file_symbols(
        &mut self,
        file_id: FileId,
        symbols: impl IntoIterator<Item = Arc<crate::Symbol>>,
    ) {
        let symbols = symbols.into_iter().collect::<Vec<_>>();
        let file_symbols = self.file_symbols.entry(file_id).or_default();
        file_symbols.reserve(symbols.len());
        for symbol in symbols {
            let fqn = Arc::clone(&symbol.fqn);
            let ordinal = u32::try_from(file_symbols.len())
                .expect("one source cannot contribute more than u32::MAX symbols");
            file_symbols.push(Arc::clone(&symbol));
            self.symbol_definition_files
                .entry(Arc::clone(&fqn))
                .or_default()
                .push(SourceSymbolCandidate { file_id, ordinal });
            if let Some(container) = &symbol.container {
                self.owned_declarations_index
                    .entry(Arc::clone(container))
                    .or_default()
                    .insert(fqn);
            }
        }
    }

    fn take_file_symbols(&mut self, file_id: FileId) -> Vec<Arc<crate::Symbol>> {
        let symbols = self.file_symbols.remove(&file_id).unwrap_or_default();
        for symbol in &symbols {
            let fqn = &symbol.fqn;
            let remove_key = self
                .symbol_definition_files
                .get_mut(fqn.as_ref())
                .is_some_and(|candidates| {
                    candidates.retain(|candidate| candidate.file_id != file_id);
                    candidates.is_empty()
                });
            if remove_key {
                self.symbol_definition_files.remove(fqn.as_ref());
            }
            if let Some(container) = &symbol.container {
                let still_declared_for_container = self
                    .symbol_definition_files
                    .get(fqn.as_ref())
                    .into_iter()
                    .flatten()
                    .any(|candidate| {
                        self.file_symbols
                            .get(&candidate.file_id)
                            .and_then(|symbols| symbols.get(candidate.ordinal as usize))
                            .is_some_and(|symbol| {
                                symbol.container.as_deref() == Some(container.as_ref())
                            })
                    });
                if !still_declared_for_container {
                    if let Some(members) = self.owned_declarations_index.get_mut(container.as_ref())
                    {
                        members.remove(fqn.as_ref());
                        if members.is_empty() {
                            self.owned_declarations_index.remove(container.as_ref());
                        }
                    }
                }
            }
        }
        symbols
    }

    fn file_id_for_key(&self, key: &PathLookupKey) -> Option<FileId> {
        self.paths.file_id_for_key(key)
    }

    fn registered_path(&self, file_id: FileId) -> Option<&RegisteredFilePath> {
        self.paths.get(file_id)
    }

    fn file_path(&self, file_id: FileId) -> Option<&Path> {
        self.registered_path(file_id)
            .map(RegisteredFilePath::display_path)
    }

    fn file_path_key(&self, file_id: FileId) -> Option<&PathLookupKey> {
        self.registered_path(file_id)
            .map(RegisteredFilePath::lookup_key)
    }

    /// Resolve an exact stored display spelling only when it has one owner.
    ///
    /// Display spelling is cleanup metadata, not filesystem identity. Callers
    /// must prove the path absent before using this fallback and revalidate that
    /// proof under every identity lock involved in the cleanup transaction.
    fn file_id_for_unique_display(&self, path: &Path) -> Option<FileId> {
        let mut matches = self.paths.iter().filter_map(|(&file_id, registered)| {
            (registered.display_path() == path).then_some(file_id)
        });
        let candidate = matches.next()?;
        matches.next().is_none().then_some(candidate)
    }

    /// Install both sides of the FileId/path bijection, replacing any obsolete
    /// path previously owned by this FileId.
    fn register_file_path(&mut self, file_id: FileId, path: PathBuf, key: PathLookupKey) {
        self.paths.bind(file_id, RegisteredFilePath::new(path, key));
    }

    /// Remove both sides of the FileId/path bijection after validating their
    /// exact inverse relationship.
    fn remove_file_path(&mut self, file_id: FileId) -> Option<PathLookupKey> {
        self.paths
            .remove(file_id)
            .map(|registered| registered.lookup_key().clone())
    }

    fn get_or_create_file_id(&mut self, path: PathBuf, key: PathLookupKey) -> FileId {
        if let Some(id) = self.file_id_for_key(&key) {
            return id;
        }

        let id = self.allocate_workspace_file_id();
        self.register_file_path(id, path, key);
        id
    }

    fn allocate_workspace_file_id(&mut self) -> FileId {
        loop {
            let id = self.next_file_id;
            self.next_file_id = FileId::new(
                self.next_file_id
                    .as_u32()
                    .checked_add(1)
                    .expect("workspace FileId space exhausted"),
            );

            // Editors and other callers may publish an independently allocated
            // FileId before the workspace allocator reaches it. Treat every
            // published identity as reserved: issuing it again would relocate
            // a live revision behind the caller's back and break cross-layer
            // state keyed by that stable FileId.
            if !self.paths.contains_file_id(id) && !self.external_file_ids.contains(&id) {
                return id;
            }
        }
    }

    /// Allocate unique FileId for external source (JAR, JMOD, JDK)
    ///
    /// External files use IDs from u32::MAX counting down to avoid collisions
    /// with workspace files (which count up from 1).
    ///
    /// # Arguments
    /// * `source` - Exact identity and retained display spelling of the source
    ///
    /// # Returns
    /// FileId for this source (reuses existing ID if already allocated)
    fn allocate_external_file_id(&mut self, source: RegisteredExternalSource) -> FileId {
        // Check if we already have an ID for this source
        if let Some(&existing_id) = self.external_source_ids.get(source.key()) {
            return existing_id;
        }

        // Allocate from the high range, but do not assume caller-owned
        // workspace identities came from our low-to-high allocator. Both ID
        // domains are public, so the collision check must be symmetric.
        let id = loop {
            let id = FileId::new(self.next_external_id);
            self.next_external_id = self
                .next_external_id
                .checked_sub(1)
                .expect("external FileId space exhausted");
            if !self.paths.contains_file_id(id) && !self.external_file_ids.contains(&id) {
                break id;
            }
        };

        // Track this allocation
        self.external_source_ids.insert(source.key().clone(), id);
        self.external_file_id_to_source.insert(id, source);
        self.external_file_ids.insert(id);

        id
    }

    fn allocate_external_virtual_file_id(&mut self, source_id: &str) -> FileId {
        self.allocate_external_file_id(RegisteredExternalSource::virtual_source(source_id))
    }

    #[cfg(test)]
    fn external_file_id_for_path(&self, path: &Path) -> Option<FileId> {
        self.external_source_ids
            .get(&ExternalSourceIdentity::for_legacy_path(path))
            .copied()
    }

    #[cfg(test)]
    fn external_virtual_file_id(&self, source_id: &str) -> Option<FileId> {
        self.external_source_ids
            .get(&ExternalSourceIdentity::for_virtual(source_id))
            .copied()
    }

    fn reserve_external_source_revision(
        &mut self,
        file_id: FileId,
    ) -> Option<ExternalSourceRevisionToken> {
        if !self.external_file_ids.contains(&file_id) {
            return None;
        }
        let generation = self.next_external_source_generation;
        self.next_external_source_generation = NonZeroU64::new(
            generation
                .get()
                .checked_add(1)
                .expect("external source generation space exhausted"),
        )
        .expect("incrementing a non-zero external source generation cannot produce zero");
        self.external_source_generations.insert(file_id, generation);
        Some(ExternalSourceRevisionToken {
            file_id,
            generation,
        })
    }

    fn external_source_revision_is_current(&self, revision: ExternalSourceRevisionToken) -> bool {
        self.external_file_ids.contains(&revision.file_id)
            && self
                .external_source_generations
                .get(&revision.file_id)
                .is_some_and(|generation| *generation == revision.generation)
    }

    fn external_source_revision_is_published(&self, revision: ExternalSourceRevisionToken) -> bool {
        self.external_source_revision_is_current(revision)
            && self
                .published_external_source_generations
                .get(&revision.file_id)
                .is_some_and(|generation| *generation == revision.generation)
    }

    fn mark_external_source_publication_in_progress(
        &mut self,
        revision: ExternalSourceRevisionToken,
    ) {
        debug_assert!(self.external_source_revision_is_current(revision));
        self.published_external_source_generations
            .insert(revision.file_id, revision.generation);
        let previous = self
            .external_source_publications_in_progress
            .insert(revision.file_id, revision.generation);
        debug_assert!(previous.is_none() || previous == Some(revision.generation));
    }

    fn finish_external_source_publication(&mut self, revision: ExternalSourceRevisionToken) {
        debug_assert!(self.external_source_revision_is_current(revision));
        self.published_external_source_generations
            .insert(revision.file_id, revision.generation);
        if self
            .external_source_publications_in_progress
            .get(&revision.file_id)
            .is_some_and(|generation| *generation == revision.generation)
        {
            self.external_source_publications_in_progress
                .remove(&revision.file_id);
        }
    }

    fn rebuild_external_source_precedence(&mut self) {
        self.external_source_precedence.clear();
        for (owner, group) in &self.external_source_groups {
            for (position, source) in group.sources.iter().enumerate() {
                let Some(file_id) = self.external_source_ids.get(source.key()).copied() else {
                    continue;
                };
                let candidate = (group.priority, position, Arc::clone(owner));
                if self
                    .external_source_precedence
                    .get(&file_id)
                    .is_none_or(|current| candidate < *current)
                {
                    self.external_source_precedence.insert(file_id, candidate);
                    self.external_file_id_to_source
                        .insert(file_id, source.clone());
                }
            }
        }
    }
}

// ===== Test module =====

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "invalidation_tests.rs"]
mod invalidation_tests;
