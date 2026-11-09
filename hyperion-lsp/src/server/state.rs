// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Ownership boundaries for the LSP protocol facade.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use hyperion_semantics::db::FileId;
use hyperion_workspace::{Language, QueryEngine, WorkspaceIndex, WorkspaceRevision};
use parking_lot::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tower_lsp::Client;

use crate::completion::CompletionProvider;
use crate::file_registry::FileIdRegistry;
use crate::symbols::SymbolsProvider;

/// Language-feature implementations owned by the LSP transport facade.
///
/// Keeping the provider graph behind one boundary makes the distinction between
/// protocol/session state and language services explicit. Request handlers still
/// borrow the individual providers; this registry does not add synchronization or
/// indirection to the request path.
pub(super) struct LanguageServices {
    pub(super) completion: CompletionProvider,
    pub(super) type_hierarchy: crate::type_hierarchy::TypeHierarchyProvider,
    pub(super) symbols: SymbolsProvider,
    pub(super) source_providers: Arc<crate::source_provider::SourceProviderChain>,
    pub(super) rename: crate::rename::RenameProvider,
    pub(super) semantic_tokens: crate::semantic_tokens::SemanticTokensProvider,
    pub(super) folding: crate::folding::FoldingProvider,
    pub(super) code_lens: crate::code_lens::CodeLensProvider,
    pub(super) document_links: crate::document_links::DocumentLinksProvider,
    pub(super) security: Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>,
}

/// Open-document state and the identity maps that must stay consistent with it.
///
/// `open_by_id[file_id] == uri` if and only if `by_uri[uri].file_id == file_id`.
/// The reverse map keeps file-watcher open checks constant-time, while `file_ids`
/// remains authoritative for URI/path identity beyond the lifetime of an open buffer.
pub(super) struct DocumentStore {
    pub(super) by_uri: Arc<DashMap<String, DocumentState>>,
    pub(super) open_by_id: Arc<DashMap<FileId, String>>,
    /// FileIds owned by the editor, including the brief didOpen publication
    /// interval before the paired URI/document maps become visible.
    pub(super) editor_owned: Arc<DashSet<FileId>>,
    pub(super) file_ids: Arc<FileIdRegistry>,
    pub(super) next_change_version: Arc<std::sync::atomic::AtomicU64>,
}

impl DocumentStore {
    pub(super) fn new() -> Self {
        Self {
            by_uri: Arc::new(DashMap::new()),
            open_by_id: Arc::new(DashMap::new()),
            editor_owned: Arc::new(DashSet::new()),
            file_ids: Arc::new(FileIdRegistry::new()),
            next_change_version: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }
}

/// Exact authority for one phased diagnostic analysis.
///
/// A document version alone is insufficient: unchanged editor text can acquire
/// different bindings after any workspace publication. `generation` is unique
/// within the server session and identifies the URI analysis, while the other
/// fields pin the source and workspace revisions consumed by that analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticAnalysisTicket {
    generation: u64,
    document_change_version: u64,
    workspace_revision: WorkspaceRevision,
    file_id: FileId,
}

const _: () = assert!(std::mem::size_of::<DiagnosticAnalysisTicket>() <= 32);

#[derive(Default)]
struct DiagnosticUriState {
    generation: u64,
    latest_document_version: Option<i32>,
    ticket: Option<DiagnosticAnalysisTicket>,
}

/// What the client currently shows for a URI.
pub(crate) struct PublishedDiagnostics {
    pub(crate) diagnostics: Vec<tower_lsp::lsp_types::Diagnostic>,
    /// How many of them are parse errors (the first phase of that analysis).
    pub(crate) parse_errors: usize,
}

pub(crate) struct DiagnosticUriPublication {
    state: Mutex<DiagnosticUriState>,
    /// The set last sent to the client; see `publish_stable`.
    pub(crate) published: Mutex<Option<PublishedDiagnostics>>,
    /// Serializes every client notification for this URI, including didClose.
    pub(crate) publication: tokio::sync::Mutex<()>,
}

impl DiagnosticUriPublication {
    fn new() -> Self {
        Self {
            state: Mutex::new(DiagnosticUriState::default()),
            published: Mutex::new(None),
            publication: tokio::sync::Mutex::new(()),
        }
    }
}

/// Session-owned supersession and ordering state for diagnostic publications.
///
/// Computation may outlive the task that requested it. Only this authority can
/// turn a computed vector into a client notification, so aborting a task is an
/// optimization rather than a correctness mechanism.
pub(crate) struct DiagnosticPublicationState {
    active: AtomicBool,
    next_generation: AtomicU64,
    by_uri: DashMap<String, Arc<DiagnosticUriPublication>>,
}

/// How a supersession updates the tracked document lifetime.
enum DocumentVersionUpdate {
    /// Preserve the current open or closed document lifetime.
    Preserve,
    /// Replace the tracked version. `Some` marks the latest open version,
    /// while `None` retires the document lifetime.
    Set(Option<i32>),
}

impl DiagnosticPublicationState {
    pub(crate) fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
            next_generation: AtomicU64::new(1),
            by_uri: DashMap::new(),
        }
    }

    fn next_generation(&self) -> u64 {
        // Keep this atomic API available on older supported Rust compilers; its
        // replacement must not raise the project's compiler requirement for a lint.
        #[allow(deprecated)]
        let previous =
            self.next_generation
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                    generation.checked_add(1)
                });
        previous.expect("diagnostic publication generation overflowed")
    }

    pub(crate) fn uri_publication(&self, uri: &str) -> Arc<DiagnosticUriPublication> {
        Arc::clone(
            self.by_uri
                .entry(uri.to_owned())
                .or_insert_with(|| Arc::new(DiagnosticUriPublication::new()))
                .value(),
        )
    }

    fn supersede(&self, uri: &str, version_update: DocumentVersionUpdate) -> u64 {
        let generation = self.next_generation();
        let publication = self.uri_publication(uri);
        let mut state = publication.state.lock();
        state.generation = generation;
        if let DocumentVersionUpdate::Set(latest_document_version) = version_update {
            state.latest_document_version = latest_document_version;
        }
        state.ticket = None;
        generation
    }

    /// Reserve authority for an accepted didOpen, replacing every older URI lifetime.
    pub(crate) fn supersede_for_open(&self, uri: &str, version: i32) -> u64 {
        self.supersede(uri, DocumentVersionUpdate::Set(Some(version)))
    }

    /// Reserve authority for a monotonic didChange before its blocking publication.
    pub(crate) fn supersede_for_change(&self, uri: &str, version: i32) -> Option<u64> {
        let publication = self.uri_publication(uri);
        let mut state = publication.state.lock();
        let latest = state.latest_document_version?;
        if version <= latest {
            return None;
        }
        let generation = self.next_generation();
        state.generation = generation;
        state.latest_document_version = Some(version);
        state.ticket = None;
        Some(generation)
    }

    /// Supersede diagnostics because non-document semantic state changed.
    pub(crate) fn supersede_uri(&self, uri: &str) -> u64 {
        self.supersede(uri, DocumentVersionUpdate::Preserve)
    }

    /// Supersede diagnostics only while the URI still has an open document
    /// lifetime. A workspace publication may race with didClose after the open
    /// document snapshot was taken; it must not resurrect authority which the
    /// close already retired.
    pub(crate) fn supersede_open_uri(&self, uri: &str) -> Option<u64> {
        if !self.active.load(Ordering::Acquire) {
            return None;
        }
        let publication = self.uri_publication(uri);
        let mut state = publication.state.lock();
        if !self.active.load(Ordering::Acquire) || state.latest_document_version.is_none() {
            return None;
        }
        let generation = self.next_generation();
        state.generation = generation;
        state.ticket = None;
        Some(generation)
    }

    /// Retire the URI lifetime. The returned generation authorizes exactly one clear.
    pub(crate) fn supersede_for_close(&self, uri: &str) -> u64 {
        self.supersede(uri, DocumentVersionUpdate::Set(None))
    }

    pub(crate) fn is_generation_current(&self, uri: &str, generation: u64) -> bool {
        self.active.load(Ordering::Acquire)
            && self.by_uri.get(uri).is_some_and(|publication| {
                let state = publication.state.lock();
                state.generation == generation
            })
    }

    pub(crate) fn begin_analysis(
        &self,
        uri: &str,
        expected_generation: u64,
        document_change_version: u64,
        workspace_revision: WorkspaceRevision,
        file_id: FileId,
    ) -> Option<DiagnosticAnalysisTicket> {
        if !self.active.load(Ordering::Acquire) {
            return None;
        }
        let publication = self.uri_publication(uri);
        let mut state = publication.state.lock();
        if !self.active.load(Ordering::Acquire) || state.generation != expected_generation {
            return None;
        }
        let ticket = DiagnosticAnalysisTicket {
            generation: expected_generation,
            document_change_version,
            workspace_revision,
            file_id,
        };
        state.ticket = Some(ticket);
        Some(ticket)
    }

    pub(crate) fn is_current(&self, uri: &str, ticket: DiagnosticAnalysisTicket) -> bool {
        self.active.load(Ordering::Acquire)
            && self.by_uri.get(uri).is_some_and(|publication| {
                let state = publication.state.lock();
                state.ticket == Some(ticket)
            })
    }

    pub(crate) fn is_current_for(
        &self,
        uri: &str,
        ticket: DiagnosticAnalysisTicket,
        file_id: FileId,
        document_change_version: u64,
        workspace_revision: WorkspaceRevision,
    ) -> bool {
        self.is_current(uri, ticket)
            && ticket.file_id == file_id
            && ticket.document_change_version == document_change_version
            && ticket.workspace_revision == workspace_revision
    }

    pub(crate) fn is_closed_generation_current(&self, uri: &str, generation: u64) -> bool {
        self.active.load(Ordering::Acquire)
            && self.by_uri.get(uri).is_some_and(|publication| {
                let state = publication.state.lock();
                state.generation == generation && state.ticket.is_none()
            })
    }

    /// Permanently invalidate this session and wait for publications which had
    /// already crossed their final validation point.
    pub(crate) async fn shutdown(&self) {
        self.active.store(false, Ordering::Release);
        let publications: Vec<_> = self
            .by_uri
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect();
        for publication in &publications {
            publication.state.lock().ticket = None;
        }
        for publication in publications {
            let _guard = publication.publication.lock().await;
        }
        self.by_uri.clear();
    }
}

#[cfg(test)]
mod diagnostic_publication_tests {
    use super::*;

    async fn record_if_current(
        authority: &DiagnosticPublicationState,
        uri: &str,
        ticket: DiagnosticAnalysisTicket,
        file_id: FileId,
        change_version: u64,
        workspace_revision: WorkspaceRevision,
        value: &'static str,
        sink: &Mutex<Vec<&'static str>>,
    ) -> bool {
        let publication = authority.uri_publication(uri);
        let _guard = publication.publication.lock().await;
        if !authority.is_current_for(uri, ticket, file_id, change_version, workspace_revision) {
            return false;
        }
        sink.lock().push(value);
        true
    }

    fn fixture() -> (
        tempfile::TempDir,
        Arc<WorkspaceIndex>,
        &'static str,
        FileId,
        WorkspaceRevision,
    ) {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let uri = "file:///workspace/Analysis.java";
        let path = temp.path().join("Analysis.java");
        index
            .index_file_with_source(&path, "class Analysis {}")
            .expect("initial index");
        let file_id = index.get_file_id(&path).expect("file id");
        let revision = index.current_workspace_revision();
        (temp, index, uri, file_id, revision)
    }

    #[test]
    fn ticket_stays_within_one_compact_cache_line_half() {
        assert!(std::mem::size_of::<DiagnosticAnalysisTicket>() <= 32);
    }

    #[test]
    fn workspace_supersession_cannot_resurrect_a_closed_uri() {
        let authority = DiagnosticPublicationState::new();
        let uri = "file:///workspace/Closed.java";
        authority.supersede_for_open(uri, 1);
        let close_generation = authority.supersede_for_close(uri);

        assert_eq!(authority.supersede_open_uri(uri), None);
        assert!(authority.is_closed_generation_current(uri, close_generation));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocked_w1_phase_cannot_publish_after_w2() {
        let (_temp, index, uri, file_id, w1) = fixture();
        let authority = Arc::new(DiagnosticPublicationState::new());
        let sink = Arc::new(Mutex::new(Vec::new()));
        let generation1 = authority.supersede_for_open(uri, 1);
        let ticket1 = authority
            .begin_analysis(uri, generation1, 1, w1, file_id)
            .expect("W1 ticket");
        let reached = Arc::new(tokio::sync::Barrier::new(2));
        let release = Arc::new(tokio::sync::Barrier::new(2));
        let old = {
            let authority = Arc::clone(&authority);
            let sink = Arc::clone(&sink);
            let reached = Arc::clone(&reached);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                reached.wait().await;
                release.wait().await;
                record_if_current(&authority, uri, ticket1, file_id, 1, w1, "W1", &sink).await
            })
        };
        reached.wait().await;

        let path = index.get_file_path(file_id).expect("indexed path");
        index
            .index_file_with_source(&path, "class Analysis { int newer; }")
            .expect("W2 index");
        let w2 = index.current_workspace_revision();
        assert_ne!(w1, w2);
        let generation2 = authority.supersede_uri(uri);
        let ticket2 = authority
            .begin_analysis(uri, generation2, 1, w2, file_id)
            .expect("W2 ticket");
        assert!(record_if_current(&authority, uri, ticket2, file_id, 1, w2, "W2", &sink).await);
        release.wait().await;
        assert!(!old.await.expect("old phase task"));
        assert_eq!(sink.lock().as_slice(), ["W2"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn did_close_clear_is_final_for_a_blocked_phase() {
        let (_temp, _index, uri, file_id, revision) = fixture();
        let authority = Arc::new(DiagnosticPublicationState::new());
        let sink = Arc::new(Mutex::new(Vec::new()));
        let generation = authority.supersede_for_open(uri, 1);
        let ticket = authority
            .begin_analysis(uri, generation, 1, revision, file_id)
            .expect("analysis ticket");
        let reached = Arc::new(tokio::sync::Barrier::new(2));
        let release = Arc::new(tokio::sync::Barrier::new(2));
        let old = {
            let authority = Arc::clone(&authority);
            let sink = Arc::clone(&sink);
            let reached = Arc::clone(&reached);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                reached.wait().await;
                release.wait().await;
                record_if_current(&authority, uri, ticket, file_id, 1, revision, "old", &sink).await
            })
        };
        reached.wait().await;
        let close_generation = authority.supersede_for_close(uri);
        {
            let publication = authority.uri_publication(uri);
            let _guard = publication.publication.lock().await;
            assert!(authority.is_closed_generation_current(uri, close_generation));
            sink.lock().push("empty");
        }
        release.wait().await;
        assert!(!old.await.expect("old phase task"));
        assert_eq!(sink.lock().as_slice(), ["empty"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn document_change_during_phase_two_rejects_the_old_ticket() {
        let (_temp, _index, uri, file_id, revision) = fixture();
        let authority = Arc::new(DiagnosticPublicationState::new());
        let sink = Arc::new(Mutex::new(Vec::new()));
        let generation1 = authority.supersede_for_open(uri, 1);
        let ticket1 = authority
            .begin_analysis(uri, generation1, 1, revision, file_id)
            .expect("old ticket");
        let reached = Arc::new(tokio::sync::Barrier::new(2));
        let release = Arc::new(tokio::sync::Barrier::new(2));
        let old = {
            let authority = Arc::clone(&authority);
            let sink = Arc::clone(&sink);
            let reached = Arc::clone(&reached);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                reached.wait().await;
                release.wait().await;
                record_if_current(&authority, uri, ticket1, file_id, 1, revision, "old", &sink)
                    .await
            })
        };
        reached.wait().await;
        let generation2 = authority.supersede_for_change(uri, 2).expect("new change");
        let ticket2 = authority
            .begin_analysis(uri, generation2, 2, revision, file_id)
            .expect("new ticket");
        assert!(
            record_if_current(&authority, uri, ticket2, file_id, 2, revision, "new", &sink).await
        );
        release.wait().await;
        assert!(!old.await.expect("old phase task"));
        assert_eq!(sink.lock().as_slice(), ["new"]);
    }

    #[test]
    fn did_change_requires_an_open_document_lifetime() {
        let authority = DiagnosticPublicationState::new();
        let uri = "file:///workspace/Unknown.java";

        assert!(authority.supersede_for_change(uri, 1).is_none());
        authority.supersede_for_open(uri, 4);
        assert!(authority.supersede_for_change(uri, 5).is_some());
        authority.supersede_for_close(uri);
        assert!(authority.supersede_for_change(uri, 6).is_none());
    }

    #[test]
    fn semantic_supersession_preserves_document_lifetime_tracking() {
        let authority = DiagnosticPublicationState::new();
        let uri = "file:///workspace/Tracked.java";

        authority.supersede_for_open(uri, 4);
        authority.supersede_uri(uri);
        assert!(authority.supersede_for_change(uri, 5).is_some());

        authority.supersede_for_close(uri);
        authority.supersede_uri(uri);
        assert!(authority.supersede_for_change(uri, 6).is_none());
    }

    #[tokio::test]
    async fn phase_publications_are_serial_and_ordered() {
        let (_temp, _index, uri, file_id, revision) = fixture();
        let authority = DiagnosticPublicationState::new();
        let sink = Mutex::new(Vec::new());
        let generation = authority.supersede_for_open(uri, 1);
        let ticket = authority
            .begin_analysis(uri, generation, 1, revision, file_id)
            .expect("analysis ticket");
        for phase in ["phase1", "phase2", "phase3"] {
            assert!(
                record_if_current(&authority, uri, ticket, file_id, 1, revision, phase, &sink)
                    .await
            );
        }
        assert_eq!(sink.lock().as_slice(), ["phase1", "phase2", "phase3"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_permanently_supersedes_a_blocked_session() {
        let (_temp, _index, uri, file_id, revision) = fixture();
        let authority = Arc::new(DiagnosticPublicationState::new());
        let sink = Arc::new(Mutex::new(Vec::new()));
        let generation = authority.supersede_for_open(uri, 1);
        let ticket = authority
            .begin_analysis(uri, generation, 1, revision, file_id)
            .expect("analysis ticket");
        let reached = Arc::new(tokio::sync::Barrier::new(2));
        let release = Arc::new(tokio::sync::Barrier::new(2));
        let old = {
            let authority = Arc::clone(&authority);
            let sink = Arc::clone(&sink);
            let reached = Arc::clone(&reached);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                reached.wait().await;
                release.wait().await;
                record_if_current(
                    &authority, uri, ticket, file_id, 1, revision, "stale", &sink,
                )
                .await
            })
        };
        reached.wait().await;
        authority.shutdown().await;
        release.wait().await;
        assert!(!old.await.expect("old phase task"));
        assert!(authority
            .begin_analysis(uri, generation, 1, revision, file_id)
            .is_none());
        assert!(sink.lock().is_empty());
    }
}

type DebouncedTasks<K> = Arc<Mutex<std::collections::HashMap<K, (u64, JoinHandle<()>)>>>;

/// One global dependent-reindex worker with a deduplicated live queue.
///
/// A single worker avoids overlapping dependency walks from unrelated edits.
/// Files enqueued again while a batch is running remain pending for the next
/// batch, so the newest workspace revision always gets the final publication.
pub(crate) struct InvalidationCoordinator {
    pending_files: std::collections::HashSet<FileId>,
    pending_origins: std::collections::HashSet<FileId>,
    pending_readiness: Vec<SourceMutationReadinessGuard>,
    worker: Option<JoinHandle<()>>,
}

impl InvalidationCoordinator {
    pub(super) fn new() -> Self {
        Self {
            pending_files: std::collections::HashSet::new(),
            pending_origins: std::collections::HashSet::new(),
            pending_readiness: Vec::new(),
            worker: None,
        }
    }

    pub(super) fn enqueue(&mut self, origin: FileId, files: impl IntoIterator<Item = FileId>) {
        self.pending_origins.insert(origin);
        self.pending_files.extend(files);
    }

    pub(super) fn enqueue_readiness(&mut self, readiness: SourceMutationReadinessGuard) {
        self.pending_readiness.push(readiness);
    }

    pub(super) fn take_batch(
        &mut self,
    ) -> Option<(Vec<FileId>, Vec<FileId>, Vec<SourceMutationReadinessGuard>)> {
        if self.pending_files.is_empty() {
            self.pending_origins.clear();
            debug_assert!(self.pending_readiness.is_empty());
            return None;
        }

        let mut origins: Vec<_> = self.pending_origins.drain().collect();
        let mut files: Vec<_> = self.pending_files.drain().collect();
        let readiness = std::mem::take(&mut self.pending_readiness);
        origins.sort_unstable_by_key(|file_id| file_id.as_u32());
        files.sort_unstable_by_key(|file_id| file_id.as_u32());
        Some((origins, files, readiness))
    }

    pub(super) fn should_defer(&self, file_id: FileId) -> bool {
        self.pending_files.contains(&file_id)
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending_files.is_empty()
    }

    pub(super) fn worker_is_running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }

    pub(super) fn replace_worker(&mut self, worker: JoinHandle<()>) {
        self.worker.take();
        self.worker = Some(worker);
    }

    pub(super) fn clear_worker(&mut self) {
        self.worker.take();
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.pending_files.is_empty()
            && self.pending_readiness.is_empty()
            && self
                .worker
                .as_ref()
                .is_none_or(tokio::task::JoinHandle::is_finished)
    }
}

#[cfg(test)]
mod invalidation_coordinator_tests {
    use super::*;

    #[test]
    fn later_batches_coalesce_overlap_without_dropping_unique_history() {
        let mut coordinator = InvalidationCoordinator::new();
        let origin = FileId::new(1);
        let first_only = FileId::new(2);
        let overlap = FileId::new(3);
        let latest_only = FileId::new(4);

        coordinator.enqueue(origin, [first_only, overlap]);
        let (_, first_batch, _) = coordinator.take_batch().expect("first batch");
        assert_eq!(first_batch, vec![first_only, overlap]);

        // Simulate a newer edit arriving after the worker drained its batch.
        coordinator.enqueue(origin, [overlap, overlap, latest_only]);
        assert!(!coordinator.should_defer(first_only));
        assert!(coordinator.should_defer(overlap));

        let (_, latest_batch, _) = coordinator.take_batch().expect("latest batch");
        assert_eq!(latest_batch, vec![overlap, latest_only]);
        assert!(coordinator.take_batch().is_none());
    }

    #[test]
    fn queued_invalidation_owns_source_readiness_until_batch_completion() {
        let readiness = Arc::new(DestructiveRefactoringReadiness::new());
        for component in [
            RefactoringIndexComponent::Sources,
            RefactoringIndexComponent::Jdk,
            RefactoringIndexComponent::Gradle,
            RefactoringIndexComponent::Maven,
        ] {
            let ticket = readiness.begin(component);
            assert!(readiness.finish(ticket, true));
        }
        let guard = readiness.begin_source_mutation([PathBuf::from("src/Origin.java")]);

        let mut coordinator = InvalidationCoordinator::new();
        coordinator.enqueue(FileId::new(1), [FileId::new(2)]);
        coordinator.enqueue_readiness(guard);
        assert!(!readiness.is_ready());

        let (_, _, guards) = coordinator.take_batch().expect("queued batch");
        assert!(!readiness.is_ready());
        for guard in guards {
            guard.finish(true);
        }
        assert!(readiness.is_ready());
    }
}

/// Cancellation and supersession state for background work.
pub(super) struct BackgroundTasks {
    /// Pending parse per URI; the sequence prevents stale tasks removing replacements.
    pub(super) parsing: DebouncedTasks<String>,
    /// Coalesced Gradle/Maven refreshes keyed by workspace.
    pub(super) build_refresh: crate::handlers::build_system::BuildRefreshCoordinator,
    /// Coalesced transitive re-indexing, kept off the main LSP loop.
    pub(super) invalidation: Arc<Mutex<InvalidationCoordinator>>,
    /// Exact, session-scoped authority for every client diagnostic publication.
    pub(super) diagnostic_publication: Arc<DiagnosticPublicationState>,
}

impl BackgroundTasks {
    pub(super) fn new() -> Self {
        Self {
            parsing: Arc::new(Mutex::new(std::collections::HashMap::new())),
            build_refresh: crate::handlers::build_system::BuildRefreshCoordinator::default(),
            invalidation: Arc::new(Mutex::new(InvalidationCoordinator::new())),
            diagnostic_publication: Arc::new(DiagnosticPublicationState::new()),
        }
    }
}

/// Semantic and indexing state for the active workspace.
pub(super) struct WorkspaceAnalysis {
    /// Owner of the incremental semantic database and the read/write gate in
    /// front of it. Every reader takes a `ReadTxn`, every writer the `WriteTxn`.
    pub(super) host: Arc<crate::semantic_host::SemanticHost>,
    /// Cross-file declarations/usages and the resolver built over them.
    pub(super) index: Arc<WorkspaceIndex>,
    pub(super) query: Arc<QueryEngine>,
    /// Resolved build state belongs to this server session, not the process.
    pub(super) project_classpaths: crate::project_classpath::ProjectClasspathStore,
    pub(super) root: Arc<RwLock<Option<PathBuf>>>,
    /// Nested repositories already indexed despite a parent ignore rule.
    pub(super) indexed_repos: Arc<DashMap<hyperion_workspace::PathLookupKey, ()>>,
    /// Open-file fast-path indexing gates; complete before exact navigation answers.
    pub(super) source_root_gates: DashMap<hyperion_workspace::PathLookupKey, Arc<IndexGate>>,
    /// False until initial indexing completes, suppressing premature semantic errors.
    pub(super) ready: Arc<AtomicBool>,
    /// Exact completeness gate for destructive workspace refactorings. Unlike
    /// `ready`, this includes the source manifest, JDK, and both build-system
    /// resolvers and remains false after any partial result.
    pub(super) destructive_refactoring: Arc<DestructiveRefactoringReadiness>,
    pub(super) diagnostic_options: Arc<RwLock<crate::diagnostic_engine::DiagnosticOptions>>,
    /// Disk-change reconciler shared by the file watcher and by client-sent
    /// file notifications (`didChangeWatchedFiles`, `didRenameFiles`, ...).
    pub(super) file_events:
        Arc<std::sync::OnceLock<Arc<crate::handlers::file_events::FileEventContext>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefactoringIndexComponent {
    Sources = 0,
    Jdk = 1,
    Gradle = 2,
    Maven = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RefactoringReadinessTicket {
    component: RefactoringIndexComponent,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefactoringReadinessStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug)]
struct RefactoringComponentState {
    generation: u64,
    status: RefactoringReadinessStatus,
}

impl Default for RefactoringComponentState {
    fn default() -> Self {
        Self {
            generation: 0,
            status: RefactoringReadinessStatus::Pending,
        }
    }
}

#[derive(Debug)]
struct SourceMutationState {
    active: u32,
    generation: u64,
    latest_succeeded: bool,
}

#[derive(Debug, Default)]
struct RefactoringReadinessState {
    components: [RefactoringComponentState; 4],
    source_mutations:
        std::collections::HashMap<hyperion_workspace::PathLookupKey, SourceMutationState>,
    sources_permanently_failed: bool,
}

/// RAII fence for one source publication transaction.
///
/// The guard is deliberately moved into `spawn_blocking` closures. Aborting the
/// async request therefore cannot make readiness true while its blocking source
/// publication is still running. An unwind or forgotten explicit completion
/// records failure; a later successful publication of the same path recovers it.
#[derive(Debug)]
pub(crate) struct SourceMutationReadinessGuard {
    readiness: Arc<DestructiveRefactoringReadiness>,
    ticket_paths: Option<Box<[(hyperion_workspace::PathLookupKey, u64)]>>,
}

// `Box<[T]>` is a pointer/length pair; together with the shared authority this
// keeps the transient guard to three machine words without adding a second heap
// allocation solely to make the handle narrower.
const _: () = assert!(std::mem::size_of::<SourceMutationReadinessGuard>() <= 24);

impl SourceMutationReadinessGuard {
    pub(crate) fn finish(mut self, succeeded: bool) {
        if let Some(ticket_paths) = self.ticket_paths.take() {
            self.readiness
                .finish_source_mutation(ticket_paths, succeeded);
        }
    }
}

impl Drop for SourceMutationReadinessGuard {
    fn drop(&mut self) {
        if let Some(ticket_paths) = self.ticket_paths.take() {
            self.readiness.finish_source_mutation(ticket_paths, false);
        }
    }
}

/// One lock protects the complete readiness decision. Component generations
/// prevent an old build refresh from superseding a newer one, while counted
/// per-path source generations allow overlapping publications without either a
/// torn ready snapshot or an older completion overwriting the newer outcome.
#[derive(Debug)]
pub(crate) struct DestructiveRefactoringReadiness {
    state: Mutex<RefactoringReadinessState>,
}

impl DestructiveRefactoringReadiness {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(RefactoringReadinessState::default()),
        }
    }

    pub(crate) fn begin(&self, component: RefactoringIndexComponent) -> RefactoringReadinessTicket {
        let mut state = self.state.lock();
        let component_state = &mut state.components[component as usize];
        component_state.generation = component_state.generation.wrapping_add(1);
        component_state.status = RefactoringReadinessStatus::Pending;
        RefactoringReadinessTicket {
            component,
            generation: component_state.generation,
        }
    }

    pub(crate) fn finish(&self, ticket: RefactoringReadinessTicket, succeeded: bool) -> bool {
        let mut state = self.state.lock();
        let component_state = &mut state.components[ticket.component as usize];
        if component_state.generation != ticket.generation {
            return false;
        }
        component_state.status = if succeeded {
            RefactoringReadinessStatus::Succeeded
        } else {
            RefactoringReadinessStatus::Failed
        };
        true
    }

    pub(crate) fn begin_source_mutation<I, P>(
        self: &Arc<Self>,
        paths: I,
    ) -> SourceMutationReadinessGuard
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        let mut paths: Vec<_> = paths
            .into_iter()
            .map(|path| hyperion_workspace::path_lookup_key(path.as_ref()))
            .collect();
        paths.sort_unstable();
        paths.dedup();

        let mut state = self.state.lock();
        let mut ticket_paths = Vec::with_capacity(paths.len());
        for path in paths {
            let mutation =
                state
                    .source_mutations
                    .entry(path.clone())
                    .or_insert(SourceMutationState {
                        active: 0,
                        generation: 0,
                        latest_succeeded: false,
                    });
            mutation.active = mutation
                .active
                .checked_add(1)
                .expect("too many concurrent source mutations for one path");
            mutation.generation = mutation.generation.wrapping_add(1);
            mutation.latest_succeeded = false;
            ticket_paths.push((path, mutation.generation));
        }
        drop(state);

        SourceMutationReadinessGuard {
            readiness: Arc::clone(self),
            ticket_paths: Some(ticket_paths.into_boxed_slice()),
        }
    }

    fn finish_source_mutation(
        &self,
        ticket_paths: Box<[(hyperion_workspace::PathLookupKey, u64)]>,
        succeeded: bool,
    ) {
        let mut state = self.state.lock();
        for (path, generation) in ticket_paths.into_vec() {
            let remove = {
                let mutation = state
                    .source_mutations
                    .get_mut(&path)
                    .expect("source readiness ticket outlived its path state");
                debug_assert!(mutation.active > 0);
                mutation.active -= 1;
                if mutation.generation == generation {
                    mutation.latest_succeeded = succeeded;
                }
                mutation.active == 0 && mutation.latest_succeeded
            };
            if remove {
                state.source_mutations.remove(&path);
            }
        }
    }

    pub(crate) fn fail_sources_permanently(&self) {
        self.state.lock().sources_permanently_failed = true;
    }

    pub(crate) fn is_ready(&self) -> bool {
        let state = self.state.lock();
        !state.sources_permanently_failed
            && state.source_mutations.is_empty()
            && state
                .components
                .iter()
                .all(|component| component.status == RefactoringReadinessStatus::Succeeded)
    }
}

impl Default for DestructiveRefactoringReadiness {
    fn default() -> Self {
        Self::new()
    }
}

/// The main backend implementation for the Hyperion LSP server.
///
/// This is the `tower-lsp` protocol facade. Feature handlers live in focused child
/// modules and borrow four explicit state boundaries: open documents, workspace
/// analysis, language services, and cancellable background work.
///
/// Concurrent maps permit independent document requests, while the bounded snapshot
/// semaphore and cancellation state protect the Salsa database from unbounded reads
/// and input-write starvation.
pub struct Backend {
    pub(super) client: Client,
    pub(super) documents: DocumentStore,
    pub(super) workspace: WorkspaceAnalysis,
    pub(super) services: LanguageServices,
    pub(super) background_tasks: BackgroundTasks,
    pub(super) client_supports_progress: AtomicBool,
    pub(super) client_supports_dynamic_type_hierarchy: AtomicBool,
    /// Client accepts dynamic `workspace/didChangeWatchedFiles` registration.
    pub(super) client_supports_dynamic_watchers: AtomicBool,
    /// Bit 0: workspace/applyEdit. Bit 1: versioned documentChanges.
    /// Bit 2: workspaceEdit.resourceOperations includes `rename`.
    pub(super) client_workspace_edit_capabilities: AtomicU8,
}

/// A source root that fails to index usually fails for a reason that will not
/// change on its own — an unreadable or unparseable file that is still there
/// on the next attempt. Retrying it without a cap turned every interactive
/// request into a full re-index, so attempts are bounded.
const MAX_SOURCE_ROOT_INDEX_ATTEMPTS: u32 = 3;

/// Minimum spacing between retries of a failed source root, so a retry that is
/// still allowed cannot be triggered back to back by a burst of requests.
const SOURCE_ROOT_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub(super) struct IndexGate {
    status: AtomicU8,
    notify: tokio::sync::Notify,
    attempts: std::sync::atomic::AtomicU32,
    last_attempt_at: Mutex<Option<std::time::Instant>>,
}

impl IndexGate {
    pub(super) fn new() -> Self {
        Self {
            status: AtomicU8::new(0),
            notify: tokio::sync::Notify::new(),
            attempts: std::sync::atomic::AtomicU32::new(0),
            last_attempt_at: Mutex::new(None),
        }
    }

    /// Record the first indexing attempt for a freshly created gate.
    pub(super) fn begin_initial_attempt(&self) {
        self.attempts.store(1, Ordering::Release);
        *self.last_attempt_at.lock() = Some(std::time::Instant::now());
    }

    /// Claim the right to re-run indexing for a source root that failed.
    ///
    /// Returns `true` at most [`MAX_SOURCE_ROOT_INDEX_ATTEMPTS`] times, and
    /// never twice within [`SOURCE_ROOT_RETRY_COOLDOWN`]. A gate that is
    /// running, succeeded, or out of budget yields `false`, which leaves it in
    /// its terminal state so `wait_done` returns immediately instead of
    /// stalling the caller for the full timeout.
    pub(super) fn claim_retry(&self) -> bool {
        if !self.failed() {
            return false;
        }
        if self.attempts.load(Ordering::Acquire) >= MAX_SOURCE_ROOT_INDEX_ATTEMPTS {
            return false;
        }

        let now = std::time::Instant::now();
        let mut last = self.last_attempt_at.lock();
        if last.is_some_and(|at| now.duration_since(at) < SOURCE_ROOT_RETRY_COOLDOWN) {
            return false;
        }

        // Re-arm under the lock so two concurrent requests cannot both claim
        // the same retry slot.
        if self
            .status
            .compare_exchange(2, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.attempts.fetch_add(1, Ordering::AcqRel);
        *last = Some(now);
        true
    }

    pub(super) fn mark_done(&self, succeeded: bool) {
        self.status
            .store(if succeeded { 1 } else { 2 }, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub(super) fn failed(&self) -> bool {
        self.status.load(Ordering::Acquire) == 2
    }

    pub(super) async fn wait_done(&self, timeout: Duration) -> bool {
        let notified = self.notify.notified();
        tokio::pin!(notified);
        // Register before observing status so `mark_done` cannot land between
        // the terminal-state check and waiter registration.
        notified.as_mut().enable();
        if self.status.load(Ordering::Acquire) == 0 {
            let _ = tokio::time::timeout(timeout, &mut notified).await;
        }
        self.status.load(Ordering::Acquire) == 1
    }
}

#[cfg(test)]
mod index_gate_tests {
    use super::{
        DestructiveRefactoringReadiness, IndexGate, RefactoringIndexComponent,
        MAX_SOURCE_ROOT_INDEX_ATTEMPTS,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::time::Duration;

    #[tokio::test]
    async fn wait_distinguishes_success_failure_and_timeout() {
        let succeeded = IndexGate::new();
        succeeded.mark_done(true);
        assert!(succeeded.wait_done(Duration::from_millis(1)).await);

        let failed = IndexGate::new();
        failed.mark_done(false);
        assert!(failed.failed());
        assert!(!failed.wait_done(Duration::from_millis(1)).await);

        let pending = IndexGate::new();
        assert!(!pending.wait_done(Duration::from_millis(1)).await);
        assert!(!pending.failed());
    }

    /// A failed source root used to be retried by every interactive request,
    /// each retry re-spawning a full re-index and re-arming the wait, so one
    /// permanently unindexable file put a multi-second floor under every
    /// goto-definition and find-references call.
    #[tokio::test]
    async fn a_failed_gate_stops_retrying_and_stops_stalling_callers() {
        let gate = IndexGate::new();
        gate.begin_initial_attempt();
        gate.mark_done(false);

        // Attempt 1 was the initial index, so two retries remain — but the
        // cooldown has not elapsed, so neither is available yet.
        assert!(
            !gate.claim_retry(),
            "a retry must not be claimable inside the cooldown window"
        );
        assert!(gate.failed(), "a declined retry must leave the gate failed");

        // A failed gate must resolve immediately rather than burning the
        // caller's timeout, which is what made the request slow.
        let started = std::time::Instant::now();
        assert!(!gate.wait_done(Duration::from_secs(3)).await);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "waiting on a failed gate must return immediately, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn retry_budget_is_exhausted_after_the_capped_number_of_attempts() {
        let gate = IndexGate::new();
        gate.begin_initial_attempt();
        gate.mark_done(false);

        // Drive the clock forward by clearing the cooldown stamp, which is the
        // only thing the cooldown check consults.
        let mut claimed = 0;
        for _ in 0..10 {
            *gate.last_attempt_at.lock() = None;
            if gate.claim_retry() {
                claimed += 1;
                gate.mark_done(false);
            }
        }

        assert_eq!(
            claimed,
            MAX_SOURCE_ROOT_INDEX_ATTEMPTS - 1,
            "the initial attempt plus retries must not exceed the cap"
        );
        assert!(gate.failed());
    }

    #[tokio::test]
    async fn a_succeeded_gate_is_never_retried() {
        let gate = IndexGate::new();
        gate.begin_initial_attempt();
        gate.mark_done(true);

        *gate.last_attempt_at.lock() = None;
        assert!(
            !gate.claim_retry(),
            "success must not schedule more indexing"
        );
        assert!(gate.wait_done(Duration::from_millis(1)).await);
    }

    #[test]
    fn destructive_readiness_requires_every_current_component_generation() {
        let readiness = Arc::new(DestructiveRefactoringReadiness::new());
        let sources = readiness.begin(RefactoringIndexComponent::Sources);
        let jdk = readiness.begin(RefactoringIndexComponent::Jdk);
        let gradle = readiness.begin(RefactoringIndexComponent::Gradle);
        let maven = readiness.begin(RefactoringIndexComponent::Maven);
        for ticket in [sources, jdk, gradle] {
            assert!(readiness.finish(ticket, true));
        }
        assert!(!readiness.is_ready());
        assert!(readiness.finish(maven, true));
        assert!(readiness.is_ready());

        let stale = readiness.begin(RefactoringIndexComponent::Gradle);
        let current = readiness.begin(RefactoringIndexComponent::Gradle);
        assert!(!readiness.finish(stale, true));
        assert!(!readiness.is_ready());
        assert!(readiness.finish(current, false));
        assert!(!readiness.is_ready());
    }

    fn ready_readiness() -> Arc<DestructiveRefactoringReadiness> {
        let readiness = Arc::new(DestructiveRefactoringReadiness::new());
        for component in [
            RefactoringIndexComponent::Sources,
            RefactoringIndexComponent::Jdk,
            RefactoringIndexComponent::Gradle,
            RefactoringIndexComponent::Maven,
        ] {
            let ticket = readiness.begin(component);
            assert!(readiness.finish(ticket, true));
        }
        readiness
    }

    #[test]
    fn overlapping_source_mutations_are_counted_and_newest_outcome_wins() {
        let readiness = ready_readiness();
        let older = readiness.begin_source_mutation([PathBuf::from("src/Entity.java")]);
        let newer = readiness.begin_source_mutation([PathBuf::from("src/Entity.java")]);
        assert!(!readiness.is_ready());

        newer.finish(true);
        assert!(!readiness.is_ready(), "older publication is still active");
        older.finish(false);
        assert!(
            readiness.is_ready(),
            "an older failure must not supersede the newer successful revision"
        );
    }

    #[test]
    fn successful_source_mutation_recovers_only_the_same_failed_path() {
        let readiness = ready_readiness();
        readiness
            .begin_source_mutation([PathBuf::from("src/A.java")])
            .finish(false);
        readiness
            .begin_source_mutation([PathBuf::from("src/B.java")])
            .finish(true);
        assert!(!readiness.is_ready());

        readiness
            .begin_source_mutation([PathBuf::from("src/A.java")])
            .finish(true);
        assert!(readiness.is_ready());
    }

    #[test]
    fn dropped_source_ticket_fails_closed_and_watcher_failure_is_permanent() {
        let readiness = ready_readiness();
        drop(readiness.begin_source_mutation([PathBuf::from("src/Drop.java")]));
        assert!(!readiness.is_ready());
        readiness
            .begin_source_mutation([PathBuf::from("src/Drop.java")])
            .finish(true);
        assert!(readiness.is_ready());

        readiness.fail_sources_permanently();
        readiness
            .begin_source_mutation([PathBuf::from("src/Drop.java")])
            .finish(true);
        assert!(!readiness.is_ready());
    }
}

/// State information for a single open document.
#[derive(Debug, Clone)]
pub struct DocumentState {
    pub uri: String,
    pub language_id: String,
    /// Derived from the URI; the client-supplied language ID is not trusted for dispatch.
    pub language: Language,
    pub version: i32,
    /// An invalid incremental notification was seen. The retained text is
    /// still coherent, but later ranged edits cannot use it as their base.
    pub desynchronized: bool,
    /// Shared because every request takes an immutable snapshot of the current text.
    pub text: Arc<String>,
    pub file_id: FileId,
    /// Server-monotonic version used to reject stale file-watcher updates.
    pub change_version: u64,
}
