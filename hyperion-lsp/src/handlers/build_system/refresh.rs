// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Deterministic build-owner selection and cross-registry refresh transactions.

use std::path::Path;
use std::sync::Arc;

use tower_lsp::lsp_types::{Diagnostic, MessageType, Url};
use tower_lsp::Client;
use tracing::{debug, info, warn};

use super::BuildManifestFamily;
use crate::semantic_host::SemanticHost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceBuildSystem {
    None,
    Gradle,
    Maven,
}

pub(crate) struct BuildSystemRefreshOutcome {
    pub(crate) selected: WorkspaceBuildSystem,
    pub(crate) mixed_root_models: bool,
    pub(crate) gradle_complete: bool,
    pub(crate) maven_complete: bool,
    /// Dependencies that could not be resolved or read. Never blocks anything:
    /// every dependency that did resolve is indexed.
    pub(crate) dependency_issues: Vec<String>,
    pub(crate) dependency_jars_indexed: usize,
}

/// Outcome of one Gradle refresh attempt.
#[derive(Default)]
pub(crate) struct GradleRefreshResult {
    /// Build structure fully understood (gates destructive refactorings).
    pub(crate) refactor_ready: bool,
    pub(crate) dependency_issues: Vec<String>,
    pub(crate) dependency_jars_indexed: usize,
}

enum MavenRefreshOutcome {
    Published(Box<crate::maven_integration::MavenInitResult>),
    /// No root POM exists and the exact prior Maven publication was removed.
    /// A nested POM still makes the model incomplete, but cannot revoke the
    /// retirement's authority to clear obsolete root-POM diagnostics.
    Retired {
        layout_supported: bool,
    },
    Failed,
}

impl MavenRefreshOutcome {
    const fn result(&self) -> Option<&crate::maven_integration::MavenInitResult> {
        match self {
            Self::Published(result) => Some(result),
            Self::Retired { .. } | Self::Failed => None,
        }
    }

    fn is_complete(&self) -> bool {
        match self {
            Self::Published(result) => maven_result_is_complete(Some(result)),
            Self::Retired { layout_supported } => *layout_supported,
            Self::Failed => false,
        }
    }

    const fn diagnostics_are_current(&self) -> bool {
        !matches!(self, Self::Failed)
    }
}

const fn select_workspace_build_system(
    gradle_is_present: bool,
    maven_is_present: bool,
) -> WorkspaceBuildSystem {
    match (gradle_is_present, maven_is_present) {
        (true, _) => WorkspaceBuildSystem::Gradle,
        (false, true) => WorkspaceBuildSystem::Maven,
        (false, false) => WorkspaceBuildSystem::None,
    }
}

/// Atomically remove one build system's workspace, classpath, and provider
/// publications, even when its root manifest still exists.
///
/// The latter is required for deterministic mixed-build policy: when both
/// root models exist, Gradle is authoritative and any previously published
/// Maven state must be retired before Gradle can take over.
pub(crate) async fn retire_build_system(
    family: BuildManifestFamily,
    root: &Path,
    host: &Arc<SemanticHost>,
    source_providers: Option<&Arc<crate::source_provider::SourceProviderChain>>,
    project_classpaths: &crate::project_classpath::ProjectClasspathStore,
) -> anyhow::Result<()> {
    // The whole retirement is short, synchronous, and mutates the index, so it
    // is one write transaction.
    let txn = host.write().await;
    let workspace_index = txn.index();
    let refresh_ticket = workspace_index.begin_external_source_refresh(family.owner());
    let source_refresh = match source_providers {
        Some(providers) => Some(
            providers
                .observe_group_refresh_guard(&refresh_ticket)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{} source-provider retirement was superseded",
                        family.display_name()
                    )
                })?,
        ),
        None => None,
    };
    let mut prepared_provider_replacement = source_providers
        .map(|_| {
            crate::source_provider::SourceProviderChain::prepare_group_replacement(None).map_err(
                |()| {
                    anyhow::anyhow!(
                        "{} source-provider retirement preparation failed",
                        family.display_name()
                    )
                },
            )
        })
        .transpose()?;
    let prepared_classpath_retirement =
        project_classpaths.prepare_project_classpath_retirement(root);
    // Retirement is a publication of the empty generation. Hold the same
    // owner gate as Maven/Gradle installation so a successor cannot be minted
    // between workspace removal and the two companion-registry removals.
    let publication_guard = workspace_index
        .begin_external_source_publication(&refresh_ticket)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} retirement was superseded before commit",
                family.display_name()
            )
        })?;
    if workspace_index
        .replace_external_sources_for_refresh(&refresh_ticket, &[])
        .is_none()
    {
        anyhow::bail!("{} retirement was superseded", family.display_name());
    }
    let classpath_retirement = project_classpaths
        .retire_prepared_project_classpath_for_external_refresh(
            &prepared_classpath_retirement,
            &refresh_ticket,
        )
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} project-classpath retirement was superseded",
                family.display_name()
            )
        })?;
    let retirement = source_providers
        .map(|providers| {
            providers
                .replace_group_for_refresh_prepared(
                    &refresh_ticket,
                    &mut prepared_provider_replacement,
                )
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{} source-provider retirement was superseded",
                        family.display_name()
                    )
                })
        })
        .transpose()?;
    if !workspace_index.retire_source_scopes_for_refresh(&refresh_ticket) {
        anyhow::bail!(
            "{} source-scope retirement was superseded",
            family.display_name()
        );
    }
    drop(publication_guard);
    drop(retirement);
    drop(classpath_retirement);
    if let Some(source_refresh) = source_refresh {
        source_refresh.disarm();
    }
    Ok(())
}

async fn refresh_gradle(
    root: &Path,
    host: &Arc<SemanticHost>,
    workspace_ready: &Arc<std::sync::atomic::AtomicBool>,
    source_providers: &Arc<crate::source_provider::SourceProviderChain>,
    bulk_source_index: crate::server::source_indexing::BulkSourceIndexContext,
    project_classpaths: &crate::project_classpath::ProjectClasspathStore,
    client: &Client,
) -> GradleRefreshResult {
    bulk_source_index
        .report_build_progress(
            if crate::gradle_integration::GradleIntegration::new(root.to_path_buf())
                .is_gradle_workspace()
            {
                "Loading Gradle build model..."
            } else {
                "Checking workspace directories for nested Gradle builds..."
            },
        )
        .await;
    match crate::gradle_integration::initialize_gradle_support_after_workspace_with_sources(
        root,
        Arc::clone(host),
        Arc::clone(workspace_ready),
        Arc::clone(source_providers),
        bulk_source_index,
        project_classpaths.clone(),
    )
    .await
    {
        Ok(outcome) => {
            if !outcome.refactor_ready {
                let () = client
                    .log_message(
                        MessageType::WARNING,
                        format!(
                            "Gradle build structure is not fully understood; destructive refactorings are disabled: {}",
                            outcome.structural_issues.join("; ")
                        ),
                    )
                    .await;
            }
            GradleRefreshResult {
                refactor_ready: outcome.refactor_ready,
                dependency_issues: outcome.dependency_issues,
                dependency_jars_indexed: outcome.dependency_jars_indexed,
            }
        }
        Err(error) => {
            debug!("Gradle refresh failed (continuing anyway): {error}");
            let () = client
                .log_message(
                    MessageType::WARNING,
                    format!("Gradle refresh failed: {error}"),
                )
                .await;
            GradleRefreshResult::default()
        }
    }
}

async fn refresh_maven(
    root: &Path,
    host: &Arc<SemanticHost>,
    source_providers: &Arc<crate::source_provider::SourceProviderChain>,
    bulk_source_index: crate::server::source_indexing::BulkSourceIndexContext,
    project_classpaths: &crate::project_classpath::ProjectClasspathStore,
    client: &Client,
) -> MavenRefreshOutcome {
    let integration = crate::maven_integration::MavenIntegration::new(root.to_path_buf());
    if !integration.is_maven_workspace() {
        bulk_source_index
            .report_build_progress("Checking workspace directories for nested Maven builds...")
            .await;
        return match retire_absent_maven(root, host, Some(source_providers), project_classpaths)
            .await
        {
            Ok(layout_supported) => {
                if !layout_supported {
                    let error = format!(
                        "workspace {} contains a nested or uninspectable Maven build, but no root reactor POM",
                        root.display()
                    );
                    debug!("Maven refresh failed (continuing anyway): {error}");
                    let () = client
                        .log_message(
                            MessageType::WARNING,
                            format!("Maven refresh failed: {error}"),
                        )
                        .await;
                }
                MavenRefreshOutcome::Retired { layout_supported }
            }
            Err(error) => {
                debug!("Maven refresh failed (continuing anyway): {error}");
                let () = client
                    .log_message(
                        MessageType::WARNING,
                        format!("Maven refresh failed: {error}"),
                    )
                    .await;
                MavenRefreshOutcome::Failed
            }
        };
    }

    bulk_source_index
        .report_build_progress("Resolving Maven reactor and dependencies...")
        .await;
    match crate::maven_integration::initialize_maven_support_with_sources(
        root,
        Arc::clone(host),
        Arc::clone(source_providers),
        bulk_source_index,
        project_classpaths.clone(),
    )
    .await
    {
        Ok(result) => MavenRefreshOutcome::Published(Box::new(result)),
        Err(error) => {
            debug!("Maven refresh failed (continuing anyway): {error}");
            let () = client
                .log_message(
                    MessageType::WARNING,
                    format!("Maven refresh failed: {error}"),
                )
                .await;
            MavenRefreshOutcome::Failed
        }
    }
}

async fn retire_absent_maven(
    root: &Path,
    host: &Arc<SemanticHost>,
    source_providers: Option<&Arc<crate::source_provider::SourceProviderChain>>,
    project_classpaths: &crate::project_classpath::ProjectClasspathStore,
) -> anyhow::Result<bool> {
    retire_build_system(
        BuildManifestFamily::Maven,
        root,
        host,
        source_providers,
        project_classpaths,
    )
    .await?;
    Ok(
        !crate::handlers::build_system::has_nested_build_manifest_or_unknown(
            root,
            BuildManifestFamily::Maven,
        )
        .await,
    )
}

fn maven_result_is_complete(result: Option<&crate::maven_integration::MavenInitResult>) -> bool {
    // Unresolvable libraries are per-dependency issues, not a reason to distrust
    // the reactor's own sources.
    result.is_some_and(|result| result.index_complete)
}

fn maven_dependency_diagnostics(
    result: Option<&crate::maven_integration::MavenInitResult>,
) -> Vec<Diagnostic> {
    let Some(result) = result else {
        return Vec::new();
    };
    result
        .resolution_report
        .missing_dependencies
        .iter()
        .map(|missing_dep| {
            crate::diagnostics::missing_dependency_diagnostic(
                &missing_dep.coordinates,
                &missing_dep.reason,
                0,
            )
        })
        .collect()
}

async fn publish_maven_dependency_diagnostics(
    root: &Path,
    result: Option<&crate::maven_integration::MavenInitResult>,
    client: &Client,
) {
    let diagnostics = maven_dependency_diagnostics(result);
    let count = diagnostics.len();
    if let Ok(pom_url) = Url::from_file_path(root.join("pom.xml")) {
        let () = client.publish_diagnostics(pom_url, diagnostics, None).await;
        if count > 0 {
            info!("Published {count} dependency diagnostics to pom.xml");
        }
    }
}

/// Tell the user exactly which dependencies are unavailable: every issue goes
/// to the log, and one `window/showMessage` names the first few. The popup is
/// suppressed when it would repeat the previous one for the same set.
async fn report_dependency_issues(client: &Client, issues: &[String]) {
    static LAST_SHOWN: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
    if issues.is_empty() {
        *LAST_SHOWN.lock().unwrap() = None;
        return;
    }
    for issue in issues.iter().take(200) {
        let () = client
            .log_message(MessageType::WARNING, format!("Dependency: {issue}"))
            .await;
    }
    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        issues.hash(&mut hasher);
        hasher.finish()
    };
    if LAST_SHOWN.lock().unwrap().replace(fingerprint) == Some(fingerprint) {
        return;
    }
    let shown: Vec<String> = issues
        .iter()
        .take(3)
        .map(|issue| {
            let mut chars = issue.chars();
            let bounded: String = chars.by_ref().take(140).collect();
            if chars.next().is_some() {
                format!("{bounded}...")
            } else {
                bounded
            }
        })
        .collect();
    let more = issues.len().saturating_sub(shown.len());
    let () = client
        .show_message(
            MessageType::WARNING,
            format!(
                "Hyperion: {} dependency problem(s); available libraries are indexed, the rest are unavailable. {}{}",
                issues.len(),
                shown.join(" | "),
                if more > 0 {
                    format!(" (+{more} more in the log)")
                } else {
                    String::new()
                }
            ) + if crate::build_tool_classpath::is_enabled() {
                ""
            } else {
                " Building the project once downloads them, or enable hyperion.buildToolClasspath to let Hyperion run Gradle/Maven itself (this executes the project's build)."
            },
        )
        .await;
}

/// Refresh the workspace's build model under one deterministic ownership
/// policy shared by startup and file-watcher refreshes.
///
/// Gradle wins when both root manifests exist. The Maven publication is
/// explicitly retired first, but readiness remains incomplete because silently
/// claiming that two conflicting root models were both understood would make
/// destructive refactorings unsafe.
pub(crate) async fn refresh_workspace_build_systems(
    root: &Path,
    host: &Arc<SemanticHost>,
    workspace_ready: &Arc<std::sync::atomic::AtomicBool>,
    source_providers: &Arc<crate::source_provider::SourceProviderChain>,
    bulk_source_index: crate::server::source_indexing::BulkSourceIndexContext,
    project_classpaths: &crate::project_classpath::ProjectClasspathStore,
    client: &Client,
) -> BuildSystemRefreshOutcome {
    let gradle_is_present =
        crate::gradle_integration::GradleIntegration::new(root.to_path_buf()).is_gradle_workspace();
    let maven_is_present =
        crate::maven_integration::MavenIntegration::new(root.to_path_buf()).is_maven_workspace();
    let selected = select_workspace_build_system(gradle_is_present, maven_is_present);
    let mixed_root_models = gradle_is_present && maven_is_present;

    let (gradle, maven_outcome) = match selected {
        WorkspaceBuildSystem::Gradle if mixed_root_models => {
            warn!(
                "Both Gradle and Maven root manifests are present at {}; using Gradle and retiring Maven state",
                root.display()
            );
            let () = client
                .log_message(
                    MessageType::WARNING,
                    "Both Gradle and Maven root manifests are present; Hyperion is using Gradle and disabling destructive refactorings until the ambiguity is removed".to_string(),
                )
                .await;
            let maven_outcome = match retire_build_system(
                BuildManifestFamily::Maven,
                root,
                host,
                Some(source_providers),
                project_classpaths,
            )
            .await
            {
                Ok(()) => MavenRefreshOutcome::Retired {
                    layout_supported: false,
                },
                Err(error) => {
                    debug!("Maven retirement failed: {error}");
                    let () = client
                        .log_message(
                            MessageType::WARNING,
                            format!("Maven retirement failed: {error}"),
                        )
                        .await;
                    MavenRefreshOutcome::Failed
                }
            };
            let gradle = refresh_gradle(
                root,
                host,
                workspace_ready,
                source_providers,
                bulk_source_index,
                project_classpaths,
                client,
            )
            .await;
            // Mixed root models are intentionally incomplete even when the
            // losing publication was retired successfully.
            (gradle, maven_outcome)
        }
        WorkspaceBuildSystem::Gradle => {
            let maven_outcome = refresh_maven(
                root,
                host,
                source_providers,
                bulk_source_index.clone(),
                project_classpaths,
                client,
            )
            .await;
            let gradle = refresh_gradle(
                root,
                host,
                workspace_ready,
                source_providers,
                bulk_source_index,
                project_classpaths,
                client,
            )
            .await;
            (gradle, maven_outcome)
        }
        WorkspaceBuildSystem::Maven | WorkspaceBuildSystem::None => {
            let gradle = refresh_gradle(
                root,
                host,
                workspace_ready,
                source_providers,
                bulk_source_index.clone(),
                project_classpaths,
                client,
            )
            .await;
            let maven_outcome = refresh_maven(
                root,
                host,
                source_providers,
                bulk_source_index,
                project_classpaths,
                client,
            )
            .await;
            (gradle, maven_outcome)
        }
    };

    let maven_complete = maven_outcome.is_complete();

    // A failed/superseded Maven attempt has no authority to clear diagnostics
    // produced by a newer or last-good model. Successful absence and explicit
    // mixed-model retirement do return an exact empty result/retirement proof.
    if maven_outcome.diagnostics_are_current() {
        publish_maven_dependency_diagnostics(root, maven_outcome.result(), client).await;
    }
    let mut dependency_issues = gradle.dependency_issues;
    let mut dependency_jars_indexed = gradle.dependency_jars_indexed;
    if let Some(result) = maven_outcome.result() {
        dependency_issues.extend(result.dependency_issues.iter().cloned());
        dependency_jars_indexed += result.jar_paths.len();
    }
    dependency_issues.sort();
    dependency_issues.dedup();
    report_dependency_issues(client, &dependency_issues).await;
    BuildSystemRefreshOutcome {
        selected,
        mixed_root_models,
        gradle_complete: gradle.refactor_ready,
        maven_complete,
        dependency_issues,
        dependency_jars_indexed,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        maven_dependency_diagnostics, retire_absent_maven, retire_build_system,
        select_workspace_build_system, MavenRefreshOutcome, WorkspaceBuildSystem,
    };
    use crate::handlers::build_system::BuildManifestFamily;
    use crate::semantic_host::SemanticHost;
    use hyperion_build::maven::{MissingDependency, ResolutionReport};
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;

    #[test]
    fn build_system_selection_is_total_and_gradle_first() {
        assert_eq!(
            select_workspace_build_system(false, false),
            WorkspaceBuildSystem::None
        );
        assert_eq!(
            select_workspace_build_system(true, false),
            WorkspaceBuildSystem::Gradle
        );
        assert_eq!(
            select_workspace_build_system(false, true),
            WorkspaceBuildSystem::Maven
        );
        assert_eq!(
            select_workspace_build_system(true, true),
            WorkspaceBuildSystem::Gradle
        );
    }

    #[tokio::test]
    async fn forced_retirement_allows_deterministic_build_owner_handoff() {
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path();
        let stale_jar = root.join("stale-maven.jar");
        let store = crate::project_classpath::ProjectClasspathStore::new();
        let index = WorkspaceIndex::new(root.to_path_buf());
        let maven = index.begin_external_source_refresh("build:maven");
        let maven_classpath = store
            .begin_project_classpath_refresh(root, &maven)
            .expect("Maven owns the old classpath");
        let maven_sources = index
            .replace_external_sources_for_refresh(&maven, std::slice::from_ref(&stale_jar))
            .expect("Maven workspace publication commits");
        let stale_file = maven_sources
            .revision_for(&stale_jar)
            .expect("Maven archive receives an exact revision")
            .file_id();
        assert!(
            crate::project_classpath::publish_maven_dependency_snapshot_for_refresh(
                &maven_classpath,
                vec![stale_jar],
            )
        );

        retire_build_system(
            BuildManifestFamily::Maven,
            root,
            &SemanticHost::new(Arc::new(index.clone())),
            None,
            &store,
        )
        .await
        .expect("Maven is retired even if a root POM still exists");

        assert_eq!(store.known_dependency_jars(root), None);
        assert_eq!(index.get_external_source_name(stale_file), None);
        let gradle = index.begin_external_source_refresh("build:gradle");
        assert!(store
            .begin_project_classpath_refresh(root, &gradle)
            .is_some());
    }

    #[tokio::test]
    async fn nested_layout_does_not_hide_exact_maven_retirement() {
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path();
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("pom.xml"), "<project/>").unwrap();

        let stale_jar = root.join("stale-maven.jar");
        let store = crate::project_classpath::ProjectClasspathStore::new();
        let index = WorkspaceIndex::new(root.to_path_buf());
        let maven = index.begin_external_source_refresh("build:maven");
        let maven_classpath = store
            .begin_project_classpath_refresh(root, &maven)
            .expect("Maven owns the old classpath");
        let sources = index
            .replace_external_sources_for_refresh(&maven, std::slice::from_ref(&stale_jar))
            .expect("old Maven publication commits");
        let stale_file = sources.revision_for(&stale_jar).unwrap().file_id();
        assert!(
            crate::project_classpath::publish_maven_dependency_snapshot_for_refresh(
                &maven_classpath,
                vec![stale_jar],
            )
        );

        let layout_supported = retire_absent_maven(
            root,
            &SemanticHost::new(Arc::new(index.clone())),
            None,
            &store,
        )
        .await
        .expect("absence retirement remains exact despite the nested layout");
        let outcome = MavenRefreshOutcome::Retired { layout_supported };

        assert!(!outcome.is_complete());
        assert!(outcome.diagnostics_are_current());
        assert_eq!(store.known_dependency_jars(root), None);
        assert_eq!(index.get_external_source_name(stale_file), None);
    }

    #[test]
    fn maven_diagnostics_are_replaced_with_the_latest_result() {
        let missing = crate::maven_integration::MavenInitResult {
            source_paths: Vec::new(),
            jar_paths: Vec::new(),
            jars_with_docs: Vec::new(),
            resolution_report: ResolutionReport {
                resolved_jars: Vec::new(),
                missing_dependencies: vec![MissingDependency {
                    coordinates: "example:missing:1".to_owned(),
                    expected_path: "missing.jar".into(),
                    reason: "not found".to_owned(),
                }],
                complete: false,
                main_jars: Vec::new(),
                issues: Vec::new(),
            },
            index_complete: true,
            dependency_issues: Vec::new(),
            modules: Vec::new(),
        };

        assert_eq!(maven_dependency_diagnostics(Some(&missing)).len(), 1);
        assert!(maven_dependency_diagnostics(None).is_empty());
    }
}
