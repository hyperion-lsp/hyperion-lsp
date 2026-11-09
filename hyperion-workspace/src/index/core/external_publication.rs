// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! External classpath registration and archive publication.

use super::*;

fn root_owner_in_batch(
    symbol: &crate::Symbol,
    declaration_parents: &HashMap<Arc<str>, Arc<str>>,
) -> Arc<str> {
    let mut owner: Arc<str> = Arc::from(immediate_owner_fqn(symbol));
    let mut remaining = declaration_parents.len().saturating_add(1);
    while let Some(parent) = declaration_parents.get(owner.as_ref()) {
        if remaining == 0 || parent == &owner {
            break;
        }
        owner = Arc::clone(parent);
        remaining -= 1;
    }
    owner
}

fn owner_complete_external_chunks(
    symbols: Vec<Arc<crate::Symbol>>,
    chunk_size: usize,
) -> Vec<Vec<Arc<crate::Symbol>>> {
    assert!(
        chunk_size > 0,
        "external symbol chunk size must be non-zero"
    );

    let declaration_parents: HashMap<_, _> = symbols
        .iter()
        .filter_map(|symbol| {
            symbol
                .container
                .as_deref()
                .map(|container| (Arc::clone(&symbol.fqn), Arc::from(container)))
        })
        .collect();
    let mut groups = Vec::<Vec<Arc<crate::Symbol>>>::new();
    let mut group_by_owner = HashMap::<Arc<str>, usize>::new();
    for symbol in symbols {
        let owner = root_owner_in_batch(&symbol, &declaration_parents);
        let group_index = *group_by_owner.entry(owner).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group_index].push(symbol);
    }

    let mut chunks = Vec::new();
    let mut chunk = Vec::with_capacity(chunk_size);
    for group in groups {
        if !chunk.is_empty() && chunk.len().saturating_add(group.len()) > chunk_size {
            chunks.push(std::mem::take(&mut chunk));
            chunk = Vec::with_capacity(chunk_size);
        }
        chunk.extend(group);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

/// Insert external symbols while preserving workspace-source precedence.
///
/// Registration only appends candidates for `file_id`; nothing previously
/// visible is retracted, so concurrent readers keep their captured generation.
fn insert_external_symbols(
    inner: &mut IndexInner,
    symbols: &[crate::Symbol],
    file_id: FileId,
) -> usize {
    debug_assert!(inner.external_file_ids.contains(&file_id));
    let symbols = register_source_symbols_without_election(
        inner,
        symbols.iter().cloned().map(Arc::new),
        file_id,
    );
    reelect_appended_symbols(inner, symbols)
}

impl WorkspaceIndex {
    pub fn add_external_symbol(&self, symbol: crate::Symbol) {
        let mut inner = self.write_inner();
        let file_id = inner.allocate_external_virtual_file_id("jdk");
        let revision = self
            .reserve_external_source_revision_internal(&mut inner, file_id)
            .expect("JDK external source must remain live");
        insert_external_symbols(&mut inner, std::slice::from_ref(&symbol), file_id);
        inner.finish_external_source_publication(revision);
        self.clear_resolution_caches();
    }

    pub fn add_external_symbols_batch(&self, symbols: Vec<crate::Symbol>) {
        if symbols.is_empty() {
            return;
        }

        // Single write lock for all symbols - major performance improvement
        let mut inner = self.write_inner();

        // Allocate unique FileId for JDK symbols (fixes HashMap collision bug)
        let file_id = inner.allocate_external_virtual_file_id("jdk");
        let revision = self
            .reserve_external_source_revision_internal(&mut inner, file_id)
            .expect("JDK external source must remain live");

        insert_external_symbols(&mut inner, &symbols, file_id);
        inner.finish_external_source_publication(revision);
        self.clear_resolution_caches();
    }

    /// Add a large external symbol set without monopolizing the index write
    /// lock for the full restore.
    ///
    /// JDK cache restoration happens in the background while editors may
    /// already issue completion requests. Publishing bounded owner-complete
    /// chunks lets those readers make progress between writes without exposing
    /// a class whose fields and methods have not been installed yet.
    pub fn add_external_symbols_responsive(&self, symbols: Vec<crate::Symbol>) {
        self.add_external_index_data_responsive(symbols, Vec::new());
    }

    /// Add a large external symbol set together with its generic hierarchy
    /// metadata without monopolizing the index write lock.
    pub fn add_external_index_data_responsive(
        &self,
        symbols: Vec<crate::Symbol>,
        generic_supertypes: Vec<crate::TypeGenericSupertypes>,
    ) {
        const CHUNK_SIZE: usize = 2_048;

        let revision = {
            let mut inner = self.write_inner();
            let file_id = inner.allocate_external_virtual_file_id("jdk");
            self.reserve_external_source_revision_internal(&mut inner, file_id)
                .expect("JDK external source must remain live")
        };
        self.add_external_symbols_in_chunks(
            symbols,
            generic_supertypes,
            revision,
            CHUNK_SIZE,
            || {
                std::thread::yield_now();
            },
        );
    }

    /// Publish cached JDK symbols for an exact classpath refresh.
    pub fn add_external_symbols_responsive_for_revision(
        &self,
        symbols: Vec<crate::Symbol>,
        revision: ExternalSourceRevisionToken,
    ) -> bool {
        self.add_external_index_data_responsive_for_revision(symbols, Vec::new(), revision)
    }

    /// Publish cached external declarations and their generic hierarchy metadata
    /// for an exact classpath refresh.
    ///
    /// Keeping both pieces in the same revision-fenced publication is required:
    /// restoring symbols alone leaves inherited generic methods visible but
    /// erases the receiver-to-declaring-owner type projection used to resolve
    /// their parameter and return types.
    pub fn add_external_index_data_responsive_for_revision(
        &self,
        symbols: Vec<crate::Symbol>,
        generic_supertypes: Vec<crate::TypeGenericSupertypes>,
        revision: ExternalSourceRevisionToken,
    ) -> bool {
        const CHUNK_SIZE: usize = 2_048;

        self.add_external_symbols_in_chunks(
            symbols,
            generic_supertypes,
            revision,
            CHUNK_SIZE,
            || {
                std::thread::yield_now();
            },
        )
    }

    fn add_external_symbols_in_chunks(
        &self,
        symbols: Vec<crate::Symbol>,
        generic_supertypes: Vec<crate::TypeGenericSupertypes>,
        revision: ExternalSourceRevisionToken,
        chunk_size: usize,
        mut between_chunks: impl FnMut(),
    ) -> bool {
        let file_id = revision.file_id;
        let file_lock = self.get_file_lock(file_id);
        let _file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let chunks =
            owner_complete_external_chunks(symbols.into_iter().map(Arc::new).collect(), chunk_size);
        let all_symbols = chunks.iter().flatten().cloned().collect::<Vec<_>>();

        // A repeated cache restore is a replacement, not an append. Keeping the
        // swap atomic avoids retaining declarations removed from the new JDK
        // image and prevents readers from seeing a torn stdlib revision.
        if self.inner.read().file_symbols.contains_key(&file_id) {
            let mut inner = self.write_inner();
            if !inner.external_source_revision_is_current(revision) {
                return false;
            }
            let mut affected = unregister_source_symbols_without_election(&mut inner, file_id);
            inner.replace_file_generic_supertypes(file_id, generic_supertypes);
            affected.extend(register_source_symbols_without_election(
                &mut inner,
                all_symbols,
                file_id,
            ));
            reelect_affected_symbols(&mut inner, affected);
            inner.finish_external_source_publication(revision);
            self.clear_resolution_caches();
            return true;
        }

        // An empty archive is still a completed publication. Recording its
        // generation lets provider and cache transactions distinguish it from
        // a failed extraction which left no declarations behind.
        if chunks.is_empty() {
            let mut inner = self.write_inner();
            if !inner.external_source_revision_is_current(revision) {
                return false;
            }
            inner.replace_file_generic_supertypes(file_id, generic_supertypes);
            inner.finish_external_source_publication(revision);
            return true;
        }

        let chunk_count = chunks.len();
        for (index, chunk) in chunks.into_iter().enumerate() {
            let mut inner = self.write_inner();
            if !inner.external_source_revision_is_current(revision) {
                return false;
            }
            if index == 0 {
                inner.mark_external_source_publication_in_progress(revision);
                inner.replace_file_generic_supertypes(file_id, generic_supertypes.clone());
            }
            // Candidate registration and election are one reader-visible
            // transaction. Registering later chunks up front would let typed
            // queries see declarations whose secondary/hierarchy indices do
            // not exist yet.
            register_source_symbols_without_election(&mut inner, chunk.iter().cloned(), file_id);
            reelect_appended_symbols(&mut inner, chunk.iter().cloned());
            if index + 1 == chunk_count {
                inner.finish_external_source_publication(revision);
            }
            // Requests may run between chunks. Never retain a negative or
            // partial hierarchy result across newly registered candidates,
            // including a shadowed duplicate that changes ambiguity only.
            self.clear_resolution_caches();
            drop(inner);
            if index + 1 < chunk_count {
                between_chunks();
            }
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn add_external_symbols_in_chunks_for_test(
        &self,
        symbols: Vec<crate::Symbol>,
        chunk_size: usize,
        between_chunks: impl FnMut(),
    ) {
        let revision = {
            let mut inner = self.write_inner();
            let file_id = inner.allocate_external_virtual_file_id("jdk");
            self.reserve_external_source_revision_internal(&mut inner, file_id)
                .expect("JDK external source must remain live")
        };
        self.add_external_symbols_in_chunks(
            symbols,
            Vec::new(),
            revision,
            chunk_size,
            between_chunks,
        );
    }
}

impl WorkspaceIndex {
    /// Reserve a new extraction generation while retiring any torn first
    /// publication from the previous generation in the same index transaction.
    ///
    /// A completed publication has no in-progress marker and therefore remains
    /// the last-good revision until its replacement is published atomically.
    /// Conversely, the candidates registered by a chunked first publication
    /// are not a usable fallback once that extraction is superseded: remove the
    /// complete candidate vector and elect surviving sources before minting the
    /// generation that invalidates the old publisher.
    fn reserve_external_source_revision_internal(
        &self,
        inner: &mut IndexInner,
        file_id: FileId,
    ) -> Option<ExternalSourceRevisionToken> {
        if let Some(incomplete_generation) = inner
            .external_source_publications_in_progress
            .remove(&file_id)
        {
            debug_assert_eq!(
                inner.external_source_generations.get(&file_id),
                Some(&incomplete_generation),
                "an in-progress publication must identify the current generation"
            );
            let partial_symbols = unregister_source_symbols_without_election(inner, file_id);
            if inner
                .published_external_source_generations
                .get(&file_id)
                .is_some_and(|published| *published == incomplete_generation)
            {
                inner.published_external_source_generations.remove(&file_id);
            }
            if !partial_symbols.is_empty() {
                reelect_affected_symbols(inner, partial_symbols);
                self.clear_resolution_caches();
            }
        }
        inner.reserve_external_source_revision(file_id)
    }

    /// Reserve external source identities in classpath order before parallel
    /// extraction starts. Publication may then finish out of order without
    /// changing which duplicate class is authoritative.
    #[must_use = "archive indexing must use the revisions reserved before extraction"]
    pub fn register_external_sources(&self, sources: &[PathBuf]) -> ExternalSourceRefresh {
        const LEGACY_GROUP: &str = "unowned";
        const LEGACY_PRIORITY: u32 = u32::MAX;

        let mut registered_sources = Vec::with_capacity(sources.len());
        let mut seen = HashSet::with_capacity(sources.len());
        for source in sources {
            let source = RegisteredExternalSource::for_legacy_path(source);
            if seen.insert(source.key().clone()) {
                registered_sources.push(source);
            }
        }
        let requested_keys: Vec<_> = registered_sources
            .iter()
            .map(|source| source.key().clone())
            .collect();
        let mut inner = self.write_inner();
        for source in &registered_sources {
            inner.allocate_external_file_id(source.clone());
        }
        let group = inner
            .external_source_groups
            .entry(Arc::from(LEGACY_GROUP))
            .or_insert_with(|| ExternalSourceGroup {
                priority: LEGACY_PRIORITY,
                sources: Vec::new(),
            });
        for source in registered_sources {
            if !group
                .sources
                .iter()
                .any(|candidate| candidate.key() == source.key())
            {
                group.sources.push(source);
            }
        }
        inner.rebuild_external_source_precedence();
        reelect_all_external_symbols(&mut inner);
        // Classpath rank and ambiguity are contextual inputs even when the
        // currently elected declaration happens to remain unchanged.
        self.clear_resolution_caches();
        // The legacy group is additive, but this call owns extraction only for
        // the identities it explicitly requested. Re-reserving every prior
        // member would cancel an unrelated in-flight publication whenever a
        // later archive is registered.
        let revisions = requested_keys
            .into_iter()
            .filter_map(|source_key| {
                let file_id = inner.external_source_ids.get(&source_key).copied()?;
                let revision =
                    self.reserve_external_source_revision_internal(&mut inner, file_id)?;
                Some((source_key, revision))
            })
            .collect();
        ExternalSourceRefresh {
            revisions,
            released_file_ids: Vec::new(),
        }
    }

    /// Atomically replace the external sources owned by one logical classpath.
    ///
    /// Identities for the new classpath are reserved in the provided order so
    /// duplicate declarations remain deterministic. Sources released by this
    /// owner are removed only when no other owner still references them.
    pub fn replace_external_sources(&self, owner: &str, sources: &[PathBuf]) -> Vec<FileId> {
        self.replace_external_sources_revisioned(owner, sources)
            .released_file_ids
    }

    /// Replace one classpath with an explicit precedence tier.
    ///
    /// Lower priorities win. Position within `sources` breaks ties inside the
    /// classpath, followed by owner name for deterministic shared-source ties.
    pub fn replace_external_sources_with_priority(
        &self,
        owner: &str,
        priority: u32,
        sources: &[PathBuf],
    ) -> Vec<FileId> {
        self.replace_external_sources_with_priority_revisioned(owner, priority, sources)
            .released_file_ids
    }

    /// Atomically replace one classpath and reserve the only revisions allowed
    /// to publish its archive extractions.
    ///
    /// Reservation happens before callers launch or await extraction work.
    /// Holding these tokens, rather than reacquiring authority by path later,
    /// is what makes cancellation safe when consecutive refreshes retain the
    /// same archive path and therefore the same `FileId`.
    pub fn replace_external_sources_revisioned(
        &self,
        owner: &str,
        sources: &[PathBuf],
    ) -> ExternalSourceRefresh {
        let ticket = self.begin_external_source_refresh(owner);
        self.replace_external_sources_for_refresh(&ticket, sources)
            .expect("a newly reserved external refresh ticket must remain current")
    }

    /// Revisioned variant of [`Self::replace_external_sources_with_priority`].
    pub fn replace_external_sources_with_priority_revisioned(
        &self,
        owner: &str,
        priority: u32,
        sources: &[PathBuf],
    ) -> ExternalSourceRefresh {
        let ticket = self.begin_external_source_refresh(owner);
        self.replace_external_sources_with_priority_for_refresh(&ticket, priority, sources)
            .expect("a newly reserved external refresh ticket must remain current")
    }

    /// Reserve ownership of a logical classpath refresh before asynchronous
    /// dependency resolution begins.
    pub fn begin_external_source_refresh(&self, owner: &str) -> ExternalSourceRefreshTicket {
        // A previous resolver may already have entered its short cross-registry
        // commit section. Let that generation linearize before minting the next
        // ticket; archive extraction itself never holds this gate.
        let _publication =
            ExternalSourcePublicationGuard::acquire(self.external_source_publication_gate(owner));
        let mut inner = self.write_inner();
        let generation = inner.next_external_source_owner_generation;
        inner.next_external_source_owner_generation = NonZeroU64::new(
            generation
                .get()
                .checked_add(1)
                .expect("external source owner generation space exhausted"),
        )
        .expect("incrementing a non-zero owner generation cannot produce zero");
        let owner: Arc<str> = Arc::from(owner);
        let authority = Arc::clone(
            inner
                .external_source_owner_authorities
                .entry(Arc::clone(&owner))
                .or_insert_with(|| {
                    Arc::new(ExternalSourceRefreshAuthority {
                        owner,
                        latest_generation: AtomicU64::new(0),
                        committed_generation: AtomicU64::new(0),
                    })
                }),
        );
        authority
            .latest_generation
            .store(generation.get(), Ordering::Release);
        ExternalSourceRefreshTicket {
            authority,
            generation,
        }
    }

    fn external_source_publication_gate(&self, owner: &str) -> Arc<ExternalSourcePublicationGate> {
        Arc::clone(
            self.external_source_publication_gates
                .entry(Arc::from(owner))
                .or_default()
                .value(),
        )
    }

    /// Enter the short commit section for an already-resolved external
    /// classpath refresh.
    ///
    /// The ticket is revalidated after waiting. While the returned guard lives,
    /// no newer ticket for this owner can be minted, so callers may publish the
    /// WorkspaceIndex membership and matching side registries coherently.
    pub fn begin_external_source_publication(
        &self,
        ticket: &ExternalSourceRefreshTicket,
    ) -> Option<ExternalSourcePublicationGuard> {
        let guard = ExternalSourcePublicationGuard::acquire(
            self.external_source_publication_gate(ticket.owner()),
        );
        let owns_authority = self
            .inner
            .read()
            .external_source_owner_authorities
            .get(ticket.owner())
            .is_some_and(|authority| Arc::ptr_eq(authority, &ticket.authority));
        (owns_authority && ticket.is_latest()).then_some(guard)
    }

    /// Enter a publication step for the currently committed generation.
    ///
    /// Archive parsing happens outside this guard. Holding it only for the
    /// bounded WorkspaceIndex/provider mutation prevents the next classpath
    /// commit from interleaving between symbol publication and source routing.
    pub fn begin_committed_external_source_publication(
        &self,
        ticket: &ExternalSourceRefreshTicket,
    ) -> Option<ExternalSourcePublicationGuard> {
        let guard = ExternalSourcePublicationGuard::acquire(
            self.external_source_publication_gate(ticket.owner()),
        );
        let owns_authority = self
            .inner
            .read()
            .external_source_owner_authorities
            .get(ticket.owner())
            .is_some_and(|authority| Arc::ptr_eq(authority, &ticket.authority));
        (owns_authority && ticket.is_committed()).then_some(guard)
    }

    /// Replace a default-priority classpath only if `ticket` still owns the
    /// latest refresh attempt for that logical owner.
    pub fn replace_external_sources_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        sources: &[PathBuf],
    ) -> Option<ExternalSourceRefresh> {
        self.replace_external_sources_with_priority_for_refresh(ticket, 100, sources)
    }

    /// Replace a classpath only if `ticket` still owns its logical refresh.
    /// The owner check, group mutation, source-generation reservation, and
    /// release cleanup are one index transaction.
    pub fn replace_external_sources_with_priority_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        priority: u32,
        sources: &[PathBuf],
    ) -> Option<ExternalSourceRefresh> {
        let sources = sources
            .iter()
            .map(|source| RegisteredExternalSource::for_legacy_path(source))
            .collect();
        self.replace_registered_external_sources_for_refresh(ticket, priority, sources)
    }

    /// Replace a default-priority classpath using archive identities captured
    /// once by the caller for the complete refresh transaction.
    pub fn replace_external_archives_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        sources: &[crate::ExternalSourceArchive],
    ) -> Option<ExternalSourceRefresh> {
        self.replace_external_archives_with_priority_for_refresh(ticket, 100, sources)
    }

    /// Typed variant of
    /// [`Self::replace_external_sources_with_priority_for_refresh`].
    pub fn replace_external_archives_with_priority_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        priority: u32,
        sources: &[crate::ExternalSourceArchive],
    ) -> Option<ExternalSourceRefresh> {
        let sources = sources
            .iter()
            .map(RegisteredExternalSource::for_archive)
            .collect();
        self.replace_registered_external_sources_for_refresh(ticket, priority, sources)
    }

    fn replace_registered_external_sources_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        priority: u32,
        sources: Vec<RegisteredExternalSource>,
    ) -> Option<ExternalSourceRefresh> {
        let mut desired_order = Vec::with_capacity(sources.len());
        let mut desired_set = HashSet::with_capacity(sources.len());
        for source in sources {
            if desired_set.insert(source.key().clone()) {
                desired_order.push(source);
            }
        }

        let (revisions, released_ids) = {
            let mut inner = self.write_inner();
            if !inner
                .external_source_owner_authorities
                .get(ticket.owner())
                .is_some_and(|authority| {
                    Arc::ptr_eq(authority, &ticket.authority) && ticket.is_latest()
                })
            {
                return None;
            }
            for source in &desired_order {
                inner.allocate_external_file_id(source.clone());
            }

            let previous = inner
                .external_source_groups
                .insert(
                    Arc::clone(&ticket.authority.owner),
                    ExternalSourceGroup {
                        priority,
                        sources: desired_order.clone(),
                    },
                )
                .map(|group| group.sources)
                .unwrap_or_default();
            if inner
                .external_source_groups
                .get(ticket.owner())
                .is_some_and(|group| group.sources.is_empty())
            {
                inner.external_source_groups.remove(ticket.owner());
            }
            inner.rebuild_external_source_precedence();
            reelect_all_external_symbols(&mut inner);
            // Even an unchanged active declaration can now resolve imports or
            // hierarchy edges differently because classpath rank changed.
            self.clear_resolution_caches();

            let mut released: Vec<_> = previous
                .into_iter()
                .filter(|source| {
                    !inner.external_source_groups.values().any(|group| {
                        group
                            .sources
                            .iter()
                            .any(|candidate| candidate.key() == source.key())
                    })
                })
                .filter_map(|source| inner.external_source_ids.get(source.key()).copied())
                .collect();
            released.sort_unstable_by_key(|file_id| file_id.as_u32());

            // Releasing the last owner invalidates in-flight extraction before
            // `remove_file` waits on its publication lock. Retained paths get a
            // fresh generation in this same owner/classpath transaction.
            for file_id in &released {
                inner.external_source_generations.remove(file_id);
                inner.published_external_source_generations.remove(file_id);
                self.force_clean_file_state_internal(&mut inner, *file_id);
            }
            let revisions = desired_order
                .iter()
                .map(|source| {
                    let file_id = inner.external_source_ids[source.key()];
                    let revision = self
                        .reserve_external_source_revision_internal(&mut inner, file_id)
                        .expect("newly reserved external source must remain live");
                    (source.key().clone(), revision)
                })
                .collect();
            ticket
                .authority
                .committed_generation
                .store(ticket.generation.get(), Ordering::Release);
            (revisions, released)
        };

        for file_id in &released_ids {
            self.remove_file_lock_if_unused(*file_id);
        }
        Some(ExternalSourceRefresh {
            revisions,
            released_file_ids: released_ids,
        })
    }

    pub fn index_jar_file(&self, jar_path: &Path) -> Result<crate::JarIndexStats> {
        let revision = self.reserve_direct_external_source_revision(jar_path)?;
        self.index_jar_file_for_revision(jar_path, revision)
    }

    /// Extract and publish a JAR for the exact classpath refresh that reserved
    /// `revision`.
    pub fn index_jar_file_for_revision(
        &self,
        jar_path: &Path,
        revision: ExternalSourceRevisionToken,
    ) -> Result<crate::JarIndexStats> {
        self.validate_external_source_revision(jar_path, revision)?;

        // Extract symbols from JAR
        let (data, jar_stats) =
            crate::jar_indexer::extract_jar_index_data(jar_path, revision.file_id)?;

        if !self.insert_symbols_batch_for_revision(data.symbols, data.generic_supertypes, revision)
        {
            anyhow::bail!(
                "discarded superseded JAR extraction for {}",
                jar_path.display()
            );
        }

        Ok(jar_stats)
    }

    /// Extract and publish the immutable bytes captured for this refresh.
    pub fn index_jar_snapshot_for_revision(
        &self,
        snapshot: crate::ExternalArchiveSnapshot,
        revision: ExternalSourceRevisionToken,
    ) -> Result<crate::JarIndexStats> {
        let prepared = self.prepare_jar_snapshot_for_revision(snapshot, revision)?;
        self.publish_prepared_jar(prepared)
    }

    /// Parse an immutable JAR snapshot without mutating WorkspaceIndex.
    pub fn prepare_jar_snapshot_for_revision(
        &self,
        snapshot: crate::ExternalArchiveSnapshot,
        revision: ExternalSourceRevisionToken,
    ) -> Result<PreparedExternalArchivePublication> {
        let (archive, snapshot) = snapshot.into_parts();
        self.validate_external_source_identity(archive.identity(), revision, archive.path())?;
        let (data, jar_stats) =
            crate::jar_indexer::extract_jar_index_data_from_snapshot(snapshot, revision.file_id)?;
        Ok(PreparedExternalArchivePublication {
            archive,
            revision,
            symbols: data.symbols,
            generic_supertypes: data.generic_supertypes,
            stats: jar_stats,
        })
    }

    /// Atomically publish an already-parsed archive generation.
    pub fn publish_prepared_jar(
        &self,
        prepared: PreparedExternalArchivePublication,
    ) -> Result<crate::JarIndexStats> {
        self.validate_external_source_identity(
            prepared.archive.identity(),
            prepared.revision,
            prepared.archive.path(),
        )?;
        if !self.insert_symbols_batch_for_revision(
            prepared.symbols,
            prepared.generic_supertypes,
            prepared.revision,
        ) {
            anyhow::bail!(
                "discarded superseded JAR extraction for {}",
                prepared.archive.path().display()
            );
        }
        Ok(prepared.stats)
    }

    pub fn index_jar_file_with_docs(
        &self,
        jar_path: &Path,
        javadoc_jar_path: &Path,
    ) -> Result<crate::JarIndexStats> {
        let revision = self.reserve_direct_external_source_revision(jar_path)?;
        self.index_jar_file_with_docs_for_revision(jar_path, javadoc_jar_path, revision)
    }

    /// Extract and publish a documented JAR for one exact classpath refresh.
    pub fn index_jar_file_with_docs_for_revision(
        &self,
        jar_path: &Path,
        javadoc_jar_path: &Path,
        revision: ExternalSourceRevisionToken,
    ) -> Result<crate::JarIndexStats> {
        self.validate_external_source_revision(jar_path, revision)?;

        // Extract symbols from JAR with javadoc
        let (data, jar_stats) = crate::jar_indexer::extract_jar_index_data_with_docs(
            jar_path,
            javadoc_jar_path,
            revision.file_id,
        )?;

        if !self.insert_symbols_batch_for_revision(data.symbols, data.generic_supertypes, revision)
        {
            anyhow::bail!(
                "discarded superseded documented JAR extraction for {}",
                jar_path.display()
            );
        }

        Ok(jar_stats)
    }

    /// Extract a binary archive and its documentation from immutable private
    /// snapshots owned by one refresh.
    pub fn index_jar_snapshots_with_docs_for_revision(
        &self,
        binary: crate::ExternalArchiveSnapshot,
        javadoc: crate::ArchiveSnapshot,
        revision: ExternalSourceRevisionToken,
    ) -> Result<crate::JarIndexStats> {
        let prepared =
            self.prepare_jar_snapshots_with_docs_for_revision(binary, javadoc, revision)?;
        self.publish_prepared_jar(prepared)
    }

    /// Parse immutable binary and Javadoc snapshots without index mutation.
    pub fn prepare_jar_snapshots_with_docs_for_revision(
        &self,
        binary: crate::ExternalArchiveSnapshot,
        javadoc: crate::ArchiveSnapshot,
        revision: ExternalSourceRevisionToken,
    ) -> Result<PreparedExternalArchivePublication> {
        let (archive, binary) = binary.into_parts();
        self.validate_external_source_identity(archive.identity(), revision, archive.path())?;
        let (data, jar_stats) =
            crate::jar_indexer::extract_jar_index_data_with_docs_from_snapshots(
                binary,
                javadoc,
                revision.file_id,
            )?;
        Ok(PreparedExternalArchivePublication {
            archive,
            revision,
            symbols: data.symbols,
            generic_supertypes: data.generic_supertypes,
            stats: jar_stats,
        })
    }

    pub fn index_jmod_file(&self, jmod_path: &Path) -> Result<crate::JarIndexStats> {
        let revision = self.reserve_direct_external_source_revision(jmod_path)?;
        self.index_jmod_file_for_revision(jmod_path, revision)
    }

    /// Extract and publish a JMOD for one exact classpath refresh.
    pub fn index_jmod_file_for_revision(
        &self,
        jmod_path: &Path,
        revision: ExternalSourceRevisionToken,
    ) -> Result<crate::JarIndexStats> {
        self.validate_external_source_revision(jmod_path, revision)?;

        // Extract symbols from JMOD
        let (data, jmod_stats) =
            crate::jmod_indexer::extract_jmod_index_data(jmod_path, revision.file_id)?;

        if !self.insert_symbols_batch_for_revision(data.symbols, data.generic_supertypes, revision)
        {
            anyhow::bail!(
                "discarded superseded JMOD extraction for {}",
                jmod_path.display()
            );
        }

        Ok(jmod_stats)
    }

    /// Extract and publish the immutable JMOD bytes captured for this refresh.
    pub fn index_jmod_snapshot_for_revision(
        &self,
        snapshot: crate::ExternalArchiveSnapshot,
        revision: ExternalSourceRevisionToken,
    ) -> Result<crate::JarIndexStats> {
        let prepared = self.prepare_jmod_snapshot_for_revision(snapshot, revision)?;
        self.publish_prepared_jar(prepared)
    }

    /// Parse immutable JMOD bytes without mutating WorkspaceIndex.
    pub fn prepare_jmod_snapshot_for_revision(
        &self,
        snapshot: crate::ExternalArchiveSnapshot,
        revision: ExternalSourceRevisionToken,
    ) -> Result<PreparedExternalArchivePublication> {
        let (archive, snapshot) = snapshot.into_parts();
        self.validate_external_source_identity(archive.identity(), revision, archive.path())?;
        let (data, jmod_stats) =
            crate::jmod_indexer::extract_jmod_index_data_from_snapshot(snapshot, revision.file_id)?;
        Ok(PreparedExternalArchivePublication {
            archive,
            revision,
            symbols: data.symbols,
            generic_supertypes: data.generic_supertypes,
            stats: jmod_stats,
        })
    }

    fn reserve_direct_external_source_revision(
        &self,
        source_path: &Path,
    ) -> Result<ExternalSourceRevisionToken> {
        let source = RegisteredExternalSource::for_legacy_path(source_path);
        let mut inner = self.write_inner();
        let file_id = inner.allocate_external_file_id(source);
        if inner.external_source_precedence.contains_key(&file_id) {
            anyhow::bail!(
                "external source {} belongs to a classpath refresh; use its reserved revision",
                source_path.display()
            );
        }
        Ok(self
            .reserve_external_source_revision_internal(&mut inner, file_id)
            .expect("newly allocated external source must remain live"))
    }

    fn validate_external_source_revision(
        &self,
        source_path: &Path,
        revision: ExternalSourceRevisionToken,
    ) -> Result<()> {
        let source_key = ExternalSourceIdentity::for_legacy_path(source_path);
        self.validate_external_source_identity(&source_key, revision, source_path)
    }

    fn validate_external_source_identity(
        &self,
        source_key: &ExternalSourceIdentity,
        revision: ExternalSourceRevisionToken,
        display_path: &Path,
    ) -> Result<()> {
        let inner = self.inner.read();
        let source_matches = inner
            .external_file_id_to_source
            .get(&revision.file_id)
            .is_some_and(|registered| registered.key() == source_key);
        if !source_matches || !inner.external_source_revision_is_current(revision) {
            anyhow::bail!(
                "external source revision for {} is no longer current",
                display_path.display()
            );
        }
        Ok(())
    }
}

impl WorkspaceIndex {
    /// Index a JAR file
    ///
    /// Extracts classes, methods, and fields from a JAR file and adds them to the index.
    /// This enables navigation into external dependencies.
    ///
    /// # Arguments
    ///
    /// * `jar_path` - Path to the JAR file
    ///
    /// # Returns
    ///
    /// A count of symbols indexed.
    #[cfg(test)]
    pub(in crate::index) fn insert_symbols_batch(
        &self,
        symbols: Vec<crate::Symbol>,
        file_id: FileId,
    ) {
        let revision = {
            let mut inner = self.write_inner();
            self.reserve_external_source_revision_internal(&mut inner, file_id)
        };
        let Some(revision) = revision else {
            warn!(
                "Discarding symbols for released external source {:?}",
                file_id
            );
            return;
        };
        self.insert_symbols_batch_internal(symbols, Vec::new(), revision, || {}, || {});
    }

    #[cfg(test)]
    pub(crate) fn insert_symbols_batch_before_replacement_for_test(
        &self,
        symbols: Vec<crate::Symbol>,
        file_id: FileId,
        before_insert: impl FnOnce(),
    ) {
        let revision = {
            let mut inner = self.write_inner();
            self.reserve_external_source_revision_internal(&mut inner, file_id)
                .expect("test external source must remain live")
        };
        self.insert_symbols_batch_internal(symbols, Vec::new(), revision, before_insert, || {});
    }

    #[cfg(test)]
    pub(crate) fn insert_symbols_batch_for_revision_for_test(
        &self,
        symbols: Vec<crate::Symbol>,
        revision: ExternalSourceRevisionToken,
    ) -> bool {
        self.insert_symbols_batch_for_revision(symbols, Vec::new(), revision)
    }

    #[cfg(test)]
    pub(crate) fn insert_symbols_batch_for_revision_after_first_chunk_for_test(
        &self,
        symbols: Vec<crate::Symbol>,
        revision: ExternalSourceRevisionToken,
        after_first_chunk: impl FnOnce(),
    ) -> bool {
        self.insert_symbols_batch_internal(symbols, Vec::new(), revision, || {}, after_first_chunk)
    }

    #[cfg(test)]
    pub(crate) fn insert_symbols_and_generic_supertypes_after_first_chunk_for_test(
        &self,
        symbols: Vec<crate::Symbol>,
        generic_supertypes: Vec<crate::TypeGenericSupertypes>,
        revision: ExternalSourceRevisionToken,
        after_first_chunk: impl FnOnce(),
    ) -> bool {
        self.insert_symbols_batch_internal(
            symbols,
            generic_supertypes,
            revision,
            || {},
            after_first_chunk,
        )
    }

    fn insert_symbols_batch_for_revision(
        &self,
        symbols: Vec<crate::Symbol>,
        generic_supertypes: Vec<crate::TypeGenericSupertypes>,
        revision: ExternalSourceRevisionToken,
    ) -> bool {
        self.insert_symbols_batch_internal(symbols, generic_supertypes, revision, || {}, || {})
    }

    fn insert_symbols_batch_internal(
        &self,
        symbols: Vec<crate::Symbol>,
        generic_supertypes: Vec<crate::TypeGenericSupertypes>,
        revision: ExternalSourceRevisionToken,
        before_replacement_insert: impl FnOnce(),
        after_first_chunk: impl FnOnce(),
    ) -> bool {
        const BATCH_SIZE: usize = 100; // Insert 100 symbols at a time
        let file_id = revision.file_id;

        // Extraction runs outside the index and can outlive a cancelled build
        // refresh. Serialize publication with competing publications/removal,
        // then require the exact generation reserved before extraction. A
        // stable FileId alone cannot distinguish two refreshes of one retained
        // archive path.
        let file_lock = self.get_file_lock(file_id);
        let _file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self
            .inner
            .read()
            .external_source_revision_is_current(revision)
        {
            warn!(
                "Discarding symbols for superseded external source revision {:?}",
                revision
            );
            return false;
        }

        let is_replacement = self.inner.read().file_symbols.contains_key(&file_id);
        let symbols = symbols.into_iter().map(Arc::new).collect::<Vec<_>>();

        // A replacement is one externally visible source revision. Removing
        // the old declarations and then yielding between insertion chunks
        // exposed an absent/partially rebuilt dependency to readers. Keep the
        // index write lock across the complete swap. Extraction already ran
        // outside the lock, so this critical section is only map maintenance.
        if is_replacement {
            let mut inner = self.write_inner();
            if !inner.external_source_revision_is_current(revision) {
                warn!(
                    "Discarding replacement symbols for superseded external source revision {:?}",
                    revision
                );
                return false;
            }
            let mut affected = unregister_source_symbols_without_election(&mut inner, file_id);
            before_replacement_insert();
            inner.replace_file_generic_supertypes(file_id, generic_supertypes);
            affected.extend(register_source_symbols_without_election(
                &mut inner, symbols, file_id,
            ));
            reelect_affected_symbols(&mut inner, affected);
            inner.finish_external_source_publication(revision);
            self.clear_resolution_caches();
            return true;
        }

        // First publication may yield between owner-complete chunks: no prior
        // revision is being torn down, and a class is never split from its
        // fields/methods even when extraction order is interleaved.
        let chunks = owner_complete_external_chunks(symbols, BATCH_SIZE);
        if chunks.is_empty() {
            let mut inner = self.write_inner();
            if !inner.external_source_revision_is_current(revision) {
                warn!(
                    "Discarding empty publication for superseded external source revision {:?}",
                    revision
                );
                return false;
            }
            inner.replace_file_generic_supertypes(file_id, generic_supertypes);
            inner.finish_external_source_publication(revision);
            return true;
        }
        let chunk_count = chunks.len();
        let mut after_first_chunk = Some(after_first_chunk);
        for (index, chunk) in chunks.into_iter().enumerate() {
            {
                let mut inner = self.write_inner();
                if !inner.external_source_revision_is_current(revision) {
                    warn!(
                        "Stopping chunk publication for superseded external source revision {:?}",
                        revision
                    );
                    return false;
                }
                if index == 0 {
                    inner.mark_external_source_publication_in_progress(revision);
                    inner.replace_file_generic_supertypes(file_id, generic_supertypes.clone());
                }
                // Make exactly this owner-complete chunk queryable. Typed
                // definition readers consult the candidate registry directly,
                // so candidate registration cannot precede election.
                register_source_symbols_without_election(
                    &mut inner,
                    chunk.iter().cloned(),
                    file_id,
                );
                reelect_appended_symbols(&mut inner, chunk);
                if index + 1 == chunk_count {
                    inner.finish_external_source_publication(revision);
                }
                self.clear_resolution_caches();
            }
            // Test hooks and production yielding must run without the index
            // lock so a newer refresh can atomically retract this generation.
            if index == 0 {
                after_first_chunk
                    .take()
                    .expect("first-chunk callback runs at most once")();
            }
        }
        true
    }
}
