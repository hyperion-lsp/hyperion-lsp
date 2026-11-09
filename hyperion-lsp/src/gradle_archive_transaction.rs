// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cancellation-safe publication of one resolved Gradle archive generation.
//!
//! Dependency resolution and archive parsing are deliberately outside the
//! owner gate. Only cross-registry commits take the gate, so a successor sees
//! either the old complete generation or the new complete generation.

use crate::project_classpath::ProjectClasspathRefreshGuard;
use crate::semantic_host::SemanticHost;
use crate::source_provider::{
    DependencyArchivePlan, SourceProvider, SourceProviderChain, SourceProviderRefreshGuard,
};
use hyperion_workspace::{
    ExternalSourceArchive, ExternalSourceIdentity, ExternalSourceRefreshTicket,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Publication capability for a dependency model proven complete by the
/// Gradle resolver. Incomplete models never construct this value, so the
/// destructive membership replacement below cannot accidentally interpret a
/// partial dependency set as authoritative.
pub(super) struct CompleteGradleArchiveTransaction {
    host: Arc<SemanticHost>,
    source_providers: Option<Arc<SourceProviderChain>>,
    source_refresh: Option<SourceProviderRefreshGuard>,
    classpath_refresh: ProjectClasspathRefreshGuard,
    refresh_ticket: ExternalSourceRefreshTicket,
    classpath_snapshot: (Vec<PathBuf>, Vec<PathBuf>),
    archive_plan: Vec<DependencyArchivePlan>,
    staged_provider: Option<Arc<dyn SourceProvider>>,
    external_archives: Vec<ExternalSourceArchive>,
    source_identities: Vec<ExternalSourceIdentity>,
}

pub(super) struct GradleArchiveTransactionResult {
    pub(super) jars_failed: usize,
    pub(super) jars_with_docs_failed: usize,
}

impl CompleteGradleArchiveTransaction {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        host: Arc<SemanticHost>,
        source_providers: Option<Arc<SourceProviderChain>>,
        refresh_ticket: ExternalSourceRefreshTicket,
        publication_attempt: super::CompleteGradlePublicationAttempt,
        archive_plan: Vec<DependencyArchivePlan>,
        staged_provider: Option<Arc<dyn SourceProvider>>,
        external_archives: Vec<ExternalSourceArchive>,
        source_identities: Vec<ExternalSourceIdentity>,
    ) -> Self {
        let super::CompleteGradlePublicationAttempt {
            source_refresh,
            classpath_refresh,
            classpath_snapshot,
        } = publication_attempt;
        Self {
            host,
            source_providers,
            source_refresh,
            classpath_refresh,
            refresh_ticket,
            classpath_snapshot,
            archive_plan,
            staged_provider,
            external_archives,
            source_identities,
        }
    }

    pub(super) fn publish(self) -> super::Result<GradleArchiveTransactionResult> {
        let Self {
            host,
            source_providers,
            mut source_refresh,
            classpath_refresh,
            refresh_ticket,
            classpath_snapshot,
            archive_plan,
            staged_provider,
            external_archives,
            source_identities,
        } = self;

        // Cancelling the async caller merely detaches this synchronous
        // transaction. Its guards still finish or roll back every registry.
        // Callback-heavy provider preparation happens off-gate. If a
        // predecessor progresses before this transaction gets the gate, the
        // helper discards the stale fallback snapshot and retries safely.
        // Each publication step is its own short write transaction, so
        // requests and editor notifications interleave between archives.
        let workspace_index = Arc::clone(host.index());
        let (provider_publication, external_refresh) =
            host.publish_blocking(|txn| -> super::Result<_> {
                let (publication_guard, provider_publication) =
                    crate::source_provider::begin_coordinated_external_publication(
                        txn.index(),
                        &refresh_ticket,
                        source_providers.as_ref(),
                        &source_identities,
                        staged_provider,
                    )
                    .map_err(|error| format!("Gradle archive publication failed: {error}"))?;
                let Some(external_refresh) = txn
                    .index()
                    .replace_external_archives_for_refresh(&refresh_ticket, &external_archives)
                else {
                    let retirement = source_providers.as_ref().and_then(|providers| {
                        provider_publication.as_ref().and_then(|publication| {
                            providers.abort_group_publication_deferred(publication)
                        })
                    });
                    drop(publication_guard);
                    drop(retirement);
                    return Err("Gradle refresh was superseded before archive extraction".into());
                };
                drop(publication_guard);
                Ok((provider_publication, external_refresh))
            })?;

        // Snapshot and extract one binary/Javadoc pair at a time. This bounds
        // temporary memory and keeps cancellation from stranding child tasks.
        let mut jars_indexed = 0;
        let mut jars_failed = 0;
        let mut jar_symbols_total = 0;
        let mut jars_with_docs_indexed = 0;
        let mut jars_with_docs_failed = 0;
        let mut javadoc_attachments_failed = 0;
        let mut docs_symbols_total = 0;
        for plan in archive_plan {
            let Some(revision) = external_refresh.revision_for_identity(plan.archive.identity())
            else {
                if plan.javadoc.is_some() {
                    jars_with_docs_failed += 1;
                } else {
                    jars_failed += 1;
                }
                warn!(
                    "Gradle archive identity changed before publication: {}",
                    plan.archive.path().display()
                );
                continue;
            };
            let requested_javadoc = plan.javadoc.is_some();
            let source_identity = plan.archive.identity().clone();
            let archive_path = plan.archive.path().to_path_buf();
            let javadoc_display = plan
                .javadoc
                .as_ref()
                .map(|javadoc| javadoc.path().to_path_buf());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::source_provider::prepare_dependency_archive(
                    &workspace_index,
                    &plan,
                    revision,
                )
            }));
            match result {
                Ok(Ok(prepared)) => {
                    let mut stats = host.publish_blocking(|txn| -> super::Result<_> {
                        let archive_publication = txn
                            .index()
                            .begin_committed_external_source_publication(&refresh_ticket)
                            .ok_or("Gradle archive publication was superseded")?;
                        let stats = txn.index().publish_prepared_jar(prepared)?;
                        if let (Some(providers), Some(publication)) =
                            (&source_providers, &provider_publication)
                        {
                            if !providers.record_group_publication_success_for_revision(
                                publication,
                                &source_identity,
                                revision,
                            ) {
                                return Err(
                                    "Gradle source-provider publication was superseded".into()
                                );
                            }
                        }
                        drop(archive_publication);
                        Ok(stats)
                    })?;
                    let symbols =
                        stats.classes_indexed + stats.methods_indexed + stats.fields_indexed;

                    if requested_javadoc
                        && !stats.documentation_loaded
                        && stats.documentation_error.is_none()
                    {
                        stats.documentation_error =
                            Some("Javadoc attachment was not loaded".to_string());
                    }
                    if stats.documentation_loaded {
                        jars_with_docs_indexed += 1;
                        docs_symbols_total += symbols;
                        debug!(
                            "Indexed JAR {} with javadoc {}: {} classes, {} methods, {} fields",
                            archive_path.display(),
                            javadoc_display
                                .as_deref()
                                .unwrap_or(&archive_path)
                                .display(),
                            stats.classes_indexed,
                            stats.methods_indexed,
                            stats.fields_indexed
                        );
                    } else {
                        jars_indexed += 1;
                        jar_symbols_total += symbols;
                        if requested_javadoc {
                            javadoc_attachments_failed += 1;
                            if let Some(error) = &stats.documentation_error {
                                warn!(
                                    "Javadoc attachment for {} was unavailable: {error}",
                                    archive_path.display()
                                );
                            }
                        }
                        debug!(
                            "Indexed JAR {}: {} classes, {} methods, {} fields",
                            archive_path.display(),
                            stats.classes_indexed,
                            stats.methods_indexed,
                            stats.fields_indexed
                        );
                    }
                }
                Ok(Err(error)) => {
                    if requested_javadoc {
                        jars_with_docs_failed += 1;
                    } else {
                        jars_failed += 1;
                    }
                    warn!("Failed to index JAR {:?}: {}", archive_path, error);
                }
                Err(_) => {
                    if requested_javadoc {
                        jars_with_docs_failed += 1;
                    } else {
                        jars_failed += 1;
                    }
                    warn!("Gradle archive extraction panicked for {:?}", archive_path);
                }
            }
        }

        log_archive_totals(
            jars_indexed,
            jars_failed,
            jar_symbols_total,
            jars_with_docs_indexed,
            jars_with_docs_failed,
            docs_symbols_total,
            javadoc_attachments_failed,
        );

        // Metadata inspection may block on the filesystem. Keep it outside
        // the owner gate; ProjectClasspathStore repeats the fence on every
        // lookup, closing the race between this check and the in-memory
        // cross-registry commit below.
        let archives_current = external_archives
            .iter()
            .all(ExternalSourceArchive::staged_version_is_current);
        if !archives_current {
            warn!("Gradle dependency archives changed before classpath publication");
        }
        let archives_complete = jars_failed == 0 && jars_with_docs_failed == 0 && archives_current;
        let prepared_classpath = if archives_complete {
            super::prepare_gradle_classpath_snapshot(
                Some(classpath_snapshot),
                classpath_refresh.ticket(),
                &external_archives,
            )?
        } else {
            None
        };

        let classpath_publication = host.publish_blocking(|txn| -> super::Result<_> {
            let mut retirement = None;
            let publication_guard = txn
                .index()
                .begin_committed_external_source_publication(&refresh_ticket)
                .ok_or("Gradle publication was superseded before completion")?;
            if let (Some(providers), Some(publication)) = (&source_providers, &provider_publication)
            {
                retirement = Some(
                    providers
                        .finish_group_publication_deferred(publication)
                        .ok_or("Gradle source-provider publication was superseded")?,
                );
            }
            let (classpath_publication, classpath_retirement) = archives_complete
                .then(|| {
                    super::publish_prepared_gradle_classpath_snapshot_deferred(
                        prepared_classpath,
                        classpath_refresh.ticket(),
                    )
                })
                .transpose()?
                .map_or((None, None), |(publication, retirement)| {
                    (Some(publication), retirement)
                });
            if matches!(
                classpath_publication,
                Some(super::GradleClasspathPublication::Published)
            ) && !txn.index().publish_source_scopes_for_refresh(
                &refresh_ticket,
                classpath_refresh.ticket().published_source_scopes(),
            ) {
                return Err("Gradle source scopes were superseded before publication".into());
            }
            drop(publication_guard);
            drop(retirement);
            drop(classpath_retirement);
            Ok(classpath_publication)
        })?;
        if let Some(source_refresh) = source_refresh.take() {
            source_refresh.disarm();
        }
        if matches!(
            classpath_publication,
            Some(super::GradleClasspathPublication::Published)
        ) {
            classpath_refresh.disarm();
        }
        Ok(GradleArchiveTransactionResult {
            jars_failed,
            jars_with_docs_failed,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn log_archive_totals(
    jars_indexed: usize,
    jars_failed: usize,
    jar_symbols_total: usize,
    jars_with_docs_indexed: usize,
    jars_with_docs_failed: usize,
    docs_symbols_total: usize,
    javadoc_attachments_failed: usize,
) {
    if jars_indexed > 0 || jars_failed > 0 {
        if jars_failed > 0 {
            warn!(
                "Indexed {} dependency JARs with {} total symbols; {} JARs failed to index",
                jars_indexed, jar_symbols_total, jars_failed
            );
        } else {
            info!(
                "Indexed {} dependency JARs with {} total symbols",
                jars_indexed, jar_symbols_total
            );
        }
    }
    if jars_with_docs_indexed > 0 || jars_with_docs_failed > 0 {
        if jars_with_docs_failed > 0 {
            warn!(
                "Indexed {} dependency JARs with javadoc, {} total symbols; {} JARs failed to index",
                jars_with_docs_indexed, docs_symbols_total, jars_with_docs_failed
            );
        } else {
            info!(
                "Indexed {} dependency JARs with javadoc, {} total symbols",
                jars_with_docs_indexed, docs_symbols_total
            );
        }
    }
    if javadoc_attachments_failed > 0 {
        warn!(
            "{} Gradle Javadoc attachments were unavailable; binary symbols remain indexed",
            javadoc_attachments_failed
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::Command;

    fn compile_dependency_jar(root: &std::path::Path) -> PathBuf {
        let source_dir = root.join("src/dep");
        let classes_dir = root.join("classes");
        std::fs::create_dir_all(&source_dir).expect("create Java source directory");
        std::fs::create_dir_all(&classes_dir).expect("create Java classes directory");
        let source_path = source_dir.join("Type.java");
        let source = b"package dep; public class Type {}";
        std::fs::write(&source_path, source).expect("write Java source fixture");
        let output = Command::new("javac")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&source_path)
            .output()
            .expect("run javac for dependency fixture");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let jar_path = root.join("dependency.jar");
        let mut jar = zip::ZipWriter::new(
            std::fs::File::create(&jar_path).expect("create binary dependency JAR"),
        );
        jar.start_file("dep/Type.class", zip::write::SimpleFileOptions::default())
            .expect("start binary class entry");
        jar.write_all(
            &std::fs::read(classes_dir.join("dep/Type.class")).expect("read compiled class"),
        )
        .expect("write binary class entry");
        jar.finish().expect("finish binary dependency JAR");

        let mut sources = zip::ZipWriter::new(
            std::fs::File::create(root.join("dependency-sources.jar"))
                .expect("create source dependency JAR"),
        );
        sources
            .start_file("dep/Type.java", zip::write::SimpleFileOptions::default())
            .expect("start source entry");
        sources.write_all(source).expect("write source entry");
        sources.finish().expect("finish source dependency JAR");
        jar_path
    }

    fn transaction_for(
        root: &std::path::Path,
        jar: PathBuf,
        source_providers: Option<Arc<SourceProviderChain>>,
    ) -> (
        CompleteGradleArchiveTransaction,
        Arc<hyperion_workspace::WorkspaceIndex>,
        crate::project_classpath::ProjectClasspathStore,
        ExternalSourceIdentity,
    ) {
        let workspace_index = Arc::new(hyperion_workspace::WorkspaceIndex::new(root.to_path_buf()));
        let project_classpaths = crate::project_classpath::ProjectClasspathStore::new();
        let refresh_ticket = workspace_index.begin_external_source_refresh("build:gradle");
        let source_refresh = source_providers.as_ref().map(|providers| {
            providers
                .observe_group_refresh_guard(&refresh_ticket)
                .expect("begin Gradle provider refresh")
        });
        let classpath_refresh = project_classpaths
            .begin_project_classpath_refresh_guard(root, &refresh_ticket)
            .expect("begin Gradle classpath refresh");
        let archive_plan = crate::source_provider::capture_dependency_archive_plan(
            std::slice::from_ref(&jar),
            &[],
        )
        .expect("stage dependency archive");
        let external_archives: Vec<_> = archive_plan
            .iter()
            .map(|plan| plan.archive.clone())
            .collect();
        let source_identities: Vec<_> = external_archives
            .iter()
            .map(|archive| archive.identity().clone())
            .collect();
        let source_identity = source_identities
            .first()
            .expect("fixture has one dependency archive")
            .clone();
        let staged_provider = source_providers.as_ref().and_then(|providers| {
            crate::source_provider::JarSourceProvider::from_binary_archives_with_store(
                &external_archives,
                providers.materialized_sources(),
            )
            .map(|provider| Arc::new(provider) as Arc<dyn SourceProvider>)
        });
        let publication_attempt = super::super::complete_gradle_publication_attempt(
            Some((vec![jar.clone()], vec![jar.clone()])),
            source_refresh,
            classpath_refresh,
        )
        .expect("classify complete Gradle fixture")
        .expect("complete fixture receives publication capability");
        (
            CompleteGradleArchiveTransaction::new(
                SemanticHost::new(Arc::clone(&workspace_index)),
                source_providers,
                refresh_ticket,
                publication_attempt,
                archive_plan,
                staged_provider,
                external_archives,
                source_identities,
            ),
            workspace_index,
            project_classpaths,
            source_identity,
        )
    }

    #[test]
    fn corrupt_mandatory_archive_commits_no_execution_classpath() {
        let temp = tempfile::tempdir().expect("temporary corrupt Gradle archive workspace");
        let jar = temp.path().join("corrupt.jar");
        std::fs::write(&jar, b"not a zip archive").expect("write corrupt archive");
        let (transaction, _index, project_classpaths, _identity) =
            transaction_for(temp.path(), jar, None);

        let result = transaction
            .publish()
            .expect("archive-local corruption is reported, not a transaction panic");

        assert_eq!(result.jars_failed, 1);
        assert_eq!(project_classpaths.known_dependency_jars(temp.path()), None);
    }

    #[test]
    fn incomplete_gradle_refresh_preserves_exact_last_good_generation() {
        let temp = tempfile::tempdir().expect("temporary Gradle workspace");
        let jar = compile_dependency_jar(temp.path());
        let providers = Arc::new(SourceProviderChain::new());
        let (transaction, index, project_classpaths, identity) =
            transaction_for(temp.path(), jar.clone(), Some(Arc::clone(&providers)));
        transaction.publish().expect("publish complete R1");
        let expected_jar = identity
            .archive_path()
            .expect("dependency identity is an archive")
            .to_path_buf();

        let r1_location = index.find_symbol("dep.Type").expect("R1 symbol");
        let r1_revision = index
            .external_source_revision(&r1_location)
            .expect("R1 external revision");
        assert!(providers
            .get_source_exact_for_revision(&identity, r1_revision, "dep.Type")
            .is_some());
        assert_eq!(
            project_classpaths.known_dependency_jars(temp.path()),
            Some(vec![expected_jar.clone()])
        );

        let r2 = index.begin_external_source_refresh("build:gradle");
        let source_refresh = Some(
            providers
                .observe_group_refresh_guard(&r2)
                .expect("observe incomplete R2"),
        );
        let classpath_refresh = project_classpaths
            .begin_project_classpath_refresh_guard(temp.path(), &r2)
            .expect("stage incomplete R2 classpath");
        assert!(super::super::complete_gradle_publication_attempt(
            None,
            source_refresh,
            classpath_refresh,
        )
        .expect("discard incomplete R2")
        .is_none());

        let retained = index.find_symbol("dep.Type").expect("retained R1 symbol");
        assert_eq!(index.external_source_revision(&retained), Some(r1_revision));
        assert!(providers
            .get_source_exact_for_revision(&identity, r1_revision, "dep.Type")
            .is_some());
        assert_eq!(
            project_classpaths.known_dependency_jars(temp.path()),
            Some(vec![expected_jar])
        );
    }

    #[test]
    fn first_failed_classpath_construction_publishes_no_dependency_generation() {
        let temp = tempfile::tempdir().expect("temporary Gradle workspace");
        let index = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        let providers = Arc::new(SourceProviderChain::new());
        let project_classpaths = crate::project_classpath::ProjectClasspathStore::new();
        let r1 = index.begin_external_source_refresh("build:gradle");
        let source_refresh = Some(
            providers
                .observe_group_refresh_guard(&r1)
                .expect("observe incomplete R1"),
        );
        let classpath_refresh = project_classpaths
            .begin_project_classpath_refresh_guard(temp.path(), &r1)
            .expect("stage incomplete R1 classpath");

        assert!(super::super::complete_gradle_publication_attempt(
            None,
            source_refresh,
            classpath_refresh,
        )
        .expect("discard incomplete R1")
        .is_none());
        assert!(index.get_external_symbols().is_empty());
        assert_eq!(project_classpaths.known_dependency_jars(temp.path()), None);
        assert!(!providers.can_provide_exact(
            &ExternalSourceIdentity::for_archive_path(&temp.path().join("missing.jar")),
            "dep.Type",
        ));
    }
}
