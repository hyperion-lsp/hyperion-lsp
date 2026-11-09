// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded, exact archive planning for one dependency refresh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hyperion_workspace::{ExternalSourceArchive, ExternalSourceIdentity};
use tracing::warn;

pub(crate) const MAX_BINARY_ARCHIVE_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_JAVADOC_ARCHIVE_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPENDENCY_ARCHIVE_CANDIDATES: usize = 8_192;
const MAX_DEPENDENCY_BINARY_ARCHIVES: usize = 4_096;
const MAX_TOTAL_DEPENDENCY_BINARY_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL_DEPENDENCY_JAVADOC_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct DependencyArchivePlan {
    pub(crate) archive: ExternalSourceArchive,
    pub(crate) javadoc: Option<ExternalSourceArchive>,
    javadoc_identity: Option<ExternalSourceIdentity>,
    pub(super) javadoc_ambiguous: bool,
}

/// Capture resolver path identities, then deduplicate filesystem aliases while
/// preserving classpath order. Conflicting documentation for one binary is
/// rejected instead of being selected by incidental resolver order.
pub(crate) fn capture_dependency_archive_plan(
    jar_paths: &[PathBuf],
    jars_with_docs: &[(PathBuf, PathBuf)],
) -> Result<Vec<DependencyArchivePlan>, String> {
    let candidate_count = jar_paths
        .len()
        .checked_add(jars_with_docs.len())
        .ok_or_else(|| "dependency archive candidate count overflowed usize".to_string())?;
    if candidate_count > MAX_DEPENDENCY_ARCHIVE_CANDIDATES {
        return Err(format!(
            "dependency resolution returned {candidate_count} archive entries (maximum {MAX_DEPENDENCY_ARCHIVE_CANDIDATES})"
        ));
    }
    let mut candidates = Vec::with_capacity(candidate_count);
    candidates.extend(jar_paths.iter().cloned().map(|path| (path, None)));
    candidates.extend(
        jars_with_docs
            .iter()
            .cloned()
            .map(|(path, javadoc)| (path, Some(javadoc))),
    );

    let mut plans: Vec<DependencyArchivePlan> = Vec::with_capacity(candidates.len());
    let mut plan_by_identity: HashMap<ExternalSourceIdentity, usize> =
        HashMap::with_capacity(candidates.len());
    let mut binary_bytes = 0_u64;
    let mut javadoc_bytes = 0_u64;
    for (path, javadoc) in candidates {
        let archive = ExternalSourceArchive::capture(&path);
        if let Some(plan_index) = plan_by_identity.get(archive.identity()).copied() {
            let plan = &mut plans[plan_index];
            let Some(javadoc_path) = javadoc else {
                continue;
            };
            if plan.javadoc_ambiguous {
                continue;
            }
            let candidate = ExternalSourceArchive::capture(&javadoc_path);
            match &plan.javadoc_identity {
                None => {
                    plan.javadoc_identity = Some(candidate.identity().clone());
                    plan.javadoc = stage_optional_javadoc(
                        candidate,
                        &javadoc_path,
                        plan.archive.path(),
                        &mut javadoc_bytes,
                    );
                }
                Some(current_identity) if current_identity != candidate.identity() => {
                    let current = plan
                        .javadoc
                        .as_ref()
                        .map_or("an unreadable attachment".into(), |current| {
                            current.path().display().to_string()
                        });
                    warn!(
                        "Ambiguous Javadoc attachments for {}: {} and {}; indexing binary symbols without documentation",
                        plan.archive.path().display(),
                        current,
                        candidate.path().display()
                    );
                    if let Some(javadoc) = plan.javadoc.take() {
                        let bytes = javadoc
                            .expected_file_version()
                            .expect("a staged Javadoc archive has a file version")
                            .bytes();
                        javadoc_bytes = javadoc_bytes
                            .checked_sub(bytes)
                            .expect("retained Javadoc bytes must include the discarded attachment");
                    }
                    plan.javadoc_identity = None;
                    plan.javadoc_ambiguous = true;
                }
                Some(_) => {}
            }
            continue;
        }

        if plans.len() == MAX_DEPENDENCY_BINARY_ARCHIVES {
            return Err(format!(
                "dependency resolution contains more than {MAX_DEPENDENCY_BINARY_ARCHIVES} unique binary archives"
            ));
        }
        let archive = archive
            .into_staged(MAX_BINARY_ARCHIVE_SNAPSHOT_BYTES)
            .map_err(|error| {
                format!(
                    "failed to stage dependency archive {}: {error}",
                    path.display()
                )
            })?;
        let bytes = archive
            .expected_file_version()
            .expect("a staged dependency archive has a file version")
            .bytes();
        binary_bytes = binary_bytes
            .checked_add(bytes)
            .ok_or_else(|| "dependency binary byte total overflowed u64".to_string())?;
        if binary_bytes > MAX_TOTAL_DEPENDENCY_BINARY_BYTES {
            return Err(format!(
                "dependency binary archives total {binary_bytes} bytes (maximum {MAX_TOTAL_DEPENDENCY_BINARY_BYTES})"
            ));
        }
        let (javadoc_identity, javadoc) = javadoc.map_or((None, None), |javadoc_path| {
            let javadoc = ExternalSourceArchive::capture(&javadoc_path);
            let identity = Some(javadoc.identity().clone());
            let staged =
                stage_optional_javadoc(javadoc, &javadoc_path, archive.path(), &mut javadoc_bytes);
            (identity, staged)
        });
        plan_by_identity.insert(archive.identity().clone(), plans.len());
        plans.push(DependencyArchivePlan {
            archive,
            javadoc,
            javadoc_identity,
            javadoc_ambiguous: false,
        });
    }

    Ok(plans)
}

fn stage_optional_javadoc(
    archive: ExternalSourceArchive,
    attachment_path: &Path,
    binary_path: &Path,
    total_bytes: &mut u64,
) -> Option<ExternalSourceArchive> {
    let archive = match archive.into_staged(MAX_JAVADOC_ARCHIVE_SNAPSHOT_BYTES) {
        Ok(archive) => archive,
        Err(error) => {
            warn!(
                "Ignoring unreadable Javadoc attachment {} for {}: {error}",
                attachment_path.display(),
                binary_path.display()
            );
            return None;
        }
    };
    let bytes = archive
        .expected_file_version()
        .expect("a staged Javadoc archive has a file version")
        .bytes();
    let Some(next) = total_bytes.checked_add(bytes) else {
        warn!(
            "Ignoring Javadoc attachment {} because the aggregate byte count overflowed",
            attachment_path.display()
        );
        return None;
    };
    if next > MAX_TOTAL_DEPENDENCY_JAVADOC_BYTES {
        warn!(
            "Ignoring Javadoc attachment {} because dependency documentation exceeds {MAX_TOTAL_DEPENDENCY_JAVADOC_BYTES} bytes",
            attachment_path.display()
        );
        return None;
    }
    *total_bytes = next;
    Some(archive)
}

/// Snapshot and parse one dependency archive without mutating WorkspaceIndex.
/// Callers may panic-contain this phase, then publish the returned value in a
/// short owner-gated commit section.
pub(crate) fn prepare_dependency_archive(
    workspace_index: &hyperion_workspace::WorkspaceIndex,
    plan: &DependencyArchivePlan,
    revision: hyperion_workspace::ExternalSourceRevisionToken,
) -> anyhow::Result<hyperion_workspace::PreparedExternalArchivePublication> {
    let binary = plan
        .archive
        .snapshot(MAX_BINARY_ARCHIVE_SNAPSHOT_BYTES)
        .map_err(anyhow::Error::msg)?;
    match &plan.javadoc {
        Some(javadoc_archive) => {
            match javadoc_archive.snapshot_contents(MAX_JAVADOC_ARCHIVE_SNAPSHOT_BYTES) {
                Ok(javadoc) => Ok(workspace_index
                    .prepare_jar_snapshots_with_docs_for_revision(binary, javadoc, revision)?),
                Err(error) => {
                    let mut prepared =
                        workspace_index.prepare_jar_snapshot_for_revision(binary, revision)?;
                    prepared.set_documentation_error(error);
                    Ok(prepared)
                }
            }
        }
        None => Ok(workspace_index.prepare_jar_snapshot_for_revision(binary, revision)?),
    }
}
