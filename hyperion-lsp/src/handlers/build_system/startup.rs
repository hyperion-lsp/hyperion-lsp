// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Initial build refresh presentation and lifecycle integration.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tower_lsp::lsp_types::MessageType;
use tracing::info;

use super::{
    refresh_workspace_build_systems, run_build_refresh, BuildReadinessCompletion,
    BuildRefreshCoordinator, BuildRefreshTicket,
};

pub(crate) struct BuildProgressInputs {
    pub(crate) root: std::path::PathBuf,
    pub(crate) supports_progress: bool,
    pub(crate) source_providers: Arc<crate::source_provider::SourceProviderChain>,
    pub(crate) workspace_source_indexing:
        Arc<crate::server::source_indexing::WorkspaceSourceIndexCompletion>,
}

#[derive(Clone)]
struct BuildProgressCompletion {
    progress: crate::progress::ProgressReporter,
    begun: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}

impl BuildProgressCompletion {
    fn new(progress: crate::progress::ProgressReporter) -> Self {
        Self {
            progress,
            begun: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn begin(&self) {
        self.progress
            .begin(
                Some("Detecting workspace build system...".to_string()),
                None,
            )
            .await;
        self.begun.store(true, Ordering::Release);
    }

    fn finish(&self, message: impl Into<String>) {
        // A superseding refresh can win before the Begin notification has
        // completed. In that case there is no progress lifecycle to close.
        if !self.begun.load(Ordering::Acquire) {
            return;
        }
        if self
            .completed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let progress = self.progress.clone();
        let message = message.into();
        tokio::spawn(async move {
            progress.end(Some(message)).await;
        });
    }
}

pub(crate) fn spawn_initial_build_refresh(
    publication: crate::server::diagnostics::InvalidationRuntime,
    inputs: BuildProgressInputs,
    build_refresh: BuildRefreshCoordinator,
) {
    let readiness = Arc::clone(&publication.destructive_refactoring);
    build_refresh.spawn_with(
        move || BuildReadinessCompletion::reserve(readiness),
        move |refresh_ticket, build_readiness| {
            run_initial_build_refresh(refresh_ticket, build_readiness, publication, inputs)
        },
    );
}

async fn run_initial_build_refresh(
    refresh_ticket: BuildRefreshTicket,
    mut build_readiness: BuildReadinessCompletion,
    publication: crate::server::diagnostics::InvalidationRuntime,
    inputs: BuildProgressInputs,
) {
    let BuildProgressInputs {
        root,
        supports_progress,
        source_providers,
        workspace_source_indexing,
    } = inputs;
    let host = Arc::clone(&publication.host);
    let client = publication.client.clone();
    let bulk_source_index =
        crate::server::source_indexing::BulkSourceIndexContext::from(&publication)
            .after_workspace_source_indexing(workspace_source_indexing);

    let progress = crate::progress::ProgressReporter::new(
        client.clone(),
        "Resolving Build System",
        supports_progress,
    );
    let bulk_source_index = bulk_source_index.with_build_progress(progress.clone());
    let progress_completion = BuildProgressCompletion::new(progress);
    let cancellation_progress = progress_completion.clone();
    let work_progress = progress_completion.clone();
    let refresh_root = root.clone();
    let refresh_host = Arc::clone(&host);
    let refresh_publication = publication.clone();
    let refresh_providers = Arc::clone(&source_providers);
    let refresh_client = client.clone();
    let work = async move {
        work_progress.begin().await;
        refresh_workspace_build_systems(
            &refresh_root,
            &refresh_host,
            &refresh_publication.workspace_ready,
            &refresh_providers,
            bulk_source_index,
            &refresh_publication.project_classpaths,
            &refresh_client,
        )
        .await
    };
    let success_progress = progress_completion;
    let cache_root = root;
    let finalize = move |outcome: super::BuildSystemRefreshOutcome| async move {
        success_progress
            .progress
            .report(Some("Publishing build diagnostics...".to_string()), None)
            .await;
        let stats = host.index().stats();
        publication.publish_workspace_revision("build-system").await;
        build_readiness.finish(outcome.gradle_complete, outcome.maven_complete);
        // The workspace cache written by the initial root scan predates
        // generated and explicitly declared build sources. Capture a second
        // snapshot after their publication so the next startup can restore
        // unchanged sources from the authoritative build-source scan.
        hyperion_workspace::IndexCache::new(&cache_root).save_index_async(Arc::clone(host.index()));
        info!(
            "Build-system integration complete: selected={:?}, mixed={}, Gradle complete={}, Maven complete={}, {} symbols from {} files",
            outcome.selected,
            outcome.mixed_root_models,
            outcome.gradle_complete,
            outcome.maven_complete,
            stats.symbols_indexed,
            stats.files_indexed,
        );

        let complete = outcome.gradle_complete && outcome.maven_complete;
        let () = client
            .log_message(
                if complete {
                    MessageType::INFO
                } else {
                    MessageType::WARNING
                },
                format!(
                    "{} Build system indexed: {} symbols from {} files",
                    if complete { "✓" } else { "⚠" },
                    stats.symbols_indexed,
                    stats.files_indexed,
                ),
            )
            .await;
        success_progress.finish(if !outcome.dependency_issues.is_empty() {
            format!(
                "⚠ {} dependency JARs indexed, {} dependency problems (see log)",
                outcome.dependency_jars_indexed,
                outcome.dependency_issues.len()
            )
        } else if complete {
            "✓ Build dependencies resolved".to_string()
        } else {
            "⚠ Build model incomplete".to_string()
        });
    };
    if !Box::pin(run_build_refresh(refresh_ticket, work, finalize)).await {
        cancellation_progress.finish("Build refresh superseded");
    }
}
