// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Ordered source-provider lookup and ownership.

mod transaction;

#[cfg(test)]
pub(super) use transaction::PreparedPublicationCommitError;
pub(crate) use transaction::{
    begin_coordinated_external_publication, SourceProviderPublication, SourceProviderRefreshGuard,
};

use super::{ExternalSourceRequest, MaterializedSourceStore, SourceContent, SourceProvider};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use hyperion_workspace::{
    ExternalSourceIdentity, ExternalSourceRefreshTicket, ExternalSourceRevisionToken,
};
use tracing::warn;

/// Manages multiple source providers with priority ordering.
///
/// Uses `RwLock` so providers can be registered after creation (e.g., from
/// a background thread that initializes the JDK source provider).
pub struct SourceProviderChain {
    state: OnceLock<parking_lot::RwLock<ProviderState>>,
    // Declared after `state` so provider Arcs are dropped before the chain's
    // store owner. JAR/JDK providers hold their own Arcs while usable.
    materialized_sources: OnceLock<Arc<MaterializedSourceStore>>,
}

pub(super) struct ProviderState {
    pub(super) providers: Vec<ProviderEntry>,
    group_tickets: HashMap<Arc<str>, ExternalSourceRefreshTicket>,
    pending_publications: HashMap<u64, PendingProviderPublication>,
    next_publication: u64,
    revision: u64,
}

/// Internal provider ownership with an isolated destructor boundary.
///
/// A single catch around a collection is insufficient: if one destructor
/// panics, drop glue releases the remaining elements while unwinding and a
/// second panic aborts the process. Every Arc is therefore released under its
/// own boundary. State mutation still moves handles off-lock before their last
/// release so reentrant destructors cannot deadlock the chain.
pub(super) struct ProviderHandle(Option<Arc<dyn SourceProvider>>);

impl ProviderHandle {
    pub(super) fn new(provider: Arc<dyn SourceProvider>) -> Self {
        Self(Some(provider))
    }

    fn as_arc(&self) -> &Arc<dyn SourceProvider> {
        self.0
            .as_ref()
            .expect("a live provider handle always owns its Arc")
    }
}

impl Clone for ProviderHandle {
    fn clone(&self) -> Self {
        Self(Some(Arc::clone(self.as_arc())))
    }
}

impl Deref for ProviderHandle {
    type Target = dyn SourceProvider;

    fn deref(&self) -> &Self::Target {
        self.as_arc().as_ref()
    }
}

impl Drop for ProviderHandle {
    fn drop(&mut self) {
        let Some(provider) = self.0.take() else {
            return;
        };
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(provider))).is_err() {
            warn!("Source provider panicked while being released");
        }
    }
}

impl ProviderState {
    fn new() -> Self {
        Self {
            providers: Vec::new(),
            group_tickets: HashMap::new(),
            pending_publications: HashMap::new(),
            next_publication: 1,
            revision: 1,
        }
    }
}

#[derive(Clone)]
pub(super) struct ProviderEntry {
    pub(super) group: Option<Arc<str>>,
    pub(super) ticket: Option<ExternalSourceRefreshTicket>,
    pub(super) provider: ProviderHandle,
    pub(super) priority: u32,
    pub(super) name: &'static str,
    pub(super) exact_routes: Option<Arc<HashMap<ExternalSourceIdentity, GenerationSourceRoute>>>,
    pub(super) publication: u64,
}

struct PreparedProvider {
    provider: ProviderHandle,
    priority: u32,
    name: &'static str,
    exact_routes: Option<Arc<HashMap<ExternalSourceIdentity, GenerationSourceRoute>>>,
}

struct PendingProviderPublication {
    group: Arc<str>,
    ticket: ExternalSourceRefreshTicket,
    blocked_source_identities: Arc<HashSet<ExternalSourceIdentity>>,
    current_routes: HashMap<ExternalSourceIdentity, GenerationSourceRoute>,
    fallback_routes: HashMap<ExternalSourceIdentity, GenerationSourceRoute>,
    successful_source_revisions:
        HashMap<ExternalSourceIdentity, Option<ExternalSourceRevisionToken>>,
}

#[derive(Default)]
struct ProviderGarbage {
    entries: Vec<ProviderEntry>,
    pending: Vec<PendingProviderPublication>,
}

impl ProviderGarbage {
    fn extend(&mut self, mut other: Self) {
        self.entries.append(&mut other.entries);
        self.pending.append(&mut other.pending);
    }
}

fn drop_provider_value_without_unwind<T>(value: T) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value))).is_err() {
        warn!("Source provider panicked while being released");
    }
}

const fn advance_provider_revision(state: &mut ProviderState) {
    state.revision = state
        .revision
        .checked_add(1)
        .expect("source-provider state revision space exhausted");
}
/// One committed classpath generation, with exact per-source fallback.
///
/// Successfully indexed sources route only to the new immutable snapshots.
/// Failed retained sources route only to the previous committed snapshots.
/// Every other source ID, including removed and newly failed archives, is
/// rejected.
#[derive(Clone)]
pub(super) enum GenerationSourceRoute {
    Available {
        provider: ProviderHandle,
        priority: u32,
        name: &'static str,
        source_revision: Option<ExternalSourceRevisionToken>,
    },
    Unavailable {
        priority: u32,
        source_revision: Option<ExternalSourceRevisionToken>,
    },
}

impl GenerationSourceRoute {
    const fn bind_source_revision(&mut self, source_revision: Option<ExternalSourceRevisionToken>) {
        match self {
            Self::Available {
                source_revision: route_revision,
                ..
            }
            | Self::Unavailable {
                source_revision: route_revision,
                ..
            } => *route_revision = source_revision,
        }
    }

    fn matches_source_revision(&self, requested: Option<ExternalSourceRevisionToken>) -> bool {
        requested.is_none_or(|requested| {
            matches!(
                self,
                Self::Available {
                    source_revision: Some(route_revision),
                    ..
                } | Self::Unavailable {
                    source_revision: Some(route_revision),
                    ..
                } if *route_revision == requested
            )
        })
    }
}

/// Flat, exact-identity routing for one committed classpath generation.
///
/// Routes point directly at immutable provider snapshots. Failed refreshes
/// copy these leaf routes instead of wrapping the prior generation, keeping
/// lookup depth and retained metadata bounded across an arbitrarily long
/// session.
struct GenerationBoundSourceProvider {
    routes: Arc<HashMap<ExternalSourceIdentity, GenerationSourceRoute>>,
    priority: u32,
}

impl SourceProvider for GenerationBoundSourceProvider {
    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        match self.routes.get(request.source_identity())? {
            route @ GenerationSourceRoute::Available { provider, .. }
                if route.matches_source_revision(request.source_revision()) =>
            {
                let can_provide = provider_callback("checking an exact source route", || {
                    provider.can_provide_exact(request)
                })
                .ok()?;
                can_provide.then_some(())?;
                provider_callback("reading an exact source route", || {
                    provider.get_source_exact(request)
                })
                .ok()
                .flatten()
            }
            GenerationSourceRoute::Available { .. } | GenerationSourceRoute::Unavailable { .. } => {
                None
            }
        }
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        self.routes
            .get(request.source_identity())
            .is_some_and(|route| match route {
                route @ GenerationSourceRoute::Available { provider, .. }
                    if route.matches_source_revision(request.source_revision()) =>
                {
                    provider_callback("checking an exact source route", || {
                        provider.can_provide_exact(request)
                    })
                    .unwrap_or(false)
                }
                GenerationSourceRoute::Available { .. }
                | GenerationSourceRoute::Unavailable { .. } => false,
            })
    }

    fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
        self.routes.contains_key(source_identity)
    }

    fn snapshot_for_sources(
        &self,
        source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        let routes: HashMap<_, _> = self
            .routes
            .iter()
            .filter(|(source_identity, _)| source_identities.contains(*source_identity))
            .map(|(source_identity, route)| (source_identity.clone(), route.clone()))
            .collect();
        if routes.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            routes: Arc::new(routes),
            priority: self.priority,
        }))
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn name(&self) -> &'static str {
        "generation-bound external source"
    }
}

const DEFAULT_SOURCE_PROVIDER_PRIORITY: u32 = 20;

fn provider_callback<T>(description: &str, callback: impl FnOnce() -> T) -> Result<T, ()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).map_err(|_| {
        warn!("Source provider panicked while {description}");
    })
}

fn provider_metadata(provider: &ProviderHandle) -> Result<(u32, &'static str), ()> {
    provider_callback("reporting its priority and name", || {
        (provider.priority(), provider.name())
    })
}

fn prepare_provider(provider: ProviderHandle) -> Result<PreparedProvider, ProviderHandle> {
    let Ok((priority, name)) = provider_metadata(&provider) else {
        return Err(provider);
    };
    Ok(PreparedProvider {
        provider,
        priority,
        name,
        exact_routes: None,
    })
}

enum ExactLookupKind {
    Routed {
        provider: Option<ProviderHandle>,
        name: &'static str,
    },
    Generic {
        provider: ProviderHandle,
        name: &'static str,
    },
}

struct ExactLookupCandidate {
    publication: u64,
    priority: u32,
    kind: ExactLookupKind,
}
impl SourceProviderChain {
    /// Create a new empty provider chain
    pub const fn new() -> Self {
        Self {
            state: OnceLock::new(),
            materialized_sources: OnceLock::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn with_materialized_sources(
        materialized_sources: Arc<MaterializedSourceStore>,
    ) -> Self {
        Self {
            state: OnceLock::from(parking_lot::RwLock::new(ProviderState::new())),
            materialized_sources: OnceLock::from(materialized_sources),
        }
    }

    pub(super) fn state(&self) -> &parking_lot::RwLock<ProviderState> {
        self.state
            .get_or_init(|| parking_lot::RwLock::new(ProviderState::new()))
    }

    /// Return the session store that must be shared by JDK and dependency
    /// source providers registered in this chain.
    pub fn materialized_sources(&self) -> Arc<MaterializedSourceStore> {
        Arc::clone(
            self.materialized_sources
                .get_or_init(|| Arc::new(MaterializedSourceStore::new())),
        )
    }

    /// Register a provider (automatically sorts by priority).
    /// Thread-safe — can be called from background threads.
    pub fn register(&self, provider: Arc<dyn SourceProvider>) {
        let provider = match prepare_provider(ProviderHandle::new(provider)) {
            Ok(provider) => provider,
            Err(provider) => {
                drop_provider_value_without_unwind(provider);
                return;
            }
        };
        let mut state = self.state().write();
        let publication = allocate_provider_publication(&mut state);
        state.providers.push(ProviderEntry {
            group: None,
            ticket: None,
            provider: provider.provider,
            priority: provider.priority,
            name: provider.name,
            exact_routes: provider.exact_routes,
            publication,
        });
        advance_provider_revision(&mut state);
        sort_provider_entries(&mut state.providers);
    }

    /// Legacy FQN-only lookup retained for source compatibility.
    ///
    /// A refreshable group is unavailable while any of its source identities
    /// are being replaced because this API cannot say which identity it means.
    pub fn get_source(&self, fqn: &str) -> Option<(SourceContent, &'static str)> {
        let (providers, revision): (Vec<_>, _) = {
            let state = self.state().read();
            (
                state
                    .providers
                    .iter()
                    .filter(|entry| legacy_provider_entry_is_visible(&state, entry))
                    .cloned()
                    .collect(),
                state.revision,
            )
        };
        for entry in providers {
            let can_provide = provider_callback("checking a legacy source route", || {
                entry.provider.can_provide(fqn)
            })
            .ok()?;
            if !can_provide {
                continue;
            }
            let content = provider_callback("reading a legacy source route", || {
                entry.provider.get_source(fqn)
            })
            .ok()??;
            if legacy_provider_entry_is_current(&self.state().read(), &entry, revision) {
                return Some((content, entry.name));
            }
            return None;
        }
        None
    }

    /// Legacy FQN-only capability check retained for source compatibility.
    pub fn can_provide(&self, fqn: &str) -> bool {
        let (providers, revision): (Vec<_>, _) = {
            let state = self.state().read();
            (
                state
                    .providers
                    .iter()
                    .filter(|entry| legacy_provider_entry_is_visible(&state, entry))
                    .cloned()
                    .collect(),
                state.revision,
            )
        };
        for entry in providers {
            let can_provide = match provider_callback("checking a legacy source route", || {
                entry.provider.can_provide(fqn)
            }) {
                Ok(can_provide) => can_provide,
                Err(()) => return false,
            };
            if can_provide {
                return legacy_provider_entry_is_current(&self.state().read(), &entry, revision);
            }
        }
        false
    }

    /// Get source for the exact external identity selected by WorkspaceIndex.
    pub fn get_source_exact(
        &self,
        source_identity: &ExternalSourceIdentity,
        fqn: &str,
    ) -> Option<(SourceContent, &'static str)> {
        self.get_source_for_request(ExternalSourceRequest::new(source_identity, fqn))
    }

    /// Get source only from the provider route bound to this exact workspace
    /// publication. Unversioned and differently versioned routes are ignored.
    pub fn get_source_exact_for_revision(
        &self,
        source_identity: &ExternalSourceIdentity,
        source_revision: ExternalSourceRevisionToken,
        fqn: &str,
    ) -> Option<(SourceContent, &'static str)> {
        self.get_source_for_request(ExternalSourceRequest::for_revision(
            source_identity,
            source_revision,
            fqn,
        ))
    }

    fn get_source_for_request(
        &self,
        request: ExternalSourceRequest<'_>,
    ) -> Option<(SourceContent, &'static str)> {
        let source_identity = request.source_identity();
        // Provider execution may open and parse an archive. Snapshot the cheap
        // Arc list so registration never waits on that I/O and providers may
        // safely trigger chain updates themselves.
        let (providers, revision) = {
            let state = self.state().read();
            if source_is_blocked(&state, source_identity) {
                return None;
            }
            (
                exact_lookup_candidates(&state, source_identity, request.source_revision()),
                state.revision,
            )
        };
        for candidate in providers {
            match candidate.kind {
                ExactLookupKind::Routed {
                    provider: Some(provider),
                    name,
                } => {
                    let can_provide = provider_callback("checking an exact source route", || {
                        provider.can_provide_exact(request)
                    })
                    .ok()?;
                    if !can_provide {
                        return None;
                    }
                    let content = provider_callback("reading an exact source route", || {
                        provider.get_source_exact(request)
                    })
                    .ok()??;
                    if provider_publication_is_current(
                        &self.state().read(),
                        candidate.publication,
                        source_identity,
                        revision,
                    ) {
                        return Some((content, name));
                    }
                    return None;
                }
                ExactLookupKind::Routed { provider: None, .. } => return None,
                ExactLookupKind::Generic { provider, name } => {
                    let authoritative =
                        provider_callback("checking exact source ownership", || {
                            provider.owns_source_identity(source_identity)
                        })
                        .ok()?;
                    let can_provide = provider_callback("checking an exact source route", || {
                        provider.can_provide_exact(request)
                    })
                    .ok()?;
                    if can_provide {
                        let content = provider_callback("reading an exact source route", || {
                            provider.get_source_exact(request)
                        })
                        .ok()??;
                        if provider_publication_is_current(
                            &self.state().read(),
                            candidate.publication,
                            source_identity,
                            revision,
                        ) {
                            return Some((content, name));
                        }
                        return None;
                    }
                    if authoritative {
                        return None;
                    }
                }
            }
        }
        None
    }

    /// Check if any provider owns this exact external source/FQN pair.
    pub fn can_provide_exact(&self, source_identity: &ExternalSourceIdentity, fqn: &str) -> bool {
        self.can_provide_for_request(ExternalSourceRequest::new(source_identity, fqn))
    }

    /// Check only the provider route bound to this exact workspace
    /// publication.
    pub fn can_provide_exact_for_revision(
        &self,
        source_identity: &ExternalSourceIdentity,
        source_revision: ExternalSourceRevisionToken,
        fqn: &str,
    ) -> bool {
        self.can_provide_for_request(ExternalSourceRequest::for_revision(
            source_identity,
            source_revision,
            fqn,
        ))
    }

    fn can_provide_for_request(&self, request: ExternalSourceRequest<'_>) -> bool {
        let source_identity = request.source_identity();
        let (providers, revision) = {
            let state = self.state().read();
            if source_is_blocked(&state, source_identity) {
                return false;
            }
            (
                exact_lookup_candidates(&state, source_identity, request.source_revision()),
                state.revision,
            )
        };
        for candidate in providers {
            match candidate.kind {
                ExactLookupKind::Routed {
                    provider: Some(provider),
                    ..
                } => {
                    let can_provide =
                        match provider_callback("checking an exact source route", || {
                            provider.can_provide_exact(request)
                        }) {
                            Ok(can_provide) => can_provide,
                            Err(()) => return false,
                        };
                    return can_provide
                        && provider_publication_is_current(
                            &self.state().read(),
                            candidate.publication,
                            source_identity,
                            revision,
                        );
                }
                ExactLookupKind::Routed { provider: None, .. } => return false,
                ExactLookupKind::Generic { provider, .. } => {
                    let can_provide =
                        match provider_callback("checking an exact source route", || {
                            provider.can_provide_exact(request)
                        }) {
                            Ok(can_provide) => can_provide,
                            Err(()) => return false,
                        };
                    let authoritative =
                        match provider_callback("checking exact source ownership", || {
                            provider.owns_source_identity(source_identity)
                        }) {
                            Ok(authoritative) => authoritative,
                            Err(()) => return false,
                        };
                    if can_provide {
                        return provider_publication_is_current(
                            &self.state().read(),
                            candidate.publication,
                            source_identity,
                            revision,
                        );
                    }
                    if authoritative {
                        return false;
                    }
                }
            }
        }
        false
    }
}
fn source_is_blocked(state: &ProviderState, source_identity: &ExternalSourceIdentity) -> bool {
    state.pending_publications.values().any(|pending| {
        pending.ticket.is_committed() && pending.blocked_source_identities.contains(source_identity)
    })
}

fn exact_lookup_candidates(
    state: &ProviderState,
    source_identity: &ExternalSourceIdentity,
    source_revision: Option<ExternalSourceRevisionToken>,
) -> Vec<ExactLookupCandidate> {
    let mut candidates: Vec<_> = state
        .providers
        .iter()
        .filter(|entry| {
            entry
                .ticket
                .as_ref()
                .is_none_or(ExternalSourceRefreshTicket::is_committed)
        })
        .filter_map(|entry| {
            let (priority, kind) = match &entry.exact_routes {
                Some(routes) => match routes
                    .get(source_identity)
                    .filter(|route| route.matches_source_revision(source_revision))?
                {
                    GenerationSourceRoute::Available {
                        provider,
                        priority,
                        name,
                        ..
                    } => (
                        *priority,
                        ExactLookupKind::Routed {
                            provider: Some(provider.clone()),
                            name,
                        },
                    ),
                    GenerationSourceRoute::Unavailable { priority, .. } => (
                        *priority,
                        ExactLookupKind::Routed {
                            provider: None,
                            name: entry.name,
                        },
                    ),
                },
                None if source_revision.is_none() => (
                    entry.priority,
                    ExactLookupKind::Generic {
                        provider: entry.provider.clone(),
                        name: entry.name,
                    },
                ),
                None => return None,
            };
            Some(ExactLookupCandidate {
                publication: entry.publication,
                priority,
                kind,
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| right.publication.cmp(&left.publication))
    });
    candidates
}

fn provider_publication_is_current(
    state: &ProviderState,
    publication: u64,
    source_identity: &ExternalSourceIdentity,
    revision: u64,
) -> bool {
    state.revision == revision
        && !source_is_blocked(state, source_identity)
        && state.providers.iter().any(|current| {
            current.publication == publication
                && current
                    .ticket
                    .as_ref()
                    .is_none_or(ExternalSourceRefreshTicket::is_committed)
        })
}

fn legacy_provider_entry_is_visible(state: &ProviderState, entry: &ProviderEntry) -> bool {
    entry
        .ticket
        .as_ref()
        .is_none_or(ExternalSourceRefreshTicket::is_committed)
        && entry.group.as_ref().is_none_or(|group| {
            !state
                .pending_publications
                .values()
                .any(|pending| pending.group == *group && pending.ticket.is_committed())
        })
}

fn legacy_provider_entry_is_current(
    state: &ProviderState,
    entry: &ProviderEntry,
    revision: u64,
) -> bool {
    state.revision == revision
        && legacy_provider_entry_is_visible(state, entry)
        && state
            .providers
            .iter()
            .any(|current| current.publication == entry.publication)
}

fn allocate_provider_publication(state: &mut ProviderState) -> u64 {
    loop {
        let publication = state.next_publication;
        state.next_publication = state
            .next_publication
            .checked_add(1)
            .expect("source-provider publication space exhausted");
        if !state.pending_publications.contains_key(&publication)
            && !state
                .providers
                .iter()
                .any(|entry| entry.publication == publication)
        {
            return publication;
        }
    }
}

fn sort_provider_entries(entries: &mut [ProviderEntry]) {
    entries.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| right.publication.cmp(&left.publication))
    });
}
impl Default for SourceProviderChain {
    fn default() -> Self {
        Self::new()
    }
}
