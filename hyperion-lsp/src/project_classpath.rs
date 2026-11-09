// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cache of dependency-JAR lists already resolved by `gradle_integration` /
//! `maven_integration`.
//!
//! This lets the "▶ Run" project-classpath path (see `run_executor`) assemble
//! a runtime classpath for a real project without re-invoking Gradle/Maven at
//! request time.
//!
//! # Why a cache instead of re-deriving from `WorkspaceIndex`
//!
//! Neither `WorkspaceIndex` nor `Backend` persists the dependency-JAR list
//! computed during Gradle/Maven initialization: `initialize_gradle_support`
//! only returns the *source* paths it indexed (the JAR list it built is
//! consumed locally to call `index_jar_file` and then dropped), and
//! `initialize_maven_support`'s `MavenInitResult::jar_paths` is likewise
//! discarded by its callers once JAR indexing finishes. This module is the
//! only place that list survives past initialization. Production owns one
//! store per server session and still keys entries by filesystem workspace
//! identity so future multi-root sessions remain unambiguous.
//!
//! `gradle_integration`/`maven_integration` call [`record_dependency_jars`]
//! once they finish resolving a project's dependencies (at startup, and
//! again on every debounced build-file-triggered refresh -- see
//! `handlers::build_system::schedule_build_system_refresh`), so the cache
//! self-updates whenever the workspace's dependencies change; nothing in
//! `run_executor` ever shells out to Gradle/Maven itself.
//!
//! Entries are `None` (never resolved) vs. `Some(vec![])` (resolved, project
//! genuinely has zero dependency JARs) so callers can tell "unknown" from
//! "known empty" -- `run_executor`'s classpath assembly must fall back to
//! the single-file path rather than silently guessing when it's the former.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

use hyperion_build::gradle::GradleResolutionReport;
use hyperion_workspace::{
    path_lookup_key, ExternalSourceArchive, ExternalSourceIdentity, ExternalSourceRefreshTicket,
    PathLookupKey, SourceScopeInput,
};

struct CacheState {
    dependency_jars: HashMap<PathLookupKey, Vec<PathBuf>>,
    runtime_dependency_jars: HashMap<PathLookupKey, Vec<PathBuf>>,
    dependency_archive_versions: HashMap<PathLookupKey, Arc<[ExternalSourceArchive]>>,
    project_classpaths: HashMap<(PathLookupKey, PathLookupKey, String), KnownProjectClasspath>,
    source_scopes: HashMap<(PathLookupKey, PathLookupKey, String), KnownSourceScope>,
    gradle_resolutions: HashMap<(PathLookupKey, PathLookupKey), Arc<GradleResolutionReport>>,
    staged_project_classpaths:
        HashMap<PathLookupKey, HashMap<(PathLookupKey, String), KnownProjectClasspath>>,
    staged_source_scopes:
        HashMap<PathLookupKey, HashMap<(PathLookupKey, String), KnownSourceScope>>,
    staged_gradle_resolutions:
        HashMap<PathLookupKey, HashMap<PathLookupKey, Arc<GradleResolutionReport>>>,
    project_ownership: HashMap<(PathLookupKey, PathLookupKey, String), Arc<Path>>,
    pending_project_ownership: HashMap<PathLookupKey, HashMap<(PathLookupKey, String), Arc<Path>>>,
    unavailable_gradle_workspaces: std::collections::HashSet<PathLookupKey>,
    refresh_tickets: HashMap<PathLookupKey, ProjectClasspathRefreshProvenance>,
    published_tickets: HashMap<PathLookupKey, ProjectClasspathRefreshProvenance>,
    next_standalone_generation: NonZeroU64,
}

/// Session-owned resolved project state.
///
/// A server session must not share refresh authority or retained classpaths
/// with another session, even when both happen to open the same directory.
/// Clones share one session's registry and dropping the last clone releases
/// every retained report and path.
#[derive(Clone)]
pub struct ProjectClasspathStore {
    inner: Arc<RwLock<CacheState>>,
}

impl ProjectClasspathStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(CacheState {
                dependency_jars: HashMap::new(),
                runtime_dependency_jars: HashMap::new(),
                dependency_archive_versions: HashMap::new(),
                project_classpaths: HashMap::new(),
                source_scopes: HashMap::new(),
                gradle_resolutions: HashMap::new(),
                staged_project_classpaths: HashMap::new(),
                staged_source_scopes: HashMap::new(),
                staged_gradle_resolutions: HashMap::new(),
                project_ownership: HashMap::new(),
                pending_project_ownership: HashMap::new(),
                unavailable_gradle_workspaces: std::collections::HashSet::new(),
                refresh_tickets: HashMap::new(),
                published_tickets: HashMap::new(),
                next_standalone_generation: NonZeroU64::MIN,
            })),
        }
    }

    fn state(&self) -> &RwLock<CacheState> {
        &self.inner
    }
}

impl Default for ProjectClasspathStore {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ProjectClasspathStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ProjectClasspathStore")
            .field(&Arc::as_ptr(&self.inner))
            .finish()
    }
}

impl PartialEq for ProjectClasspathStore {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for ProjectClasspathStore {}

fn legacy_store() -> &'static ProjectClasspathStore {
    static STORE: OnceLock<ProjectClasspathStore> = OnceLock::new();
    STORE.get_or_init(ProjectClasspathStore::new)
}

pub(crate) fn compatibility_store() -> ProjectClasspathStore {
    legacy_store().clone()
}

/// Authority to publish one workspace's resolved execution/classpath state.
///
/// The ticket is reserved before asynchronous Gradle or Maven resolution. A
/// newer attempt immediately invalidates every pending write from the older
/// generation. The prior committed snapshot remains visible during resolution;
/// after WorkspaceIndex commits the new classpath, reads fail closed until the
/// matching staged side snapshot is atomically installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectClasspathRefreshTicket {
    store: ProjectClasspathStore,
    workspace: PathLookupKey,
    provenance: ProjectClasspathRefreshProvenance,
}

/// Rolls back one unfinished side-registry attempt on every error or async
/// cancellation path. Successful publication must explicitly disarm it.
pub(crate) struct ProjectClasspathRefreshGuard {
    ticket: Option<ProjectClasspathRefreshTicket>,
}

/// Values retired by a classpath commit while the WorkspaceIndex owner gate
/// is held. Keep this alive until the caller releases that gate so dropping a
/// large prior generation cannot lengthen the cross-registry critical section.
#[must_use = "retired classpath state must be dropped after the workspace owner gate"]
pub(crate) struct DeferredClasspathRetirement {
    _dependency_lists: Vec<Vec<PathBuf>>,
    _archive_sets: Vec<Arc<[ExternalSourceArchive]>>,
    _project_classpaths: Vec<(
        (PathLookupKey, PathLookupKey, String),
        KnownProjectClasspath,
    )>,
    _source_scopes: Vec<((PathLookupKey, PathLookupKey, String), KnownSourceScope)>,
    _gradle_resolutions: Vec<((PathLookupKey, PathLookupKey), Arc<GradleResolutionReport>)>,
    _project_ownership: Vec<((PathLookupKey, PathLookupKey, String), Arc<Path>)>,
    _staged_project_classpaths: Vec<HashMap<(PathLookupKey, String), KnownProjectClasspath>>,
    _staged_source_scopes: Vec<HashMap<(PathLookupKey, String), KnownSourceScope>>,
    _staged_gradle_resolutions: Vec<HashMap<PathLookupKey, Arc<GradleResolutionReport>>>,
    _pending_project_ownership: Vec<HashMap<(PathLookupKey, String), Arc<Path>>>,
}

impl DeferredClasspathRetirement {
    const fn new() -> Self {
        Self {
            _dependency_lists: Vec::new(),
            _archive_sets: Vec::new(),
            _project_classpaths: Vec::new(),
            _source_scopes: Vec::new(),
            _gradle_resolutions: Vec::new(),
            _project_ownership: Vec::new(),
            _staged_project_classpaths: Vec::new(),
            _staged_source_scopes: Vec::new(),
            _staged_gradle_resolutions: Vec::new(),
            _pending_project_ownership: Vec::new(),
        }
    }
}

/// Filesystem-resolved key for an empty build-system publication.
///
/// Construct this before acquiring `ExternalSourcePublicationGuard`: resolving
/// a path identity may perform filesystem I/O on some platforms.
pub(crate) struct PreparedProjectClasspathRetirement {
    workspace: PathLookupKey,
}

impl ProjectClasspathRefreshGuard {
    pub(crate) const fn ticket(&self) -> &ProjectClasspathRefreshTicket {
        self.ticket
            .as_ref()
            .expect("a disarmed project-classpath guard cannot be reused")
    }

    pub(crate) fn disarm(mut self) {
        self.ticket.take();
    }
}

impl ProjectClasspathRefreshTicket {
    pub(crate) fn published_source_scopes(&self) -> Vec<SourceScopeInput> {
        let state = self.store.state().read();
        if state.published_tickets.get(&self.workspace) != Some(&self.provenance)
            || !published_refresh_is_visible(&state, &self.workspace)
        {
            return Vec::new();
        }
        state
            .source_scopes
            .iter()
            .filter(|((workspace, _, _), _)| workspace == &self.workspace)
            .map(|(_, scope)| SourceScopeInput {
                requester_roots: scope.requester_roots.clone(),
                visible_source_roots: scope.visible_source_roots.clone(),
                visible_external_archives: scope.visible_external_archives.clone(),
            })
            .collect()
    }
}

impl Drop for ProjectClasspathRefreshGuard {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            abort_project_classpath_refresh(&ticket);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectClasspathRefreshProvenance {
    Workspace(ExternalSourceRefreshTicket),
    Standalone(NonZeroU64),
}

impl ProjectClasspathRefreshProvenance {
    fn is_latest(&self) -> bool {
        match self {
            Self::Workspace(ticket) => ticket.is_latest(),
            Self::Standalone(_) => true,
        }
    }

    fn is_committed(&self) -> bool {
        match self {
            Self::Workspace(ticket) => ticket.is_committed(),
            Self::Standalone(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownProjectClasspath {
    pub output_dirs: Vec<PathBuf>,
    pub compile_output_dirs: Vec<PathBuf>,
    pub dependency_jars: Vec<PathBuf>,
    pub compile_dependency_jars: Vec<PathBuf>,
}

/// Source roots visible to one Gradle source set, in compile classpath order.
///
/// The deepest matching requester root selects this scope; the visible roots
/// preserve one owner's precedence when different projects declare the same FQN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSourceScope {
    pub requester_roots: Vec<PathBuf>,
    pub visible_source_roots: Vec<PathBuf>,
    pub visible_external_archives: Vec<PathBuf>,
}

/// One generation-coherent view of every classpath component needed by a run
/// request. Retaining the publication provenance and archive versions lets the
/// caller revalidate immediately before each process/VM consumes the paths.
#[derive(Clone)]
pub(crate) struct ExecutionClasspathSnapshot {
    store: ProjectClasspathStore,
    workspace: PathLookupKey,
    provenance: Option<ProjectClasspathRefreshProvenance>,
    archives: Option<Arc<[ExternalSourceArchive]>>,
    pub(crate) project: Option<KnownProjectClasspath>,
    pub(crate) dependency_jars: Option<Vec<PathBuf>>,
    pub(crate) runtime_dependency_jars: Option<Vec<PathBuf>>,
}

impl ExecutionClasspathSnapshot {
    pub(crate) fn is_current(&self) -> bool {
        if !archive_fence_is_current(self.archives.as_deref()) {
            return false;
        }
        let state = self.store.state().read();
        published_refresh_is_visible(&state, &self.workspace)
            && state.published_tickets.get(&self.workspace) == self.provenance.as_ref()
    }
}

fn dedupe_in_order(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        if seen.insert(path_lookup_key(&path)) {
            deduped.push(path);
        }
    }
    deduped
}

/// Observe an exact project-classpath refresh for `workspace_root`.
///
/// The workspace's external-source ticket is the sole generation authority.
/// This side registry must never mint its own ticket: two independently
/// monotonic counters could crown different refreshes under opposite arrival
/// orders.
#[must_use = "the refresh ticket must guard every staged and final publication"]
pub fn begin_project_classpath_refresh(
    workspace_root: &Path,
    workspace_ticket: &ExternalSourceRefreshTicket,
) -> Option<ProjectClasspathRefreshTicket> {
    legacy_store().begin_project_classpath_refresh(workspace_root, workspace_ticket)
}

impl ProjectClasspathStore {
    #[must_use]
    pub fn begin_project_classpath_refresh(
        &self,
        workspace_root: &Path,
        workspace_ticket: &ExternalSourceRefreshTicket,
    ) -> Option<ProjectClasspathRefreshTicket> {
        if !workspace_ticket.is_latest() {
            return None;
        }
        self.begin_project_classpath_refresh_with_provenance(
            workspace_root,
            ProjectClasspathRefreshProvenance::Workspace(workspace_ticket.clone()),
            true,
        )
    }

    pub(crate) fn begin_project_classpath_refresh_guard(
        &self,
        workspace_root: &Path,
        workspace_ticket: &ExternalSourceRefreshTicket,
    ) -> Option<ProjectClasspathRefreshGuard> {
        if !workspace_ticket.is_latest() {
            return None;
        }
        self.begin_project_classpath_refresh_with_provenance(
            workspace_root,
            ProjectClasspathRefreshProvenance::Workspace(workspace_ticket.clone()),
            false,
        )
        .map(|ticket| ProjectClasspathRefreshGuard {
            ticket: Some(ticket),
        })
    }
}

/// Begin a refresh for the standalone Gradle API, which has no WorkspaceIndex.
/// The owning store supplies one per-root monotonic order.
pub fn begin_standalone_project_classpath_refresh(
    workspace_root: &Path,
) -> ProjectClasspathRefreshTicket {
    legacy_store().begin_standalone_project_classpath_refresh(workspace_root)
}

impl ProjectClasspathStore {
    #[must_use]
    pub fn begin_standalone_project_classpath_refresh(
        &self,
        workspace_root: &Path,
    ) -> ProjectClasspathRefreshTicket {
        let workspace = path_lookup_key(workspace_root);
        let mut state = self.state().write();
        let generation = state.next_standalone_generation;
        state.next_standalone_generation = NonZeroU64::new(
            generation
                .get()
                .checked_add(1)
                .expect("standalone project-classpath generation space exhausted"),
        )
        .expect("incrementing a non-zero standalone generation cannot produce zero");
        self.begin_project_classpath_refresh_locked(
            &mut state,
            workspace,
            ProjectClasspathRefreshProvenance::Standalone(generation),
        )
    }

    pub(crate) fn begin_standalone_project_classpath_refresh_guard(
        &self,
        workspace_root: &Path,
    ) -> ProjectClasspathRefreshGuard {
        ProjectClasspathRefreshGuard {
            ticket: Some(self.begin_standalone_project_classpath_refresh(workspace_root)),
        }
    }
}

impl ProjectClasspathStore {
    fn begin_project_classpath_refresh_with_provenance(
        &self,
        workspace_root: &Path,
        provenance: ProjectClasspathRefreshProvenance,
        reuse_existing: bool,
    ) -> Option<ProjectClasspathRefreshTicket> {
        let workspace = path_lookup_key(workspace_root);
        let mut state = self.state().write();
        if let ProjectClasspathRefreshProvenance::Workspace(next) = &provenance {
            if !workspace_authority_can_take_over(&state, &workspace, next) {
                return None;
            }
        }
        if let Some(current) = state.refresh_tickets.get(&workspace) {
            match (current, &provenance) {
                (_, next) if current == next => {
                    return reuse_existing.then(|| ProjectClasspathRefreshTicket {
                        store: self.clone(),
                        workspace,
                        provenance: next.clone(),
                    });
                }
                (
                    ProjectClasspathRefreshProvenance::Workspace(current),
                    ProjectClasspathRefreshProvenance::Workspace(next),
                ) if current.has_same_authority(next)
                    && current.generation() > next.generation() =>
                {
                    return None;
                }
                _ => {}
            }
        }
        Some(self.begin_project_classpath_refresh_locked(&mut state, workspace, provenance))
    }
}

/// Whether `next` may become the WorkspaceIndex-backed authority for this root.
///
/// A normal same-owner generation change shares one authority and is always
/// allowed to stage behind its last-good publication. A different build owner
/// may take over only after the prior owner's attempt and publication have both
/// become inactive. The owner-name check deliberately rejects a ticket minted
/// by a different WorkspaceIndex for the same logical owner: equal strings are
/// not proof of shared authority.
fn workspace_authority_can_take_over(
    state: &CacheState,
    workspace: &PathLookupKey,
    next: &ExternalSourceRefreshTicket,
) -> bool {
    let current_allows_handoff =
        state
            .refresh_tickets
            .get(workspace)
            .is_none_or(|current| match current {
                ProjectClasspathRefreshProvenance::Workspace(current)
                    if current.has_same_authority(next) =>
                {
                    true
                }
                ProjectClasspathRefreshProvenance::Workspace(current) => {
                    current.owner() != next.owner() && !current.is_latest()
                }
                ProjectClasspathRefreshProvenance::Standalone(_) => false,
            });
    let published_allows_handoff = state
        .published_tickets
        .get(workspace)
        .is_none_or(|published| match published {
            ProjectClasspathRefreshProvenance::Workspace(published)
                if published.has_same_authority(next) =>
            {
                true
            }
            ProjectClasspathRefreshProvenance::Workspace(published) => {
                published.owner() != next.owner() && !published.is_committed()
            }
            ProjectClasspathRefreshProvenance::Standalone(_) => false,
        });

    current_allows_handoff && published_allows_handoff
}

impl ProjectClasspathStore {
    fn begin_project_classpath_refresh_locked(
        &self,
        state: &mut CacheState,
        workspace: PathLookupKey,
        provenance: ProjectClasspathRefreshProvenance,
    ) -> ProjectClasspathRefreshTicket {
        state
            .refresh_tickets
            .insert(workspace.clone(), provenance.clone());

        // Resolve into generation-owned staging maps. The last committed snapshot
        // remains request-visible until WorkspaceIndex commits this generation;
        // its retained ticket then turns false and readers fail closed until this
        // staged snapshot is atomically installed.
        state
            .staged_project_classpaths
            .insert(workspace.clone(), HashMap::new());
        state
            .staged_source_scopes
            .insert(workspace.clone(), HashMap::new());
        state
            .staged_gradle_resolutions
            .insert(workspace.clone(), HashMap::new());
        state
            .pending_project_ownership
            .insert(workspace.clone(), HashMap::new());

        ProjectClasspathRefreshTicket {
            store: self.clone(),
            workspace,
            provenance,
        }
    }
}

fn refresh_is_current(state: &CacheState, ticket: &ProjectClasspathRefreshTicket) -> bool {
    state.refresh_tickets.get(&ticket.workspace) == Some(&ticket.provenance)
        && ticket.provenance.is_latest()
}

fn published_refresh_is_visible(state: &CacheState, workspace: &PathLookupKey) -> bool {
    !state.unavailable_gradle_workspaces.contains(workspace)
        && state
            .published_tickets
            .get(workspace)
            .is_none_or(ProjectClasspathRefreshProvenance::is_committed)
}

fn archive_fence_is_current(archives: Option<&[ExternalSourceArchive]>) -> bool {
    archives.is_none_or(|archives| {
        archives
            .iter()
            .all(ExternalSourceArchive::staged_version_is_current)
    })
}

fn clear_refresh_authority(state: &mut CacheState, workspace: &PathLookupKey) {
    let had_versioned_refresh = state.refresh_tickets.remove(workspace).is_some();
    state.published_tickets.remove(workspace);
    state.staged_project_classpaths.remove(workspace);
    state.staged_source_scopes.remove(workspace);
    state
        .source_scopes
        .retain(|(candidate, _, _), _| candidate != workspace);
    state.staged_gradle_resolutions.remove(workspace);
    state.dependency_archive_versions.remove(workspace);
    if had_versioned_refresh {
        state.pending_project_ownership.remove(workspace);
    }
}

/// Abandon only the exact unfinished attempt represented by `ticket`.
///
/// Last-good published state remains intact. A delayed abort after a newer
/// generation starts is a no-op and cannot erase the newer staging maps.
fn abort_project_classpath_refresh(ticket: &ProjectClasspathRefreshTicket) -> bool {
    let mut state = ticket.store.state().write();
    if state.refresh_tickets.get(&ticket.workspace) != Some(&ticket.provenance) {
        return false;
    }
    state.refresh_tickets.remove(&ticket.workspace);
    state.staged_project_classpaths.remove(&ticket.workspace);
    state.staged_source_scopes.remove(&ticket.workspace);
    state.staged_gradle_resolutions.remove(&ticket.workspace);
    state.pending_project_ownership.remove(&ticket.workspace);
    true
}

fn provenance_has_workspace_authority(
    provenance: &ProjectClasspathRefreshProvenance,
    ticket: &ExternalSourceRefreshTicket,
) -> bool {
    matches!(
        provenance,
        ProjectClasspathRefreshProvenance::Workspace(current)
            if current.has_same_authority(ticket)
    )
}

/// Retire project-side state owned by an external-source authority that has
/// just committed an empty classpath.
///
/// Cleanup is exact-authority scoped. A delayed cleanup from the old build
/// system becomes a successful no-op after another authority takes over, so it
/// cannot erase the replacement's staged or published snapshot. `false` means
/// the cleanup ticket itself was superseded before its WorkspaceIndex commit.
pub fn retire_project_classpath_for_external_refresh(
    workspace_root: &Path,
    workspace_ticket: &ExternalSourceRefreshTicket,
) -> bool {
    legacy_store().retire_project_classpath_for_external_refresh(workspace_root, workspace_ticket)
}

impl ProjectClasspathStore {
    pub fn retire_project_classpath_for_external_refresh(
        &self,
        workspace_root: &Path,
        workspace_ticket: &ExternalSourceRefreshTicket,
    ) -> bool {
        let prepared = self.prepare_project_classpath_retirement(workspace_root);
        self.retire_prepared_project_classpath_for_external_refresh(&prepared, workspace_ticket)
            .is_some()
    }

    pub(crate) fn prepare_project_classpath_retirement(
        &self,
        workspace_root: &Path,
    ) -> PreparedProjectClasspathRetirement {
        PreparedProjectClasspathRetirement {
            workspace: path_lookup_key(workspace_root),
        }
    }

    /// Commit an already prepared empty generation and return all displaced
    /// state for destruction after the WorkspaceIndex owner gate is released.
    pub(crate) fn retire_prepared_project_classpath_for_external_refresh(
        &self,
        prepared: &PreparedProjectClasspathRetirement,
        workspace_ticket: &ExternalSourceRefreshTicket,
    ) -> Option<DeferredClasspathRetirement> {
        let workspace = &prepared.workspace;
        let mut state = self.state().write();
        // Validate while holding the side-registry write lock. A newer same-owner
        // attempt that reserved its WorkspaceIndex generation before this point
        // must prevent us from clearing its staging. If it starts after this load,
        // this cleanup is the earlier linearized side-registry operation and its
        // `begin_project_classpath_refresh` will install fresh staging afterward.
        if !workspace_ticket.is_latest() || !workspace_ticket.is_committed() {
            return None;
        }
        let current = state.refresh_tickets.get(workspace);
        let published = state.published_tickets.get(workspace);
        let owns_current = current.is_some_and(|provenance| {
            provenance_has_workspace_authority(provenance, workspace_ticket)
        });
        let owns_published = published.is_some_and(|provenance| {
            provenance_has_workspace_authority(provenance, workspace_ticket)
        });
        let has_foreign_authority = current.is_some_and(|provenance| {
            !provenance_has_workspace_authority(provenance, workspace_ticket)
        }) || published.is_some_and(|provenance| {
            !provenance_has_workspace_authority(provenance, workspace_ticket)
        });

        if has_foreign_authority || (!owns_current && !owns_published) {
            return Some(DeferredClasspathRetirement::new());
        }

        let mut retirement = DeferredClasspathRetirement::new();
        retirement._project_classpaths.extend(
            state
                .project_classpaths
                .extract_if(|(candidate, _, _), _| candidate == workspace),
        );
        retirement._source_scopes.extend(
            state
                .source_scopes
                .extract_if(|(candidate, _, _), _| candidate == workspace),
        );
        if let Some(old) = state.staged_source_scopes.remove(workspace) {
            retirement._staged_source_scopes.push(old);
        }
        retirement._gradle_resolutions.extend(
            state
                .gradle_resolutions
                .extract_if(|(candidate, _), _| candidate == workspace),
        );
        retirement._project_ownership.extend(
            state
                .project_ownership
                .extract_if(|(candidate, _, _), _| candidate == workspace),
        );
        if let Some(old) = state.pending_project_ownership.remove(workspace) {
            retirement._pending_project_ownership.push(old);
        }
        if let Some(old) = state.dependency_jars.remove(workspace) {
            retirement._dependency_lists.push(old);
        }
        if let Some(old) = state.runtime_dependency_jars.remove(workspace) {
            retirement._dependency_lists.push(old);
        }
        if let Some(old) = state.dependency_archive_versions.remove(workspace) {
            retirement._archive_sets.push(old);
        }
        if let Some(old) = state.staged_project_classpaths.remove(workspace) {
            retirement._staged_project_classpaths.push(old);
        }
        if let Some(old) = state.staged_gradle_resolutions.remove(workspace) {
            retirement._staged_gradle_resolutions.push(old);
        }
        state.refresh_tickets.remove(workspace);
        state.published_tickets.remove(workspace);
        state.unavailable_gradle_workspaces.remove(workspace);
        Some(retirement)
    }
}

fn install_staged_gradle_state(
    state: &mut CacheState,
    workspace: &PathLookupKey,
    retirement: &mut DeferredClasspathRetirement,
) {
    retirement._project_classpaths.extend(
        state
            .project_classpaths
            .extract_if(|(candidate, _, _), _| candidate == workspace),
    );
    let staged_classpaths = state
        .staged_project_classpaths
        .remove(workspace)
        .unwrap_or_default();
    state
        .project_classpaths
        .extend(
            staged_classpaths
                .into_iter()
                .map(|((project_root, source_set), classpath)| {
                    ((workspace.clone(), project_root, source_set), classpath)
                }),
        );

    retirement._source_scopes.extend(
        state
            .source_scopes
            .extract_if(|(candidate, _, _), _| candidate == workspace),
    );
    let staged_scopes = state
        .staged_source_scopes
        .remove(workspace)
        .unwrap_or_default();
    state.source_scopes.extend(staged_scopes.into_iter().map(
        |((project_root, source_set), scope)| {
            ((workspace.clone(), project_root, source_set), scope)
        },
    ));

    retirement._gradle_resolutions.extend(
        state
            .gradle_resolutions
            .extract_if(|(candidate, _), _| candidate == workspace),
    );
    let staged_resolutions = state
        .staged_gradle_resolutions
        .remove(workspace)
        .unwrap_or_default();
    state.gradle_resolutions.extend(
        staged_resolutions
            .into_iter()
            .map(|(project_root, report)| ((workspace.clone(), project_root), report)),
    );

    let next_ownership = state
        .pending_project_ownership
        .remove(workspace)
        .unwrap_or_default();
    retirement._project_ownership.extend(
        state
            .project_ownership
            .extract_if(|(candidate, _, _), _| candidate == workspace),
    );
    state
        .project_ownership
        .extend(
            next_ownership
                .into_iter()
                .map(|((project_root, source_set), display_path)| {
                    ((workspace.clone(), project_root, source_set), display_path)
                }),
        );
}

/// Record the dependency JARs resolved for `workspace_root`, deduplicated
/// while preserving resolver order.
///
/// Called by `gradle_integration`/`maven_integration` after they finish
/// resolving a project's classpath -- never by `run_executor` itself, which
/// only reads via [`known_dependency_jars`].
pub fn record_dependency_jars(workspace_root: &Path, jars: Vec<PathBuf>) {
    let jars = dedupe_in_order(jars);
    let workspace = path_lookup_key(workspace_root);
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    state
        .dependency_jars
        .insert(workspace.clone(), jars.clone());
    state.runtime_dependency_jars.insert(workspace, jars);
}

pub fn publish_gradle_dependency_snapshot(
    workspace_root: &Path,
    dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
) {
    let workspace = path_lookup_key(workspace_root);
    let dependency_jars = dedupe_in_order(dependency_jars);
    let runtime_dependency_jars = dedupe_in_order(runtime_dependency_jars);
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    state
        .dependency_jars
        .insert(workspace.clone(), dependency_jars);
    state
        .runtime_dependency_jars
        .insert(workspace.clone(), runtime_dependency_jars);
    if let Some(next_ownership) = state.pending_project_ownership.remove(&workspace) {
        state
            .project_ownership
            .retain(|(candidate, _, _), _| candidate != &workspace);
        state
            .project_ownership
            .extend(next_ownership.into_iter().map(
                |((project_root, source_set), display_path)| {
                    ((workspace.clone(), project_root, source_set), display_path)
                },
            ));
    }
    state.unavailable_gradle_workspaces.remove(&workspace);
}

/// Atomically make the current Gradle refresh's staged module and dependency
/// state request-visible. Superseded refreshes leave the newer state untouched.
pub fn publish_gradle_dependency_snapshot_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
) -> bool {
    // Compatibility for embedders that do not yet stage archive generations.
    publish_gradle_dependency_snapshot_impl(
        ticket,
        dedupe_in_order(dependency_jars),
        dedupe_in_order(runtime_dependency_jars),
        None,
    )
    .is_some()
}

/// Publish a Gradle classpath fenced by the exact archive generations that
/// were staged for the corresponding workspace-index transaction.
pub fn publish_gradle_dependency_snapshot_with_archives_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
    staged_archives: Vec<ExternalSourceArchive>,
) -> bool {
    publish_gradle_dependency_snapshot_with_archives_deferred_for_refresh(
        ticket,
        dependency_jars,
        runtime_dependency_jars,
        staged_archives,
    )
    .is_some()
}

pub(crate) fn publish_gradle_dependency_snapshot_with_archives_deferred_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
    staged_archives: Vec<ExternalSourceArchive>,
) -> Option<DeferredClasspathRetirement> {
    if !canonical_archive_paths_cover(
        &staged_archives,
        dependency_jars.iter().chain(&runtime_dependency_jars),
    ) {
        return None;
    }
    publish_gradle_dependency_snapshot_impl(
        ticket,
        dependency_jars,
        runtime_dependency_jars,
        Some(staged_archives.into()),
    )
}

/// Resolve every staged Gradle dependency path to the canonical archive
/// generation captured by the surrounding workspace transaction.
///
/// Path identity resolution can touch the filesystem, so this preparation
/// must run before acquiring the WorkspaceIndex owner gate. Staged state is
/// not request-visible. A compare-before-swap prevents a concurrent writer for
/// the same refresh from being lost between the read and the prepared update.
pub fn bind_staged_gradle_classpaths_to_archives_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
    archives: &[ExternalSourceArchive],
) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Vec<ExternalSourceArchive>), String> {
    if !ticket.provenance.is_latest() {
        return Err("Gradle classpath refresh was superseded before binding".to_string());
    }
    let original = {
        let state = ticket.store.state().read();
        if !refresh_is_current(&state, ticket) {
            return Err("Gradle classpath refresh was superseded before binding".to_string());
        }
        state
            .staged_project_classpaths
            .get(&ticket.workspace)
            .cloned()
            .unwrap_or_default()
    };

    let captured: HashMap<_, _> = archives
        .iter()
        .enumerate()
        .filter_map(|(index, archive)| {
            Some((
                archive.identity().clone(),
                (archive.identity().archive_path()?.to_path_buf(), index),
            ))
        })
        .collect();
    let mut used = HashSet::new();
    let mut bind_paths = |paths: Vec<PathBuf>| -> Result<Vec<PathBuf>, String> {
        let mut seen = HashSet::new();
        let mut bound = Vec::with_capacity(paths.len());
        for path in paths {
            let identity = ExternalSourceIdentity::for_archive_path(&path);
            let Some((canonical, archive_index)) = captured.get(&identity) else {
                return Err(format!(
                    "Gradle classpath entry {} was not captured in the dependency archive generation",
                    path.display()
                ));
            };
            used.insert(*archive_index);
            if seen.insert(identity) {
                bound.push(canonical.clone());
            }
        }
        Ok(bound)
    };

    let dependency_jars = bind_paths(dependency_jars)?;
    let runtime_dependency_jars = bind_paths(runtime_dependency_jars)?;
    let mut rebound = original.clone();
    for classpath in rebound.values_mut() {
        classpath.compile_dependency_jars =
            bind_paths(std::mem::take(&mut classpath.compile_dependency_jars))?;
        classpath.dependency_jars = bind_paths(std::mem::take(&mut classpath.dependency_jars))?;
    }
    let used_archives = archives
        .iter()
        .enumerate()
        .filter(|(index, _)| used.contains(index))
        .map(|(_, archive)| archive.clone())
        .collect();

    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket)
        || state.staged_project_classpaths.get(&ticket.workspace) != Some(&original)
    {
        return Err("Gradle classpath staging changed while archive paths were bound".to_string());
    }
    state
        .staged_project_classpaths
        .insert(ticket.workspace.clone(), rebound);
    Ok((dependency_jars, runtime_dependency_jars, used_archives))
}

fn publish_gradle_dependency_snapshot_impl(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
    staged_archives: Option<Arc<[ExternalSourceArchive]>>,
) -> Option<DeferredClasspathRetirement> {
    if !ticket.provenance.is_latest() {
        return None;
    }
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) || !ticket.provenance.is_committed() {
        return None;
    }
    let workspace = &ticket.workspace;
    if let Some(archives) = staged_archives.as_deref() {
        if !staged_project_classpaths_are_canonical(&state, workspace, archives) {
            return None;
        }
    }
    let mut retirement = DeferredClasspathRetirement::new();
    if let Some(old) = state
        .dependency_jars
        .insert(workspace.clone(), dependency_jars)
    {
        retirement._dependency_lists.push(old);
    }
    if let Some(old) = state
        .runtime_dependency_jars
        .insert(workspace.clone(), runtime_dependency_jars)
    {
        retirement._dependency_lists.push(old);
    }
    match staged_archives {
        Some(archives) => {
            if let Some(old) = state
                .dependency_archive_versions
                .insert(workspace.clone(), archives)
            {
                retirement._archive_sets.push(old);
            }
        }
        None => {
            if let Some(old) = state.dependency_archive_versions.remove(workspace) {
                retirement._archive_sets.push(old);
            }
        }
    }
    install_staged_gradle_state(&mut state, workspace, &mut retirement);
    state
        .published_tickets
        .insert(workspace.clone(), ticket.provenance.clone());
    state.unavailable_gradle_workspaces.remove(workspace);
    Some(retirement)
}

/// Publish Maven's complete dependency list under the same authority that
/// guarded its external archive refresh.
pub fn publish_maven_dependency_snapshot_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
) -> bool {
    // Compatibility for embedders that do not yet stage archive generations.
    publish_maven_dependency_snapshot_impl(ticket, dedupe_in_order(dependency_jars), None).is_some()
}

/// Publish a Maven classpath fenced by the exact archive generations that
/// were staged for the corresponding workspace-index transaction.
pub fn publish_maven_dependency_snapshot_with_archives_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    staged_archives: Vec<ExternalSourceArchive>,
) -> bool {
    publish_maven_dependency_snapshot_with_archives_deferred_for_refresh(
        ticket,
        dependency_jars,
        staged_archives,
    )
    .is_some()
}

pub(crate) fn publish_maven_dependency_snapshot_with_archives_deferred_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    staged_archives: Vec<ExternalSourceArchive>,
) -> Option<DeferredClasspathRetirement> {
    if !canonical_archive_paths_cover(&staged_archives, dependency_jars.iter()) {
        return None;
    }
    publish_maven_dependency_snapshot_impl(ticket, dependency_jars, Some(staged_archives.into()))
}

fn canonical_archive_paths_cover<'a>(
    archives: &[ExternalSourceArchive],
    mut dependency_paths: impl Iterator<Item = &'a PathBuf>,
) -> bool {
    // Callers prepare canonical paths before entering the WorkspaceIndex owner
    // gate. Keep this final cross-registry commit strictly in-memory: resolving
    // another PathLookupKey here can open every archive on macOS.
    let canonical_paths: HashSet<_> = archives
        .iter()
        .filter_map(|archive| archive.identity().archive_path())
        .collect();
    dependency_paths.all(|path| canonical_paths.contains(path.as_path()))
}

fn staged_project_classpaths_are_canonical(
    state: &CacheState,
    workspace: &PathLookupKey,
    archives: &[ExternalSourceArchive],
) -> bool {
    let canonical_paths: HashSet<_> = archives
        .iter()
        .filter_map(|archive| archive.identity().archive_path())
        .collect();
    state
        .staged_project_classpaths
        .get(workspace)
        .into_iter()
        .flat_map(|classpaths| classpaths.values())
        .flat_map(|classpath| {
            classpath
                .compile_dependency_jars
                .iter()
                .chain(&classpath.dependency_jars)
        })
        .all(|path| canonical_paths.contains(path.as_path()))
}

fn publish_maven_dependency_snapshot_impl(
    ticket: &ProjectClasspathRefreshTicket,
    dependency_jars: Vec<PathBuf>,
    staged_archives: Option<Arc<[ExternalSourceArchive]>>,
) -> Option<DeferredClasspathRetirement> {
    if !ticket.provenance.is_latest() {
        return None;
    }
    let jars = dependency_jars;
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) || !ticket.provenance.is_committed() {
        return None;
    }
    let workspace = ticket.workspace.clone();
    let mut retirement = DeferredClasspathRetirement::new();
    if let Some(old) = state
        .dependency_jars
        .insert(workspace.clone(), jars.clone())
    {
        retirement._dependency_lists.push(old);
    }
    if let Some(old) = state
        .runtime_dependency_jars
        .insert(workspace.clone(), jars)
    {
        retirement._dependency_lists.push(old);
    }
    match staged_archives {
        Some(archives) => {
            if let Some(old) = state
                .dependency_archive_versions
                .insert(workspace.clone(), archives)
            {
                retirement._archive_sets.push(old);
            }
        }
        None => {
            if let Some(old) = state.dependency_archive_versions.remove(&workspace) {
                retirement._archive_sets.push(old);
            }
        }
    }
    // Per-module classpaths and source scopes staged for this refresh become
    // visible atomically with the dependency snapshot (empty for embedders that
    // stage nothing, which retires any previous per-module state).
    install_staged_gradle_state(&mut state, &workspace, &mut retirement);
    state
        .published_tickets
        .insert(workspace.clone(), ticket.provenance.clone());
    state.unavailable_gradle_workspaces.remove(&workspace);
    Some(retirement)
}

/// The dependency JARs known for `workspace_root`, if Gradle/Maven
/// integration has ever resolved them for it.
///
/// `None` means "never resolved" (the caller should treat this as unknown and
/// fall back rather than silently running with an empty classpath);
/// `Some(vec![])` means "resolved, and this project genuinely has no
/// dependency JARs".
pub fn known_dependency_jars(workspace_root: &Path) -> Option<Vec<PathBuf>> {
    legacy_store().known_dependency_jars(workspace_root)
}

impl ProjectClasspathStore {
    pub(crate) fn published_archive_generation_is_current(&self, workspace_root: &Path) -> bool {
        self.execution_classpath_snapshot(workspace_root, None)
            .is_some()
    }

    /// Snapshot the global and optional per-project execution classpaths under
    /// one store read, then validate their shared archive generation once.
    pub(crate) fn execution_classpath_snapshot(
        &self,
        workspace_root: &Path,
        project: Option<(&Path, &str)>,
    ) -> Option<ExecutionClasspathSnapshot> {
        let workspace = path_lookup_key(workspace_root);
        let project_key = project.map(|(root, source_set)| {
            (
                workspace.clone(),
                path_lookup_key(root),
                source_set.to_string(),
            )
        });
        let snapshot = {
            let state = self.state().read();
            if !published_refresh_is_visible(&state, &workspace) {
                return None;
            }
            ExecutionClasspathSnapshot {
                store: self.clone(),
                workspace: workspace.clone(),
                provenance: state.published_tickets.get(&workspace).cloned(),
                archives: state.dependency_archive_versions.get(&workspace).cloned(),
                project: project_key
                    .as_ref()
                    .and_then(|key| state.project_classpaths.get(key).cloned()),
                dependency_jars: state.dependency_jars.get(&workspace).cloned(),
                runtime_dependency_jars: state.runtime_dependency_jars.get(&workspace).cloned(),
            }
        };
        snapshot.is_current().then_some(snapshot)
    }

    pub fn known_dependency_jars(&self, workspace_root: &Path) -> Option<Vec<PathBuf>> {
        let workspace = path_lookup_key(workspace_root);
        let (jars, archives) = {
            let state = self.state().read();
            if !published_refresh_is_visible(&state, &workspace) {
                return None;
            }
            (
                state.dependency_jars.get(&workspace).cloned(),
                state.dependency_archive_versions.get(&workspace).cloned(),
            )
        };
        archive_fence_is_current(archives.as_deref())
            .then_some(jars)
            .flatten()
    }
}

pub fn known_runtime_dependency_jars(workspace_root: &Path) -> Option<Vec<PathBuf>> {
    legacy_store().known_runtime_dependency_jars(workspace_root)
}

impl ProjectClasspathStore {
    pub fn known_runtime_dependency_jars(&self, workspace_root: &Path) -> Option<Vec<PathBuf>> {
        let workspace = path_lookup_key(workspace_root);
        let (jars, archives) = {
            let state = self.state().read();
            if !published_refresh_is_visible(&state, &workspace) {
                return None;
            }
            (
                state.runtime_dependency_jars.get(&workspace).cloned(),
                state.dependency_archive_versions.get(&workspace).cloned(),
            )
        };
        archive_fence_is_current(archives.as_deref())
            .then_some(jars)
            .flatten()
    }
}

pub fn record_project_classpath(
    workspace_root: &Path,
    project_root: &Path,
    source_set: &str,
    output_dirs: Vec<PathBuf>,
    dependency_jars: Vec<PathBuf>,
) {
    let compile_output_dirs = output_dirs.clone();
    record_project_classpath_scoped(
        workspace_root,
        project_root,
        source_set,
        output_dirs,
        compile_output_dirs,
        dependency_jars.clone(),
        dependency_jars,
    );
}

pub fn record_project_classpath_scoped(
    workspace_root: &Path,
    project_root: &Path,
    source_set: &str,
    output_dirs: Vec<PathBuf>,
    compile_output_dirs: Vec<PathBuf>,
    compile_dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
) {
    let workspace = path_lookup_key(workspace_root);
    let project = path_lookup_key(project_root);
    let project_display: Arc<Path> = Arc::from(project_root);
    let key = (workspace.clone(), project.clone(), source_set.to_string());
    let classpath = KnownProjectClasspath {
        output_dirs: dedupe_in_order(output_dirs),
        compile_output_dirs: dedupe_in_order(compile_output_dirs),
        dependency_jars: dedupe_in_order(runtime_dependency_jars),
        compile_dependency_jars: dedupe_in_order(compile_dependency_jars),
    };
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    state.project_classpaths.insert(key.clone(), classpath);
    if state.unavailable_gradle_workspaces.contains(&workspace) {
        state
            .pending_project_ownership
            .entry(workspace)
            .or_default()
            .insert((project, source_set.to_string()), project_display);
    } else {
        state.project_ownership.insert(key, project_display);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_project_classpath_scoped_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    project_root: &Path,
    source_set: &str,
    output_dirs: Vec<PathBuf>,
    compile_output_dirs: Vec<PathBuf>,
    compile_dependency_jars: Vec<PathBuf>,
    runtime_dependency_jars: Vec<PathBuf>,
) -> bool {
    if !ticket.provenance.is_latest() {
        return false;
    }
    let project = path_lookup_key(project_root);
    let project_display: Arc<Path> = Arc::from(project_root);
    let classpath = KnownProjectClasspath {
        output_dirs: dedupe_in_order(output_dirs),
        compile_output_dirs: dedupe_in_order(compile_output_dirs),
        dependency_jars: dedupe_in_order(runtime_dependency_jars),
        compile_dependency_jars: dedupe_in_order(compile_dependency_jars),
    };
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) {
        return false;
    }
    state
        .staged_project_classpaths
        .entry(ticket.workspace.clone())
        .or_default()
        .insert((project.clone(), source_set.to_string()), classpath);
    state
        .pending_project_ownership
        .entry(ticket.workspace.clone())
        .or_default()
        .insert((project, source_set.to_string()), project_display);
    true
}

/// Stage one source-set view under the same refresh authority as its classpath.
pub fn record_source_scope_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    project_root: &Path,
    source_set: &str,
    requester_roots: Vec<PathBuf>,
    visible_source_roots: Vec<PathBuf>,
    visible_external_archives: Vec<PathBuf>,
) -> bool {
    let scope = KnownSourceScope {
        requester_roots: dedupe_in_order(requester_roots),
        visible_source_roots: dedupe_in_order(visible_source_roots),
        visible_external_archives: dedupe_in_order(visible_external_archives),
    };
    if scope.requester_roots.is_empty() || !ticket.provenance.is_latest() {
        return false;
    }
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) {
        return false;
    }
    state
        .staged_source_scopes
        .entry(ticket.workspace.clone())
        .or_default()
        .insert(
            (path_lookup_key(project_root), source_set.to_owned()),
            scope,
        );
    true
}

impl ProjectClasspathStore {
    /// Return a generation-coherent Gradle source scope for an indexed file.
    /// Custom source roots are matched directly, independently of a conventional
    /// `project/src/main` layout.
    pub fn source_scope_for_file(
        &self,
        workspace_root: &Path,
        file_path: &Path,
    ) -> Option<KnownSourceScope> {
        let workspace = path_lookup_key(workspace_root);
        let file = path_lookup_key(file_path);
        let (scope, archives) = {
            let state = self.state().read();
            if !published_refresh_is_visible(&state, &workspace) {
                return None;
            }
            let scope = state
                .source_scopes
                .iter()
                .filter(|((candidate, _, _), _)| candidate == &workspace)
                .flat_map(|(_, scope)| {
                    let file = file.as_path();
                    scope.requester_roots.iter().filter_map(move |root| {
                        let key = path_lookup_key(root);
                        file.starts_with(key.as_path())
                            .then_some((key.components().count(), scope))
                    })
                })
                .max_by_key(|(depth, _)| *depth)
                .map(|(_, scope)| scope.clone());
            (
                scope,
                state.dependency_archive_versions.get(&workspace).cloned(),
            )
        };
        archive_fence_is_current(archives.as_deref())
            .then_some(scope)
            .flatten()
    }
}

/// Look up the published Gradle source scope in the compatibility store.
pub fn known_source_scope_for_file(
    workspace_root: &Path,
    file_path: &Path,
) -> Option<KnownSourceScope> {
    legacy_store().source_scope_for_file(workspace_root, file_path)
}

/// Retain the canonical dependency report for build-file diagnostics.
pub fn record_gradle_resolution(
    workspace_root: &Path,
    project_root: &Path,
    report: GradleResolutionReport,
) {
    let workspace = path_lookup_key(workspace_root);
    let project = path_lookup_key(project_root);
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    state
        .gradle_resolutions
        .insert((workspace, project), Arc::new(report));
}

pub fn record_gradle_resolution_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    project_root: &Path,
    report: GradleResolutionReport,
) -> bool {
    let project = path_lookup_key(project_root);
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) {
        return false;
    }
    state
        .staged_gradle_resolutions
        .entry(ticket.workspace.clone())
        .or_default()
        .insert(project, Arc::new(report));
    true
}

/// Find the deepest ready Gradle project whose root contains `path`.
pub fn known_gradle_resolution_for_path(path: &Path) -> Option<Arc<GradleResolutionReport>> {
    let path = path_lookup_key(path);
    let state = legacy_store().state().read();
    state
        .gradle_resolutions
        .iter()
        .filter(|((workspace, project), _)| {
            published_refresh_is_visible(&state, workspace) && path.starts_with(project.as_path())
        })
        .max_by_key(|((_, project), _)| project.components().count())
        .map(|(_, report)| report.clone())
}

impl ProjectClasspathStore {
    /// Find the deepest ready Gradle project within one workspace.
    pub fn known_gradle_resolution_for_path(
        &self,
        workspace_root: &Path,
        path: &Path,
    ) -> Option<Arc<GradleResolutionReport>> {
        let workspace = path_lookup_key(workspace_root);
        let path = path_lookup_key(path);
        let state = self.state().read();
        if !published_refresh_is_visible(&state, &workspace) {
            return None;
        }
        state
            .gradle_resolutions
            .iter()
            .filter(|((candidate, project), _)| {
                candidate == &workspace && path.starts_with(project.as_path())
            })
            .max_by_key(|((_, project), _)| project.components().count())
            .map(|(_, report)| Arc::clone(report))
    }
}

/// Stage module ownership discovered from Gradle project parsing before a
/// classpath snapshot is available.
///
/// This keeps source-root routing precise during a failed or partial refresh
/// without making the classpath executable.
pub fn stage_project_ownership(workspace_root: &Path, project_root: &Path, source_set: &str) {
    let workspace = path_lookup_key(workspace_root);
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    let ownership = (path_lookup_key(project_root), source_set.to_string());
    let project_display: Arc<Path> = Arc::from(project_root);
    if state.unavailable_gradle_workspaces.contains(&workspace) {
        state
            .pending_project_ownership
            .entry(workspace)
            .or_default()
            .insert(ownership, project_display);
    } else {
        state
            .project_ownership
            .insert((workspace, ownership.0, ownership.1), project_display);
    }
}

pub fn stage_project_ownership_for_refresh(
    ticket: &ProjectClasspathRefreshTicket,
    project_root: &Path,
    source_set: &str,
) -> bool {
    let project_display: Arc<Path> = Arc::from(project_root);
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) {
        return false;
    }
    state
        .pending_project_ownership
        .entry(ticket.workspace.clone())
        .or_default()
        .insert(
            (path_lookup_key(project_root), source_set.to_string()),
            project_display,
        );
    true
}

pub fn known_project_classpath(
    workspace_root: &Path,
    project_root: &Path,
    source_set: &str,
) -> Option<KnownProjectClasspath> {
    legacy_store().known_project_classpath(workspace_root, project_root, source_set)
}

impl ProjectClasspathStore {
    pub fn known_project_classpath(
        &self,
        workspace_root: &Path,
        project_root: &Path,
        source_set: &str,
    ) -> Option<KnownProjectClasspath> {
        let workspace = path_lookup_key(workspace_root);
        let project = path_lookup_key(project_root);
        let (classpath, archives) = {
            let state = self.state().read();
            if !published_refresh_is_visible(&state, &workspace) {
                return None;
            }
            (
                state
                    .project_classpaths
                    .get(&(workspace.clone(), project, source_set.to_string()))
                    .cloned(),
                state.dependency_archive_versions.get(&workspace).cloned(),
            )
        };
        archive_fence_is_current(archives.as_deref())
            .then_some(classpath)
            .flatten()
    }
}

/// Find the cached Gradle module owning a conventional source root.
///
/// This is needed for included projects whose configuration comes from the
/// root build and therefore have no build.gradle file of their own.
pub fn known_project_for_source_root(
    workspace_root: &Path,
    source_root: &Path,
) -> Option<(PathBuf, String)> {
    legacy_store().known_project_for_source_root(workspace_root, source_root)
}

impl ProjectClasspathStore {
    pub fn known_project_for_source_root(
        &self,
        workspace_root: &Path,
        source_root: &Path,
    ) -> Option<(PathBuf, String)> {
        let workspace = path_lookup_key(workspace_root);
        let source_root = path_lookup_key(source_root);
        let state = self.state().read();
        let is_versioned = state.published_tickets.contains_key(&workspace)
            || state.refresh_tickets.contains_key(&workspace);
        if is_versioned && !published_refresh_is_visible(&state, &workspace) {
            return None;
        }
        let pending = (!is_versioned)
            .then(|| state.pending_project_ownership.get(&workspace))
            .flatten()
            .into_iter()
            .flat_map(|ownership| ownership.iter())
            .map(|((project_root, source_set), display_path)| {
                (
                    project_root.clone(),
                    source_set.clone(),
                    Arc::clone(display_path),
                )
            });
        let committed = state
            .project_ownership
            .iter()
            .filter(|((candidate, _, _), _)| candidate == &workspace)
            .map(|((_, project_root, source_set), display_path)| {
                (
                    project_root.clone(),
                    source_set.clone(),
                    Arc::clone(display_path),
                )
            });

        let selected = pending
            .chain(committed)
            .filter(|(project_root, source_set, _)| {
                source_root.starts_with(project_root.join("src").join(source_set))
            })
            .max_by_key(|(project_root, _, _)| project_root.components().count());
        drop(state);

        selected.map(|(project_root, source_set, display_path)| {
            let current_display_key = path_lookup_key(&display_path);
            let path = if current_display_key == project_root {
                display_path.as_ref().to_path_buf()
            } else {
                project_root.as_path().to_path_buf()
            };
            (path, source_set)
        })
    }
}

pub fn clear_project_classpaths(workspace_root: &Path) {
    let workspace = path_lookup_key(workspace_root);
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    state
        .project_classpaths
        .retain(|(candidate, _, _), _| candidate != &workspace);
    state
        .gradle_resolutions
        .retain(|(candidate, _), _| candidate != &workspace);
    state
        .unavailable_gradle_workspaces
        .insert(workspace.clone());
    state
        .pending_project_ownership
        .insert(workspace, HashMap::new());
}

pub fn mark_project_classpaths_ready(workspace_root: &Path) {
    let workspace = path_lookup_key(workspace_root);
    let mut state = legacy_store().state().write();
    clear_refresh_authority(&mut state, &workspace);
    state.unavailable_gradle_workspaces.remove(&workspace);
}

pub fn reset_project_classpaths(workspace_root: &Path) {
    legacy_store().reset_project_classpaths(workspace_root);
}

impl ProjectClasspathStore {
    pub fn reset_project_classpaths(&self, workspace_root: &Path) {
        let workspace = path_lookup_key(workspace_root);
        let mut state = self.state().write();
        clear_refresh_authority(&mut state, &workspace);
        state
            .project_classpaths
            .retain(|(candidate, _, _), _| candidate != &workspace);
        state
            .gradle_resolutions
            .retain(|(candidate, _), _| candidate != &workspace);
        state
            .project_ownership
            .retain(|(candidate, _, _), _| candidate != &workspace);
        state.pending_project_ownership.remove(&workspace);
        state.dependency_jars.remove(&workspace);
        state.runtime_dependency_jars.remove(&workspace);
        state.dependency_archive_versions.remove(&workspace);
        state.unavailable_gradle_workspaces.remove(&workspace);
    }

    #[cfg(test)]
    pub(crate) fn has_active_refresh(&self, workspace_root: &Path) -> bool {
        self.state()
            .read()
            .refresh_tickets
            .contains_key(&path_lookup_key(workspace_root))
    }
}

/// Clear side state only if `ticket` still owns the latest refresh.
pub fn reset_project_classpaths_for_refresh(ticket: &ProjectClasspathRefreshTicket) -> bool {
    let mut state = ticket.store.state().write();
    if !refresh_is_current(&state, ticket) {
        return false;
    }
    let workspace = &ticket.workspace;
    state
        .project_classpaths
        .retain(|(candidate, _, _), _| candidate != workspace);
    state
        .gradle_resolutions
        .retain(|(candidate, _), _| candidate != workspace);
    state
        .project_ownership
        .retain(|(candidate, _, _), _| candidate != workspace);
    state.pending_project_ownership.remove(workspace);
    state.dependency_jars.remove(workspace);
    state.runtime_dependency_jars.remove(workspace);
    state.dependency_archive_versions.remove(workspace);
    state.unavailable_gradle_workspaces.remove(workspace);
    clear_refresh_authority(&mut state, workspace);
    true
}

#[cfg(test)]
#[path = "project_classpath/tests.rs"]
mod tests;
