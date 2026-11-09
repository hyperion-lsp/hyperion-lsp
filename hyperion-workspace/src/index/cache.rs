// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cache management and reverse dependency tracking
//!
//! This module handles:
//! - Cache invalidation (clear_*_cache)
//! - Reverse dependencies (get_dependent_files, get_file_symbols)
//! - Content hashing and staleness detection
//! - Usage statistics tracking

use super::*;

struct CachedFileValidation {
    path: PathBuf,
    path_key: PathLookupKey,
    matching_revision: Option<PreparedCachedFile>,
}

struct PreparedCachedFile {
    content_hash: u64,
    validation_stamp: SourceValidationStamp,
    identifiers: Option<Box<super::core::LexicalIdentifierBloom>>,
    imports: ImportMap,
    generic_supertypes: Vec<crate::TypeGenericSupertypes>,
    symbols: Vec<Arc<crate::Symbol>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceValidationStamp {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    change_seconds: i64,
    #[cfg(unix)]
    change_nanoseconds: i64,
}

fn source_validation_stamp(path: &Path) -> std::io::Result<SourceValidationStamp> {
    let metadata = std::fs::metadata(path)?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    Ok(SourceValidationStamp {
        len: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(unix)]
        change_seconds: metadata.ctime(),
        #[cfg(unix)]
        change_nanoseconds: metadata.ctime_nsec(),
    })
}

fn prepare_cached_file(
    path: &Path,
    cached_file: crate::cache::CachedFile,
) -> Option<PreparedCachedFile> {
    let before_read = source_validation_stamp(path).ok()?;
    let content = crate::read_source_file(path).ok()?;
    let after_read = source_validation_stamp(path).ok()?;
    if before_read != after_read {
        return None;
    }

    let content_hash = compute_content_hash(&content);
    if content_hash != cached_file.content_hash
        || !crate::cache::cached_file_payload_is_valid(&cached_file, &content)
    {
        return None;
    }

    Some(PreparedCachedFile {
        content_hash,
        validation_stamp: after_read,
        identifiers: super::core::lexical_identifier_hashes(&content),
        imports: cached_file.imports,
        generic_supertypes: cached_file.generic_supertypes,
        symbols: cached_file
            .symbols
            .into_iter()
            .map(crate::cache::validated_cached_to_symbol_owned)
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .map(Arc::new)
            .collect(),
    })
}

pub(super) fn collect_dependent_files_from_inner(
    inner: &IndexInner,
    fqn: &str,
    dependents: &mut HashSet<FileId>,
) {
    collect_dependent_files_for_fqns_from_inner(inner, std::iter::once(fqn), dependents);
}

fn collect_dependency_key(
    inner: &IndexInner,
    dependency_key: &str,
    visited_keys: &mut HashSet<String>,
    dependents: &mut HashSet<FileId>,
) {
    if !visited_keys.insert(dependency_key.to_owned()) {
        return;
    }

    // Exact semantic bindings are the primary reverse edge. Unlike an import,
    // this covers same-package and fully-qualified references.
    if let Some(usages) = inner.symbol_usages.get(dependency_key) {
        dependents.extend(usages.iter().map(|usage| usage.file_id));
    }

    // Imports remain a conservative fallback for unresolved references and
    // for language extractors that do not yet publish every usage.
    if let Some(explicit_deps) = inner.reverse_dependencies.get(dependency_key) {
        dependents.extend(explicit_deps.iter().copied());
    }
}

/// Collect declaration-derived and conservative import dependents for one
/// coherent old or new declaration set.
///
/// Files often contain hundreds of members with the same owner. De-duplicating
/// lookup keys here avoids walking the owner's usage/import vectors once per
/// member while the publication write lock is held.
pub(super) fn collect_dependent_files_for_fqns_from_inner<'a>(
    inner: &IndexInner,
    fqns: impl IntoIterator<Item = &'a str>,
    dependents: &mut HashSet<FileId>,
) {
    let mut visited_keys = HashSet::new();
    let mut visited_packages = HashSet::new();

    for fqn in fqns {
        collect_dependency_key(inner, fqn, &mut visited_keys, dependents);

        // Source imports cannot name a JVM-style method descriptor. An
        // explicit `import static api.Utils.helper` therefore needs to match
        // every changed declaration such as `api.Utils.helper(String)`.
        let descriptorless = fqn
            .split_once('(')
            .map_or(fqn, |(descriptorless, _)| descriptorless);
        if descriptorless != fqn {
            collect_dependency_key(inner, descriptorless, &mut visited_keys, dependents);
        }

        // A declaration affects users of its owning type as well as users that
        // import members/nested types through `Owner.*`. Walk outward so
        // members of nested classes also reach importers of the top-level
        // declaration.
        let symbol = inner.symbols.get(fqn);
        let mut owner = symbol.and_then(|symbol| symbol.container.clone());
        let mut top_level_owner = None;
        let mut visited_owners = HashSet::new();
        while let Some(owner_fqn) = owner {
            if !visited_owners.insert(Arc::clone(&owner_fqn)) {
                break;
            }

            collect_dependency_key(inner, &owner_fqn, &mut visited_keys, dependents);
            collect_dependency_key(
                inner,
                &format!("{owner_fqn}.*"),
                &mut visited_keys,
                dependents,
            );
            top_level_owner = Some(Arc::clone(&owner_fqn));
            owner = inner
                .symbols
                .get(owner_fqn.as_ref())
                .and_then(|owner_symbol| owner_symbol.container.clone());
        }

        // A declaration's semantics also depends on the types it extends or
        // implements. The inheritance index is already a bounded reverse
        // graph, including one simple-name bucket for contextual source
        // spellings. Use its conservative view so both removal and missing ->
        // present rebinding invalidate the declaring files without scanning
        // the workspace.
        if symbol.is_some_and(|symbol| crate::is_type_declaration_kind(symbol.kind)) {
            for subtype_fqn in inner.potential_direct_subtypes(fqn) {
                if let Some(candidates) = inner.symbol_definition_files.get(subtype_fqn.as_ref()) {
                    dependents.extend(candidates.iter().map(|candidate| candidate.file_id));
                } else if let Some(location) = inner.symbol_locations.get(subtype_fqn.as_ref()) {
                    // Compatibility for restored legacy state that predates
                    // the complete source-candidate reverse index.
                    dependents.insert(location.file_id);
                }
            }
        }

        let package_subject = top_level_owner.as_deref().unwrap_or(descriptorless);
        if let Some(package) = package_subject.rsplit_once('.').map(|(package, _)| package) {
            if visited_packages.insert(package.to_owned()) {
                if let Some(wildcard_deps) = inner.package_index.get(package) {
                    dependents.extend(wildcard_deps.iter().copied());
                }
            }
        }
    }
}

impl WorkspaceIndex {
    /// Return the normalized path lock used to serialize disk reads, identity
    /// selection, and source publication. Callers that also take a file lock
    /// must always acquire this one first.
    pub fn get_path_lock(&self, path: &Path) -> Arc<std::sync::Mutex<()>> {
        self.establish_path_lock(path).1
    }

    /// Capture one path identity and return the lock for that exact key.
    ///
    /// Locks-held transactions must retain the returned key and pass it to the
    /// corresponding keyed publication method. Recomputing after disk I/O can
    /// select a different identity if a symlink is retargeted concurrently.
    pub fn establish_path_lock(&self, path: &Path) -> (PathLookupKey, Arc<std::sync::Mutex<()>>) {
        let key = path_lookup_key(path);
        let lock = self.get_path_lock_for_key(&key);
        (key, lock)
    }

    /// Return the serialization lock for an already-established path identity.
    pub fn get_path_lock_for_key(&self, key: &PathLookupKey) -> Arc<std::sync::Mutex<()>> {
        self.path_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(std::sync::Mutex::new(())))
            .clone()
    }

    /// Reclaim a serialization lock only when the map is its sole owner.
    ///
    /// `remove_if` holds the DashMap shard lock while checking the strong count,
    /// so a concurrent indexer cannot clone the old lock between the check and
    /// removal and then run alongside a newly-created lock for the same path.
    pub(super) fn remove_path_lock_if_unused(&self, path: &Path) {
        let key = path_lookup_key(path);
        self.remove_path_lock_key_if_unused(&key);
    }

    /// Reclaim a lock by the immutable identity captured at registration.
    pub(super) fn remove_path_lock_key_if_unused(&self, key: &PathLookupKey) {
        self.path_locks
            .remove_if(key, |_, lock| Arc::strong_count(lock) == 1);
    }

    /// Reclaim an idle path lock after a caller-owned multi-path transaction.
    ///
    /// The DashMap strong-count check makes this safe even for a published path:
    /// a concurrent publisher either already owns this lock or creates the next
    /// canonical lock after removal, never both.
    pub fn reclaim_path_lock_if_unused(&self, path: &Path) {
        self.remove_path_lock_if_unused(path);
    }

    /// Reclaim an idle lock by the exact key captured for its transaction.
    pub fn reclaim_path_lock_key_if_unused(&self, key: &PathLookupKey) {
        self.remove_path_lock_key_if_unused(key);
    }

    pub fn clear_file_cache(&self) {
        let mut inner = self.write_inner();
        let cache_size = inner.file_content_cache.len();
        inner.file_content_cache.clear();
        if cache_size > 0 {
            debug!("Cleared file content cache ({} entries)", cache_size);
        }
    }

    pub fn clear_inheritance_cache(&self) {
        self.resolution_cache_generation
            .fetch_add(1, Ordering::Release);
        let mut cache = self.inheritance_cache.lock();
        let count = cache.len();
        cache.clear();
        debug!("Cleared inheritance cache ({} entries)", count);
    }

    pub fn clear_method_resolution_cache(&self) {
        self.resolution_cache_generation
            .fetch_add(1, Ordering::Release);
        let mut cache = self.method_resolution_cache.lock();
        let count = cache.len();
        cache.clear();
        debug!("Cleared method resolution cache ({} entries)", count);
    }

    /// Invalidate both graph-derived caches as one index publication step.
    pub(super) fn clear_resolution_caches(&self) {
        self.resolution_cache_generation
            .fetch_add(1, Ordering::Release);

        let method_count = {
            let mut cache = self.method_resolution_cache.lock();
            let count = cache.len();
            cache.clear();
            count
        };
        let inheritance_count = {
            let mut cache = self.inheritance_cache.lock();
            let count = cache.len();
            cache.clear();
            count
        };
        debug!(
            "Cleared resolution caches ({} method entries, {} inheritance entries)",
            method_count, inheritance_count
        );
    }

    pub fn extract_cache_data(&self) -> HashMap<PathBuf, crate::cache::CachedFile> {
        use rayon::prelude::*;

        // Snapshot one coherent index revision while holding the read lock,
        // but never perform filesystem I/O under that monolithic lock.
        let candidates = {
            let inner = self.inner.read();
            let mut candidates = Vec::with_capacity(inner.paths.len());

            // The reverse registry contains normalized lookup keys. Filesystem
            // I/O and persisted cache entries must use the original display path.
            for (&file_id, registered) in inner.paths.iter() {
                if inner.external_file_ids.contains(&file_id) {
                    continue;
                }
                let path = registered.display_path();
                let Some(content_hash) = inner
                    .published_revisions
                    .get(&file_id)
                    .map(|revision| revision.content_hash)
                else {
                    continue;
                };
                let Some(file_symbols) = inner.file_symbols.get(&file_id) else {
                    continue;
                };

                let symbols = file_symbols
                    .iter()
                    .map(|symbol| crate::cache::symbol_to_cached(symbol, symbol.range.start))
                    .collect();
                let generic_supertypes = inner
                    .file_generic_supertypes
                    .get(&file_id)
                    .into_iter()
                    .flat_map(|by_type| by_type.iter())
                    .map(|(type_fqn, edges)| crate::TypeGenericSupertypes {
                        type_fqn: type_fqn.to_string(),
                        edges: edges.to_vec(),
                    })
                    .collect();
                let imports = inner
                    .file_imports
                    .get(&file_id)
                    .cloned()
                    .unwrap_or_default();
                candidates.push((
                    path.to_path_buf(),
                    content_hash,
                    symbols,
                    generic_supertypes,
                    imports,
                ));
            }
            candidates
        };

        candidates
            .into_par_iter()
            .filter_map(
                |(path, content_hash, symbols, generic_supertypes, imports)| {
                    let mtime = std::fs::metadata(&path).ok()?.modified().ok()?;
                    Some((
                        path,
                        crate::cache::CachedFile {
                            mtime,
                            content_hash,
                            symbols,
                            generic_supertypes,
                            imports,
                        },
                    ))
                },
            )
            .collect()
    }

    pub fn extract_usage_stats(&self) -> HashMap<String, u32> {
        self.usage_stats
            .iter()
            .filter_map(|entry| {
                (entry.interactions > 0).then(|| (entry.key().to_string(), entry.interactions))
            })
            .collect()
    }

    pub fn get_content_hash(&self, file_id: FileId) -> Option<u64> {
        let inner = self.inner.read();
        inner
            .published_revisions
            .get(&file_id)
            .map(|revision| revision.content_hash)
    }

    /// Current publication token, for compatibility consumers that begin with
    /// a FileId rather than a revisioned range query.
    pub fn current_file_revision(&self, file_id: FileId) -> Option<FileRevisionToken> {
        self.inner.read().revision_token(file_id)
    }

    pub fn get_revision_origin(&self, file_id: FileId) -> Option<SourceRevisionOrigin> {
        self.inner
            .read()
            .published_revisions
            .get(&file_id)
            .map(|revision| revision.origin)
    }

    /// Returns true if the file is already indexed with content matching the given text.
    /// Uses fast hash comparison (SipHash) to avoid re-indexing unchanged files on did_open.
    pub fn is_content_unchanged(&self, file_id: FileId, content: &str) -> bool {
        if let Some(stored_hash) = self.get_content_hash(file_id) {
            stored_hash == compute_content_hash(content)
        } else {
            false
        }
    }

    pub fn get_dependent_files(&self, fqn: &str) -> Vec<FileId> {
        let inner = self.inner.read();
        let mut dependents_set = HashSet::new();
        collect_dependent_files_from_inner(&inner, fqn, &mut dependents_set);
        dependents_set.into_iter().collect()
    }

    /// Return the per-file publication lock. When a path lock is also needed,
    /// acquire the path lock first to preserve the global lock order.
    pub fn get_file_lock(&self, file_id: FileId) -> Arc<std::sync::Mutex<()>> {
        self.file_locks
            .entry(file_id)
            .or_insert_with(|| Arc::new(std::sync::Mutex::new(())))
            .clone()
    }

    pub(super) fn remove_file_lock_if_unused(&self, file_id: FileId) {
        self.file_locks
            .remove_if(&file_id, |_, lock| Arc::strong_count(lock) == 1);
    }

    /// Remove lock entries allocated for an indexing attempt that failed before
    /// it published an identity. Existing path/FileId owners keep their locks.
    ///
    /// The caller must release its path and file guards, and its cloned lock
    /// handles, before calling this method; live handles deliberately prevent
    /// reclamation through the strong-count check.
    pub fn reclaim_unpublished_revision_locks(&self, path: &Path, file_id: FileId) {
        let path_key = path_lookup_key(path);
        self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
    }

    pub fn reclaim_unpublished_revision_locks_for_key(
        &self,
        path_key: &PathLookupKey,
        file_id: FileId,
    ) {
        let (path_is_registered, file_id_is_registered) = {
            let inner = self.inner.read();
            (
                inner.file_id_for_key(path_key).is_some(),
                inner.paths.contains_file_id(file_id),
            )
        };
        if !file_id_is_registered {
            self.remove_file_lock_if_unused(file_id);
        }
        if !path_is_registered {
            self.remove_path_lock_key_if_unused(path_key);
        }
    }

    pub fn get_file_symbols(&self, file_id: FileId) -> Vec<String> {
        let inner = self.inner.read();
        inner
            .file_symbols
            .get(&file_id)
            .map(|symbols| {
                symbols
                    .iter()
                    .map(|symbol| symbol.fqn.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return method references resolved by the workspace index for one file.
    /// Ranges identify the method-name token, and each FQN names the selected
    /// declaration after overload and inheritance resolution.
    pub fn get_file_resolved_method_usages(&self, file_id: FileId) -> ResolvedMethodUsages {
        self.inner
            .read()
            .file_method_usages
            .get(&file_id)
            .map(|published| Arc::clone(&published.usages))
            .unwrap_or_else(|| Arc::from([]))
    }

    /// Capture resolved method ranges and their source publication under one
    /// read lock so later content retrieval can reject a stale result.
    pub fn get_file_resolved_method_usages_revisioned(
        &self,
        file_id: FileId,
    ) -> Option<RevisionedResolvedMethodUsages> {
        let inner = self.inner.read();
        let published = inner.file_method_usages.get(&file_id)?;
        let revision = FileRevisionToken {
            file_id,
            revision_id: published.source_revision_id,
        };
        Some(RevisionedResolvedMethodUsages {
            revision,
            workspace_revision: published.workspace_revision,
            usages: Arc::clone(&published.usages),
        })
    }

    /// Revalidate exact resolved call bindings under one coherent index read.
    pub fn resolved_method_usages_are_current(
        &self,
        resolved: &RevisionedResolvedMethodUsages,
    ) -> bool {
        let inner = self.inner.read();
        inner.revision_token(resolved.revision.file_id()) == Some(resolved.revision)
            && inner.workspace_revision == resolved.workspace_revision
    }

    /// Re-resolve Java method calls from exact source against one coherent,
    /// current workspace view.
    ///
    /// Stored forward usages are indexing artifacts and may predate unrelated
    /// workspace publications. Re-extraction here is deliberate: overload and
    /// inherited-owner selection observe exactly the same graph revision that
    /// is attached to the installed semantic bindings.
    pub fn resolve_current_method_usages(
        &self,
        indexed: &crate::IndexedContent,
    ) -> Option<RevisionedResolvedMethodUsages> {
        let file_id = indexed.revision.file_id();
        let inner = self.inner.read();
        let published = inner.published_revisions.get(&file_id)?;
        let retained_source_matches = inner
            .file_content_cache
            .peek(&file_id)
            .is_some_and(|source| source.as_str() == indexed.content.as_str())
            || inner
                .parse_tree_cache
                .peek(&file_id)
                .is_some_and(|(_, source)| source.as_str() == indexed.content.as_str());
        if published.token(file_id) != indexed.revision
            || published.content_hash != indexed.content_hash
            || !retained_source_matches
            || inner.file_path(file_id) != Some(indexed.file_path.as_path())
            || inner.file_path(file_id).and_then(Language::from_path) != Some(Language::Java)
        {
            return None;
        }
        let imports = inner.file_imports.get(&file_id)?;
        let symbols = inner.file_symbols.get(&file_id)?;
        let revision = crate::import_resolver::RevisionSymbolOverlay::from_published_file(symbols);
        let syntax = inner
            .parse_tree_cache
            .peek(&file_id)
            .filter(|(_, source)| source.as_str() == indexed.content.as_str())
            .map(|(green, _)| hyperion_parser::SyntaxNode::new_root(green.as_ref().clone()))
            .unwrap_or_else(|| {
                let tokens = hyperion_parser::Lexer::new(indexed.content.as_str()).collect();
                let (green, _errors) = hyperion_parser::Parser::parse(tokens);
                hyperion_parser::SyntaxNode::new_root(green)
            });
        let usages =
            crate::usage_extractor::UsageExtractor::extract_from_syntax_for_coherent_index(
                &syntax, file_id, imports, &inner, &revision,
            )
            .into_iter()
            .filter(|usage| {
                matches!(
                    usage.location.kind,
                    lsp_types::SymbolKind::METHOD | lsp_types::SymbolKind::FUNCTION
                )
            })
            .map(|usage| (usage.location.range, Arc::<str>::from(usage.fqn)))
            .collect::<Vec<_>>();
        let batch = RevisionedResolvedMethodUsages {
            revision: indexed.revision,
            workspace_revision: inner.workspace_revision,
            usages: Arc::from(usages),
        };
        Some(batch)
    }

    /// Atomically validate every source, hierarchy batch, and optional call
    /// batch participating in one semantic hydration transaction.
    ///
    /// The caller must acquire its semantic DB mutex first. Holding this read
    /// guard through `install` prevents a request snapshot from ever combining
    /// facts resolved under different workspace generations.
    pub fn with_current_semantic_publication<R>(
        &self,
        workspace_revision: WorkspaceRevision,
        file_revisions: &[FileRevisionToken],
        indexed_sources: &[&crate::IndexedContent],
        hierarchies: &[&crate::ResolvedFileSupertypes],
        calls: Option<&RevisionedResolvedMethodUsages>,
        install: impl FnOnce() -> R,
    ) -> Option<R> {
        let inner = self.inner.read();
        if inner.workspace_revision != workspace_revision {
            return None;
        }
        if !file_revisions
            .iter()
            .all(|revision| inner.revision_token(revision.file_id()) == Some(*revision))
        {
            return None;
        }
        let sources_are_current = indexed_sources.iter().all(|indexed| {
            let file_id = indexed.revision.file_id();
            let Some(published) = inner.published_revisions.get(&file_id) else {
                return false;
            };
            // `IndexedContent` already owns bytes that were validated against
            // this token, hash, and path when read. Cache residency is not an
            // identity invariant: a large semantic batch can legitimately
            // evict its own early entries from the bounded source/parse caches.
            // Requiring every entry to remain resident made batches larger
            // than the caches impossible to publish and their callers retry
            // forever.
            published.token(file_id) == indexed.revision
                && published.content_hash == indexed.content_hash
                && inner.file_path(file_id) == Some(indexed.file_path.as_path())
        });
        if !sources_are_current {
            return None;
        }
        let hierarchies_are_current = hierarchies.iter().all(|resolved| {
            resolved.workspace_revision == workspace_revision
                && inner.revision_token(resolved.file_revision.file_id())
                    == Some(resolved.file_revision)
        });
        if !hierarchies_are_current {
            return None;
        }
        let calls_are_current = calls.is_none_or(|resolved| {
            resolved.workspace_revision == workspace_revision
                && inner.revision_token(resolved.revision.file_id()) == Some(resolved.revision)
        });
        calls_are_current.then(install)
    }

    /// Get all files that depend on ANY symbol defined in the given file.
    ///
    /// This is a convenience method that combines `get_file_symbols` with
    /// `get_dependent_files` to find all files that import symbols from
    /// the given file.
    ///
    /// # Use Case
    ///
    /// When a file changes, this method helps find all files that need
    /// to be re-indexed or have their diagnostics refreshed because they
    /// import symbols from the changed file.
    ///
    /// # Example
    ///
    /// ```text
    /// // File A defines: com.example.User, com.example.UserService
    /// // File B imports: com.example.User
    /// // File C imports: com.example.UserService
    /// // File D imports both
    ///
    /// get_dependent_files_for_file(FileA) -> [FileB, FileC, FileD]
    /// ```
    pub fn get_dependent_files_for_file(&self, file_id: FileId) -> Vec<FileId> {
        let inner = self.inner.read();
        let mut dependents = HashSet::new();
        let fqns = inner
            .file_symbols
            .get(&file_id)
            .into_iter()
            .flatten()
            .map(|symbol| symbol.fqn.as_ref());
        collect_dependent_files_for_fqns_from_inner(&inner, fqns, &mut dependents);
        dependents.remove(&file_id);
        let mut dependents = dependents.into_iter().collect::<Vec<_>>();
        dependents.sort_unstable_by_key(|dependent| dependent.as_u32());
        dependents
    }

    pub fn get_inheritance_chain(&self, class_fqn: &str) -> Vec<String> {
        let generation = self.resolution_cache_generation.load(Ordering::Acquire);
        // Check cache first
        {
            let mut cache = self.inheritance_cache.lock();
            if let Some(cached) = cache.get(class_fqn) {
                return cached.clone();
            }
        }

        // Compute the chain
        let chain = self.compute_inheritance_chain(class_fqn);

        // A concurrent publication may have invalidated after this lookup read
        // the old graph. Return that request's coherent result, but never put it
        // back into a cache owned by the newer generation.
        if self.resolution_cache_generation.load(Ordering::Acquire) == generation {
            let mut cache = self.inheritance_cache.lock();
            if self.resolution_cache_generation.load(Ordering::Acquire) == generation {
                cache.put(class_fqn.to_string(), chain.clone());
            }
        }

        chain
    }

    pub fn get_or_cache_method_resolution<F>(
        &self,
        receiver_fqn: &str,
        method_name: &str,
        lookup_fn: F,
    ) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        let generation = self.resolution_cache_generation.load(Ordering::Acquire);
        let cache_key = (receiver_fqn.to_string(), method_name.to_string());

        // Check cache first (get requires &mut for LRU bookkeeping)
        {
            let mut cache = self.method_resolution_cache.lock();
            if let Some(cached_result) = cache.get(&cache_key) {
                debug!(
                    "Method resolution cache hit: {}.{} -> {:?}",
                    receiver_fqn, method_name, cached_result
                );
                return cached_result.clone();
            }
        }

        // Cache miss - perform lookup
        debug!(
            "Method resolution cache miss: {}.{}, performing lookup",
            receiver_fqn, method_name
        );
        let result = lookup_fn();

        // Store both positive and negative results only if no publication raced
        // the lookup. Recheck under the cache lock to close the final window.
        if self.resolution_cache_generation.load(Ordering::Acquire) == generation {
            let mut cache = self.method_resolution_cache.lock();
            if self.resolution_cache_generation.load(Ordering::Acquire) == generation {
                cache.put(cache_key, result.clone());
            }
        }

        result
    }

    pub fn get_top_symbols(&self, n: usize) -> Vec<(String, u32)> {
        let mut entries: Vec<_> = self
            .usage_stats
            .iter()
            .map(|entry| (entry.key().to_string(), entry.value().total()))
            .collect();
        entries.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        entries.truncate(n);
        entries
    }

    pub fn get_usage_score(&self, fqn: &str) -> f64 {
        self.get_usage_scores(&[fqn])[0]
    }

    /// Return normalized usage scores for several symbols with one full scan of
    /// the usage table. Descriptor-less method names retain the legacy
    /// semantics: an exact entry wins; otherwise all overload entries are
    /// summed.
    pub fn get_usage_scores(&self, fqns: &[&str]) -> Vec<f64> {
        if fqns.is_empty() {
            return Vec::new();
        }

        let counts: Vec<u32> = fqns
            .iter()
            .map(|fqn| {
                self.usage_stats
                    .get(*fqn)
                    .map_or(0, |entry| entry.value().total())
            })
            .collect();
        let mut missing_by_base: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, fqn) in fqns.iter().enumerate() {
            if counts[index] == 0 {
                missing_by_base.entry(fqn).or_default().push(index);
            }
        }
        let mut overload_counts = vec![0_u32; fqns.len()];
        let mut max_usage = 0_u32;

        for entry in self.usage_stats.iter() {
            let value = entry.value().total();
            max_usage = max_usage.max(value);
            if let Some(open_paren) = entry.key().find('(') {
                let descriptorless = &entry.key()[..open_paren];
                if let Some(indices) = missing_by_base.get(descriptorless) {
                    for &index in indices {
                        overload_counts[index] = overload_counts[index].saturating_add(value);
                    }
                }
            }
        }

        let normalization_max = overload_counts.iter().copied().fold(max_usage, u32::max);
        if normalization_max == 0 {
            return vec![0.0; fqns.len()];
        }
        counts
            .iter()
            .enumerate()
            .map(|(index, count)| {
                let count = if *count > 0 {
                    *count
                } else {
                    overload_counts[index]
                };
                f64::from(count) / f64::from(normalization_max)
            })
            .collect()
    }

    pub fn is_content_stale(&self, file_id: FileId, current_content: &str) -> Option<bool> {
        let inner = self.inner.read();
        let stored_hash = inner.published_revisions.get(&file_id)?.content_hash;
        let current_hash = compute_content_hash(current_content);
        Some(stored_hash != current_hash)
    }

    pub fn load_usage_stats(&self, stats: HashMap<String, u32>) {
        for mut entry in self.usage_stats.iter_mut() {
            entry.interactions = 0;
        }
        self.usage_stats
            .retain(|_, counts| counts.indexed > 0 || counts.interactions > 0);
        for (fqn, count) in stats {
            if count > 0 {
                self.usage_stats
                    .entry(Arc::from(fqn))
                    .and_modify(|counts| counts.interactions = count)
                    .or_insert(UsageCounts {
                        indexed: 0,
                        interactions: count,
                    });
            }
        }
    }

    pub(super) fn replace_indexed_usage_counts(
        &self,
        old_counts: &HashMap<Arc<str>, u32>,
        new_counts: &HashMap<Arc<str>, u32>,
    ) {
        for (fqn, old_count) in old_counts {
            let new_count = new_counts.get(fqn).copied().unwrap_or(0);
            if *old_count > new_count {
                let decrement = *old_count - new_count;
                if let Some(mut counts) = self.usage_stats.get_mut(fqn.as_ref()) {
                    counts.indexed = counts.indexed.saturating_sub(decrement);
                }
                self.usage_stats
                    .remove_if(fqn.as_ref(), |_, counts| counts.total() == 0);
            }
        }
        for (fqn, new_count) in new_counts {
            let old_count = old_counts.get(fqn).copied().unwrap_or(0);
            if *new_count > old_count {
                let increment = *new_count - old_count;
                self.usage_stats
                    .entry(Arc::clone(fqn))
                    .and_modify(|counts| counts.indexed = counts.indexed.saturating_add(increment))
                    .or_insert(UsageCounts {
                        indexed: increment,
                        interactions: 0,
                    });
            }
        }
    }

    pub fn record_symbol_usage(&self, fqn: &str) {
        self.usage_stats
            .entry(Arc::from(fqn))
            .and_modify(|counts| counts.interactions = counts.interactions.saturating_add(1))
            .or_insert(UsageCounts {
                indexed: 0,
                interactions: 1,
            });
    }

    /// Restore multiple files from cache in parallel (much faster than restore_from_cache per-file)
    ///
    /// CRITICAL FIX: Computes content hashes in parallel for skip-cached optimization.
    /// When Gradle integration runs index_files_parallel_skip_cached(), it checks:
    ///   1. get_file_id(path) - will succeed (we create it below)
    ///   2. get_content_hash(file_id) - MUST succeed or skip fails
    ///
    /// Without hashes, all 2874 files get re-indexed on every warm start (41+ seconds).
    /// With parallel hash computation: ~1 second for 2874 files, saving 40 seconds.
    /// Restore entries whose current source content matches the revision that
    /// produced the cached declarations.
    ///
    /// Returns paths that could not be read or whose content no longer
    /// matches. Callers must reindex those paths before declaring the
    /// workspace ready.
    pub fn restore_from_cache_batch(
        &self,
        entries: &HashMap<PathBuf, crate::cache::CachedFile>,
    ) -> Vec<PathBuf> {
        self.restore_from_cache_batch_with_revisions(entries).0
    }

    /// Owned counterpart to [`Self::restore_from_cache_batch`]. Prefer this at
    /// startup after consuming [`crate::CachedIndex`]: cached strings and
    /// vectors move into the live graph instead of being cloned wholesale.
    pub fn restore_from_cache_batch_owned(
        &self,
        entries: HashMap<PathBuf, crate::cache::CachedFile>,
    ) -> Vec<PathBuf> {
        self.restore_from_cache_batch_owned_with_revisions(entries)
            .0
    }

    /// Restore a cache batch and return the exact disk publications installed by
    /// this transaction alongside rejected paths.
    ///
    /// The revision tokens let startup reconcile an authoritative fresh source
    /// manifest without deleting an editor or build-system publication that won
    /// a race after cache restoration completed.
    pub fn restore_from_cache_batch_with_revisions(
        &self,
        entries: &HashMap<PathBuf, crate::cache::CachedFile>,
    ) -> (Vec<PathBuf>, Vec<RestoredFileRevision>) {
        self.restore_from_cache_batch_owned_with_revisions(entries.clone())
    }

    /// Consume a cache batch and return the exact publications installed by one
    /// atomic index transaction.
    pub fn restore_from_cache_batch_owned_with_revisions(
        &self,
        entries: HashMap<PathBuf, crate::cache::CachedFile>,
    ) -> (Vec<PathBuf>, Vec<RestoredFileRevision>) {
        self.restore_from_cache_batch_owned_with_revisions_after_validation(
            entries,
            || {},
            |install| install(),
        )
    }

    /// [`Self::restore_from_cache_batch_owned_with_revisions`] where the
    /// installing phase (validation is long and lock-free; installation is one
    /// short write-locked step) runs inside `publish_scope`. A host that
    /// serializes index writers passes a scope that opens its write transaction
    /// around `install` so validation never blocks other writers or readers.
    pub fn restore_from_cache_batch_owned_with_revisions_scoped(
        &self,
        entries: HashMap<PathBuf, crate::cache::CachedFile>,
        publish_scope: impl FnOnce(&mut dyn FnMut()),
    ) -> (Vec<PathBuf>, Vec<RestoredFileRevision>) {
        self.restore_from_cache_batch_owned_with_revisions_after_validation(
            entries,
            || {},
            publish_scope,
        )
    }

    #[cfg(test)]
    pub(crate) fn restore_from_cache_batch_after_validation_for_test(
        &self,
        entries: &HashMap<PathBuf, crate::cache::CachedFile>,
        after_validation: impl FnOnce(),
    ) -> (Vec<PathBuf>, Vec<RestoredFileRevision>) {
        self.restore_from_cache_batch_owned_with_revisions_after_validation(
            entries.clone(),
            after_validation,
            |install| install(),
        )
    }

    fn restore_from_cache_batch_owned_with_revisions_after_validation(
        &self,
        entries: HashMap<PathBuf, crate::cache::CachedFile>,
        after_validation: impl FnOnce(),
        publish_scope: impl FnOnce(&mut dyn FnMut()),
    ) -> (Vec<PathBuf>, Vec<RestoredFileRevision>) {
        use rayon::prelude::*;

        if entries.is_empty() {
            return (Vec::new(), Vec::new());
        }

        info!(
            "Computing content hashes for {} cached files in parallel...",
            entries.len()
        );

        let restore_started = std::time::Instant::now();
        let hash_started = std::time::Instant::now();

        // Phase 1: Compute hashes in parallel (NO LOCK - pure I/O)
        let validation: Vec<CachedFileValidation> = entries
            .into_par_iter()
            .map(|(path, cached_file)| {
                let path_key = path_lookup_key(&path);
                let matching_revision = prepare_cached_file(&path, cached_file);
                let matching_revision = (path_lookup_key(&path) == path_key)
                    .then_some(matching_revision)
                    .flatten();
                CachedFileValidation {
                    path,
                    path_key,
                    matching_revision,
                }
            })
            .collect();
        after_validation();

        let mut validated = Vec::with_capacity(validation.len());
        let mut rejected_paths = Vec::new();
        for validation in validation {
            if validation.matching_revision.is_some() {
                validated.push(validation);
            } else {
                rejected_paths.push(validation.path);
            }
        }

        // A cache is keyed by display paths, but aliases may converge to one
        // filesystem identity. Restoring an arbitrary HashMap entry would make
        // declarations depend on randomized iteration order. Fail closed for
        // every ambiguous identity so the source scanner rebuilds it instead.
        validated.sort_unstable_by(|left, right| {
            left.path_key
                .cmp(&right.path_key)
                .then_with(|| left.path.cmp(&right.path))
        });
        let mut unambiguous = Vec::with_capacity(validated.len());
        let mut validated = validated.into_iter().peekable();
        while let Some(first) = validated.next() {
            let mut same_identity = vec![first];
            while validated
                .peek()
                .is_some_and(|next| next.path_key == same_identity[0].path_key)
            {
                same_identity.push(
                    validated
                        .next()
                        .expect("peeked cache validation remains available"),
                );
            }
            if same_identity.len() == 1 {
                unambiguous.push(
                    same_identity
                        .pop()
                        .expect("one unambiguous validation remains"),
                );
            } else {
                rejected_paths.extend(same_identity.into_iter().map(|validation| validation.path));
            }
        }

        let hash_elapsed = hash_started.elapsed();
        let rebuild_started = std::time::Instant::now();
        let mut restored_count = 0;
        let mut restored_revisions = Vec::new();

        // Phase 2: Restore absent entries (single write lock, run inside the
        // caller's `publish_scope` so a host can make it one write transaction). Cache restore is
        // startup seeding, never a replacement mechanism: an editor or watcher
        // may have published a newer in-memory revision while validation ran.
        // Preserving every pre-existing path makes that publication authoritative
        // without holding thousands of path locks across parallel disk reads.
        publish_scope(&mut || {
            // Validation may race a source write or symlink retarget after the
            // source was read. Recheck its cheap filesystem stamp and identity at
            // the publication boundary; either change is a cache miss.
            unambiguous.retain(|validation| {
                let still_matches = validation
                    .matching_revision
                    .as_ref()
                    .is_some_and(|prepared| {
                        path_lookup_key(&validation.path) == validation.path_key
                            && source_validation_stamp(&validation.path).ok()
                                == Some(prepared.validation_stamp)
                    });
                if still_matches {
                    true
                } else {
                    rejected_paths.push(validation.path.clone());
                    false
                }
            });
            rejected_paths.sort_unstable();
            rejected_paths.dedup();
            {
                let mut inner = self.write_inner();
                let mut affected_fqns = Vec::new();
                let preexisting_keys: HashSet<PathLookupKey> = unambiguous
                    .iter()
                    .filter(|validation| inner.file_id_for_key(&validation.path_key).is_some())
                    .map(|validation| validation.path_key.clone())
                    .collect();

                // Pre-allocate FileIds and store hashes
                for validation in &mut unambiguous {
                    if preexisting_keys.contains(&validation.path_key) {
                        continue;
                    }
                    let prepared = validation
                        .matching_revision
                        .as_mut()
                        .expect("unambiguous cache entries passed content validation");
                    let file_id = inner.get_or_create_file_id(
                        validation.path.clone(),
                        validation.path_key.clone(),
                    );
                    let revision = inner.allocate_revision(
                        file_id,
                        prepared.content_hash,
                        SourceRevisionOrigin::Disk,
                    );
                    let registered_path = inner
                        .registered_path(file_id)
                        .expect("new cache revision must retain its path registration")
                        .clone();
                    restored_revisions.push(RestoredFileRevision {
                        registered_path,
                        revision,
                    });
                    if let Some(identifiers) = prepared.identifiers.take() {
                        inner.file_lexical_identifiers.insert(file_id, identifiers);
                    }
                }

                // Restore declarations and imports for every validated file before
                // rebuilding any resolved call bindings. A caller may sort before
                // the file that declares its selected method in the cache map; the
                // binding validator must observe the complete restored symbol set.
                let mut affected_symbols = Vec::new();
                for validation in &mut unambiguous {
                    if preexisting_keys.contains(&validation.path_key) {
                        debug!(
                            "Skipping cache entry already published in memory: {:?}",
                            validation.path
                        );
                        continue;
                    }
                    let Some(file_id) = inner.file_id_for_key(&validation.path_key) else {
                        continue;
                    };
                    let prepared = validation
                        .matching_revision
                        .as_mut()
                        .expect("every validated path retained its prepared payload");

                    // Persistent entries carry declarations and a content hash,
                    // but not the source buffer or parse tree. Never leave those
                    // products from a previous in-memory revision paired with the
                    // restored symbols. The next content request will read the
                    // validated disk revision and populate the bounded cache.
                    inner.file_content_cache.pop(&file_id);
                    inner.parse_tree_cache.remove(&file_id);
                    let removed = super::core::unregister_source_symbols_without_election(
                        &mut inner, file_id,
                    );
                    affected_fqns.extend(removed.iter().map(|symbol| Arc::clone(&symbol.fqn)));
                    affected_symbols.extend(removed);
                    // Structural insertion needs the declaration's imports in
                    // place to rebuild contextual-supertype candidates.
                    let imports = std::mem::take(&mut prepared.imports);
                    super::core::process_imports_for_file(&mut inner, file_id, &imports);
                    inner.replace_file_generic_supertypes(
                        file_id,
                        std::mem::take(&mut prepared.generic_supertypes),
                    );

                    // Restore symbols. String-to-Arc conversion and payload
                    // validation happened before the publication lock.
                    for symbol in &prepared.symbols {
                        affected_fqns.push(Arc::clone(&symbol.fqn));
                    }

                    affected_symbols.extend(super::core::register_source_symbols_without_election(
                        &mut inner,
                        std::mem::take(&mut prepared.symbols),
                        file_id,
                    ));
                }
                // Classes written against a typealias either revision declares
                // are reached through the alias names.
                affected_fqns.extend(super::inheritance::declared_kotlin_type_alias_fqns(
                    affected_symbols.iter().map(AsRef::as_ref),
                ));
                // Elect only after every validated source candidate is registered,
                // making duplicate ownership independent of cache HashMap order.
                super::core::reelect_affected_symbols(&mut inner, affected_symbols);

                // Cached resolved FQNs are intentionally not restored. They were
                // selected under an earlier workspace graph and cannot be made
                // current by attaching today's file token. Exact usage queries
                // re-extract from the validated bytes after the complete graph is
                // visible.

                // Publish the rebuilt graph and its cache generation together.
                self.invalidate_caches_for_fqns_in_state(&inner, &affected_fqns);
                restored_count = unambiguous.len().saturating_sub(preexisting_keys.len());
            } // Lock dropped
        });
        let rebuild_elapsed = rebuild_started.elapsed();

        info!(
            "Restored {} files from cache in {:.2}s (content read/hash {:.2}s, index rebuild {:.2}s)",
            restored_count,
            restore_started.elapsed().as_secs_f64(),
            hash_elapsed.as_secs_f64(),
            rebuild_elapsed.as_secs_f64(),
        );

        (rejected_paths, restored_revisions)
    }

    pub fn restore_from_cache(&self, path: &Path, cached_file: &crate::cache::CachedFile) {
        self.restore_from_cache_after_read(path, cached_file, || {});
    }

    fn restore_from_cache_after_read(
        &self,
        path: &Path,
        cached_file: &crate::cache::CachedFile,
        after_read: impl FnOnce(),
    ) {
        // CRITICAL FIX: Compute content hash BEFORE acquiring write lock
        // When Gradle integration runs index_files_parallel_skip_cached(), it checks:
        //   1. get_file_id(path) - will succeed (we're creating it below)
        //   2. get_content_hash(file_id) - MUST succeed or skip fails
        //
        // Without this, all 2874 files get re-indexed on every warm start (41+ seconds).
        // Reading file and hashing takes ~1ms per file = ~3 seconds for 2874 files.
        // This is done ONCE during cache restore, saving 38 seconds on every warm start.
        let path_key = path_lookup_key(path);
        let Some(mut prepared) = prepare_cached_file(path, cached_file.clone()) else {
            debug!("Skipping unreadable file during cache restore: {:?}", path);
            return;
        };
        after_read();
        if path_lookup_key(path) != path_key
            || source_validation_stamp(path).ok() != Some(prepared.validation_stamp)
        {
            debug!(
                "Skipping cache entry whose path identity changed while reading: {:?}",
                path
            );
            return;
        }
        let mut inner = self.write_inner();

        if inner.file_id_for_key(&path_key).is_some() {
            debug!(
                "Skipping cache entry already published in memory: {:?}",
                path
            );
            return;
        }

        // Get or create FileId
        let file_id = inner.get_or_create_file_id(path.to_path_buf(), path_key);

        inner.allocate_revision(file_id, prepared.content_hash, SourceRevisionOrigin::Disk);
        if let Some(identifiers) = prepared.identifiers {
            inner.file_lexical_identifiers.insert(file_id, identifiers);
        }
        inner.file_content_cache.pop(&file_id);
        inner.parse_tree_cache.remove(&file_id);
        let mut affected_symbols =
            super::core::unregister_source_symbols_without_election(&mut inner, file_id);
        let mut affected_fqns = affected_symbols
            .iter()
            .map(|symbol| Arc::clone(&symbol.fqn))
            .collect::<Vec<_>>();
        // Keep single-file restoration identical to fresh and batch indexing.
        let imports = std::mem::take(&mut prepared.imports);
        super::core::process_imports_for_file(&mut inner, file_id, &imports);
        inner.replace_file_generic_supertypes(
            file_id,
            std::mem::take(&mut prepared.generic_supertypes),
        );

        // Restore exact per-source candidates before electing the active shape.
        for symbol in &prepared.symbols {
            affected_fqns.push(Arc::clone(&symbol.fqn));
        }

        affected_symbols.extend(super::core::register_source_symbols_without_election(
            &mut inner,
            prepared.symbols,
            file_id,
        ));
        // Classes written against a typealias either revision declares are
        // reached through the alias names.
        affected_fqns.extend(super::inheritance::declared_kotlin_type_alias_fqns(
            affected_symbols.iter().map(AsRef::as_ref),
        ));
        super::core::reelect_affected_symbols(&mut inner, affected_symbols);

        // Do not restore resolved usage bindings from an earlier workspace.
        // Current consumers re-extract them from this exact disk publication.
        self.invalidate_caches_for_fqns_in_state(&inner, &affected_fqns);
    }

    // ===== Private helper methods =====

    #[cfg(test)]
    pub(super) fn invalidate_caches_for_fqns(&self, affected_fqns: &[Arc<str>]) {
        if affected_fqns.is_empty() {
            return;
        }
        let inner = self.inner.read();
        self.invalidate_caches_for_fqns_in_state(&inner, affected_fqns);
    }

    /// Invalidate derived caches against a specific published index state.
    /// Callers holding the index write lock use this before releasing it so no
    /// request can observe the new declarations with old resolution results.
    pub(super) fn invalidate_caches_for_fqns_in_state(
        &self,
        inner: &IndexInner,
        affected_fqns: &[Arc<str>],
    ) {
        if affected_fqns.is_empty() {
            return;
        }

        self.resolution_cache_generation
            .fetch_add(1, Ordering::Release);

        // H5 FIX: Collect all FQNs to invalidate (affected classes + their subclasses)
        // When a parent class changes, subclass method resolution may change too
        // Example: Parent adds foo() → Child.foo() cache entry becomes stale
        let mut fqns_to_invalidate = HashSet::new();
        fqns_to_invalidate.extend(affected_fqns.iter().cloned());

        // For each affected FQN, collect all subclasses recursively
        for fqn in affected_fqns {
            inner.collect_potential_transitive_subtypes(fqn, &mut fqns_to_invalidate);
        }

        // Invalidate method resolution cache entries
        // Cache key is (receiver_fqn, method_name)
        // Remove entries where receiver_fqn is in fqns_to_invalidate (includes subclasses)
        let method_entries_removed = {
            let mut cache = self.method_resolution_cache.lock();
            let keys_to_remove: Vec<_> = cache
                .iter()
                .filter(|((receiver_fqn, _), _)| fqns_to_invalidate.contains(receiver_fqn.as_str()))
                .map(|(key, _)| key.clone())
                .collect();

            let count = keys_to_remove.len();
            for key in keys_to_remove {
                cache.pop(&key);
            }
            count
        };

        // Invalidate inheritance cache entries
        // Cache key is class_fqn
        // Remove entries where class_fqn is in affected_fqns
        // Also need to invalidate entries for subclasses (their parent chain changed)
        let inheritance_entries_removed = {
            let mut cache = self.inheritance_cache.lock();

            // Direct and potential-context invalidation. A newly indexed
            // same-package type, or a second on-demand candidate, can change a
            // source edge even when the old cached chain never contained that
            // new FQN. The conservative contextual reverse index above is the
            // only complete way to discover those receivers.
            let mut keys_to_remove: Vec<_> = cache
                .iter()
                .filter(|(class_fqn, _)| fqns_to_invalidate.contains(class_fqn.as_str()))
                .map(|(key, _)| key.clone())
                .collect();

            // Transitive invalidation: classes whose parent chain contains affected classes
            // If User class changes, invalidate AdminUser's cached chain (User is in it)
            let transitive_keys: Vec<_> = cache
                .iter()
                .filter(|(class_fqn, parent_chain)| {
                    !affected_fqns
                        .iter()
                        .any(|fqn| fqn.as_ref() == class_fqn.as_str())
                        && parent_chain.iter().any(|parent| {
                            affected_fqns
                                .iter()
                                .any(|fqn| fqn.as_ref() == parent.as_str())
                        })
                })
                .map(|(key, _)| key.clone())
                .collect();

            keys_to_remove.extend(transitive_keys);

            let count = keys_to_remove.len();
            for key in keys_to_remove {
                cache.pop(&key);
            }
            count
        };

        debug!(
            "Granular cache invalidation: {} affected FQNs → removed {} method resolution entries, {} inheritance entries",
            affected_fqns.len(),
            method_entries_removed,
            inheritance_entries_removed
        );
    }

    /// Get the complete inheritance chain for a class with caching.
    ///
    /// Includes the class itself, then its parent classes and implemented/super
    /// interfaces in a stable transitive order.
    ///
    /// Returns a vector of FQNs representing the inheritance hierarchy.
    ///
    /// # Arguments
    /// * `class_fqn` - Fully qualified name of the class
    ///
    /// # Returns
    /// Vector of FQNs in the inheritance chain.
    ///
    /// # Performance
    /// Results are cached in an LRU cache to avoid repeated traversal
    fn compute_inheritance_chain(&self, class_fqn: &str) -> Vec<String> {
        let inner = self.inner.read();
        let mut chain = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut interfaces = std::collections::VecDeque::new();

        // Java member lookup exhausts the concrete superclass chain before it
        // considers any interface default/member. A breadth-first graph walk
        // interleaves Child's interfaces before Grandparent and fabricates
        // interface precedence.
        let mut class_name = Some(class_fqn.to_owned());
        while let Some(current) = class_name.take() {
            if !visited.insert(current.clone()) {
                break;
            }
            let class_name_ref = current.as_str();
            chain.push(current.clone());
            if !inner.symbols.contains_key(class_name_ref) {
                break;
            }
            let (parent, direct_interfaces) = inner.resolved_supertype_roles(class_name_ref);
            interfaces.extend(direct_interfaces);
            class_name = parent;
        }

        while let Some(interface) = interfaces.pop_front() {
            if !visited.insert(interface.clone()) {
                continue;
            }
            let interface_ref = interface.as_str();
            chain.push(interface.clone());
            if !inner.symbols.contains_key(interface_ref) {
                continue;
            }
            let (parent, direct_interfaces) = inner.resolved_supertype_roles(interface_ref);
            interfaces.extend(parent);
            interfaces.extend(direct_interfaces);
        }

        chain
    }
}

#[cfg(test)]
mod usage_score_tests {
    use super::*;

    fn index() -> WorkspaceIndex {
        WorkspaceIndex::new(PathBuf::new())
    }

    #[test]
    fn batch_usage_scores_preserve_exact_and_overload_semantics() {
        let index = index();
        index.load_usage_stats(HashMap::from([
            ("pkg.Type.exact".to_string(), 8),
            ("pkg.Type.exact(int)".to_string(), 100),
            ("pkg.Type.overloaded(int)".to_string(), 4),
            ("pkg.Type.overloaded(String)".to_string(), 6),
            ("pkg.Other.max".to_string(), 20),
        ]));

        let scores =
            index.get_usage_scores(&["pkg.Type.exact", "pkg.Type.overloaded", "pkg.Type.missing"]);

        assert_eq!(scores, vec![0.08, 0.10, 0.0]);
        assert_eq!(index.get_usage_score("pkg.Type.overloaded"), scores[1]);
    }

    #[test]
    fn batch_usage_scores_handle_empty_inputs_and_empty_stats() {
        let index = index();
        assert!(index.get_usage_scores(&[]).is_empty());
        assert_eq!(index.get_usage_scores(&["missing"]), vec![0.0]);
        assert_eq!(index.get_usage_score("missing"), 0.0);
    }

    #[test]
    fn aggregated_overload_score_stays_normalized_without_losing_order() {
        let index = index();
        index.load_usage_stats(HashMap::from([
            ("pkg.Type.call(int)".to_string(), 8),
            ("pkg.Type.call(String)".to_string(), 7),
            ("pkg.Other.max".to_string(), 10),
        ]));

        let scores = index.get_usage_scores(&["pkg.Type.call", "pkg.Other.max"]);
        assert_eq!(scores[0], 1.0);
        assert!((scores[1] - (10.0 / 15.0)).abs() < f64::EPSILON);
        assert!(scores[0] > scores[1]);
    }

    #[test]
    fn loading_persisted_interactions_replaces_instead_of_accumulating() {
        let index = index();
        index.load_usage_stats(HashMap::from([
            ("pkg.A".to_string(), 2),
            ("pkg.Stale".to_string(), 9),
        ]));
        index.load_usage_stats(HashMap::from([("pkg.A".to_string(), 3)]));

        assert_eq!(
            index.extract_usage_stats(),
            HashMap::from([("pkg.A".to_string(), 3)])
        );
        assert_eq!(index.get_usage_score("pkg.A"), 1.0);
        assert_eq!(index.get_usage_score("pkg.Stale"), 0.0);
    }

    #[test]
    fn ranking_combines_interactions_with_current_index_counts_only_at_query_time() {
        let index = index();
        index.load_usage_stats(HashMap::from([("pkg.A".to_string(), 2)]));
        index.replace_indexed_usage_counts(
            &HashMap::new(),
            &HashMap::from([(Arc::from("pkg.A"), 3), (Arc::from("pkg.B"), 10)]),
        );

        assert_eq!(index.get_usage_scores(&["pkg.A", "pkg.B"]), [0.5, 1.0]);
        assert_eq!(
            index.extract_usage_stats(),
            HashMap::from([("pkg.A".to_string(), 2)]),
            "derived source counts must not be persisted as interaction history"
        );
    }

    #[test]
    fn reloading_interactions_preserves_current_index_counts() {
        let index = index();
        index.replace_indexed_usage_counts(
            &HashMap::new(),
            &HashMap::from([(Arc::from("pkg.Indexed"), 7)]),
        );
        index.load_usage_stats(HashMap::from([("pkg.Old".to_string(), 11)]));

        index.load_usage_stats(HashMap::from([("pkg.New".to_string(), 3)]));

        assert_eq!(
            index.extract_usage_stats(),
            HashMap::from([("pkg.New".to_string(), 3)])
        );
        assert_eq!(
            index.get_top_symbols(3),
            vec![("pkg.Indexed".to_string(), 7), ("pkg.New".to_string(), 3)]
        );
    }
}

#[cfg(test)]
mod persistent_cache_restore_tests {
    use super::*;

    fn cached_source() -> (
        tempfile::TempDir,
        PathBuf,
        HashMap<PathBuf, crate::CachedFile>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Cached.java");
        let source = "package cache; class Cached { int field; }";
        std::fs::write(&path, source).unwrap();
        let index = WorkspaceIndex::new(directory.path().to_path_buf());
        index.index_file_with_source(&path, source).unwrap();
        let cache = index.extract_cache_data();
        assert!(cache[&path].symbols.len() >= 2);
        (directory, path, cache)
    }

    #[test]
    fn malformed_symbol_kind_or_range_rejects_the_whole_file() {
        let (_directory, path, cache) = cached_source();
        let pristine = cache[&path].clone();

        for corrupt in [
            {
                let mut corrupt = pristine.clone();
                corrupt.symbols[1].kind = "future-corrupt-kind".to_string();
                corrupt
            },
            {
                let mut corrupt = pristine.clone();
                corrupt.symbols[1].end_offset = u32::MAX;
                corrupt
            },
            {
                let mut corrupt = pristine.clone();
                corrupt.imports.wildcards = vec!["cache.types".into(), "cache.types".into()];
                corrupt
            },
        ] {
            let restored = WorkspaceIndex::new(PathBuf::new());
            let rejected =
                restored.restore_from_cache_batch(&HashMap::from([(path.clone(), corrupt)]));

            assert_eq!(rejected, vec![path.clone()]);
            assert!(restored.get_file_id(&path).is_none());
            assert!(restored.get_symbol("cache.Cached").is_none());
            assert!(restored.all_file_ids().is_empty());
        }
    }

    #[test]
    fn source_changed_after_validation_is_not_published() {
        let (_directory, path, cache) = cached_source();
        let changed = "package cache; class Changed { int field; }";
        let restored = WorkspaceIndex::new(PathBuf::new());

        let (rejected, revisions) =
            restored.restore_from_cache_batch_after_validation_for_test(&cache, || {
                std::fs::write(&path, changed).unwrap();
            });

        assert_eq!(rejected, vec![path.clone()]);
        assert!(revisions.is_empty());
        assert!(restored.get_file_id(&path).is_none());
        assert!(restored.get_symbol("cache.Cached").is_none());
        assert!(restored.get_symbol("cache.Changed").is_none());
    }
}
