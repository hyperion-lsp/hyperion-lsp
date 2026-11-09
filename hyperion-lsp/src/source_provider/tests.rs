// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::temp_file::AttachmentLanguage;
use super::{
    ExternalSourceRequest, JarSourceProvider, JdkSourceProvider, MaterializedSourceStore,
    PreparedPublicationCommitError, SourceContent, SourceFile, SourceProvider, SourceProviderChain,
};
use hyperion_workspace::{ExternalSourceArchive, ExternalSourceIdentity, WorkspaceIndex};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Weak};

static _EMPTY_CHAIN_FOR_COMPATIBILITY: SourceProviderChain = SourceProviderChain::new();

fn virtual_identity(name: &str) -> ExternalSourceIdentity {
    ExternalSourceIdentity::for_virtual(name)
}

#[test]
fn dropped_observation_releases_exact_provider_authority() {
    let temp = tempfile::tempdir().unwrap();
    let first_index = WorkspaceIndex::new(temp.path().to_path_buf());
    let second_index = WorkspaceIndex::new(temp.path().to_path_buf());
    let chain = Arc::new(SourceProviderChain::new());
    let first = first_index.begin_external_source_refresh("build:gradle");
    let guard = chain
        .observe_group_refresh_guard(&first)
        .expect("first authority is observed");

    drop(guard);

    let second = second_index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&second));
}

#[test]
fn guard_cannot_claim_an_existing_provider_observation() {
    let temp = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let chain = Arc::new(SourceProviderChain::new());
    let ticket = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&ticket));

    assert!(chain.observe_group_refresh_guard(&ticket).is_none());
    let _ = index
        .replace_external_sources_for_refresh(&ticket, &[])
        .expect("workspace generation commits");
    assert!(chain.replace_group_for_refresh(&ticket, None));
}

#[test]
fn late_observation_abort_cannot_erase_newer_generation() {
    let temp = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let chain = Arc::new(SourceProviderChain::new());
    let old = index.begin_external_source_refresh("build:gradle");
    let old_guard = chain
        .observe_group_refresh_guard(&old)
        .expect("old generation is observed");
    let new = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&new));

    drop(old_guard);

    let _ = index
        .replace_external_sources_for_refresh(&new, &[])
        .expect("new generation commits");
    assert!(chain.replace_group_for_refresh(&new, None));
}

struct StaticProvider;

struct LegacyOnlyProvider;

impl SourceProvider for LegacyOnlyProvider {
    fn get_source(&self, fqn: &str) -> Option<SourceContent> {
        (fqn == "legacy.Dependency").then(|| SourceContent::Stub {
            source: Arc::new(SourceFile::new(Arc::from("class Dependency {}"))),
            temp_file: PathBuf::from("/legacy-provider.java"),
        })
    }

    fn can_provide(&self, fqn: &str) -> bool {
        fqn == "legacy.Dependency"
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "legacy-only"
    }
}

impl SourceProvider for StaticProvider {
    fn get_source_exact(&self, _request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        None
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        request.fqn() == "registered.DuringLookup"
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "static"
    }
}

#[test]
fn legacy_provider_api_remains_source_compatible_but_cannot_claim_an_exact_source() {
    let _: fn(&[PathBuf]) -> Option<JarSourceProvider> = JarSourceProvider::from_binary_jars;
    let _: fn(&Path) -> Option<JdkSourceProvider> = JdkSourceProvider::new;
    let _: fn(&JdkSourceProvider) -> Result<(), String> = JdkSourceProvider::cleanup;

    let chain = SourceProviderChain::new();
    chain.register(Arc::new(LegacyOnlyProvider));

    let (legacy, legacy_name) = chain
        .get_source("legacy.Dependency")
        .expect("legacy lookup reaches an old-style provider");
    assert_eq!(legacy.content(), "class Dependency {}");
    assert_eq!(legacy_name, "legacy-only");

    assert!(chain
        .get_source_exact(
            &virtual_identity("unrelated-source-id"),
            "legacy.Dependency"
        )
        .is_none());
    assert!(!chain.can_provide_exact(
        &virtual_identity("unrelated-source-id"),
        "legacy.Dependency"
    ));
}

struct NewStaticProvider;

impl SourceProvider for NewStaticProvider {
    fn get_source_exact(&self, _request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        None
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        request.fqn() == "new.Dependency"
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "new-static"
    }
}

struct VersionedProvider {
    sources: HashMap<ExternalSourceIdentity, Arc<str>>,
}

impl VersionedProvider {
    fn new(sources: impl IntoIterator<Item = (PathBuf, &'static str)>) -> Self {
        Self {
            sources: sources
                .into_iter()
                .map(|(path, source)| {
                    (
                        ExternalSourceIdentity::for_archive_path(&path),
                        Arc::from(source),
                    )
                })
                .collect(),
        }
    }

    fn from_identities(
        sources: impl IntoIterator<Item = (ExternalSourceIdentity, &'static str)>,
    ) -> Self {
        Self {
            sources: sources
                .into_iter()
                .map(|(identity, source)| (identity, Arc::from(source)))
                .collect(),
        }
    }
}

impl SourceProvider for VersionedProvider {
    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        let source = self.sources.get(request.source_identity())?;
        Some(SourceContent::Stub {
            source: Arc::new(SourceFile::new(Arc::clone(source))),
            temp_file: PathBuf::from("/generation-bound-test.java"),
        })
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        request.fqn() == "dep.Type" && self.sources.contains_key(request.source_identity())
    }

    fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
        self.sources.contains_key(source_identity)
    }

    fn snapshot_for_sources(
        &self,
        source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        let sources: HashMap<_, _> = self
            .sources
            .iter()
            .filter(|(source_identity, _)| source_identities.contains(*source_identity))
            .map(|(source_identity, source)| (source_identity.clone(), Arc::clone(source)))
            .collect();
        (!sources.is_empty()).then(|| Arc::new(Self { sources }) as Arc<dyn SourceProvider>)
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "versioned test source"
    }
}

#[test]
fn external_source_request_preserves_unversioned_compatibility_and_exact_revision() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-request-revision"));
    let source = PathBuf::from("/libs/request.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);
    let refresh = index.replace_external_sources_with_priority_revisioned(
        "request",
        0,
        std::slice::from_ref(&source),
    );
    let revision = refresh.revision_for_identity(&identity).unwrap();

    assert_eq!(
        ExternalSourceRequest::new(&identity, "dep.Type").source_revision(),
        None
    );
    assert_eq!(
        ExternalSourceRequest::for_revision(&identity, revision, "dep.Type").source_revision(),
        Some(revision)
    );
}

#[test]
fn revisioned_routes_isolate_shared_identity_across_owners_and_delayed_finish() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-owner-revisions"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let a1 = index.begin_external_source_refresh("build:a");
    assert!(chain.observe_group_refresh(&a1));
    let a1_publication = chain
        .begin_group_publication(
            &a1,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "A1",
            )]))),
        )
        .unwrap();
    let a1_refresh = index
        .replace_external_sources_for_refresh(&a1, std::slice::from_ref(&source))
        .unwrap();
    let a1_revision = a1_refresh.revision_for_identity(&identity).unwrap();
    assert!(chain.record_group_publication_success_for_revision(
        &a1_publication,
        &identity,
        a1_revision,
    ));
    assert!(chain.finish_group_publication(&a1_publication));

    let b1 = index.begin_external_source_refresh("build:b");
    assert!(chain.observe_group_refresh(&b1));
    let b1_publication = chain
        .begin_group_publication(
            &b1,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "B1",
            )]))),
        )
        .unwrap();
    let b1_refresh = index
        .replace_external_sources_for_refresh(&b1, std::slice::from_ref(&source))
        .unwrap();
    let b1_revision = b1_refresh.revision_for_identity(&identity).unwrap();
    assert!(chain.record_group_publication_success_for_revision(
        &b1_publication,
        &identity,
        b1_revision,
    ));
    assert!(chain.finish_group_publication(&b1_publication));

    assert_eq!(
        chain
            .get_source_exact_for_revision(&identity, a1_revision, "dep.Type")
            .unwrap()
            .0
            .content(),
        "A1"
    );
    assert_eq!(
        chain
            .get_source_exact_for_revision(&identity, b1_revision, "dep.Type")
            .unwrap()
            .0
            .content(),
        "B1"
    );

    let a2 = index.begin_external_source_refresh("build:a");
    assert!(chain.observe_group_refresh(&a2));
    let a2_publication = chain
        .begin_group_publication(
            &a2,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "A2",
            )]))),
        )
        .unwrap();
    let a2_refresh = index
        .replace_external_sources_for_refresh(&a2, std::slice::from_ref(&source))
        .unwrap();
    let a2_revision = a2_refresh.revision_for_identity(&identity).unwrap();
    assert!(chain.record_group_publication_success_for_revision(
        &a2_publication,
        &identity,
        a2_revision,
    ));

    let b2 = index.begin_external_source_refresh("build:b");
    assert!(chain.observe_group_refresh(&b2));
    let b2_publication = chain
        .begin_group_publication(
            &b2,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "B2",
            )]))),
        )
        .unwrap();
    let b2_refresh = index
        .replace_external_sources_for_refresh(&b2, std::slice::from_ref(&source))
        .unwrap();
    let b2_revision = b2_refresh.revision_for_identity(&identity).unwrap();
    assert!(chain.record_group_publication_success_for_revision(
        &b2_publication,
        &identity,
        b2_revision,
    ));
    assert!(chain.finish_group_publication(&b2_publication));

    assert!(chain.finish_group_publication(&a2_publication));
    assert!(chain
        .get_source_exact_for_revision(&identity, a1_revision, "dep.Type")
        .is_none());
    assert!(chain
        .get_source_exact_for_revision(&identity, b1_revision, "dep.Type")
        .is_none());
    assert_eq!(
        chain
            .get_source_exact_for_revision(&identity, a2_revision, "dep.Type")
            .unwrap()
            .0
            .content(),
        "A2"
    );
    assert_eq!(
        chain
            .get_source_exact_for_revision(&identity, b2_revision, "dep.Type")
            .unwrap()
            .0
            .content(),
        "B2"
    );
}

#[test]
fn revisioned_failure_retains_old_route_but_source_less_success_is_terminal() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-revision-fallback"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let r1 = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&r1));
    let r1_publication = chain
        .begin_group_publication(
            &r1,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "R1",
            )]))),
        )
        .unwrap();
    let r1_refresh = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    let r1_revision = r1_refresh.revision_for_identity(&identity).unwrap();
    assert!(chain.record_group_publication_success_for_revision(
        &r1_publication,
        &identity,
        r1_revision,
    ));
    assert!(chain.finish_group_publication(&r1_publication));

    let failed = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&failed));
    let failed_publication = chain
        .begin_group_publication(
            &failed,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "unpublished",
            )]))),
        )
        .unwrap();
    let failed_refresh = index
        .replace_external_sources_for_refresh(&failed, std::slice::from_ref(&source))
        .unwrap();
    let failed_revision = failed_refresh.revision_for_identity(&identity).unwrap();
    assert!(chain.finish_group_publication(&failed_publication));
    assert_eq!(
        chain
            .get_source_exact_for_revision(&identity, r1_revision, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R1"
    );
    assert!(chain
        .get_source_exact_for_revision(&identity, failed_revision, "dep.Type")
        .is_none());

    let source_less = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&source_less));
    let source_less_publication = chain
        .begin_group_publication(
            &source_less,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities(
                std::iter::empty::<(ExternalSourceIdentity, &'static str)>(),
            ))),
        )
        .unwrap();
    let source_less_refresh = index
        .replace_external_sources_for_refresh(&source_less, std::slice::from_ref(&source))
        .unwrap();
    let source_less_revision = source_less_refresh
        .revision_for_identity(&identity)
        .unwrap();
    assert!(chain.record_group_publication_success_for_revision(
        &source_less_publication,
        &identity,
        source_less_revision,
    ));
    assert!(chain.finish_group_publication(&source_less_publication));

    assert!(chain
        .get_source_exact_for_revision(&identity, source_less_revision, "dep.Type")
        .is_none());
    assert!(chain
        .get_source_exact_for_revision(&identity, r1_revision, "dep.Type")
        .is_none());
}

struct BlockingProvider {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl SourceProvider for BlockingProvider {
    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        self.entered.wait();
        self.release.wait();
        Some(SourceContent::Stub {
            source: Arc::new(SourceFile::new(Arc::from(format!(
                "// {:?}",
                request.source_identity()
            )))),
            temp_file: PathBuf::from("/blocking-provider.java"),
        })
    }

    fn can_provide_exact(&self, _request: ExternalSourceRequest<'_>) -> bool {
        true
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "blocking test source"
    }
}

struct RegisteringProvider {
    chain: Weak<SourceProviderChain>,
}

impl SourceProvider for RegisteringProvider {
    fn get_source_exact(&self, _request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        self.chain
            .upgrade()
            .expect("chain remains alive")
            .register(Arc::new(StaticProvider));
        None
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        request.fqn() == "trigger.Lookup"
    }

    fn priority(&self) -> u32 {
        10
    }

    fn name(&self) -> &'static str {
        "registering"
    }
}

struct RegisteringCapabilityProvider {
    chain: Weak<SourceProviderChain>,
    registered: AtomicBool,
}

impl SourceProvider for RegisteringCapabilityProvider {
    fn get_source_exact(&self, _request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        None
    }

    fn can_provide_exact(&self, _request: ExternalSourceRequest<'_>) -> bool {
        if !self.registered.swap(true, Ordering::AcqRel) {
            self.chain
                .upgrade()
                .expect("chain remains alive")
                .register(Arc::new(StaticProvider));
        }
        false
    }

    fn priority(&self) -> u32 {
        10
    }

    fn name(&self) -> &'static str {
        "registering capability"
    }
}

struct MaterializingProvider {
    store: Arc<MaterializedSourceStore>,
    source: Arc<SourceFile>,
}

impl SourceProvider for MaterializingProvider {
    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        (request.fqn() == "external.Persistent").then(|| SourceContent::Stub {
            temp_file: self
                .store
                .materialize(self.source.content().as_bytes(), AttachmentLanguage::Java)
                .expect("materialize test source"),
            source: Arc::clone(&self.source),
        })
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        request.fqn() == "external.Persistent"
    }

    fn priority(&self) -> u32 {
        10
    }

    fn name(&self) -> &'static str {
        "materializing test provider"
    }
}

struct ReentrantDropProvider {
    chain: Weak<SourceProviderChain>,
    dropped_off_lock: Arc<AtomicBool>,
}

struct PanickingPriorityDropProvider {
    chain: Weak<SourceProviderChain>,
    dropped_off_lock: Arc<AtomicBool>,
}

impl SourceProvider for PanickingPriorityDropProvider {
    fn priority(&self) -> u32 {
        panic!("priority callback panic")
    }

    fn name(&self) -> &'static str {
        "panicking priority"
    }
}

impl Drop for PanickingPriorityDropProvider {
    fn drop(&mut self) {
        let Some(chain) = self.chain.upgrade() else {
            return;
        };
        let lock_available = chain.state().try_write().is_some();
        self.dropped_off_lock
            .store(lock_available, Ordering::Release);
        if lock_available {
            chain.register(Arc::new(StaticProvider));
        }
    }
}

struct PanickingDropProvider {
    chain: Weak<SourceProviderChain>,
    dropped_off_lock: Arc<AtomicBool>,
}

struct CountingPanickingDropProvider {
    chain: Weak<SourceProviderChain>,
    dropped_off_lock: Arc<AtomicUsize>,
}

impl SourceProvider for CountingPanickingDropProvider {
    fn priority(&self) -> u32 {
        10
    }

    fn name(&self) -> &'static str {
        "counting panicking drop"
    }
}

impl Drop for CountingPanickingDropProvider {
    fn drop(&mut self) {
        if self
            .chain
            .upgrade()
            .is_some_and(|chain| chain.state().try_write().is_some())
        {
            self.dropped_off_lock.fetch_add(1, Ordering::AcqRel);
        }
        panic!("counted provider drop panic")
    }
}

impl SourceProvider for PanickingDropProvider {
    fn priority(&self) -> u32 {
        10
    }

    fn name(&self) -> &'static str {
        "panicking drop"
    }
}

impl Drop for PanickingDropProvider {
    fn drop(&mut self) {
        let Some(chain) = self.chain.upgrade() else {
            return;
        };
        self.dropped_off_lock
            .store(chain.state().try_write().is_some(), Ordering::Release);
        panic!("provider drop panic")
    }
}

struct RankedProvider {
    sources: HashMap<ExternalSourceIdentity, Arc<str>>,
    priority: u32,
    capability_calls: Arc<AtomicUsize>,
}

struct RefreshingSnapshotProvider {
    index: Arc<WorkspaceIndex>,
    owner: &'static str,
    refreshes: Arc<AtomicUsize>,
}

impl SourceProvider for RefreshingSnapshotProvider {
    fn snapshot_for_sources(
        &self,
        _source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        let _ = self.index.begin_external_source_refresh(self.owner);
        self.refreshes.fetch_add(1, Ordering::AcqRel);
        None
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "refreshing snapshot"
    }
}

struct RefreshingDropProvider {
    index: Arc<WorkspaceIndex>,
    owner: &'static str,
    identity: ExternalSourceIdentity,
    drops: Arc<AtomicUsize>,
}

struct OneShotChainMutatingSnapshotProvider {
    chain: std::sync::Weak<SourceProviderChain>,
    identity: ExternalSourceIdentity,
    mutated: Arc<AtomicBool>,
}

impl SourceProvider for OneShotChainMutatingSnapshotProvider {
    fn snapshot_for_sources(
        &self,
        source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        if !self.mutated.swap(true, Ordering::AcqRel) {
            self.chain.upgrade()?.register(Arc::new(StaticProvider));
        }
        source_identities.contains(&self.identity).then(|| {
            Arc::new(VersionedProvider::from_identities([(
                self.identity.clone(),
                "R1",
            )])) as Arc<dyn SourceProvider>
        })
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "one-shot chain-mutating snapshot"
    }
}

impl SourceProvider for RefreshingDropProvider {
    fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
        source_identity == &self.identity
    }

    fn snapshot_for_sources(
        &self,
        source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        source_identities.contains(&self.identity).then(|| {
            Arc::new(Self {
                index: Arc::clone(&self.index),
                owner: self.owner,
                identity: self.identity.clone(),
                drops: Arc::clone(&self.drops),
            }) as Arc<dyn SourceProvider>
        })
    }

    fn priority(&self) -> u32 {
        20
    }

    fn name(&self) -> &'static str {
        "refreshing drop"
    }
}

impl Drop for RefreshingDropProvider {
    fn drop(&mut self) {
        let _ = self.index.begin_external_source_refresh(self.owner);
        self.drops.fetch_add(1, Ordering::AcqRel);
    }
}

impl RankedProvider {
    fn new(
        sources: impl IntoIterator<Item = (ExternalSourceIdentity, &'static str)>,
        priority: u32,
        capability_calls: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            sources: sources
                .into_iter()
                .map(|(identity, source)| (identity, Arc::from(source)))
                .collect(),
            priority,
            capability_calls,
        }
    }
}

impl SourceProvider for RankedProvider {
    fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        let source = self.sources.get(request.source_identity())?;
        Some(SourceContent::Stub {
            source: Arc::new(SourceFile::new(Arc::clone(source))),
            temp_file: PathBuf::from("/ranked-provider.java"),
        })
    }

    fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
        self.capability_calls.fetch_add(1, Ordering::Relaxed);
        request.fqn() == "dep.Type" && self.sources.contains_key(request.source_identity())
    }

    fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
        self.sources.contains_key(source_identity)
    }

    fn snapshot_for_sources(
        &self,
        source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        let sources: HashMap<_, _> = self
            .sources
            .iter()
            .filter(|(source_identity, _)| source_identities.contains(*source_identity))
            .map(|(source_identity, source)| (source_identity.clone(), Arc::clone(source)))
            .collect();
        (!sources.is_empty()).then(|| {
            Arc::new(Self {
                sources,
                priority: self.priority,
                capability_calls: Arc::clone(&self.capability_calls),
            }) as Arc<dyn SourceProvider>
        })
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn name(&self) -> &'static str {
        "ranked test source"
    }
}

impl SourceProvider for ReentrantDropProvider {
    fn get_source_exact(&self, _request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        None
    }

    fn can_provide_exact(&self, _request: ExternalSourceRequest<'_>) -> bool {
        false
    }

    fn priority(&self) -> u32 {
        10
    }

    fn name(&self) -> &'static str {
        "reentrant drop test provider"
    }
}

impl Drop for ReentrantDropProvider {
    fn drop(&mut self) {
        let Some(chain) = self.chain.upgrade() else {
            return;
        };
        let lock_available = chain.state().try_write().is_some();
        self.dropped_off_lock
            .store(lock_available, Ordering::Release);
        if lock_available {
            chain.register(Arc::new(StaticProvider));
        }
    }
}

#[test]
fn provider_execution_does_not_hold_the_chain_lock() {
    let chain = Arc::new(SourceProviderChain::new());
    chain.register(Arc::new(RegisteringProvider {
        chain: Arc::downgrade(&chain),
    }));

    assert!(chain
        .get_source_exact(&virtual_identity("test"), "trigger.Lookup")
        .is_none());
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn capability_checks_do_not_hold_the_chain_lock() {
    let chain = Arc::new(SourceProviderChain::new());
    chain.register(Arc::new(RegisteringCapabilityProvider {
        chain: Arc::downgrade(&chain),
        registered: AtomicBool::new(false),
    }));

    assert!(!chain.can_provide_exact(&virtual_identity("test"), "trigger.Registration"));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn replacing_a_provider_group_drops_the_obsolete_provider() {
    let chain = SourceProviderChain::new();
    let obsolete: Arc<dyn SourceProvider> = Arc::new(StaticProvider);
    let obsolete_weak = Arc::downgrade(&obsolete);
    chain.replace_group("maven:/workspace", Some(obsolete));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));

    chain.replace_group("maven:/workspace", None);
    assert!(obsolete_weak.upgrade().is_none());
    assert!(!chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn replacing_provider_keeps_already_returned_uri_readable() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(MaterializedSourceStore::new_for_test(temp.path()));
    let chain = SourceProviderChain::with_materialized_sources(Arc::clone(&store));
    chain.replace_group(
        "maven:/workspace",
        Some(Arc::new(MaterializingProvider {
            store: Arc::clone(&store),
            source: Arc::new(SourceFile::new(Arc::from(
                "package external; public class Persistent {}",
            ))),
        })),
    );
    drop(store);
    let path = chain
        .get_source_exact(&virtual_identity("test"), "external.Persistent")
        .expect("source before provider replacement")
        .0
        .temp_file()
        .clone();

    chain.replace_group("maven:/workspace", None);

    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "package external; public class Persistent {}"
    );
}

#[test]
fn dropping_chain_cleans_its_session_materialization_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = {
        let store = Arc::new(MaterializedSourceStore::new_for_test(temp.path()));
        let root = store.root().to_path_buf();
        let chain = SourceProviderChain::with_materialized_sources(Arc::clone(&store));
        drop(store);
        assert!(root.is_dir());
        drop(chain);
        root
    };
    assert!(!root.exists());
}

#[test]
fn removed_providers_are_dropped_after_releasing_chain_lock() {
    let chain = Arc::new(SourceProviderChain::new());
    let dropped_off_lock = Arc::new(AtomicBool::new(false));
    chain.replace_group(
        "maven:/workspace",
        Some(Arc::new(ReentrantDropProvider {
            chain: Arc::downgrade(&chain),
            dropped_off_lock: Arc::clone(&dropped_off_lock),
        })),
    );

    chain.replace_group("maven:/workspace", None);

    assert!(dropped_off_lock.load(Ordering::Acquire));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn panicking_priority_is_rejected_and_dropped_after_releasing_chain_lock() {
    let chain = Arc::new(SourceProviderChain::new());
    let dropped_off_lock = Arc::new(AtomicBool::new(false));

    chain.register(Arc::new(PanickingPriorityDropProvider {
        chain: Arc::downgrade(&chain),
        dropped_off_lock: Arc::clone(&dropped_off_lock),
    }));

    assert!(dropped_off_lock.load(Ordering::Acquire));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn panicking_replacement_metadata_leaves_the_existing_group_unchanged() {
    let chain = Arc::new(SourceProviderChain::new());
    let identity = virtual_identity("stable-before-bad-replacement");
    chain.replace_group(
        "stable-group",
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "stable source",
        )]))),
    );
    let dropped_off_lock = Arc::new(AtomicBool::new(false));

    chain.replace_group(
        "stable-group",
        Some(Arc::new(PanickingPriorityDropProvider {
            chain: Arc::downgrade(&chain),
            dropped_off_lock: Arc::clone(&dropped_off_lock),
        })),
    );

    assert!(dropped_off_lock.load(Ordering::Acquire));
    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "stable source"
    );
}

#[test]
fn panicking_provider_drop_is_contained_after_releasing_chain_lock() {
    let chain = Arc::new(SourceProviderChain::new());
    let dropped_off_lock = Arc::new(AtomicBool::new(false));
    chain.replace_group(
        "panicking-drop",
        Some(Arc::new(PanickingDropProvider {
            chain: Arc::downgrade(&chain),
            dropped_off_lock: Arc::clone(&dropped_off_lock),
        })),
    );

    chain.replace_group("panicking-drop", None);

    assert!(dropped_off_lock.load(Ordering::Acquire));
    chain.register(Arc::new(StaticProvider));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn multiple_panicking_provider_drops_are_isolated_from_each_other() {
    let chain = Arc::new(SourceProviderChain::new());
    let dropped_off_lock = Arc::new(AtomicUsize::new(0));
    {
        let mut state = chain.state().write();
        for publication in [10_001, 10_002] {
            state.providers.push(super::ProviderEntry {
                group: Some(Arc::from("panicking-batch")),
                ticket: None,
                provider: super::ProviderHandle::new(Arc::new(CountingPanickingDropProvider {
                    chain: Arc::downgrade(&chain),
                    dropped_off_lock: Arc::clone(&dropped_off_lock),
                })),
                priority: 10,
                name: "counting panicking drop",
                exact_routes: None,
                publication,
            });
        }
    }

    chain.replace_group("panicking-batch", None);

    assert_eq!(dropped_off_lock.load(Ordering::Acquire), 2);
    chain.register(Arc::new(StaticProvider));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
}

#[test]
fn uncommitted_group_entry_is_not_request_visible() {
    let chain = SourceProviderChain::new();
    let index = WorkspaceIndex::new(PathBuf::from("/provider-uncommitted-test"));
    let ticket = index.begin_external_source_refresh("maven:/workspace");
    assert!(chain.observe_group_refresh(&ticket));
    assert!(chain.replace_group_for_refresh(&ticket, Some(Arc::new(NewStaticProvider))));
    assert!(!chain.can_provide_exact(&virtual_identity("test"), "new.Dependency"));

    let _ = index
        .replace_external_sources_for_refresh(&ticket, &[])
        .expect("workspace classpath commits");
    assert!(chain.can_provide_exact(&virtual_identity("test"), "new.Dependency"));
}

#[test]
fn superseded_group_refresh_cannot_replace_newer_provider() {
    let chain = Arc::new(SourceProviderChain::new());
    let temp = tempfile::tempdir().expect("temporary provider refresh workspace");
    let workspace = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace.clone());
    let old_ticket = index.begin_external_source_refresh("gradle:/workspace");
    assert!(chain.observe_group_refresh(&old_ticket));
    let _ = index
        .replace_external_sources_for_refresh(&old_ticket, &[])
        .expect("initial workspace classpath commits");
    assert!(chain.replace_group_for_refresh(&old_ticket, Some(Arc::new(StaticProvider))));
    let old_classpath =
        crate::project_classpath::begin_project_classpath_refresh(&workspace, &old_ticket)
            .expect("first workspace ticket owns the classpath side refresh");
    assert!(
        crate::project_classpath::publish_maven_dependency_snapshot_for_refresh(
            &old_classpath,
            vec![PathBuf::from("/libs/old.jar")],
        )
    );
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
    let impostor_index = WorkspaceIndex::new(workspace.clone());
    let impostor = impostor_index.begin_external_source_refresh("gradle:/workspace");
    assert!(!chain.observe_group_refresh(&impostor));
    let old_ready = Arc::new(Barrier::new(2));
    let release_old = Arc::new(Barrier::new(2));
    let old_task = {
        let chain = Arc::clone(&chain);
        let old_ready = Arc::clone(&old_ready);
        let release_old = Arc::clone(&release_old);
        std::thread::spawn(move || {
            old_ready.wait();
            release_old.wait();
            (
                chain.replace_group_for_refresh(&old_ticket, Some(Arc::new(StaticProvider))),
                crate::project_classpath::publish_maven_dependency_snapshot_for_refresh(
                    &old_classpath,
                    vec![PathBuf::from("/libs/old.jar")],
                ),
            )
        })
    };

    old_ready.wait();
    let new_ticket = index.begin_external_source_refresh("gradle:/workspace");
    assert!(chain.observe_group_refresh(&new_ticket));
    let new_classpath =
        crate::project_classpath::begin_project_classpath_refresh(&workspace, &new_ticket)
            .expect("newer workspace ticket supersedes the classpath side refresh");
    assert!(chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
    assert!(!chain.can_provide_exact(&virtual_identity("test"), "new.Dependency"));
    assert_eq!(
        crate::project_classpath::known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/old.jar")])
    );

    let _ = index
        .replace_external_sources_for_refresh(&new_ticket, &[])
        .expect("newest workspace classpath commits");
    assert!(!chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
    assert!(!chain.can_provide_exact(&virtual_identity("test"), "new.Dependency"));
    assert_eq!(
        crate::project_classpath::known_dependency_jars(&workspace),
        None
    );

    assert!(chain.replace_group_for_refresh(&new_ticket, Some(Arc::new(NewStaticProvider))));
    assert!(
        crate::project_classpath::publish_maven_dependency_snapshot_for_refresh(
            &new_classpath,
            vec![PathBuf::from("/libs/new.jar")],
        )
    );
    release_old.wait();

    assert_eq!(old_task.join().expect("old refresh task"), (false, false));
    assert!(!chain.can_provide_exact(&virtual_identity("test"), "registered.DuringLookup"));
    assert!(chain.can_provide_exact(&virtual_identity("test"), "new.Dependency"));
    assert_eq!(
        crate::project_classpath::known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new.jar")])
    );
    assert!(std::mem::size_of_val(&new_ticket) <= 24);
    assert!(crate::project_classpath::reset_project_classpaths_for_refresh(&new_classpath));
}

#[test]
fn partial_refresh_routes_successes_to_r2_and_failures_only_to_retained_r1() {
    let chain = SourceProviderChain::new();
    let index = WorkspaceIndex::new(PathBuf::from("/partial-provider-refresh"));
    let first = PathBuf::from("/libs/first.jar");
    let retained_failure = PathBuf::from("/libs/retained-failure.jar");
    let new_failure = PathBuf::from("/libs/new-failure.jar");
    let removed = PathBuf::from("/libs/removed.jar");
    let old_paths = vec![first.clone(), retained_failure.clone(), removed.clone()];

    let r1 = index.begin_external_source_refresh("maven:/workspace");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, &old_paths)
        .expect("R1 classpath");
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(VersionedProvider::new([
            (first.clone(), "R1 first"),
            (retained_failure.clone(), "R1 retained"),
            (removed.clone(), "R1 removed"),
        ])))
    ));

    let r2 = index.begin_external_source_refresh("maven:/workspace");
    assert!(chain.observe_group_refresh(&r2));
    let desired = vec![first.clone(), retained_failure.clone(), new_failure.clone()];
    let desired_identities: Vec<_> = desired
        .iter()
        .map(|path| ExternalSourceIdentity::for_archive_path(path))
        .collect();
    let staged: Arc<dyn SourceProvider> = Arc::new(VersionedProvider::new([
        (first.clone(), "R2 first"),
        (retained_failure.clone(), "R2 retained"),
        (new_failure.clone(), "R2 new"),
    ]));
    let publication = chain
        .begin_group_publication(&r2, &desired_identities, Some(Arc::clone(&staged)))
        .expect("R2 provider publication begins");
    assert!(
        chain
            .get_source_exact(
                &ExternalSourceIdentity::for_archive_path(&first),
                "dep.Type",
            )
            .is_some(),
        "an uncommitted successor must not hide the last committed generation"
    );
    assert!(
        chain
            .get_source_exact(
                &ExternalSourceIdentity::for_archive_path(&removed),
                "dep.Type",
            )
            .is_some(),
        "the old classpath remains coherent before the index commit"
    );

    let _ = index
        .replace_external_sources_for_refresh(&r2, &desired)
        .expect("R2 classpath reservation commits");
    assert!(chain
        .get_source_exact(
            &ExternalSourceIdentity::for_archive_path(&first),
            "dep.Type",
        )
        .is_none());

    assert!(chain.record_group_publication_success(
        &publication,
        &ExternalSourceIdentity::for_archive_path(&first),
    ));
    assert!(chain.finish_group_publication(&publication));

    let source = |path: &Path| {
        chain
            .get_source_exact(&ExternalSourceIdentity::for_archive_path(path), "dep.Type")
            .map(|(source, _)| source.content().to_string())
    };
    assert_eq!(source(&first).as_deref(), Some("R2 first"));
    assert_eq!(source(&retained_failure).as_deref(), Some("R1 retained"));
    assert_eq!(source(&new_failure), None);
    assert_eq!(source(&removed), None);
}

#[test]
fn source_return_is_rejected_when_its_group_ticket_changes_during_io() {
    let chain = Arc::new(SourceProviderChain::new());
    let index = WorkspaceIndex::new(PathBuf::from("/provider-final-fence"));
    let source_path = PathBuf::from("/libs/blocking.jar");
    let r1 = index.begin_external_source_refresh("gradle:/workspace");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source_path))
        .expect("R1 commit");
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(BlockingProvider {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }))
    ));

    let lookup = {
        let chain = Arc::clone(&chain);
        let source_identity = ExternalSourceIdentity::for_archive_path(&source_path);
        std::thread::spawn(move || chain.get_source_exact(&source_identity, "dep.Type"))
    };
    entered.wait();

    let r2 = index.begin_external_source_refresh("gradle:/workspace");
    assert!(chain.observe_group_refresh(&r2));
    let _ = index
        .replace_external_sources_for_refresh(&r2, &[])
        .expect("R2 commit supersedes R1 provider");
    release.wait();

    assert!(lookup.join().unwrap().is_none());
}

#[test]
fn virtual_jdk_and_relative_archive_jdk_never_cross_route() {
    let virtual_jdk = ExternalSourceIdentity::for_virtual("jdk");
    let archive_jdk = ExternalSourceIdentity::for_archive_path(Path::new("jdk"));
    assert_ne!(virtual_jdk, archive_jdk);

    let chain = SourceProviderChain::new();
    chain.register(Arc::new(VersionedProvider::from_identities([
        (virtual_jdk.clone(), "virtual JDK"),
        (archive_jdk.clone(), "archive named JDK"),
    ])));
    assert_eq!(
        chain
            .get_source_exact(&virtual_jdk, "dep.Type")
            .unwrap()
            .0
            .content(),
        "virtual JDK"
    );
    assert_eq!(
        chain
            .get_source_exact(&archive_jdk, "dep.Type")
            .unwrap()
            .0
            .content(),
        "archive named JDK"
    );
}

#[cfg(unix)]
#[test]
fn lossy_colliding_archive_names_route_to_distinct_sources() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join(OsString::from_vec(vec![0x80]));
    let second = temp.path().join(OsString::from_vec(vec![0x81]));
    assert_eq!(first.to_string_lossy(), second.to_string_lossy());
    let first_identity = ExternalSourceIdentity::for_archive_path(&first);
    let second_identity = ExternalSourceIdentity::for_archive_path(&second);
    assert_ne!(first_identity, second_identity);

    let chain = SourceProviderChain::new();
    chain.register(Arc::new(VersionedProvider::from_identities([
        (first_identity.clone(), "first invalid-byte archive"),
        (second_identity.clone(), "second invalid-byte archive"),
    ])));
    assert_eq!(
        chain
            .get_source_exact(&first_identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "first invalid-byte archive"
    );
    assert_eq!(
        chain
            .get_source_exact(&second_identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "second invalid-byte archive"
    );
}

#[cfg(unix)]
#[test]
fn archive_plan_deduplicates_aliases_and_keeps_attachment() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("library.jar");
    let alias = temp.path().join("alias.jar");
    let javadoc = temp.path().join("library-javadoc.jar");
    std::fs::write(&target, b"binary").unwrap();
    std::fs::write(&javadoc, b"docs").unwrap();
    symlink(&target, &alias).unwrap();

    let plans = super::capture_dependency_archive_plan(
        &[alias, target],
        &[(temp.path().join("library.jar"), javadoc.clone())],
    )
    .expect("dependency archive plan");
    assert_eq!(plans.len(), 1);
    assert_eq!(
        plans[0].javadoc.as_ref().map(ExternalSourceArchive::path),
        Some(javadoc.as_path())
    );
}

#[cfg(unix)]
#[test]
fn archive_plan_keeps_the_first_captured_target_after_symlink_retarget() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.jar");
    let second = temp.path().join("second.jar");
    let alias = temp.path().join("current.jar");
    std::fs::write(&first, b"first generation").unwrap();
    std::fs::write(&second, b"second generation").unwrap();
    symlink(&first, &alias).unwrap();

    let plans = super::capture_dependency_archive_plan(std::slice::from_ref(&alias), &[])
        .expect("dependency archive plan");
    assert_eq!(plans.len(), 1);
    let first_identity = plans[0].archive.identity().clone();
    assert_eq!(
        first_identity,
        ExternalSourceIdentity::for_archive_path(&first)
    );

    std::fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    assert_ne!(
        first_identity,
        ExternalSourceIdentity::for_archive_path(&alias)
    );
    assert!(
        plans[0]
            .archive
            .snapshot(super::MAX_BINARY_ARCHIVE_SNAPSHOT_BYTES)
            .is_ok(),
        "the plan must reopen its captured canonical target, not the retargeted display alias"
    );
}

#[test]
fn archive_plan_rejects_conflicting_attachments_for_one_binary_identity() {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("library.jar");
    let first = temp.path().join("first-javadoc.jar");
    let second = temp.path().join("second-javadoc.jar");
    std::fs::write(&binary, b"binary").unwrap();
    std::fs::write(&first, b"first docs").unwrap();
    std::fs::write(&second, b"second docs").unwrap();

    let plans = super::capture_dependency_archive_plan(
        &[],
        &[(binary.clone(), first.clone()), (binary, second)],
    )
    .expect("dependency archive plan");

    assert_eq!(plans.len(), 1);
    assert!(
        plans[0].javadoc.is_none(),
        "a binary identity with conflicting documentation has no provably correct attachment"
    );
}

#[test]
fn unreadable_first_attachment_still_makes_a_conflicting_second_one_ambiguous() {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("library.jar");
    let missing = temp.path().join("missing-javadoc.jar");
    let readable = temp.path().join("readable-javadoc.jar");
    std::fs::write(&binary, b"binary").unwrap();
    std::fs::write(&readable, b"docs").unwrap();

    let plans = super::capture_dependency_archive_plan(
        &[],
        &[(binary.clone(), missing), (binary, readable)],
    )
    .expect("dependency archive plan");

    assert_eq!(plans.len(), 1);
    assert!(plans[0].javadoc_ambiguous);
    assert!(plans[0].javadoc.is_none());
}

#[test]
fn unreadable_binary_aborts_the_complete_plan_instead_of_truncating_it() {
    let temp = tempfile::tempdir().unwrap();
    let readable = temp.path().join("readable.jar");
    let missing = temp.path().join("missing.jar");
    std::fs::write(&readable, b"binary").unwrap();

    let error = super::capture_dependency_archive_plan(&[readable, missing.clone()], &[])
        .expect_err("one missing mandatory binary must reject the whole generation");

    assert!(error.contains("failed to stage dependency archive"));
    assert!(error.contains(&missing.display().to_string()));
}

#[test]
fn dependency_plan_stages_metadata_without_a_throwaway_content_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("library.jar");
    std::fs::write(&binary, b"binary bytes").unwrap();

    let plans = super::capture_dependency_archive_plan(std::slice::from_ref(&binary), &[])
        .expect("dependency archive plan");

    assert_eq!(plans.len(), 1);
    assert!(plans[0].archive.expected_file_version().is_some());
    assert!(plans[0].archive.expected_content().is_none());
}

#[cfg(unix)]
#[test]
fn publication_plan_survives_symlink_retarget_without_rekeying() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.jar");
    let second = temp.path().join("second.jar");
    let alias = temp.path().join("current.jar");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();
    symlink(&first, &alias).unwrap();

    let archive = ExternalSourceArchive::capture(&alias);
    let old_identity = archive.identity().clone();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let chain = SourceProviderChain::new();
    let r1 = index.begin_external_source_refresh("gradle:retarget");
    assert!(chain.observe_group_refresh(&r1));
    let _refresh = index
        .replace_external_archives_for_refresh(&r1, std::slice::from_ref(&archive))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(VersionedProvider::from_identities([(
            old_identity.clone(),
            "R1",
        )])))
    ));

    let r2 = index.begin_external_source_refresh("gradle:retarget");
    assert!(chain.observe_group_refresh(&r2));
    let publication = chain
        .begin_group_publication(&r2, std::slice::from_ref(&old_identity), None)
        .unwrap();
    let refresh = index
        .replace_external_archives_for_refresh(&r2, std::slice::from_ref(&archive))
        .unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    let new_identity = ExternalSourceIdentity::for_archive_path(&alias);
    assert_ne!(old_identity, new_identity);
    assert!(refresh.revision_for_identity(&old_identity).is_some());
    assert!(refresh.revision_for(&alias).is_none());
    assert!(chain.get_source_exact(&old_identity, "dep.Type").is_none());

    assert!(chain.finish_group_publication(&publication));
    assert_eq!(
        chain
            .get_source_exact(&old_identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R1"
    );
    assert!(chain.get_source_exact(&new_identity, "dep.Type").is_none());
}

#[test]
fn publication_rejects_success_outside_captured_plan() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-plan-membership"));
    let chain = SourceProviderChain::new();
    let planned = ExternalSourceArchive::capture(Path::new("/libs/planned.jar"));
    let unplanned = ExternalSourceIdentity::for_archive_path(Path::new("/libs/other.jar"));
    let ticket = index.begin_external_source_refresh("maven:membership");
    assert!(chain.observe_group_refresh(&ticket));
    let publication = chain
        .begin_group_publication(&ticket, std::slice::from_ref(planned.identity()), None)
        .unwrap();
    let _refresh = index
        .replace_external_archives_for_refresh(&ticket, std::slice::from_ref(&planned))
        .unwrap();

    assert!(!chain.record_group_publication_success(&publication, &unplanned));
    assert!(chain.abort_group_publication(&publication));
}

#[test]
fn publication_capability_cannot_be_replaced_or_abort_its_successor() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-publication-capability"));
    let chain = SourceProviderChain::new();
    let identity = ExternalSourceIdentity::for_archive_path(Path::new("/libs/planned.jar"));
    let ticket = index.begin_external_source_refresh("maven:capability");
    assert!(chain.observe_group_refresh(&ticket));

    let first = chain
        .begin_group_publication(&ticket, std::slice::from_ref(&identity), None)
        .expect("first publication owns the pending slot");
    assert!(
        chain
            .begin_group_publication(&ticket, std::slice::from_ref(&identity), None)
            .is_none(),
        "the same refresh ticket must not replace its live publication capability"
    );
    assert!(chain.abort_group_publication(&first));

    let successor = chain
        .begin_group_publication(&ticket, std::slice::from_ref(&identity), None)
        .expect("an explicitly aborted slot can be reused");
    assert!(
        !chain.abort_group_publication(&first),
        "a stale capability must not erase its same-ticket successor"
    );
    assert!(chain.abort_group_publication(&successor));
}

#[test]
fn failed_publication_never_adopts_another_groups_provider() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-group-isolation"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let first = index.begin_external_source_refresh("build:first");
    assert!(chain.observe_group_refresh(&first));
    let _ = index
        .replace_external_sources_for_refresh(&first, std::slice::from_ref(&source))
        .expect("first group commits");
    assert!(chain.replace_group_for_refresh(
        &first,
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "first group",
        )])))
    ));

    let second = index.begin_external_source_refresh("build:second");
    assert!(chain.observe_group_refresh(&second));
    let publication = chain
        .begin_group_publication(&second, std::slice::from_ref(&identity), None)
        .expect("second group publication");
    let _ = index
        .replace_external_sources_for_refresh(&second, std::slice::from_ref(&source))
        .expect("second group commits");
    assert!(chain.finish_group_publication(&publication));
    assert!(
        chain.get_source_exact(&identity, "dep.Type").is_none(),
        "a failed group may retain only its own previous provider generation"
    );
}

#[test]
fn failed_successor_cannot_strand_the_committed_predecessor_publication() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-detached-predecessor"));
    let chain = Arc::new(SourceProviderChain::new());
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let r1 = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "R1",
        )])))
    ));

    let r2 = index.begin_external_source_refresh("build:gradle");
    let r2_guard = chain.observe_group_refresh_guard(&r2).unwrap();
    let r2_provider: Arc<dyn SourceProvider> = Arc::new(VersionedProvider::from_identities([(
        identity.clone(),
        "R2",
    )]));
    let r2_publication = chain
        .begin_group_publication(&r2, std::slice::from_ref(&identity), Some(r2_provider))
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source))
        .unwrap();

    let r3 = index.begin_external_source_refresh("build:gradle");
    let r3_guard = chain
        .observe_group_refresh_guard(&r3)
        .expect("successor begins resolving");
    drop(r3_guard);

    assert!(r2.is_committed());
    assert!(!r2.is_latest());
    assert!(chain.record_group_publication_success(&r2_publication, &identity));
    assert!(chain.finish_group_publication(&r2_publication));
    r2_guard.disarm();

    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R2"
    );
}

#[test]
fn successor_inherits_the_committed_pending_generations_effective_routes() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-pending-fallback"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let r1 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "R1",
        )])))
    ));

    let r2 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r2));
    let r2_provider: Arc<dyn SourceProvider> = Arc::new(VersionedProvider::from_identities([(
        identity.clone(),
        "R2",
    )]));
    let r2_publication = chain
        .begin_group_publication(&r2, std::slice::from_ref(&identity), Some(r2_provider))
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.record_group_publication_success(&r2_publication, &identity));

    let r3 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r3));
    let r3_publication = chain
        .begin_group_publication(&r3, std::slice::from_ref(&identity), None)
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r3, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.finish_group_publication(&r3_publication));

    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R2"
    );
    assert!(!chain.finish_group_publication(&r2_publication));
}

#[test]
fn prepared_successor_retries_when_predecessor_progress_changes_its_fallback() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-stale-preparation"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let r1 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "R1",
        )])))
    ));

    let r2 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r2));
    let r2_publication = chain
        .begin_group_publication(
            &r2,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "R2",
            )]))),
        )
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source))
        .unwrap();

    let r3 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r3));
    let mut stale_preparation = Some(
        chain
            .prepare_group_publication(&r3, std::slice::from_ref(&identity), None)
            .unwrap(),
    );

    assert!(chain.record_group_publication_success(&r2_publication, &identity));
    assert!(chain.finish_group_publication(&r2_publication));
    assert!(matches!(
        chain.begin_prepared_group_publication(&mut stale_preparation),
        Err(PreparedPublicationCommitError::StaleProviderState)
    ));
    assert!(
        stale_preparation.is_some(),
        "a stale commit must not consume callback-owned preparation"
    );
    drop(stale_preparation);

    let mut retried_preparation = Some(
        chain
            .prepare_group_publication(&r3, std::slice::from_ref(&identity), None)
            .unwrap(),
    );
    let r3_publication = chain
        .begin_prepared_group_publication(&mut retried_preparation)
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r3, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.finish_group_publication(&r3_publication));

    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R2"
    );
}

#[test]
fn coordinated_publication_automatically_retries_one_stale_preparation() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-coordinated-retry"));
    let chain = Arc::new(SourceProviderChain::new());
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);
    let mutated = Arc::new(AtomicBool::new(false));

    let r1 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(OneShotChainMutatingSnapshotProvider {
            chain: Arc::downgrade(&chain),
            identity: identity.clone(),
            mutated: Arc::clone(&mutated),
        })),
    ));

    let r2 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r2));
    let (guard, publication) = super::begin_coordinated_external_publication(
        &index,
        &r2,
        Some(&chain),
        std::slice::from_ref(&identity),
        None,
    )
    .expect("coordinator retries the revision changed by the first callback");
    let publication = publication.expect("provider chain returns a publication capability");
    let _ = index
        .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source))
        .unwrap();
    let retirement = chain
        .finish_group_publication_deferred(&publication)
        .expect("finish retried publication");
    drop(guard);
    drop(retirement);

    assert!(mutated.load(Ordering::Acquire));
    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .expect("R2 retained the prepared R1 fallback")
            .0
            .content(),
        "R1"
    );
}

#[test]
fn older_publication_finish_preserves_a_newer_staged_provider_entry() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-staged-successor"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);

    let r1 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "R1",
        )])))
    ));

    let r2 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r2));
    let r2_publication = chain
        .begin_group_publication(
            &r2,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "R2",
            )]))),
        )
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.record_group_publication_success(&r2_publication, &identity));

    let r3 = index.begin_external_source_refresh("build:maven");
    assert!(chain.observe_group_refresh(&r3));
    assert!(chain.replace_group_for_refresh(
        &r3,
        Some(Arc::new(VersionedProvider::from_identities([(
            identity.clone(),
            "R3",
        )])))
    ));

    assert!(chain.finish_group_publication(&r2_publication));
    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R2"
    );

    let _ = index
        .replace_external_sources_for_refresh(&r3, std::slice::from_ref(&source))
        .unwrap();
    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "R3"
    );
}

#[test]
fn exact_route_priority_is_selected_per_identity() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-per-route-priority"));
    let chain = SourceProviderChain::new();
    let first = PathBuf::from("/libs/first.jar");
    let second = PathBuf::from("/libs/second.jar");
    let first_identity = ExternalSourceIdentity::for_archive_path(&first);
    let second_identity = ExternalSourceIdentity::for_archive_path(&second);

    let a1 = index.begin_external_source_refresh("build:a");
    assert!(chain.observe_group_refresh(&a1));
    let _ = index
        .replace_external_sources_for_refresh(&a1, std::slice::from_ref(&first))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &a1,
        Some(Arc::new(RankedProvider::new(
            [(first_identity.clone(), "A fallback p10")],
            10,
            Arc::new(AtomicUsize::new(0)),
        )))
    ));

    let a2 = index.begin_external_source_refresh("build:a");
    assert!(chain.observe_group_refresh(&a2));
    let a2_publication = chain
        .begin_group_publication(
            &a2,
            &[first_identity.clone(), second_identity.clone()],
            Some(Arc::new(RankedProvider::new(
                [(second_identity.clone(), "A current p30")],
                30,
                Arc::new(AtomicUsize::new(0)),
            ))),
        )
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&a2, &[first.clone(), second.clone()])
        .unwrap();
    assert!(chain.record_group_publication_success(&a2_publication, &second_identity));
    assert!(chain.finish_group_publication(&a2_publication));

    let b1 = index.begin_external_source_refresh("build:b");
    assert!(chain.observe_group_refresh(&b1));
    let _ = index
        .replace_external_sources_for_refresh(&b1, std::slice::from_ref(&second))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &b1,
        Some(Arc::new(RankedProvider::new(
            [(second_identity.clone(), "B p20")],
            20,
            Arc::new(AtomicUsize::new(0)),
        )))
    ));

    assert_eq!(
        chain
            .get_source_exact(&first_identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "A fallback p10"
    );
    assert_eq!(
        chain
            .get_source_exact(&second_identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "B p20"
    );
}

#[test]
fn repeated_failed_refreshes_keep_fallback_lookup_depth_constant() {
    let index = WorkspaceIndex::new(PathBuf::from("/provider-flat-fallback"));
    let chain = SourceProviderChain::new();
    let source = PathBuf::from("/libs/shared.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);
    let capability_calls = Arc::new(AtomicUsize::new(0));

    let first = index.begin_external_source_refresh("build:gradle");
    assert!(chain.observe_group_refresh(&first));
    let _ = index
        .replace_external_sources_for_refresh(&first, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &first,
        Some(Arc::new(RankedProvider::new(
            [(identity.clone(), "stable fallback")],
            20,
            Arc::clone(&capability_calls),
        )))
    ));

    for _ in 0..128 {
        let ticket = index.begin_external_source_refresh("build:gradle");
        assert!(chain.observe_group_refresh(&ticket));
        let publication = chain
            .begin_group_publication(&ticket, std::slice::from_ref(&identity), None)
            .unwrap();
        let _ = index
            .replace_external_sources_for_refresh(&ticket, std::slice::from_ref(&source))
            .unwrap();
        assert!(chain.finish_group_publication(&publication));
    }

    capability_calls.store(0, Ordering::Relaxed);
    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "stable fallback"
    );
    assert_eq!(
        capability_calls.load(Ordering::Relaxed),
        1,
        "lookup must reach the retained leaf directly instead of traversing one wrapper per refresh"
    );
}

#[test]
fn lookup_result_is_rejected_when_provider_order_changes_during_io() {
    let chain = Arc::new(SourceProviderChain::new());
    let identity = virtual_identity("ordering-fence");
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    chain.register(Arc::new(BlockingProvider {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));

    let lookup = {
        let chain = Arc::clone(&chain);
        let identity = identity.clone();
        std::thread::spawn(move || chain.get_source_exact(&identity, "dep.Type"))
    };
    entered.wait();
    chain.register(Arc::new(RankedProvider::new(
        [(identity.clone(), "higher priority")],
        10,
        Arc::new(AtomicUsize::new(0)),
    )));
    release.wait();

    assert!(lookup.join().unwrap().is_none());
    assert_eq!(
        chain
            .get_source_exact(&identity, "dep.Type")
            .unwrap()
            .0
            .content(),
        "higher priority"
    );
}

#[test]
fn publication_preparation_allows_snapshot_to_reenter_same_owner_refresh() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from(
        "/provider-snapshot-owner-reentry",
    )));
    let chain = SourceProviderChain::new();
    let owner = "build:snapshot-reentry";
    let identity = virtual_identity("snapshot-reentry");
    let refreshes = Arc::new(AtomicUsize::new(0));
    let ticket = index.begin_external_source_refresh(owner);
    assert!(chain.observe_group_refresh(&ticket));

    let prepared = chain
        .prepare_group_publication(
            &ticket,
            std::slice::from_ref(&identity),
            Some(Arc::new(RefreshingSnapshotProvider {
                index: Arc::clone(&index),
                owner,
                refreshes: Arc::clone(&refreshes),
            })),
        )
        .expect("callback-heavy preparation completes outside the owner gate");

    assert_eq!(refreshes.load(Ordering::Acquire), 1);
    assert!(!ticket.is_latest());
    let mut prepared = Some(prepared);
    assert!(matches!(
        chain.begin_prepared_group_publication(&mut prepared),
        Err(PreparedPublicationCommitError::Superseded)
    ));
    assert!(
        prepared.is_some(),
        "failed commit retains prepared providers"
    );
}

#[test]
fn deferred_finish_releases_reentrant_provider_only_after_owner_gate() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from(
        "/provider-drop-owner-reentry",
    )));
    let chain = SourceProviderChain::new();
    let owner = "build:drop-reentry";
    let source = PathBuf::from("/libs/reentrant-drop.jar");
    let identity = ExternalSourceIdentity::for_archive_path(&source);
    let drops = Arc::new(AtomicUsize::new(0));

    let r1 = index.begin_external_source_refresh(owner);
    assert!(chain.observe_group_refresh(&r1));
    let _ = index
        .replace_external_sources_for_refresh(&r1, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.replace_group_for_refresh(
        &r1,
        Some(Arc::new(RefreshingDropProvider {
            index: Arc::clone(&index),
            owner,
            identity: identity.clone(),
            drops: Arc::clone(&drops),
        }))
    ));

    let r2 = index.begin_external_source_refresh(owner);
    assert!(chain.observe_group_refresh(&r2));
    let prepared = chain
        .prepare_group_publication(
            &r2,
            std::slice::from_ref(&identity),
            Some(Arc::new(VersionedProvider::from_identities([(
                identity.clone(),
                "R2",
            )]))),
        )
        .unwrap();
    let mut prepared = Some(prepared);

    let owner_gate = index.begin_external_source_publication(&r2).unwrap();
    let publication = chain
        .begin_prepared_group_publication(&mut prepared)
        .unwrap();
    let _ = index
        .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source))
        .unwrap();
    assert!(chain.record_group_publication_success(&publication, &identity));
    let retirement = chain
        .finish_group_publication_deferred(&publication)
        .unwrap();

    assert_eq!(
        drops.load(Ordering::Acquire),
        0,
        "commit mutation must not execute provider destructors beneath the owner gate"
    );
    drop(owner_gate);
    drop(retirement);
    assert_eq!(drops.load(Ordering::Acquire), 2);
}
