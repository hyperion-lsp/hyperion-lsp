// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Atomic provider-generation publication and rollback.

use super::{
    advance_provider_revision, allocate_provider_publication, drop_provider_value_without_unwind,
    prepare_provider, provider_callback, provider_metadata, sort_provider_entries, Arc,
    ExternalSourceIdentity, ExternalSourceRefreshTicket, ExternalSourceRevisionToken,
    GenerationBoundSourceProvider, GenerationSourceRoute, HashMap, HashSet,
    PendingProviderPublication, PreparedProvider, ProviderEntry, ProviderGarbage, ProviderHandle,
    ProviderState, SourceProvider, SourceProviderChain, DEFAULT_SOURCE_PROVIDER_PRIORITY,
};

/// Exact source-provider transaction captured before archive publication.
///
/// The planned identities are shared with the chain's pending state. Finishing
/// a transaction can therefore classify results without resolving paths or
/// trusting a caller-supplied failure set after the filesystem has changed.
#[must_use = "a source-provider publication must be finished or aborted"]
pub(crate) struct SourceProviderPublication {
    publication: u64,
    ticket: ExternalSourceRefreshTicket,
    planned_source_identities: Arc<HashSet<ExternalSourceIdentity>>,
}

/// Callback-heavy, immutable preparation for one provider publication.
///
/// Construct this before acquiring the matching WorkspaceIndex owner gate.
/// Commit consumes it through [`SourceProviderChain::begin_prepared_group_publication`],
/// which performs only bounded in-memory state mutation.
#[must_use = "prepared provider state may own snapshots whose Drop must run outside the workspace owner gate"]
pub(crate) struct PreparedSourceProviderPublication {
    ticket: ExternalSourceRefreshTicket,
    provider_state_revision: u64,
    planned_source_identities: Arc<HashSet<ExternalSourceIdentity>>,
    current_routes: HashMap<ExternalSourceIdentity, GenerationSourceRoute>,
    fallback_routes: HashMap<ExternalSourceIdentity, GenerationSourceRoute>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreparedPublicationCommitError {
    MissingPreparation,
    Superseded,
    StaleProviderState,
}

/// Callback-free provider replacement prepared before entering a workspace
/// owner gate.
#[must_use = "prepared provider state may own a provider whose Drop must run outside the workspace owner gate"]
pub(crate) struct PreparedSourceProviderReplacement {
    provider: Option<PreparedProvider>,
}

/// Provider state retired by a callback-free commit mutation.
///
/// Keep this value alive until after releasing the matching WorkspaceIndex
/// owner gate. Its destructor releases provider snapshots under isolated panic
/// boundaries and may execute arbitrary provider destructors.
#[must_use = "retired providers must be held until after the workspace owner gate is released"]
pub(crate) struct DeferredProviderRetirement {
    garbage: Option<ProviderGarbage>,
}

impl DeferredProviderRetirement {
    const fn new(garbage: ProviderGarbage) -> Self {
        Self {
            garbage: Some(garbage),
        }
    }
}

impl Drop for DeferredProviderRetirement {
    fn drop(&mut self) {
        if let Some(garbage) = self.garbage.take() {
            drop_provider_value_without_unwind(garbage);
        }
    }
}

/// Exact rollback for an observed provider refresh which never reaches a
/// successful group replacement/publication.
pub(crate) struct SourceProviderRefreshGuard {
    chain: Arc<SourceProviderChain>,
    ticket: Option<ExternalSourceRefreshTicket>,
}

impl SourceProviderRefreshGuard {
    pub(crate) fn disarm(mut self) {
        self.ticket.take();
    }
}

impl Drop for SourceProviderRefreshGuard {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.chain.abort_group_refresh(&ticket);
        }
    }
}
struct PreparedProviderSnapshot {
    provider: Option<ProviderHandle>,
    priority: u32,
    name: &'static str,
}

enum InstalledProviderFallback {
    None,
    DirectRoutes(HashMap<ExternalSourceIdentity, GenerationSourceRoute>),
    Provider {
        provider: ProviderHandle,
        priority: u32,
        name: &'static str,
    },
}

fn snapshot_provider_without_unwind(
    provider: Option<&ProviderHandle>,
    source_identities: &HashSet<ExternalSourceIdentity>,
    cached_metadata: Option<(u32, &'static str)>,
) -> Option<PreparedProviderSnapshot> {
    let Some(provider) = provider else {
        return Some(PreparedProviderSnapshot {
            provider: None,
            priority: DEFAULT_SOURCE_PROVIDER_PRIORITY,
            name: "unavailable external source",
        });
    };
    let (priority, name) = match cached_metadata {
        Some(metadata) => metadata,
        None => provider_metadata(provider).ok()?,
    };
    let snapshot = provider_callback("preparing a bounded refresh snapshot", || {
        provider.snapshot_for_sources(source_identities)
    })
    .ok()?;
    match snapshot {
        Some(snapshot) => {
            let snapshot = ProviderHandle::new(snapshot);
            let (priority, name) = provider_metadata(&snapshot).ok()?;
            Some(PreparedProviderSnapshot {
                provider: Some(snapshot),
                priority,
                name,
            })
        }
        None => Some(PreparedProviderSnapshot {
            provider: None,
            priority,
            name,
        }),
    }
}

fn routes_for_snapshot_without_unwind(
    snapshot: PreparedProviderSnapshot,
    source_identities: &HashSet<ExternalSourceIdentity>,
) -> Option<HashMap<ExternalSourceIdentity, GenerationSourceRoute>> {
    provider_callback("preparing exact identity routes", || {
        source_identities
            .iter()
            .map(|source_identity| {
                let route = snapshot
                    .provider
                    .as_ref()
                    .filter(|provider| provider.owns_source_identity(source_identity))
                    .map_or(
                        GenerationSourceRoute::Unavailable {
                            priority: snapshot.priority,
                            source_revision: None,
                        },
                        |provider| GenerationSourceRoute::Available {
                            provider: provider.clone(),
                            priority: snapshot.priority,
                            name: snapshot.name,
                            source_revision: None,
                        },
                    );
                (source_identity.clone(), route)
            })
            .collect()
    })
    .ok()
}

fn effective_pending_routes(
    pending: &PendingProviderPublication,
) -> HashMap<ExternalSourceIdentity, GenerationSourceRoute> {
    pending
        .blocked_source_identities
        .iter()
        .map(|source_identity| {
            let route = if let Some(source_revision) =
                pending.successful_source_revisions.get(source_identity)
            {
                let mut route = pending
                    .current_routes
                    .get(source_identity)
                    .cloned()
                    .unwrap_or(GenerationSourceRoute::Unavailable {
                        priority: DEFAULT_SOURCE_PROVIDER_PRIORITY,
                        source_revision: None,
                    });
                route.bind_source_revision(*source_revision);
                route
            } else {
                pending
                    .fallback_routes
                    .get(source_identity)
                    .cloned()
                    .unwrap_or(GenerationSourceRoute::Unavailable {
                        priority: DEFAULT_SOURCE_PROVIDER_PRIORITY,
                        source_revision: None,
                    })
            };
            (source_identity.clone(), route)
        })
        .collect()
}

fn pending_matches(
    pending: &PendingProviderPublication,
    publication: &SourceProviderPublication,
) -> bool {
    pending.ticket == publication.ticket
        && Arc::ptr_eq(
            &pending.blocked_source_identities,
            &publication.planned_source_identities,
        )
}

/// Convert one pending capability into terminal exact routes without invoking
/// provider code. The caller owns the provider-state write lock.
fn finalize_pending_publication(state: &mut ProviderState, publication: u64) -> ProviderGarbage {
    let Some(pending) = state.pending_publications.remove(&publication) else {
        return ProviderGarbage::default();
    };
    let routes = effective_pending_routes(&pending);
    let priority = routes
        .values()
        .map(|route| match route {
            GenerationSourceRoute::Available { priority, .. }
            | GenerationSourceRoute::Unavailable { priority, .. } => *priority,
        })
        .min()
        .unwrap_or(DEFAULT_SOURCE_PROVIDER_PRIORITY);
    let routes = Arc::new(routes);
    let routed: Arc<dyn SourceProvider> = Arc::new(GenerationBoundSourceProvider {
        routes: Arc::clone(&routes),
        priority,
    });
    let entries = replace_group_in_state(
        state,
        pending.group.as_ref(),
        Some(&pending.ticket),
        Some(PreparedProvider {
            provider: ProviderHandle::new(routed),
            priority,
            name: "generation-bound external source",
            exact_routes: Some(routes),
        }),
    );
    ProviderGarbage {
        entries,
        pending: vec![pending],
    }
}

fn remove_pending_where(
    state: &mut ProviderState,
    mut predicate: impl FnMut(&PendingProviderPublication) -> bool,
) -> Vec<PendingProviderPublication> {
    let publications: Vec<_> = state
        .pending_publications
        .iter()
        .filter_map(|(publication, pending)| predicate(pending).then_some(*publication))
        .collect();
    publications
        .into_iter()
        .filter_map(|publication| state.pending_publications.remove(&publication))
        .collect()
}

fn remove_provider_entries_where(
    state: &mut ProviderState,
    mut predicate: impl FnMut(&ProviderEntry) -> bool,
) -> Vec<ProviderEntry> {
    let mut retained = Vec::with_capacity(state.providers.len());
    let mut removed = Vec::new();
    for entry in std::mem::take(&mut state.providers) {
        if predicate(&entry) {
            removed.push(entry);
        } else {
            retained.push(entry);
        }
    }
    state.providers = retained;
    removed
}

impl SourceProviderChain {
    /// Observe the workspace's logical refresh before asynchronous resolution.
    ///
    /// The workspace ticket is the sole generation authority. An older task
    /// that reaches this side registry after a newer workspace refresh began is
    /// rejected instead of minting an independently newer side generation.
    pub fn observe_group_refresh(&self, ticket: &ExternalSourceRefreshTicket) -> bool {
        self.observe_group_refresh_internal(ticket, true)
    }

    fn observe_group_refresh_internal(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        reuse_existing: bool,
    ) -> bool {
        if !ticket.is_latest() {
            return false;
        }
        let mut state = self.state().write();
        match state.group_tickets.get(ticket.owner()) {
            Some(current) if !current.has_same_authority(ticket) => false,
            Some(current) if current.generation() > ticket.generation() => false,
            Some(current) if current == ticket => reuse_existing,
            _ => {
                state
                    .group_tickets
                    .insert(Arc::from(ticket.owner()), ticket.clone());
                advance_provider_revision(&mut state);
                true
            }
        }
    }

    pub(crate) fn observe_group_refresh_guard(
        self: &Arc<Self>,
        ticket: &ExternalSourceRefreshTicket,
    ) -> Option<SourceProviderRefreshGuard> {
        self.observe_group_refresh_internal(ticket, false)
            .then(|| SourceProviderRefreshGuard {
                chain: Arc::clone(self),
                ticket: Some(ticket.clone()),
            })
    }

    /// Replace the provider owned by a refreshable subsystem.
    ///
    /// Passing `None` removes the group's previous provider. This keeps build
    /// refreshes from retaining obsolete archive handles and source caches.
    pub fn replace_group(&self, group: &str, provider: Option<Arc<dyn SourceProvider>>) {
        let provider = match provider
            .map(ProviderHandle::new)
            .map(prepare_provider)
            .transpose()
        {
            Ok(provider) => provider,
            Err(provider) => {
                drop_provider_value_without_unwind(provider);
                return;
            }
        };
        let garbage = {
            let mut state = self.state().write();
            state.group_tickets.remove(group);
            let pending =
                remove_pending_where(&mut state, |pending| pending.group.as_ref() == group);
            let entries = replace_group_in_state(&mut state, group, None, provider);
            ProviderGarbage { entries, pending }
        };
        // Provider Drop implementations may release archive handles or call
        // back into the chain. Never execute them beneath the write lock.
        drop_provider_value_without_unwind(garbage);
    }

    /// Replace a group only if `ticket` still owns its latest refresh attempt.
    ///
    /// Returns `false` without changing the chain when a newer refresh was
    /// reserved while this task was resolving archives.
    pub fn replace_group_for_refresh(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        provider: Option<Arc<dyn SourceProvider>>,
    ) -> bool {
        let Ok(prepared) = Self::prepare_group_replacement(provider) else {
            return false;
        };
        let mut prepared = Some(prepared);
        let Some(retirement) = self.replace_group_for_refresh_prepared(ticket, &mut prepared)
        else {
            return false;
        };
        drop(retirement);
        true
    }

    /// Run provider metadata callbacks before entering a WorkspaceIndex owner
    /// gate. A failed callback rejects the replacement without changing state.
    pub(crate) fn prepare_group_replacement(
        provider: Option<Arc<dyn SourceProvider>>,
    ) -> Result<PreparedSourceProviderReplacement, ()> {
        let provider = match provider
            .map(ProviderHandle::new)
            .map(prepare_provider)
            .transpose()
        {
            Ok(provider) => provider,
            Err(provider) => {
                drop_provider_value_without_unwind(provider);
                return Err(());
            }
        };
        Ok(PreparedSourceProviderReplacement { provider })
    }

    /// Commit a prevalidated provider replacement without callbacks or provider
    /// destruction. On failure `prepared` remains owned by the caller. On
    /// success, hold the returned retirement until the workspace owner gate has
    /// been released.
    pub(crate) fn replace_group_for_refresh_prepared(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        prepared: &mut Option<PreparedSourceProviderReplacement>,
    ) -> Option<DeferredProviderRetirement> {
        if !ticket.is_latest() || prepared.is_none() {
            return None;
        }
        let garbage = {
            let mut state = self.state().write();
            if state.group_tickets.get(ticket.owner()) != Some(ticket) {
                return None;
            }
            let provider = prepared.take()?.provider;
            let pending = remove_pending_where(&mut state, |pending| {
                pending.group.as_ref() == ticket.owner() && !pending.ticket.is_committed()
            });
            let entries =
                replace_group_in_state(&mut state, ticket.owner(), Some(ticket), provider);
            ProviderGarbage { entries, pending }
        };
        Some(DeferredProviderRetirement::new(garbage))
    }

    /// Make every desired source ID request-invisible before WorkspaceIndex
    /// starts publishing the matching archive generation.
    ///
    /// The prior group providers are retained as immutable snapshots for
    /// per-source fallback if individual archive publication later fails.
    #[cfg(test)]
    pub(crate) fn begin_group_publication(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        source_identities: &[ExternalSourceIdentity],
        current: Option<Arc<dyn SourceProvider>>,
    ) -> Option<SourceProviderPublication> {
        let prepared = self.prepare_group_publication(ticket, source_identities, current)?;
        let mut prepared = Some(prepared);
        self.begin_prepared_group_publication(&mut prepared).ok()
    }

    /// Prepare bounded exact routes while no WorkspaceIndex owner gate is held.
    /// This is the only publication phase which invokes provider callbacks.
    pub(crate) fn prepare_group_publication(
        &self,
        ticket: &ExternalSourceRefreshTicket,
        source_identities: &[ExternalSourceIdentity],
        current: Option<Arc<dyn SourceProvider>>,
    ) -> Option<PreparedSourceProviderPublication> {
        let current = current.map(ProviderHandle::new);
        if !ticket.is_latest() {
            return None;
        }
        let planned_source_identities: Arc<HashSet<ExternalSourceIdentity>> =
            Arc::new(source_identities.iter().cloned().collect());
        let (pending_fallback, installed_fallback, provider_state_revision) = {
            let state = self.state().read();
            if state.group_tickets.get(ticket.owner()) != Some(ticket)
                || state
                    .pending_publications
                    .values()
                    .any(|pending| pending.ticket == *ticket)
            {
                return None;
            }
            let pending_fallback = state
                .pending_publications
                .values()
                .find(|pending| {
                    pending.group.as_ref() == ticket.owner() && pending.ticket.is_committed()
                })
                .map(effective_pending_routes);
            let installed_fallback = if pending_fallback.is_some() {
                InstalledProviderFallback::None
            } else {
                state
                    .providers
                    .iter()
                    .find(|entry| {
                        entry.group.as_deref() == Some(ticket.owner())
                            && entry
                                .ticket
                                .as_ref()
                                .is_some_and(ExternalSourceRefreshTicket::is_committed)
                    })
                    .map_or(InstalledProviderFallback::None, |entry| {
                        entry.exact_routes.as_ref().map_or_else(
                            || InstalledProviderFallback::Provider {
                                provider: entry.provider.clone(),
                                priority: entry.priority,
                                name: entry.name,
                            },
                            |routes| {
                                InstalledProviderFallback::DirectRoutes(
                                    planned_source_identities
                                        .iter()
                                        .map(|source_identity| {
                                            (
                                                source_identity.clone(),
                                                routes.get(source_identity).cloned().unwrap_or(
                                                    GenerationSourceRoute::Unavailable {
                                                        priority: entry.priority,
                                                        source_revision: None,
                                                    },
                                                ),
                                            )
                                        })
                                        .collect(),
                                )
                            },
                        )
                    })
            };
            (pending_fallback, installed_fallback, state.revision)
        };

        // All provider callbacks run before the WorkspaceIndex commit. A bad
        // plugin/provider can therefore fail this refresh without stranding a
        // committed workspace generation or unwinding through shared mutation.
        let current =
            snapshot_provider_without_unwind(current.as_ref(), &planned_source_identities, None)?;
        let current_routes =
            routes_for_snapshot_without_unwind(current, &planned_source_identities)?;
        let fallback_routes = match pending_fallback {
            Some(routes) => planned_source_identities
                .iter()
                .map(|source_identity| {
                    (
                        source_identity.clone(),
                        routes.get(source_identity).cloned().unwrap_or(
                            GenerationSourceRoute::Unavailable {
                                priority: DEFAULT_SOURCE_PROVIDER_PRIORITY,
                                source_revision: None,
                            },
                        ),
                    )
                })
                .collect(),
            None => match installed_fallback {
                InstalledProviderFallback::None => routes_for_snapshot_without_unwind(
                    snapshot_provider_without_unwind(None, &planned_source_identities, None)?,
                    &planned_source_identities,
                )?,
                InstalledProviderFallback::DirectRoutes(routes) => routes,
                InstalledProviderFallback::Provider {
                    provider,
                    priority,
                    name,
                } => routes_for_snapshot_without_unwind(
                    snapshot_provider_without_unwind(
                        Some(&provider),
                        &planned_source_identities,
                        Some((priority, name)),
                    )?,
                    &planned_source_identities,
                )?,
            },
        };

        Some(PreparedSourceProviderPublication {
            ticket: ticket.clone(),
            provider_state_revision,
            planned_source_identities,
            current_routes,
            fallback_routes,
        })
    }

    /// Install one prepared publication without invoking or destroying provider
    /// code. On failure the caller retains `prepared` and must drop it only after
    /// releasing any WorkspaceIndex owner gate.
    pub(crate) fn begin_prepared_group_publication(
        &self,
        prepared: &mut Option<PreparedSourceProviderPublication>,
    ) -> Result<SourceProviderPublication, PreparedPublicationCommitError> {
        let prepared_ref = prepared
            .as_ref()
            .ok_or(PreparedPublicationCommitError::MissingPreparation)?;
        let ticket = &prepared_ref.ticket;
        let mut state = self.state().write();
        if !ticket.is_latest()
            || state.group_tickets.get(ticket.owner()) != Some(ticket)
            || state
                .pending_publications
                .values()
                .any(|pending| pending.ticket == *ticket)
        {
            return Err(PreparedPublicationCommitError::Superseded);
        }
        if state.revision != prepared_ref.provider_state_revision {
            return Err(PreparedPublicationCommitError::StaleProviderState);
        }
        let PreparedSourceProviderPublication {
            ticket,
            provider_state_revision: _,
            planned_source_identities,
            current_routes,
            fallback_routes,
        } = prepared
            .take()
            .ok_or(PreparedPublicationCommitError::MissingPreparation)?;
        let publication = allocate_provider_publication(&mut state);
        state.pending_publications.insert(
            publication,
            PendingProviderPublication {
                group: Arc::from(ticket.owner()),
                ticket: ticket.clone(),
                blocked_source_identities: Arc::clone(&planned_source_identities),
                current_routes,
                fallback_routes,
                successful_source_revisions: HashMap::new(),
            },
        );
        advance_provider_revision(&mut state);
        Ok(SourceProviderPublication {
            publication,
            ticket,
            planned_source_identities,
        })
    }

    /// Bind one successfully published archive to the exact WorkspaceIndex
    /// revision that owns its symbols and source route.
    pub(crate) fn record_group_publication_success_for_revision(
        &self,
        publication: &SourceProviderPublication,
        source_identity: &ExternalSourceIdentity,
        source_revision: ExternalSourceRevisionToken,
    ) -> bool {
        self.record_group_publication_success_inner(
            publication,
            source_identity,
            Some(source_revision),
        )
    }

    fn record_group_publication_success_inner(
        &self,
        publication: &SourceProviderPublication,
        source_identity: &ExternalSourceIdentity,
        source_revision: Option<ExternalSourceRevisionToken>,
    ) -> bool {
        let ticket = &publication.ticket;
        if !ticket.is_committed() {
            return false;
        }
        let mut state = self.state().write();
        let Some(pending) = state.pending_publications.get_mut(&publication.publication) else {
            return false;
        };
        if !pending_matches(pending, publication)
            || !pending.blocked_source_identities.contains(source_identity)
            || !ticket.is_committed()
        {
            return false;
        }
        if let Some(recorded_revision) = pending.successful_source_revisions.get(source_identity) {
            return *recorded_revision == source_revision;
        }
        pending
            .successful_source_revisions
            .insert(source_identity.clone(), source_revision);
        advance_provider_revision(&mut state);
        true
    }

    /// Publish the exact, already-recorded provider generation after archive
    /// indexing finishes. This performs no provider callbacks.
    #[cfg(test)]
    pub(crate) fn finish_group_publication(&self, publication: &SourceProviderPublication) -> bool {
        let Some(retirement) = self.finish_group_publication_deferred(publication) else {
            return false;
        };
        drop(retirement);
        true
    }

    /// Callback-free finish mutation for use beneath a WorkspaceIndex owner
    /// gate. Drop the returned retirement only after releasing that gate.
    pub(crate) fn finish_group_publication_deferred(
        &self,
        publication: &SourceProviderPublication,
    ) -> Option<DeferredProviderRetirement> {
        let ticket = &publication.ticket;
        if !ticket.is_committed() {
            return None;
        }
        let garbage = {
            let mut state = self.state().write();
            if !ticket.is_committed()
                || state
                    .pending_publications
                    .get(&publication.publication)
                    .is_none_or(|pending| !pending_matches(pending, publication))
            {
                return None;
            }
            let mut garbage = finalize_pending_publication(&mut state, publication.publication);
            garbage
                .pending
                .extend(remove_pending_where(&mut state, |pending| {
                    pending.group.as_ref() == ticket.owner()
                        && pending.ticket.has_same_authority(ticket)
                        && pending.ticket.generation() < ticket.generation()
                        && !pending.ticket.is_committed()
                }));
            garbage
        };
        Some(DeferredProviderRetirement::new(garbage))
    }

    /// Cancel request blocking when WorkspaceIndex rejected a refresh before
    /// committing its ticket. A superseded refresh cannot clear newer state.
    #[cfg(test)]
    pub(crate) fn abort_group_publication(&self, publication: &SourceProviderPublication) -> bool {
        let Some(retirement) = self.abort_group_publication_deferred(publication) else {
            return false;
        };
        drop(retirement);
        true
    }

    /// Callback-free abort mutation for use beneath a WorkspaceIndex owner
    /// gate. Drop the returned retirement only after releasing that gate.
    pub(crate) fn abort_group_publication_deferred(
        &self,
        publication: &SourceProviderPublication,
    ) -> Option<DeferredProviderRetirement> {
        let garbage = {
            let mut state = self.state().write();
            if state
                .pending_publications
                .get(&publication.publication)
                .is_none_or(|pending| !pending_matches(pending, publication))
            {
                return None;
            }
            if publication.ticket.is_committed() {
                finalize_pending_publication(&mut state, publication.publication)
            } else {
                let garbage = ProviderGarbage {
                    entries: Vec::new(),
                    pending: state
                        .pending_publications
                        .remove(&publication.publication)
                        .into_iter()
                        .collect(),
                };
                advance_provider_revision(&mut state);
                garbage
            }
        };
        Some(DeferredProviderRetirement::new(garbage))
    }

    /// Forget an exact observed attempt which did not publish a provider.
    /// Existing committed providers stay installed and a delayed abort cannot
    /// remove a newer observation or its blocked-source transaction.
    fn abort_group_refresh(&self, ticket: &ExternalSourceRefreshTicket) -> bool {
        let (garbage, changed) = {
            let mut state = self.state().write();
            let mut changed = if state.group_tickets.get(ticket.owner()) == Some(ticket) {
                state.group_tickets.remove(ticket.owner());
                advance_provider_revision(&mut state);
                true
            } else {
                false
            };
            let publications: Vec<_> = state
                .pending_publications
                .iter()
                .filter_map(|(publication, pending)| {
                    (pending.ticket == *ticket).then_some(*publication)
                })
                .collect();
            let mut garbage = ProviderGarbage::default();
            for publication in publications {
                changed = true;
                if ticket.is_committed() {
                    garbage.extend(finalize_pending_publication(&mut state, publication));
                } else if let Some(pending) = state.pending_publications.remove(&publication) {
                    garbage.pending.push(pending);
                    advance_provider_revision(&mut state);
                }
            }
            if !ticket.is_committed() {
                let entries = remove_provider_entries_where(&mut state, |entry| {
                    entry.ticket.as_ref() == Some(ticket)
                });
                if !entries.is_empty() {
                    changed = true;
                    garbage.entries.extend(entries);
                    advance_provider_revision(&mut state);
                }
            }
            (garbage, changed)
        };
        drop_provider_value_without_unwind(garbage);
        changed
    }
}

/// Prepare provider callbacks off-gate, then atomically install their pending
/// routes under the matching WorkspaceIndex owner gate.
///
/// A predecessor may finish another archive after preparation but before this
/// task acquires the gate. The provider revision fence detects that race; this
/// helper releases the gate, discards the stale snapshot off-gate, and retries
/// from current state. A misbehaving provider cannot force an unbounded loop.
pub(crate) fn begin_coordinated_external_publication(
    workspace_index: &hyperion_workspace::WorkspaceIndex,
    ticket: &ExternalSourceRefreshTicket,
    source_providers: Option<&Arc<SourceProviderChain>>,
    source_identities: &[ExternalSourceIdentity],
    current: Option<Arc<dyn SourceProvider>>,
) -> Result<
    (
        hyperion_workspace::ExternalSourcePublicationGuard,
        Option<SourceProviderPublication>,
    ),
    String,
> {
    const MAX_PROVIDER_STATE_RETRIES: usize = 16;

    let Some(providers) = source_providers else {
        let guard = workspace_index
            .begin_external_source_publication(ticket)
            .ok_or_else(|| "external publication was superseded before commit".to_string())?;
        return Ok((guard, None));
    };

    for attempt in 0..=MAX_PROVIDER_STATE_RETRIES {
        let mut prepared = Some(
            providers
                .prepare_group_publication(
                    ticket,
                    source_identities,
                    current.as_ref().map(Arc::clone),
                )
                .ok_or_else(|| {
                    "source-provider publication was superseded during preparation".to_string()
                })?,
        );
        let guard = workspace_index
            .begin_external_source_publication(ticket)
            .ok_or_else(|| "external publication was superseded before commit".to_string())?;
        match providers.begin_prepared_group_publication(&mut prepared) {
            Ok(publication) => return Ok((guard, Some(publication))),
            Err(PreparedPublicationCommitError::StaleProviderState)
                if attempt < MAX_PROVIDER_STATE_RETRIES =>
            {
                drop(guard);
                drop(prepared);
                std::thread::yield_now();
            }
            Err(PreparedPublicationCommitError::StaleProviderState) => {
                drop(guard);
                drop(prepared);
                return Err(format!(
                    "source-provider state changed during {} consecutive publication attempts",
                    MAX_PROVIDER_STATE_RETRIES + 1
                ));
            }
            Err(PreparedPublicationCommitError::Superseded) => {
                drop(guard);
                drop(prepared);
                return Err("source-provider publication was superseded before commit".to_string());
            }
            Err(PreparedPublicationCommitError::MissingPreparation) => {
                drop(guard);
                drop(prepared);
                return Err("source-provider publication preparation was lost".to_string());
            }
        }
    }
    unreachable!("the bounded publication retry loop returns on its final attempt")
}
fn replace_group_in_state(
    state: &mut ProviderState,
    group: &str,
    ticket: Option<&ExternalSourceRefreshTicket>,
    provider: Option<PreparedProvider>,
) -> Vec<ProviderEntry> {
    let mut retained = Vec::with_capacity(
        state
            .providers
            .len()
            .saturating_add(usize::from(provider.is_some())),
    );
    let mut removed = Vec::new();
    for entry in std::mem::take(&mut state.providers) {
        let keep = if entry.group.as_deref() != Some(group) {
            true
        } else if let Some(ticket) = ticket {
            match &entry.ticket {
                Some(existing) if existing == ticket => false,
                Some(existing)
                    if existing.has_same_authority(ticket)
                        && existing.generation() > ticket.generation() =>
                {
                    true
                }
                Some(existing) => existing.is_committed(),
                None => !ticket.is_committed(),
            }
        } else {
            false
        };
        if keep {
            retained.push(entry);
        } else {
            removed.push(entry);
        }
    }
    if let Some(provider) = provider {
        let publication = allocate_provider_publication(state);
        retained.push(ProviderEntry {
            group: Some(Arc::from(group)),
            ticket: ticket.cloned(),
            provider: provider.provider,
            priority: provider.priority,
            name: provider.name,
            exact_routes: provider.exact_routes,
            publication,
        });
    }
    sort_provider_entries(&mut retained);
    state.providers = retained;
    advance_provider_revision(state);
    removed
}

#[cfg(test)]
impl SourceProviderChain {
    /// Record one archive publication while the caller holds the matching
    /// WorkspaceIndex owner gate. This makes workspace symbols and their source
    /// route one indivisible step relative to a successor's classpath commit.
    pub(crate) fn record_group_publication_success(
        &self,
        publication: &SourceProviderPublication,
        source_identity: &ExternalSourceIdentity,
    ) -> bool {
        self.record_group_publication_success_inner(publication, source_identity, None)
    }
}
