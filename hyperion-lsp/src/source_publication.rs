// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Publication of one source revision to the request-facing semantic mirrors.
//!
//! The workspace index owns symbols, usages, imports, and indexed source text.
//! Salsa, the Java method-signature cache, and completion's inferred-variable
//! cache mirror that revision for interactive requests. Keeping their updates
//! here prevents individual editor, watcher, rename, and bulk-index paths from
//! silently omitting one of those mirrors.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyperion_parser::{Lexer, Parser, SyntaxNode};
use hyperion_semantics::db::{Database, FileId, SourceDatabase};
use hyperion_semantics::nullability::{
    extract_declared_type_names, extract_field_signatures, extract_method_signatures,
    extract_type_hierarchy_facts, FieldSignature, MethodSignature, TypeHierarchyFact,
    WorkspaceFactRevision, WorkspaceMemberFactRevision,
};
use hyperion_semantics::Type;
use hyperion_workspace::{Language, ResolvedFileSupertypes, WorkspaceIndex};
use lru::LruCache;
use parking_lot::Mutex;
use tracing::{debug, warn};

use crate::semantic_host::{SemanticHost, WriteTxn};

pub(crate) type VarTypeCache = Mutex<LruCache<(FileId, u32), Type>>;

#[derive(Clone)]
struct ExtractedMemberFacts {
    signatures: Vec<(String, MethodSignature)>,
    fields: Vec<(String, FieldSignature)>,
    declared_types: Vec<String>,
    type_hierarchies: Vec<TypeHierarchyFact>,
    resolved_hierarchy: Option<ResolvedFileSupertypes>,
}

struct PreparedHydration {
    file_id: FileId,
    file_revision: hyperion_workspace::FileRevisionToken,
    indexed: Option<hyperion_workspace::IndexedContent>,
    member_facts: PreparedMemberFacts,
}

enum PreparedMemberFacts {
    Current(ResolvedFileSupertypes),
    Install(ExtractedMemberFacts),
    Unavailable,
}

impl PreparedMemberFacts {
    const fn resolved_hierarchy(&self) -> Option<&ResolvedFileSupertypes> {
        match self {
            Self::Current(resolved) => Some(resolved),
            Self::Install(facts) => facts.resolved_hierarchy.as_ref(),
            Self::Unavailable => None,
        }
    }
}

/// Publish one live revision and retire identities displaced by the matching
/// workspace-index transaction. Returns `false` without mutating semantic state
/// when `text` is no longer the exact indexed revision for `file_id`.
///
/// The write transaction is exclusive, so nothing can move the workspace index
/// between the checks below and the Salsa install; a failed check therefore
/// means the caller published different bytes, not that a retry could succeed.
pub(crate) fn publish_semantic_revision(
    txn: &mut WriteTxn,
    var_type_cache: &VarTypeCache,
    file_id: FileId,
    text: Arc<String>,
    language: Language,
    retired_file_ids: &[FileId],
) -> bool {
    debug_assert!(!retired_file_ids.contains(&file_id));

    let workspace_index = Arc::clone(txn.index());
    let revision_facts = extract_member_facts(&text, file_id, language, Some(&workspace_index));
    let indexed = workspace_index
        .current_file_revision(file_id)
        .and_then(|revision| workspace_index.get_indexed_revision_content(revision))
        .filter(|indexed| indexed.content.as_str() == text.as_str());
    let absent_retired_file_ids = retired_file_ids
        .iter()
        .copied()
        .filter(|retired_file_id| {
            workspace_index
                .current_file_revision(*retired_file_id)
                .is_none()
        })
        .collect::<Vec<_>>();
    let mut db = txn.db();
    let publication = match revision_facts {
        Some(facts) => {
            let resolved = facts
                .resolved_hierarchy
                .expect("workspace-backed Java facts carry exact hierarchy provenance");
            let revision = member_fact_revision(&resolved);
            workspace_index.with_current_resolved_file_supertypes_and_absent_files(
                &resolved,
                &absent_retired_file_ids,
                || {
                    db.set_file_text_tracked(file_id, Arc::clone(&text));
                    db.set_file_language(file_id, language);
                    let mut changed = db
                        .method_signature_cache()
                        .update_workspace_file_member_facts(
                            file_id,
                            revision,
                            facts.signatures,
                            facts.fields,
                            facts.declared_types,
                            facts.type_hierarchies,
                        );
                    changed |= retire_absent_files(&mut db, &absent_retired_file_ids, language);
                    if changed {
                        db.bump_method_signature_version();
                    }
                },
            )
        }
        None => indexed.as_ref().and_then(|indexed| {
            workspace_index.with_current_indexed_content_and_absent_files(
                indexed,
                &absent_retired_file_ids,
                || {
                    db.set_file_text_tracked(file_id, Arc::clone(&text));
                    db.set_file_language(file_id, language);
                    let mut changed = db.method_signature_cache().remove_file_signatures(file_id);
                    changed |= retire_absent_files(&mut db, &absent_retired_file_ids, language);
                    if changed {
                        db.bump_method_signature_version();
                    }
                },
            )
        }),
    };
    drop(db);
    if publication.is_none() {
        return false;
    }

    invalidate_var_type_cache(var_type_cache, file_id);
    for retired_file_id in absent_retired_file_ids {
        invalidate_var_type_cache(var_type_cache, retired_file_id);
    }
    true
}

/// Blank the semantic mirrors of identities the index no longer owns. Returns
/// whether any member-fact signature was removed.
fn retire_absent_files(db: &mut Database, file_ids: &[FileId], language: Language) -> bool {
    let empty_text = Arc::new(String::new());
    let mut changed = false;
    for file_id in file_ids {
        changed |= db.method_signature_cache().remove_file_signatures(*file_id);
        db.set_file_text_tracked(*file_id, Arc::clone(&empty_text));
        db.set_file_language(*file_id, language);
    }
    changed
}

/// Retire semantic state after the workspace index removes a source revision.
pub(crate) fn retire_semantic_revisions(
    txn: &mut WriteTxn,
    var_type_cache: &VarTypeCache,
    file_ids: &[FileId],
    language: Language,
) {
    if file_ids.is_empty() {
        return;
    }

    retire_semantic_revisions_locked(&mut txn.db(), file_ids, language);

    for file_id in file_ids {
        invalidate_var_type_cache(var_type_cache, *file_id);
    }
}

/// Retire only candidate identities that remain absent from the workspace.
///
/// A candidate the index re-adopted since the caller removed it is left alone;
/// exclusivity of the write transaction means that set cannot change while
/// this runs.
pub(crate) fn retire_absent_semantic_revisions(
    txn: &mut WriteTxn,
    var_type_cache: &VarTypeCache,
    candidate_file_ids: &[FileId],
    language: Language,
) {
    let absent_file_ids: Vec<_> = candidate_file_ids
        .iter()
        .copied()
        .filter(|file_id| txn.index().current_file_revision(*file_id).is_none())
        .collect();
    if absent_file_ids.is_empty() {
        return;
    }
    retire_semantic_revisions_locked(&mut txn.db(), &absent_file_ids, language);
    for file_id in absent_file_ids {
        invalidate_var_type_cache(var_type_cache, file_id);
    }
}

fn retire_semantic_revisions_locked(db: &mut Database, file_ids: &[FileId], language: Language) {
    if retire_absent_files(db, file_ids, language) {
        db.bump_method_signature_version();
    }
}

/// Synchronize Java signatures for the current registered text, or remove a
/// prior Java revision when this FileId now belongs to another language.
#[cfg(test)]
pub(crate) fn sync_method_signatures_for_revision(
    txn: &mut WriteTxn,
    file_id: FileId,
    language: Language,
) -> bool {
    let mut db = txn.db();
    let changed = if language == Language::Java {
        let text = db.file_text(file_id);
        let facts = extract_member_facts(&text, file_id, language, None)
            .expect("Java source must produce member facts");
        db.method_signature_cache().update_file_member_facts(
            file_id,
            facts.signatures,
            facts.fields,
            facts.declared_types,
            facts.type_hierarchies,
        )
    } else {
        db.method_signature_cache().remove_file_signatures(file_id)
    };
    if changed {
        db.bump_method_signature_version();
    }
    changed
}

fn extract_member_facts(
    text: &str,
    file_id: FileId,
    language: Language,
    workspace_index: Option<&WorkspaceIndex>,
) -> Option<ExtractedMemberFacts> {
    if language != Language::Java {
        return None;
    }
    let resolved_hierarchy = match workspace_index {
        Some(workspace_index) => Some(workspace_index.file_supertypes_for_revision(file_id, text)?),
        None => None,
    };
    extract_member_facts_with_hierarchy(text, file_id, workspace_index, resolved_hierarchy)
}

fn extract_member_facts_with_hierarchy(
    text: &str,
    file_id: FileId,
    workspace_index: Option<&WorkspaceIndex>,
    resolved_hierarchy: Option<ResolvedFileSupertypes>,
) -> Option<ExtractedMemberFacts> {
    let (syntax, _) = member_fact_syntax(text, file_id, workspace_index);
    let declared_types = extract_declared_type_names(&syntax);
    let type_hierarchies = match &resolved_hierarchy {
        Some(resolved) => resolved_type_hierarchy_facts(resolved, &declared_types)?,
        None => extract_type_hierarchy_facts(&syntax),
    };
    Some(ExtractedMemberFacts {
        signatures: extract_method_signatures(&syntax, file_id),
        fields: extract_field_signatures(&syntax, file_id),
        declared_types,
        type_hierarchies,
        resolved_hierarchy,
    })
}

fn member_fact_syntax(
    text: &str,
    file_id: FileId,
    workspace_index: Option<&WorkspaceIndex>,
) -> (SyntaxNode, bool) {
    if let Some(green) =
        workspace_index.and_then(|workspace_index| workspace_index.cached_java_parse(file_id, text))
    {
        return (SyntaxNode::new_root(green.as_ref().clone()), true);
    }
    let tokens = Lexer::new(text).collect();
    let (green, _errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green), false)
}

fn resolved_type_hierarchy_facts(
    resolved: &ResolvedFileSupertypes,
    declared_types: &[String],
) -> Option<Vec<TypeHierarchyFact>> {
    if resolved.types.len() != declared_types.len() {
        return None;
    }
    let mut by_owner = resolved
        .types
        .iter()
        .map(|resolved| (resolved.type_fqn.as_str(), resolved))
        .collect::<std::collections::HashMap<_, _>>();
    if by_owner.len() != declared_types.len() {
        return None;
    }
    declared_types
        .iter()
        .map(|owner| {
            let resolved = by_owner.remove(owner.as_str())?;
            Some(TypeHierarchyFact::from_resolved_supertype_roles(
                owner.clone(),
                resolved.superclass.clone(),
                resolved.interfaces.clone(),
            ))
        })
        .collect()
}

const fn member_fact_revision(resolved: &ResolvedFileSupertypes) -> WorkspaceMemberFactRevision {
    WorkspaceMemberFactRevision::new(
        resolved.file_revision,
        resolved.workspace_revision,
        resolved.fingerprint,
    )
}

pub(crate) fn invalidate_var_type_cache(cache: &VarTypeCache, file_id: FileId) {
    let mut cache = cache.lock();
    let keys_to_remove: Vec<_> = cache
        .iter()
        .filter(|((cached_file_id, _), _)| *cached_file_id == file_id)
        .map(|(key, _)| *key)
        .collect();
    for key in &keys_to_remove {
        cache.pop(key);
    }
    debug!(
        "Invalidated var type cache for file {:?}, removed {} entries",
        file_id,
        keys_to_remove.len()
    );
}

/// Lazily publish contracts for cold workspace files referenced by `file_id`.
///
/// Workspace indexing already resolves each usage to a declaration FQN. This
/// uses those edges to parse only files needed by the current diagnostic
/// request, avoiding another whole-workspace parse pass at startup.
///
/// Two steps, so the expensive one never blocks a writer: the dependency scan,
/// parsing and overload resolution run on a blocking thread *outside* the gate
/// ([`prepare_hydration`]); only the install takes the [`WriteTxn`] and
/// revalidates the prepared facts against the exact index revisions under it
/// ([`install_hydration`]). A stale preparation is retried, a bounded number of
/// times. The caller must not hold a transaction.
pub(crate) async fn hydrate_dependency_method_signatures(
    host: &Arc<SemanticHost>,
    file_id: FileId,
) -> usize {
    const MAX_HYDRATION_ATTEMPTS: usize = 3;

    for attempt in 1..=MAX_HYDRATION_ATTEMPTS {
        let prepare_host = Arc::clone(host);
        let step = tokio::task::spawn_blocking(move || prepare_hydration(&prepare_host, file_id))
            .await
            .unwrap_or(HydrationStep::Done(0));
        let plan = match step {
            HydrationStep::Done(hydrated) => return hydrated,
            HydrationStep::Retry => {
                debug!(?file_id, attempt, "Retrying dependency hydration");
                continue;
            }
            HydrationStep::Install(plan) => plan,
        };
        let started = Instant::now();
        debug!(?file_id, "Starting hydration semantic publication");
        let outcome = host
            .write()
            .await
            .run_blocking(move |txn| install_hydration(txn, file_id, &plan))
            .await;
        log_hydration_stage(file_id, "semantic publication", started.elapsed());
        match outcome {
            Ok(Some(HydrationInstall::Published(hydrated))) => return hydrated,
            Ok(Some(HydrationInstall::RetryExtraction) | None) | Err(_) => {
                debug!(?file_id, attempt, "Retrying dependency hydration");
            }
        }
    }

    warn!(
        ?file_id,
        attempts = MAX_HYDRATION_ATTEMPTS,
        "Dependency hydration remained stale; deferring until a later diagnostic pass"
    );
    0
}

/// Everything [`install_hydration`] needs, prepared against one workspace revision.
struct HydrationPlan {
    workspace_revision: hyperion_workspace::WorkspaceRevision,
    prepared: Vec<PreparedHydration>,
    resolved_calls: Option<hyperion_workspace::RevisionedResolvedMethodUsages>,
}

enum HydrationStep {
    /// Nothing to install (already hydrated, or the exact dependency set is unavailable).
    Done(usize),
    /// The index moved under the preparation; prepare again.
    Retry,
    Install(Box<HydrationPlan>),
}

enum HydrationInstall {
    Published(usize),
    RetryExtraction,
}

/// The read-only, gate-free half of hydration. Must run on a blocking thread.
fn prepare_hydration(host: &SemanticHost, file_id: FileId) -> HydrationStep {
    let workspace_index = host.index();
    let member_facts = host.member_fact_cache();

    // The exact caller bindings and every dependency-member outcome are
    // deterministic for a file/workspace revision pair. Repeated diagnostics
    // used to redo the full dependency scan and overload resolver even after
    // that transaction had already published successfully (several seconds on
    // large Java files). Sample the marker cheaply, then validate both tokens
    // together so a concurrent index mutation can never turn this into stale
    // semantic reuse.
    if let Some(file_revision) = workspace_index.current_file_revision(file_id) {
        let workspace_revision = workspace_index.current_workspace_revision();
        let revision = WorkspaceFactRevision::new(file_revision, workspace_revision);
        let already_hydrated =
            member_facts.contains_workspace_hydration_revision(file_id, revision);
        if already_hydrated
            && workspace_index.workspace_revisions_are_current(
                workspace_revision,
                std::slice::from_ref(&file_revision),
            )
        {
            debug!(?file_id, "Reusing exact dependency hydration publication");
            return HydrationStep::Done(0);
        }
    }

    let started = Instant::now();
    debug!(?file_id, "Starting hydration dependency snapshot");
    let dependencies = match workspace_index.get_current_file_dependency_fqns(file_id) {
        Ok(dependencies) => dependencies,
        Err(error) => {
            debug!(
                "Skipping dependency hydration for {:?}: exact dependency set unavailable: {}",
                file_id, error
            );
            return HydrationStep::Done(0);
        }
    };
    log_hydration_stage(file_id, "dependency snapshot", started.elapsed());
    let workspace_revision = dependencies.workspace_revision;
    let mut dependency_fqns = dependencies.fqns;

    // Member lookup depends on the caller's complete declared hierarchy,
    // including parents that have no explicit member/type use in the
    // method bodies. Do not make semantic hydration depend on the usage
    // extractor happening to report every extends/implements spelling:
    // bind those edges from the caller's exact source hierarchy under the
    // same workspace generation as the dependency scan.
    let started = Instant::now();
    debug!(?file_id, "Starting hydration caller hierarchy resolution");
    if let Some(indexed) = workspace_index
        .current_file_revision(file_id)
        .and_then(|revision| workspace_index.get_indexed_revision_content(revision))
    {
        if let Some(resolved) =
            workspace_index.file_supertypes_for_revision(file_id, &indexed.content)
        {
            if resolved.workspace_revision != workspace_revision {
                return HydrationStep::Retry;
            }
            dependency_fqns.extend(resolved.types.iter().flat_map(|resolved_type| {
                resolved_type
                    .superclass
                    .iter()
                    .chain(&resolved_type.interfaces)
                    .map(|supertype| Arc::<str>::from(supertype.as_str()))
            }));
            dependency_fqns.sort_unstable();
            dependency_fqns.dedup();
        }
    }
    log_hydration_stage(file_id, "caller hierarchy", started.elapsed());
    let started = Instant::now();
    debug!(?file_id, "Starting hydration dependency file lookup");
    let mut dependency_file_ids: HashSet<_> = workspace_index
        .find_files_defining_symbols(&dependency_fqns)
        .into_iter()
        .filter(|dependency_file| *dependency_file != file_id)
        .collect();
    log_hydration_stage(file_id, "dependency file lookup", started.elapsed());

    // A field selected on `Derived` can be declared in `Base` while the
    // path between them crosses files absent from the caller's usage set.
    let dependency_type_roots: HashSet<_> = dependency_fqns
        .iter()
        .filter_map(|dependency_fqn| workspace_index.find_symbol(dependency_fqn))
        .filter_map(|location| {
            hyperion_workspace::is_type_declaration_kind(location.kind)
                .then(|| location.fqn.to_string())
                .or_else(|| location.container_name.map(|owner| owner.to_string()))
        })
        .collect();
    let started = Instant::now();
    debug!(
        ?file_id,
        "Starting hydration inherited dependency expansion"
    );
    for root in dependency_type_roots {
        for owner in workspace_index.get_inheritance_chain(&root) {
            if let Some(location) = workspace_index.find_symbol(&owner) {
                if location.file_id != file_id {
                    dependency_file_ids.insert(location.file_id);
                }
            }
        }
    }
    log_hydration_stage(file_id, "inherited dependency expansion", started.elapsed());
    dependency_file_ids.insert(file_id);
    if workspace_index.current_workspace_revision() != workspace_revision {
        return HydrationStep::Retry;
    }

    let mut dependency_files: Vec<_> = dependency_file_ids.into_iter().collect();
    dependency_files.sort_unstable_by_key(|dependency_file| dependency_file.as_u32());
    let mut prepared = Vec::with_capacity(dependency_files.len());
    let started = Instant::now();
    debug!(
        ?file_id,
        dependencies = dependency_files.len(),
        "Starting hydration member preparation"
    );
    for dependency_file in dependency_files {
        let file_lock = workspace_index.get_file_lock(dependency_file);
        let _file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(file_revision) = workspace_index.current_file_revision(dependency_file) else {
            if dependency_file == file_id {
                return HydrationStep::Retry;
            }
            // Bytecode-only/JAR declarations have no workspace source
            // publication to hydrate; their contracts are owned by the
            // external signature loader.
            continue;
        };
        let indexed = workspace_index.get_indexed_revision_content(file_revision);
        let resolved = indexed.as_ref().and_then(|indexed| {
            (Language::from_path(&indexed.file_path) == Some(Language::Java))
                .then(|| {
                    workspace_index.file_supertypes_for_revision(dependency_file, &indexed.content)
                })
                .flatten()
        });
        if resolved
            .as_ref()
            .is_some_and(|resolved| resolved.workspace_revision != workspace_revision)
        {
            return HydrationStep::Retry;
        }
        let member_facts = match (indexed.as_ref(), resolved) {
            (Some(indexed), Some(resolved)) => {
                let revision = member_fact_revision(&resolved);
                let source_matches = host
                    .registered_text(dependency_file)
                    .is_none_or(|registered| registered.as_str() == indexed.content.as_str());
                let already_current =
                    member_facts.contains_workspace_member_fact_revision(dependency_file, revision);
                if !source_matches || already_current {
                    PreparedMemberFacts::Current(resolved)
                } else {
                    let facts = extract_member_facts_with_hierarchy(
                        &indexed.content,
                        dependency_file,
                        Some(workspace_index),
                        Some(resolved),
                    );
                    facts.map_or(
                        PreparedMemberFacts::Unavailable,
                        PreparedMemberFacts::Install,
                    )
                }
            }
            (Some(_), None) | (None, _) => PreparedMemberFacts::Unavailable,
        };
        prepared.push(PreparedHydration {
            file_id: dependency_file,
            file_revision,
            indexed,
            member_facts,
        });
    }
    log_hydration_stage(file_id, "member preparation", started.elapsed());
    if workspace_index.current_workspace_revision() != workspace_revision {
        return HydrationStep::Retry;
    }

    // Expensive overload re-resolution also runs outside the gate. It is
    // admitted only if it came from this same all-file workspace view.
    let caller_indexed = prepared
        .iter()
        .find(|prepared| prepared.file_id == file_id)
        .and_then(|prepared| prepared.indexed.as_ref());
    let started = Instant::now();
    debug!(?file_id, "Starting hydration call resolution");
    let resolved_calls =
        caller_indexed.and_then(|indexed| workspace_index.resolve_current_method_usages(indexed));
    log_hydration_stage(file_id, "call resolution", started.elapsed());
    if resolved_calls
        .as_ref()
        .is_some_and(|resolved| resolved.workspace_revision != workspace_revision)
    {
        return HydrationStep::Retry;
    }

    HydrationStep::Install(Box::new(HydrationPlan {
        workspace_revision,
        prepared,
        resolved_calls,
    }))
}

/// The gated half of hydration: validate the plan against the exact index
/// revisions and install it, all under the exclusive write transaction.
fn install_hydration(
    txn: &mut WriteTxn,
    file_id: FileId,
    plan: &HydrationPlan,
) -> Option<HydrationInstall> {
    let workspace_index = Arc::clone(txn.index());
    let HydrationPlan {
        workspace_revision,
        prepared,
        resolved_calls,
    } = plan;
    let caller_indexed = prepared
        .iter()
        .find(|prepared| prepared.file_id == file_id)
        .and_then(|prepared| prepared.indexed.as_ref());
    let indexed_sources = prepared
        .iter()
        .filter_map(|prepared| prepared.indexed.as_ref())
        .collect::<Vec<_>>();
    let file_revisions = prepared
        .iter()
        .map(|prepared| prepared.file_revision)
        .collect::<Vec<_>>();
    let hierarchies = prepared
        .iter()
        .filter_map(|prepared| prepared.member_facts.resolved_hierarchy())
        .collect::<Vec<_>>();
    let mut db = txn.db();
    workspace_index.with_current_semantic_publication(
        *workspace_revision,
        &file_revisions,
        &indexed_sources,
        &hierarchies,
        resolved_calls.as_ref(),
        || {
            // The parse-skipping fast path was sampled outside the gate.
            // Revalidate every such cache entry before changing any file,
            // otherwise a semantic writer between preparation and install
            // could turn an all-file transaction into a partial one.
            for prepared in prepared {
                let PreparedMemberFacts::Current(resolved) = &prepared.member_facts else {
                    continue;
                };
                let source_matches = db.try_file_text(prepared.file_id).is_none_or(|registered| {
                    prepared
                        .indexed
                        .as_ref()
                        .is_some_and(|indexed| registered.as_str() == indexed.content.as_str())
                });
                if source_matches
                    && !db
                        .method_signature_cache()
                        .contains_workspace_member_fact_revision(
                            prepared.file_id,
                            member_fact_revision(resolved),
                        )
                {
                    return HydrationInstall::RetryExtraction;
                }
            }

            let mut changed = false;
            let mut hydrated = 0;
            for prepared in prepared {
                let source_matches = db.try_file_text(prepared.file_id).is_none_or(|registered| {
                    prepared
                        .indexed
                        .as_ref()
                        .is_some_and(|indexed| registered.as_str() == indexed.content.as_str())
                });
                if !source_matches {
                    changed |= db
                        .method_signature_cache()
                        .remove_workspace_file_member_facts(prepared.file_id);
                    continue;
                }
                let facts = match &prepared.member_facts {
                    PreparedMemberFacts::Current(_) => continue,
                    PreparedMemberFacts::Install(facts) => facts,
                    PreparedMemberFacts::Unavailable => {
                        changed |= db
                            .method_signature_cache()
                            .remove_workspace_file_member_facts(prepared.file_id);
                        continue;
                    }
                };
                let resolved = facts
                    .resolved_hierarchy
                    .as_ref()
                    .expect("prepared Java facts carry exact hierarchy provenance");
                let revision = member_fact_revision(resolved);
                if db
                    .method_signature_cache()
                    .contains_workspace_member_fact_revision(prepared.file_id, revision)
                {
                    continue;
                }
                let installed = db
                    .method_signature_cache()
                    .update_workspace_file_member_facts(
                        prepared.file_id,
                        revision,
                        facts.signatures.clone(),
                        facts.fields.clone(),
                        facts.declared_types.clone(),
                        facts.type_hierarchies.clone(),
                    );
                changed |= installed;
                if installed && prepared.file_id != file_id {
                    hydrated += 1;
                }
            }

            let mut completed_revision = None;
            let call_changed = if let Some(resolved) = resolved_calls {
                if db.try_file_text(file_id).is_some_and(|registered| {
                    caller_indexed
                        .is_some_and(|indexed| registered.as_str() == indexed.content.as_str())
                }) {
                    let revision =
                        WorkspaceFactRevision::new(resolved.revision, resolved.workspace_revision);
                    let bindings = resolved
                        .usages
                        .iter()
                        .map(|(range, declaration_fqn)| (range.start, Arc::clone(declaration_fqn)))
                        .collect::<Vec<_>>();
                    let changed = db
                        .method_signature_cache()
                        .update_workspace_call_bindings(file_id, revision, bindings);
                    completed_revision = Some(revision);
                    changed
                } else {
                    db.method_signature_cache()
                        .remove_workspace_call_bindings(file_id)
                }
            } else {
                db.method_signature_cache()
                    .remove_workspace_call_bindings(file_id)
            };
            changed |= call_changed;
            if changed {
                db.bump_method_signature_version();
            }
            if let Some(revision) = completed_revision {
                db.method_signature_cache()
                    .mark_workspace_hydration_complete(file_id, revision);
            }
            HydrationInstall::Published(hydrated)
        },
    )
}

fn log_hydration_stage(file_id: FileId, stage: &'static str, elapsed: Duration) {
    if elapsed >= Duration::from_millis(250) {
        warn!(
            ?file_id,
            stage,
            ?elapsed,
            "Slow dependency hydration stage completed"
        );
    } else {
        debug!(
            ?file_id,
            stage,
            ?elapsed,
            "Dependency hydration stage completed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::db::SemanticDatabase;
    use hyperion_semantics::nullability::{DiagnosticKind, Nullability};
    use std::num::NonZeroUsize;

    /// Run the async hydration to completion from a synchronous test.
    fn hydrate_blocking(host: &Arc<SemanticHost>, file_id: FileId) -> usize {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(hydrate_dependency_method_signatures(host, file_id))
    }

    fn current_member_revision(
        index: &WorkspaceIndex,
        file_id: FileId,
    ) -> WorkspaceMemberFactRevision {
        let file_revision = index
            .current_file_revision(file_id)
            .expect("current file revision");
        let source = index
            .get_indexed_revision_content(file_revision)
            .expect("exact indexed source");
        let resolved = index
            .file_supertypes_for_revision(file_id, &source.content)
            .expect("exact resolved hierarchy");
        member_fact_revision(&resolved)
    }

    #[test]
    fn live_member_publication_reuses_the_exact_indexed_parse() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("Example.java");
        let source = "package app; class Example { int value; }";
        index
            .index_file_with_source(&path, source)
            .expect("index source");
        let file_id = index.get_file_id(&path).expect("file id");

        let (_, reused) = member_fact_syntax(source, file_id, Some(&index));
        assert!(reused, "exact live revision must reuse the workspace parse");
        let (_, reused_stale) = member_fact_syntax(
            "package app; class Example { long value; }",
            file_id,
            Some(&index),
        );
        assert!(
            !reused_stale,
            "a different source revision must parse locally"
        );
    }

    #[test]
    fn semantic_publication_reports_when_the_exact_source_is_not_indexed() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("Example.java");
        let indexed_source = Arc::new("class Indexed {}".to_owned());
        let unindexed_source = Arc::new("class Unindexed {}".to_owned());
        index
            .index_file_with_source(&path, &indexed_source)
            .expect("index source");
        let file_id = index.get_file_id(&path).expect("file id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| db.register_file(file_id, Arc::clone(&indexed_source), Language::Java));
        let var_type_cache = Mutex::new(LruCache::new(
            NonZeroUsize::new(1).expect("non-zero cache capacity"),
        ));

        assert!(!publish_semantic_revision(
            &mut host.write_now(),
            &var_type_cache,
            file_id,
            unindexed_source,
            Language::Java,
            &[],
        ));
        assert_eq!(
            host.read_now().file_text(file_id).as_str(),
            indexed_source.as_str()
        );
    }

    #[test]
    fn absent_retirement_never_erases_a_republished_workspace_identity() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("Retirement.java");
        let source = Arc::new("class Retirement {}".to_owned());
        index
            .index_file_with_source(&path, &source)
            .expect("index source");
        let file_id = index.get_file_id(&path).expect("file id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| db.register_file(file_id, Arc::clone(&source), Language::Java));
        let var_type_cache = Mutex::new(LruCache::new(
            NonZeroUsize::new(1).expect("non-zero cache capacity"),
        ));

        retire_absent_semantic_revisions(
            &mut host.write_now(),
            &var_type_cache,
            &[file_id],
            Language::Java,
        );
        assert_eq!(host.read_now().file_text(file_id).as_str(), source.as_str());

        index.remove_file(file_id);
        retire_absent_semantic_revisions(
            &mut host.write_now(),
            &var_type_cache,
            &[file_id],
            Language::Java,
        );
        assert!(host.read_now().file_text(file_id).is_empty());
    }

    /// Hydration writes Salsa inputs, which waits for every live
    /// snapshot; a snapshot holder blocked on the index lock (behind a queued
    /// writer) once deadlocked the whole server. The install step is a write
    /// transaction, so it cannot start while any read transaction is live, and
    /// it completes as soon as the last one ends.
    #[tokio::test(flavor = "multi_thread")]
    async fn hydration_publishes_only_when_no_read_transaction_is_held() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let provider_path = temp.path().join("Service.java");
        let client_path = temp.path().join("Client.java");
        let client_source = concat!(
            "package app; import api.Service; class Client { ",
            "void run(Service service) { service.load(); } }"
        );
        index
            .index_file_with_source(
                &provider_path,
                "package api; class Service { @Nullable String load() { return null; } }",
            )
            .expect("index provider");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::clone(&index));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });
        let reader = host.read().await;

        let hydration_host = Arc::clone(&host);
        let mut hydration = tokio::spawn(async move {
            hydrate_dependency_method_signatures(&hydration_host, client_id).await
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(400), &mut hydration)
                .await
                .is_err(),
            "hydration published while a read transaction was held"
        );
        drop(reader);
        let hydrated = tokio::time::timeout(Duration::from_secs(10), hydration)
            .await
            .expect("hydration finishes once the reader is released")
            .expect("hydration task");
        assert_eq!(hydrated, 1);
    }

    #[test]
    fn cold_dependency_contracts_hydrate_once_per_indexed_revision() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let provider_path = temp.path().join("Service.java");
        let client_path = temp.path().join("Client.java");
        let nullable_provider = concat!(
            "package api; class Service { ",
            "@Nullable String load() { return null; } }"
        );
        let client_source = concat!(
            "package app; import api.Service; class Client { ",
            "void run(Service service) { String loaded = service.load(); loaded.length(); } }"
        );

        index
            .index_file_with_source(&provider_path, nullable_provider)
            .expect("index cold provider");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let provider_id = index.get_file_id(&provider_path).expect("provider id");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });
        assert!(!host.member_fact_cache().contains_file(provider_id));

        assert_eq!(hydrate_blocking(&host, client_id), 1);
        let hydrated_revision = WorkspaceFactRevision::new(
            index
                .current_file_revision(client_id)
                .expect("current client revision"),
            index.current_workspace_revision(),
        );
        assert!(host
            .member_fact_cache()
            .contains_workspace_hydration_revision(client_id, hydrated_revision));
        assert_eq!(
            host.member_fact_cache()
                .get_signature("api.Service.load()")
                .expect("hydrated contract")
                .return_nullability,
            Nullability::Nullable
        );
        assert!(host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
        assert_eq!(
            hydrate_blocking(&host, client_id),
            0,
            "an unchanged cold revision must not be parsed again"
        );

        index
            .index_file_with_source(
                &provider_path,
                "package api; class Service { @Nonnull String load() { return \"ok\"; } }",
            )
            .expect("replace cold provider");
        let rebound_revision = WorkspaceFactRevision::new(
            index
                .current_file_revision(client_id)
                .expect("unchanged client revision"),
            index.current_workspace_revision(),
        );
        assert!(!host
            .member_fact_cache()
            .contains_workspace_hydration_revision(client_id, rebound_revision));
        assert_eq!(
            hydrate_blocking(&host, client_id),
            1,
            "a new indexed content hash must be hydrated"
        );
        assert!(!host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
    }

    #[test]
    fn expanded_varargs_calls_use_cold_dependency_contracts() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let provider_path = temp.path().join("Logger.java");
        let client_path = temp.path().join("Client.java");
        let provider_source = concat!(
            "package api; public class Logger { ",
            "@Nullable public String first(String... values) { return null; } ",
            "public void log(@Nonnull String... values) {} }"
        );
        let client_source = concat!(
            "package app; import api.Logger; class Client { ",
            "void run(Logger logger, @Nullable String value) { ",
            "logger.first().length(); logger.log(\"safe\", value, null); } }"
        );

        index
            .index_file_with_source(&provider_path, provider_source)
            .expect("index cold provider");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });

        assert_eq!(hydrate_blocking(&host, client_id), 1);
        let analysis = host.read_now().file_nullability(client_id);
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
        assert_eq!(
            analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull)
                .count(),
            2,
            "each nullable expanded argument must use the varargs element contract"
        );
    }

    #[test]
    fn cold_same_arity_overloads_use_the_resolved_declaration_contract() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let provider_path = temp.path().join("Service.java");
        let client_path = temp.path().join("Client.java");
        let provider_source = concat!(
            "package api; public class Service { ",
            "@Nullable public String load(String value) { return null; } ",
            "@Nonnull public String load(int value) { return \"ok\"; } ",
            "public void consume(@Nonnull String value) {} ",
            "public void consume(Integer value) {} }"
        );
        let client_source = concat!(
            "package app; import api.Service; class Client { ",
            "void run(Service service, @Nullable String text, @Nullable Integer number) { ",
            "service.load(1).length(); service.load(\"nullable\").length(); ",
            "service.consume(text); service.consume(number); } }"
        );

        index
            .index_file_with_source(&provider_path, provider_source)
            .expect("index cold provider");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let method_usages = index
            .get_file_resolved_method_usages_revisioned(client_id)
            .expect("revisioned method usages");
        assert!(index.resolved_method_usages_are_current(&method_usages));
        assert!(method_usages
            .usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "api.Service.load(int)"));
        assert!(method_usages
            .usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "api.Service.load(java.lang.String)"));
        assert!(
            method_usages
                .usages
                .iter()
                .any(|(_, fqn)| fqn.as_ref() == "api.Service.consume(java.lang.String)"),
            "unexpected resolved calls: {method_usages:?}"
        );
        assert!(method_usages
            .usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "api.Service.consume(java.lang.Integer)"));

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });
        assert_eq!(hydrate_blocking(&host, client_id), 1);
        assert_eq!(
            host.member_fact_cache()
                .get_signature("api.Service.consume(String)")
                .expect("String overload contract")
                .param_nullabilities,
            vec![Nullability::NonNull]
        );

        let analysis = host.read_now().file_nullability(client_id);
        let dereferences: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
            .collect();
        assert_eq!(
            dereferences.len(),
            1,
            "only the String overload is nullable: {dereferences:?}"
        );
        let nullable_call_offset = client_source
            .find("service.load(\"nullable\")")
            .expect("nullable call offset") as u32;
        assert!(dereferences[0].location.start >= nullable_call_offset);
        let rejected_arguments: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull)
            .collect();
        assert_eq!(
            rejected_arguments.len(),
            1,
            "only the String parameter has a non-null contract: {:?}",
            analysis.diagnostics
        );
        assert_eq!(rejected_arguments[0].variable_name, "text");
    }

    #[test]
    fn stale_workspace_caller_revision_cannot_publish_exact_call_bindings() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let provider_path = temp.path().join("Dependency.java");
        let caller_path = temp.path().join("Client.java");
        let provider = concat!(
            "package api; public class Dependency { ",
            "public static void accept(int first, Object second) {} ",
            "public static void accept(Integer first, String second) {} }",
        );
        let indexed_caller = concat!(
            "package app; import api.Dependency; class Client { ",
            "void run(@Nullable Integer first) { Object marker = null; try { ",
            "Dependency.accept(first, marker = new Object()); ",
            "} catch (NullPointerException failure) { marker.toString(); } } }",
        );
        let live_caller = format!("{indexed_caller} // unsaved live revision");
        index
            .index_file_with_source(&provider_path, provider)
            .expect("index provider");
        index
            .index_file_with_source(&caller_path, indexed_caller)
            .expect("index caller");
        let caller_id = index.get_file_id(&caller_path).expect("caller id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| db.register_file(caller_id, Arc::new(live_caller), Language::Java));

        assert_eq!(hydrate_blocking(&host, caller_id), 1);
        let analysis = host.read_now().file_nullability(caller_id);
        assert!(
            !analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == DiagnosticKind::PotentialNullDereference
                    && diagnostic.variable_name == "marker"
            }),
            "a stale exact int-overload binding leaked into the live source: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn newer_registered_provider_text_retires_older_indexed_member_facts() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let provider_path = temp.path().join("Service.java");
        let client_path = temp.path().join("Client.java");
        let indexed_provider = concat!(
            "package api; class Service { ",
            "@Nullable String load() { return null; } }"
        );
        let newer_provider = concat!(
            "package api; class Service { ",
            "@Nonnull String load() { return \"safe\"; } } // unsaved"
        );
        let client = concat!(
            "package app; import api.Service; class Client { ",
            "void run(Service service) { service.load().length(); } }"
        );
        index
            .index_file_with_source(&provider_path, indexed_provider)
            .expect("index provider");
        index
            .index_file_with_source(&client_path, client)
            .expect("index client");
        let provider_id = index.get_file_id(&provider_path).expect("provider id");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                provider_id,
                Arc::new(indexed_provider.to_owned()),
                Language::Java,
            );
        });
        hydrate_blocking(&host, provider_id);
        assert!(host
            .member_fact_cache()
            .get_signature("api.Service.load()")
            .is_some());

        host.with_db(|db| {
            db.register_file(
                provider_id,
                Arc::new(newer_provider.to_owned()),
                Language::Java,
            );
        });
        host.with_db(|db| db.register_file(client_id, Arc::new(client.to_owned()), Language::Java));
        hydrate_blocking(&host, client_id);

        assert!(
            host.member_fact_cache()
                .get_signature("api.Service.load()")
                .is_none(),
            "older indexed provider facts must be retired when Salsa owns newer text"
        );
    }

    #[test]
    fn lazy_call_resolution_survives_unrelated_cli_style_publication() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let provider_path = temp.path().join("Service.java");
        let client_path = temp.path().join("Client.java");
        let provider = concat!(
            "package api; class Service { ",
            "void accept(@Nonnull String value) {} ",
            "void accept(Integer value) {} }"
        );
        let client = concat!(
            "package app; import api.Service; class Client { ",
            "void run(Service service, @Nullable String value) { service.accept(value); } }"
        );
        index
            .index_file_with_source(&provider_path, provider)
            .expect("index provider");
        index
            .index_file_with_source(&client_path, client)
            .expect("index caller before later CLI files");
        index
            .index_file_with_source(
                &temp.path().join("Later.java"),
                "package unrelated; class Later {}",
            )
            .expect("index unrelated later file");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| db.register_file(client_id, Arc::new(client.to_owned()), Language::Java));

        hydrate_blocking(&host, client_id);
        assert!(
            host.read_now()
                .file_nullability(client_id)
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::NullPassedToNonNull),
            "an unrelated later publication must not discard the selected String overload"
        );
    }

    #[test]
    fn cold_duplicate_definitions_join_contracts_conservatively() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let nullable_path = temp.path().join("one/Service.java");
        let nonnull_path = temp.path().join("two/Service.java");
        let client_path = temp.path().join("Client.java");
        let client_source = concat!(
            "package app; import api.Service; class Client { ",
            "void run(Service service) { String loaded = service.load(); loaded.length(); } }"
        );

        index
            .index_file_with_source(
                &nullable_path,
                "package api; class Service { @Nullable String load() { return null; } }",
            )
            .expect("index nullable definition");
        index
            .index_file_with_source(
                &nonnull_path,
                concat!(
                    "package api; class Service { ",
                    "@Nonnull String load() { return \"ok\"; } }"
                ),
            )
            .expect("index non-null definition");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });

        assert_eq!(hydrate_blocking(&host, client_id), 2);
        assert!(host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
    }

    #[test]
    fn shadowed_duplicate_owner_hydrates_its_own_exact_hierarchy() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let nullable_base_path = temp.path().join("nullable/Base.java");
        let shadowed_child_path = temp.path().join("z-shadowed/Child.java");
        let active_base_path = temp.path().join("active/Base.java");
        let active_child_path = temp.path().join("a-active/Child.java");
        let nullable_base = concat!(
            "package nullable; public class Base { ",
            "@Nullable protected String value; }"
        );
        let shadowed_child = concat!(
            "package dup; import nullable.Base; class Child extends Base { ",
            "void run() { value.length(); } }"
        );

        index
            .index_file_with_source(&nullable_base_path, nullable_base)
            .expect("index nullable base");
        index
            .index_file_with_source(&shadowed_child_path, shadowed_child)
            .expect("index first duplicate child");
        index
            .index_file_with_source(
                &active_base_path,
                "package active; public class Base { @Nonnull protected String other; }",
            )
            .expect("index active base");
        index
            .index_file_with_source(
                &active_child_path,
                "package dup; import active.Base; class Child extends Base {}",
            )
            .expect("index active duplicate child");

        assert_eq!(
            index.direct_supertypes("dup.Child"),
            vec!["active.Base"],
            "stable normalized-path authority must select the active duplicate"
        );

        let nullable_base_id = index
            .get_file_id(&nullable_base_path)
            .expect("nullable base id");
        let shadowed_child_id = index
            .get_file_id(&shadowed_child_path)
            .expect("shadowed child id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                nullable_base_id,
                Arc::new(nullable_base.to_owned()),
                Language::Java,
            );
        });
        assert_eq!(hydrate_blocking(&host, nullable_base_id), 0);
        host.with_db(|db| {
            db.register_file(
                shadowed_child_id,
                Arc::new(shadowed_child.to_owned()),
                Language::Java,
            );
        });
        hydrate_blocking(&host, shadowed_child_id);

        assert!(
            host.read_now().file_nullability(shadowed_child_id)
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference),
            "the shadowed source must inherit nullable.Base.value, not the active duplicate's parent"
        );
    }

    #[test]
    fn identical_source_rehydrates_after_same_package_hierarchy_rebinding() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let imported_base_path = temp.path().join("imported/Base.java");
        let child_path = temp.path().join("app/Child.java");
        let same_package_base_path = temp.path().join("app/Base.java");
        let imported_base = concat!(
            "package imported; public class Base { ",
            "@Nullable protected String value; }"
        );
        let child = concat!(
            "package app; import imported.*; class Child extends Base { ",
            "void run() { value.length(); } }"
        );
        let same_package_base = concat!(
            "package app; public class Base { ",
            "@Nonnull protected String value = \"safe\"; }"
        );

        index
            .index_file_with_source(&imported_base_path, imported_base)
            .expect("index wildcard-imported base");
        index
            .index_file_with_source(&child_path, child)
            .expect("index child");
        let imported_base_id = index
            .get_file_id(&imported_base_path)
            .expect("imported base id");
        let child_id = index.get_file_id(&child_path).expect("child id");
        let child_file_revision = index
            .current_file_revision(child_id)
            .expect("initial child revision");
        let initial_member_revision = current_member_revision(&index, child_id);

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                imported_base_id,
                Arc::new(imported_base.to_owned()),
                Language::Java,
            );
        });
        hydrate_blocking(&host, imported_base_id);
        host.with_db(|db| db.register_file(child_id, Arc::new(child.to_owned()), Language::Java));
        hydrate_blocking(&host, child_id);
        assert!(host
            .read_now()
            .file_nullability(child_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));

        index
            .index_file_with_source(&same_package_base_path, same_package_base)
            .expect("publish late same-package base");
        assert_eq!(
            index.current_file_revision(child_id),
            Some(child_file_revision),
            "publishing another file must not counterfeit a new child source revision"
        );
        let rebound_member_revision = current_member_revision(&index, child_id);
        assert_ne!(
            rebound_member_revision.workspace_revision(),
            initial_member_revision.workspace_revision()
        );
        assert_ne!(
            rebound_member_revision.hierarchy_fingerprint(),
            initial_member_revision.hierarchy_fingerprint()
        );

        let same_package_base_id = index
            .get_file_id(&same_package_base_path)
            .expect("same-package base id");
        host.with_db(|db| {
            db.register_file(
                same_package_base_id,
                Arc::new(same_package_base.to_owned()),
                Language::Java,
            );
        });
        hydrate_blocking(&host, same_package_base_id);
        hydrate_blocking(&host, child_id);
        assert!(host
            .member_fact_cache()
            .contains_workspace_member_fact_revision(child_id, rebound_member_revision));
        assert!(
            !host
                .read_now()
                .file_nullability(child_id)
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference),
            "same bytes must be rehydrated when the contextual hierarchy revision changes"
        );
    }

    #[test]
    fn inherited_interface_contract_uses_the_declaration_owner() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let contract_path = temp.path().join("Contract.java");
        let implementation_path = temp.path().join("Implementation.java");
        let client_path = temp.path().join("Client.java");
        let client_source = concat!(
            "package app; import impl.Implementation; class Client { ",
            "void run(Implementation implementation) { implementation.load().length(); } }"
        );

        index
            .index_file_with_source(
                &contract_path,
                concat!(
                    "package api; public interface Contract { ",
                    "@Nullable default String load() { return null; } }"
                ),
            )
            .expect("index interface");
        index
            .index_file_with_source(
                &implementation_path,
                concat!(
                    "package impl; import api.Contract; ",
                    "public class Implementation implements Contract {}"
                ),
            )
            .expect("index implementation");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });
        let method_usages = index
            .get_file_resolved_method_usages_revisioned(client_id)
            .expect("revisioned method usages");
        assert!(index.resolved_method_usages_are_current(&method_usages));
        assert!(
            method_usages
                .usages
                .iter()
                .any(|(_, fqn)| fqn.starts_with("api.Contract.load(")),
            "unexpected resolved calls: {method_usages:?}"
        );
        assert!(hydrate_blocking(&host, client_id) >= 1);
        assert_eq!(
            host.member_fact_cache()
                .get_signature("api.Contract.load()")
                .expect("interface contract")
                .return_nullability,
            Nullability::Nullable
        );
        assert!(host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
    }

    #[test]
    fn cold_inherited_fields_hydrate_the_bounded_multi_hop_hierarchy() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let sources = [
            (
                "Root.java",
                "package api; public interface Root { int ROOT_VALUE = 1; }",
            ),
            (
                "Left.java",
                "package api; public interface Left extends Root {}",
            ),
            (
                "Right.java",
                "package api; public interface Right extends Root {}",
            ),
            (
                "Base.java",
                "package api; public class Base { public static int BASE_VALUE = 2; }",
            ),
            (
                "Mid.java",
                concat!(
                    "package middle; import api.*; public class Mid extends Base ",
                    "implements Left, Right {}",
                ),
            ),
            (
                "Derived.java",
                "package impl; import middle.Mid; public class Derived extends Mid {}",
            ),
        ];
        let mut hierarchy_files = Vec::new();
        for (name, source) in sources {
            let path = temp.path().join(name);
            index
                .index_file_with_source(&path, source)
                .unwrap_or_else(|error| panic!("index {name}: {error}"));
            hierarchy_files.push(path);
        }
        let client_path = temp.path().join("Client.java");
        let client_source = concat!(
            "package app; import impl.Derived; class Client { ",
            "void run() { int base = Derived.BASE_VALUE; int root = Derived.ROOT_VALUE; } }",
        );
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");
        assert_eq!(
            index.get_inheritance_chain("impl.Derived"),
            vec![
                "impl.Derived",
                "middle.Mid",
                "api.Base",
                "api.Left",
                "api.Right",
                "api.Root",
            ]
        );

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_owned()),
                Language::Java,
            );
        });
        assert_eq!(
            hydrate_blocking(&host, client_id),
            hierarchy_files.len(),
            "only the six-file inheritance closure should hydrate"
        );
        for path in hierarchy_files {
            let file = index.get_file_id(&path).expect("hierarchy file id");
            let revision = current_member_revision(&index, file);
            assert!(
                host.member_fact_cache()
                    .contains_workspace_member_fact_revision(file, revision),
                "missing cold member facts for {}",
                path.display()
            );
        }
        assert_eq!(
            hydrate_blocking(&host, client_id),
            0,
            "unchanged hierarchy revisions must not reparse"
        );
    }

    #[test]
    fn inherited_class_field_dominates_same_named_interface_field() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("I.java"),
                "package api; interface I { @Nonnull String value = \"safe\"; }",
            )
            .expect("index interface");
        index
            .index_file_with_source(
                &temp.path().join("B.java"),
                "package api; class B { @Nullable public String value; }",
            )
            .expect("index base class");
        let child_path = temp.path().join("C.java");
        let child = concat!(
            "package api; class C extends B implements I { ",
            "void run() { value.length(); } }"
        );
        index
            .index_file_with_source(&child_path, child)
            .expect("index child");
        let child_id = index.get_file_id(&child_path).expect("child id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| db.register_file(child_id, Arc::new(child.to_owned()), Language::Java));

        hydrate_blocking(&host, child_id);
        assert!(
            host.read_now()
                .file_nullability(child_id)
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference),
            "B.value is nullable and must dominate I.value"
        );
    }

    #[test]
    fn cold_hierarchy_uses_canonical_inherited_lexical_member_type() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let wrong_path = temp.path().join("q/Base.java");
        let parent_path = temp.path().join("p/Parent.java");
        let outer_path = temp.path().join("p/Outer.java");
        let client_path = temp.path().join("Client.java");
        index
            .index_file_with_source(
                &wrong_path,
                "package q; public class Base { public static int VALUE = 1; }",
            )
            .expect("index imported Base");
        index
            .index_file_with_source(
                &parent_path,
                concat!(
                    "package p; public class Parent { public static class Base { ",
                    "public static Integer VALUE = null; } }",
                ),
            )
            .expect("index inherited member type");
        index
            .index_file_with_source(
                &outer_path,
                concat!(
                    "package p; import q.Base; public class Outer extends Parent { ",
                    "public static class Child extends Base {} }",
                ),
            )
            .expect("index nested subtype");
        let client_source = concat!(
            "package app; import p.Outer.Child; class Client { ",
            "void run() { int selected = Child.VALUE + 1; } }",
        );
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        assert_eq!(
            index.direct_supertypes("p.Outer.Child"),
            vec!["p.Parent.Base"],
            "an inherited lexical member type shadows the ordinary import"
        );

        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_owned()),
                Language::Java,
            );
        });
        assert!(hydrate_blocking(&host, client_id) >= 1);
        assert!(
            host.read_now()
                .file_nullability(client_id)
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference),
            "the selected Integer field must retain its unboxing risk"
        );
    }

    #[test]
    fn concrete_override_contract_wins_over_nullable_interface() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let contract_path = temp.path().join("Contract.java");
        let implementation_path = temp.path().join("Implementation.java");
        let client_path = temp.path().join("Client.java");
        let client_source = concat!(
            "package app; import impl.Implementation; class Client { ",
            "void run(Implementation implementation) { implementation.load().length(); } }"
        );

        index
            .index_file_with_source(
                &contract_path,
                "package api; public interface Contract { @Nullable String load(); }",
            )
            .expect("index interface");
        index
            .index_file_with_source(
                &implementation_path,
                concat!(
                    "package impl; import api.Contract; public class Implementation ",
                    "implements Contract { @Nonnull public String load() { return \"ok\"; } }"
                ),
            )
            .expect("index implementation");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });
        assert!(hydrate_blocking(&host, client_id) >= 1);
        assert!(!host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
    }

    #[test]
    fn unchanged_callers_refresh_bindings_when_an_override_appears_and_disappears() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let contract_path = temp.path().join("Contract.java");
        let implementation_path = temp.path().join("Implementation.java");
        let client_path = temp.path().join("Client.java");
        let inherited_implementation = concat!(
            "package impl; import api.Contract; ",
            "public class Implementation implements Contract {}"
        );
        let overriding_implementation = concat!(
            "package impl; import api.Contract; public class Implementation ",
            "implements Contract { @Nonnull public String load() { return \"ok\"; } }"
        );
        let client_source = concat!(
            "package app; import impl.Implementation; class Client { ",
            "void run(Implementation implementation) { implementation.load().length(); } }"
        );

        index
            .index_file_with_source(
                &contract_path,
                "package api; public interface Contract { @Nullable String load(); }",
            )
            .expect("index interface");
        index
            .index_file_with_source(&implementation_path, inherited_implementation)
            .expect("index implementation");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("index client");
        let client_id = index.get_file_id(&client_path).expect("client id");
        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                client_id,
                Arc::new(client_source.to_string()),
                Language::Java,
            );
        });

        assert!(hydrate_blocking(&host, client_id) >= 1);
        assert!(host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));

        index
            .index_file_with_source(&implementation_path, overriding_implementation)
            .expect("add override");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("re-resolve unchanged client");
        assert!(hydrate_blocking(&host, client_id) >= 1);
        assert!(!host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));

        index
            .index_file_with_source(&implementation_path, inherited_implementation)
            .expect("remove override");
        index
            .index_file_with_source(&client_path, client_source)
            .expect("re-resolve unchanged client again");
        assert!(hydrate_blocking(&host, client_id) >= 1);
        assert!(host
            .read_now()
            .file_nullability(client_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
    }

    #[test]
    fn unqualified_inherited_abstract_contract_uses_the_base_owner() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let base_path = temp.path().join("Base.java");
        let derived_path = temp.path().join("Derived.java");
        let derived_source = concat!(
            "package impl; import api.Base; public abstract class Derived extends Base { ",
            "void run() { load().length(); } }"
        );

        index
            .index_file_with_source(
                &base_path,
                concat!(
                    "package api; public abstract class Base { ",
                    "@Nullable public abstract String load(); }"
                ),
            )
            .expect("index abstract base");
        index
            .index_file_with_source(&derived_path, derived_source)
            .expect("index derived class");
        let derived_id = index.get_file_id(&derived_path).expect("derived id");

        let host = SemanticHost::new(Arc::new(index.clone()));
        host.with_db(|db| {
            db.register_file(
                derived_id,
                Arc::new(derived_source.to_string()),
                Language::Java,
            );
        });
        assert_eq!(hydrate_blocking(&host, derived_id), 1);
        assert!(host
            .read_now()
            .file_nullability(derived_id)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference));
    }
}
