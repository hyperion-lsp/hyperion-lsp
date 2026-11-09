// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Revision-exact usage queries and destructive-refactoring usage proofs.

use super::query::find_symbol_definition_coherent_in;
use super::refactoring::{
    callable_overloads_for_target_in, inherited_method_reference_aliases_in, CallableOverload,
};
use super::*;

mod audit;

const CURRENT_USAGE_QUERY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
struct CurrentUsageScanInput {
    revision: FileRevisionToken,
    file_path: PathBuf,
    indexed_disk_hash: Option<u64>,
    language: Language,
    imports: ImportMap,
    imported_fqns: Vec<Arc<str>>,
    known_declaration_identifiers: Vec<(Arc<str>, crate::TextRange)>,
}

#[derive(Debug)]
struct CurrentUsageScanSnapshot {
    workspace_revision: WorkspaceRevision,
    inputs: Vec<CurrentUsageScanInput>,
    targets: Vec<CurrentUsageTarget>,
    strict_lexical_audit: bool,
    resource_root: Option<PathBuf>,
    ignored_identifier_ranges: Vec<(FileRevisionToken, crate::TextRange)>,
}

#[derive(Clone, Copy, Debug)]
enum CurrentUsageQueryTargets<'a> {
    Fqns(&'a [Arc<str>]),
    Typed(&'a [crate::SymbolTarget]),
    MethodFamily(&'a [Arc<str>]),
}

#[derive(Clone, Copy, Debug)]
struct CurrentUsageQuery<'a> {
    targets: CurrentUsageQueryTargets<'a>,
    expected_workspace_revision: Option<WorkspaceRevision>,
    lexical_audit: Option<&'a [(FileRevisionToken, crate::TextRange)]>,
}

impl<'a> CurrentUsageQuery<'a> {
    fn for_fqns(fqns: &'a [Arc<str>]) -> Self {
        Self {
            targets: CurrentUsageQueryTargets::Fqns(fqns),
            expected_workspace_revision: None,
            lexical_audit: None,
        }
    }

    fn for_typed_targets(targets: &'a [crate::SymbolTarget]) -> Self {
        Self {
            targets: CurrentUsageQueryTargets::Typed(targets),
            expected_workspace_revision: None,
            lexical_audit: None,
        }
    }

    fn for_method_family(fqns: &'a [Arc<str>]) -> Self {
        Self {
            targets: CurrentUsageQueryTargets::MethodFamily(fqns),
            expected_workspace_revision: None,
            lexical_audit: None,
        }
    }

    fn at_workspace_revision(mut self, revision: Option<WorkspaceRevision>) -> Self {
        self.expected_workspace_revision = revision;
        self
    }

    fn with_lexical_audit(
        mut self,
        ignored_identifier_ranges: &'a [(FileRevisionToken, crate::TextRange)],
    ) -> Self {
        self.lexical_audit = Some(ignored_identifier_ranges);
        self
    }

    fn typed_targets(self) -> Option<&'a [crate::SymbolTarget]> {
        match self.targets {
            CurrentUsageQueryTargets::Typed(targets) => Some(targets),
            CurrentUsageQueryTargets::Fqns(_) | CurrentUsageQueryTargets::MethodFamily(_) => None,
        }
    }

    fn is_method_family(self) -> bool {
        matches!(self.targets, CurrentUsageQueryTargets::MethodFamily(_))
    }

    fn strict_lexical_audit(self) -> bool {
        self.lexical_audit.is_some()
    }

    fn ignored_identifier_ranges(self) -> &'a [(FileRevisionToken, crate::TextRange)] {
        self.lexical_audit.unwrap_or_default()
    }
}

struct IdentifierRangeAudit<'a> {
    snapshot: &'a CurrentUsageScanSnapshot,
    input: &'a CurrentUsageScanInput,
    indexed: &'a crate::IndexedContent,
    usages: &'a [crate::usage_extractor::Usage],
    identifier_ranges: &'a [crate::TextRange],
    declaration_ranges: &'a [crate::TextRange],
}

#[derive(Debug)]
struct CurrentUsageTarget {
    fqn: Arc<str>,
    namespace: Option<crate::DeclarationNamespace>,
    kind: CurrentUsageTargetKind,
}

#[derive(Debug)]
enum CurrentUsageTargetKind {
    Exact,
    /// One exact overload. A descriptorless or placeholder-bearing usage may
    /// denote it, but does not prove that it does.
    MethodOverload {
        descriptorless: Arc<str>,
        overloads: Vec<CallableOverload>,
        /// Descriptorless method references may name an inherited method
        /// through a receiver that declares no matching family member. Keep
        /// those receiver spellings so an exact refactoring fails closed
        /// instead of silently missing the reference.
        inherited_descriptorless: Vec<Arc<str>>,
    },
    /// Explicit all-overloads query used by the search CLI.
    MethodFamily {
        descriptor_prefix: String,
    },
    Type {
        constructor_prefix: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CurrentUsageMatch {
    No,
    Exact,
    AmbiguousMethod,
}

impl CurrentUsageTarget {
    fn lexical_identifier(&self) -> Option<&str> {
        let descriptorless = self
            .fqn
            .split_once('(')
            .map_or(self.fqn.as_ref(), |pair| pair.0);
        descriptorless
            .rsplit('.')
            .next()
            .filter(|name| !name.is_empty())
    }

    fn accepts_usage_kind(&self, usage_kind: SymbolKind) -> bool {
        let Some(namespace) = self.namespace else {
            return true;
        };
        namespace.accepts(usage_kind)
    }

    fn classify(&self, usage_fqn: &str, usage_kind: SymbolKind) -> CurrentUsageMatch {
        if usage_fqn == self.fqn.as_ref() {
            if !self.accepts_usage_kind(usage_kind) {
                return CurrentUsageMatch::No;
            }
            return CurrentUsageMatch::Exact;
        }
        match &self.kind {
            CurrentUsageTargetKind::Exact => CurrentUsageMatch::No,
            CurrentUsageTargetKind::MethodOverload {
                descriptorless,
                overloads,
                inherited_descriptorless,
            } => {
                if !self.accepts_usage_kind(usage_kind) {
                    return CurrentUsageMatch::No;
                }
                if inherited_descriptorless
                    .iter()
                    .any(|alias| usage_fqn == alias.as_ref())
                {
                    return CurrentUsageMatch::AmbiguousMethod;
                }
                let unique_is_target = if usage_fqn == descriptorless.as_ref() {
                    unique_overload_is_target(overloads.iter(), &self.fqn)
                } else {
                    let Some(descriptor) =
                        usage_fqn
                            .strip_prefix(descriptorless.as_ref())
                            .filter(|descriptor| {
                                descriptor.starts_with('(') && descriptor.contains('_')
                            })
                    else {
                        return CurrentUsageMatch::No;
                    };
                    let Some(arity) = descriptor_arity(descriptor) else {
                        return CurrentUsageMatch::AmbiguousMethod;
                    };
                    unique_overload_is_target(
                        overloads
                            .iter()
                            .filter(|overload| overload.accepts_arity(arity)),
                        &self.fqn,
                    )
                };
                if unique_is_target {
                    CurrentUsageMatch::Exact
                } else {
                    CurrentUsageMatch::AmbiguousMethod
                }
            }
            CurrentUsageTargetKind::MethodFamily { descriptor_prefix } => {
                if self.accepts_usage_kind(usage_kind) && usage_fqn.starts_with(descriptor_prefix) {
                    CurrentUsageMatch::Exact
                } else {
                    CurrentUsageMatch::No
                }
            }
            CurrentUsageTargetKind::Type { constructor_prefix } => {
                if usage_kind == SymbolKind::CONSTRUCTOR
                    && usage_fqn.starts_with(constructor_prefix)
                {
                    CurrentUsageMatch::Exact
                } else {
                    CurrentUsageMatch::No
                }
            }
        }
    }
}

fn unique_overload_is_target<'a>(
    mut overloads: impl Iterator<Item = &'a CallableOverload>,
    target_fqn: &str,
) -> bool {
    overloads
        .next()
        .is_some_and(|overload| overload.fqn.as_ref() == target_fqn)
        && overloads.next().is_none()
}

fn descriptor_arity(descriptor: &str) -> Option<usize> {
    let parameters = descriptor.strip_prefix('(')?.strip_suffix(')')?;
    if parameters.is_empty() {
        return Some(0);
    }
    let mut depth = 0usize;
    let mut count = 1usize;
    for character in parameters.chars() {
        match character {
            '<' | '(' | '[' => depth = depth.checked_add(1)?,
            '>' | ')' | ']' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    (depth == 0).then_some(count)
}

fn symbol_targets_for_fqn(inner: &IndexInner, fqn: &str) -> Vec<crate::SymbolTarget> {
    crate::DeclarationNamespace::ALL
        .into_iter()
        .filter_map(|namespace| {
            let target = crate::SymbolTarget::new(Arc::from(fqn), namespace);
            find_symbol_definition_coherent_in(inner, &target).map(|_| target)
        })
        .collect()
}
impl WorkspaceIndex {
    /// Re-resolve all usages from exact current workspace bytes.
    ///
    /// Ordinary Java candidates are narrowed with the compact lexical digest
    /// stored for each exact file revision. Sources whose spelling cannot be
    /// filtered soundly (including Unicode escapes and other JVM languages)
    /// deliberately retain the complete-workspace scan as their fallback.
    pub fn find_current_usages(
        &self,
        fqn: &str,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        self.find_current_usages_for_targets_inner(
            CurrentUsageQuery::for_fqns(&[Arc::from(fqn)]),
            || false,
        )
    }

    /// Re-resolve usages of one exact semantic declaration identity.
    pub fn find_current_usages_for_symbol_target(
        &self,
        target: &crate::SymbolTarget,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        self.find_current_usages_for_symbol_targets(std::slice::from_ref(target), None, || false)
    }

    /// Resolve every overload sharing one descriptorless method FQN.
    ///
    /// This is intentionally distinct from [`Self::find_current_usages`]: a
    /// field and method may legally share `Owner.name`, while an exact overload
    /// must not absorb calls to its siblings.
    pub fn find_current_method_family_usages(
        &self,
        descriptorless_fqn: &str,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        self.find_current_usages_for_targets_inner(
            CurrentUsageQuery::for_method_family(&[Arc::from(descriptorless_fqn)]),
            || false,
        )
    }

    #[cfg(test)]
    pub(super) fn find_current_usages_with_first_attempt_hook_for_test(
        &self,
        fqn: &str,
        after_first_extraction: impl FnOnce(),
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        let targets = [Arc::from(fqn)];
        let snapshot = self.snapshot_current_usage_scan(CurrentUsageQuery::for_fqns(&targets))?;
        let first = self.resolve_current_usage_snapshot(&snapshot, &|| false)?;
        after_first_extraction();
        if self.current_usage_locations_are_current(&first) {
            Ok(first)
        } else {
            self.find_current_usages(fqn)
        }
    }

    /// Resolve several declaration identities in one coherent workspace scan.
    /// `expected_workspace_revision` lets a higher-level query bind semantic
    /// target expansion and usage extraction to the same graph generation.
    pub fn find_current_usages_for_targets(
        &self,
        fqns: &[Arc<str>],
        expected_workspace_revision: Option<WorkspaceRevision>,
        is_cancelled: impl Fn() -> bool + Sync,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        self.find_current_usages_for_targets_inner(
            CurrentUsageQuery::for_fqns(fqns).at_workspace_revision(expected_workspace_revision),
            is_cancelled,
        )
    }

    /// Resolve several exact semantic identities in one coherent workspace scan.
    pub fn find_current_usages_for_symbol_targets(
        &self,
        targets: &[crate::SymbolTarget],
        expected_workspace_revision: Option<WorkspaceRevision>,
        is_cancelled: impl Fn() -> bool + Sync,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        self.find_current_usages_for_targets_inner(
            CurrentUsageQuery::for_typed_targets(targets)
                .at_workspace_revision(expected_workspace_revision),
            is_cancelled,
        )
    }

    /// Resolve exact semantic identities for a destructive refactoring.
    ///
    /// In addition to ordinary semantic extraction, every matching identifier
    /// in every clean Java source must be accounted for by an indexed
    /// declaration, an exact target/non-target binding, or an explicitly
    /// disappearing source range. Unknown syntax fails closed. Non-Java source
    /// contributes a complete negative proof only when the target spelling is
    /// absent; same-spelled identifiers remain unavailable until those
    /// extractors expose a complete tri-state binding audit.
    pub fn find_current_usages_for_symbol_targets_for_refactoring(
        &self,
        targets: &[crate::SymbolTarget],
        expected_workspace_revision: WorkspaceRevision,
        ignored_identifier_ranges: &[(FileRevisionToken, crate::TextRange)],
        is_cancelled: impl Fn() -> bool,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        self.find_current_usages_for_targets_inner_serial(
            CurrentUsageQuery::for_typed_targets(targets)
                .at_workspace_revision(Some(expected_workspace_revision))
                .with_lexical_audit(ignored_identifier_ranges),
            is_cancelled,
        )
    }

    fn find_current_usages_for_targets_inner(
        &self,
        query: CurrentUsageQuery<'_>,
        is_cancelled: impl Fn() -> bool + Sync,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        let mut last_raced = false;
        for _ in 0..CURRENT_USAGE_QUERY_ATTEMPTS {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let snapshot = self.snapshot_current_usage_scan(query)?;
            if query
                .expected_workspace_revision
                .is_some_and(|expected| expected != snapshot.workspace_revision)
            {
                return Err(CurrentUsageQueryError::ConcurrentModification);
            }
            let resolved = self.resolve_current_usage_snapshot(&snapshot, &is_cancelled);
            match resolved {
                Ok(result) if self.current_usage_locations_are_current(&result) => {
                    return Ok(result);
                }
                Ok(_) => last_raced = true,
                Err(CurrentUsageQueryError::Cancelled) => {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                Err(error) if self.current_usage_snapshot_is_current(&snapshot) => {
                    return Err(error);
                }
                Err(_) => last_raced = true,
            }
        }
        debug_assert!(last_raced);
        Err(CurrentUsageQueryError::ConcurrentModification)
    }

    fn find_current_usages_for_targets_inner_serial(
        &self,
        query: CurrentUsageQuery<'_>,
        is_cancelled: impl Fn() -> bool,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        let mut last_raced = false;
        for _ in 0..CURRENT_USAGE_QUERY_ATTEMPTS {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let snapshot = self.snapshot_current_usage_scan(query)?;
            if query
                .expected_workspace_revision
                .is_some_and(|expected| expected != snapshot.workspace_revision)
            {
                return Err(CurrentUsageQueryError::ConcurrentModification);
            }
            let resolved = self.resolve_current_usage_snapshot_serial(&snapshot, &is_cancelled);
            match resolved {
                Ok(result) if self.current_usage_locations_are_current(&result) => {
                    return Ok(result);
                }
                Ok(_) => last_raced = true,
                Err(CurrentUsageQueryError::Cancelled) => {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                Err(error) if self.current_usage_snapshot_is_current(&snapshot) => {
                    return Err(error);
                }
                Err(_) => last_raced = true,
            }
        }
        debug_assert!(last_raced);
        Err(CurrentUsageQueryError::ConcurrentModification)
    }

    /// Revalidate both positive and negative inputs from an exact usage scan.
    pub fn current_usage_locations_are_current(&self, result: &CurrentUsageLocations) -> bool {
        self.workspace_revisions_are_current(result.workspace_revision, result.scanned_revisions())
            && result
                .resource_proof()
                .is_none_or(|proof| proof.is_current())
    }

    /// Capture a revision-exact negative scan of the on-disk source manifest
    /// and name-bearing resources for higher-level destructive refactorings.
    pub fn capture_refactoring_resource_proof<F>(
        &self,
        target_names: impl IntoIterator<Item = Arc<str>>,
        is_cancelled: &F,
    ) -> std::result::Result<Arc<crate::RefactoringResourceProof>, CurrentUsageQueryError>
    where
        F: Fn() -> bool + ?Sized,
    {
        let (root, workspace_revision, source_revisions, indexed_sources) = {
            let inner = self.inner.read();
            let mut source_revisions = Vec::with_capacity(inner.paths.len());
            let mut indexed_sources = Vec::with_capacity(inner.paths.len());
            for (&file_id, registered) in inner.paths.iter() {
                if inner.external_file_ids.contains(&file_id) {
                    continue;
                }
                let path = registered.display_path();
                let published = inner.published_revisions.get(&file_id).ok_or_else(|| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "{} has no published source revision",
                        path.display()
                    ))
                })?;
                source_revisions.push(published.token(file_id));
                indexed_sources.push(crate::refactoring_resources::RefactoringSourceInput {
                    path: path.to_path_buf(),
                    indexed_disk_hash: (published.origin == SourceRevisionOrigin::Disk)
                        .then_some(published.content_hash),
                });
            }
            (
                inner.root.clone(),
                inner.workspace_revision,
                source_revisions,
                indexed_sources,
            )
        };
        let proof = crate::RefactoringResourceProof::capture(
            &root,
            target_names,
            &indexed_sources,
            is_cancelled,
        )?;
        if !self.workspace_revisions_are_current(workspace_revision, &source_revisions) {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        Ok(Arc::new(proof))
    }

    /// Validate a workspace generation and every exact source input under one
    /// index read. Higher-level result types use this to retain a single final
    /// validation fence across definitions, positive usages, and negative
    /// scanned files.
    pub fn workspace_revisions_are_current(
        &self,
        workspace_revision: WorkspaceRevision,
        file_revisions: &[FileRevisionToken],
    ) -> bool {
        let inner = self.inner.read();
        inner.workspace_bindings_are_current(workspace_revision)
            && file_revisions
                .iter()
                .all(|revision| inner.revision_token(revision.file_id()) == Some(*revision))
    }

    /// Validate a complete reference result without copying the O(workspace)
    /// negative-scan token slice into a temporary vector.
    pub(crate) fn reference_revisions_are_current(
        &self,
        workspace_revision: WorkspaceRevision,
        scanned_revisions: &[FileRevisionToken],
        definitions: &[RevisionedLocation],
    ) -> bool {
        let inner = self.inner.read();
        inner.workspace_bindings_are_current(workspace_revision)
            && scanned_revisions
                .iter()
                .all(|revision| inner.revision_token(revision.file_id()) == Some(*revision))
            && definitions.iter().all(|definition| {
                inner.revision_token(definition.revision.file_id()) == Some(definition.revision)
            })
    }

    fn snapshot_current_usage_scan(
        &self,
        query: CurrentUsageQuery<'_>,
    ) -> std::result::Result<CurrentUsageScanSnapshot, CurrentUsageQueryError> {
        let typed_targets = query.typed_targets();
        let typed_fqns = typed_targets
            .unwrap_or_default()
            .iter()
            .map(|target| Arc::clone(&target.fqn))
            .collect::<Vec<_>>();
        let fqns = match query.targets {
            CurrentUsageQueryTargets::Fqns(fqns) | CurrentUsageQueryTargets::MethodFamily(fqns) => {
                fqns
            }
            CurrentUsageQueryTargets::Typed(_) => &typed_fqns,
        };
        let method_family = query.is_method_family();
        let strict_lexical_audit = query.strict_lexical_audit();
        let inner = self.inner.read();
        let namespaces = if let Some(typed_targets) = typed_targets {
            typed_targets
                .iter()
                .map(|target| Some(target.namespace))
                .collect::<Vec<_>>()
        } else if method_family {
            vec![Some(crate::DeclarationNamespace::Callable); fqns.len()]
        } else {
            let mut namespaces = Vec::with_capacity(fqns.len());
            for fqn in fqns {
                let candidates = symbol_targets_for_fqn(&inner, fqn);
                match candidates.as_slice() {
                    [target] => namespaces.push(Some(target.namespace)),
                    [] => namespaces.push(None),
                    _ => {
                        return Err(CurrentUsageQueryError::Unavailable(format!(
                            "'{fqn}' spans declaration namespaces; use SymbolTarget"
                        )));
                    }
                }
            }
            namespaces
        };
        let targets: Vec<CurrentUsageTarget> = fqns
            .iter()
            .zip(namespaces)
            .map(|(fqn, resolved_namespace)| {
                let (kind, namespace) = if method_family {
                    (
                        CurrentUsageTargetKind::MethodFamily {
                            descriptor_prefix: format!("{fqn}("),
                        },
                        Some(crate::DeclarationNamespace::Callable),
                    )
                } else if let Some(namespace) = resolved_namespace {
                    let kind = if namespace == crate::DeclarationNamespace::Type {
                        let simple = fqn.rsplit('.').next().unwrap_or(fqn.as_ref());
                        CurrentUsageTargetKind::Type {
                            constructor_prefix: format!("{fqn}.{simple}("),
                        }
                    } else if namespace == crate::DeclarationNamespace::Callable {
                        let descriptorless = fqn
                            .split_once('(')
                            .map_or_else(|| Arc::clone(fqn), |(base, _)| Arc::from(base));
                        CurrentUsageTargetKind::MethodOverload {
                            descriptorless,
                            overloads: callable_overloads_for_target_in(&inner, fqn),
                            inherited_descriptorless: if strict_lexical_audit {
                                inherited_method_reference_aliases_in(&inner, fqn)
                            } else {
                                Vec::new()
                            },
                        }
                    } else {
                        CurrentUsageTargetKind::Exact
                    };
                    (kind, Some(namespace))
                } else if let Some((descriptorless, _)) = fqn.split_once('(') {
                    (
                        CurrentUsageTargetKind::MethodOverload {
                            descriptorless: Arc::from(descriptorless),
                            overloads: callable_overloads_for_target_in(&inner, fqn),
                            inherited_descriptorless: if strict_lexical_audit {
                                inherited_method_reference_aliases_in(&inner, fqn)
                            } else {
                                Vec::new()
                            },
                        },
                        Some(crate::DeclarationNamespace::Callable),
                    )
                } else {
                    (CurrentUsageTargetKind::Exact, None)
                };
                CurrentUsageTarget {
                    fqn: Arc::clone(fqn),
                    namespace,
                    kind,
                }
            })
            .collect();
        let target_identifier_names = targets
            .iter()
            .filter_map(CurrentUsageTarget::lexical_identifier)
            .collect::<HashSet<_>>();
        let target_identifier_hashes: Option<Vec<u64>> = targets
            .iter()
            .map(|target| {
                target
                    .lexical_identifier()
                    .and_then(super::core::ascii_identifier_hash)
            })
            .collect();

        let mut inputs = Vec::new();
        for (&file_id, registered) in inner.paths.iter() {
            if inner.external_file_ids.contains(&file_id) {
                continue;
            }
            let file_path = registered.display_path();
            let language = Language::from_path(file_path).unwrap_or(Language::Java);
            // Java source ranges for types, fields, constructors, and methods
            // always contain the declaration's identifier spelling. A sorted
            // per-revision lexical digest therefore proves that a file cannot
            // contribute a result without retaining its imports or reparsing
            // its bytes. Unknown/exotic spellings, Unicode escapes, and other
            // JVM languages deliberately take the conservative scan path.
            if language == Language::Java && !strict_lexical_audit {
                if let (Some(target_hashes), Some(file_hashes)) = (
                    target_identifier_hashes.as_ref(),
                    inner.file_lexical_identifiers.get(&file_id),
                ) {
                    if !target_hashes
                        .iter()
                        .any(|hash| file_hashes.might_contain(*hash))
                    {
                        continue;
                    }
                }
            }
            inputs.push(Self::current_usage_scan_input(
                &inner,
                file_id,
                file_path,
                strict_lexical_audit.then_some(&target_identifier_names),
            )?);
        }
        inputs.sort_unstable_by_key(|input| input.revision.file_id().as_u32());
        Ok(CurrentUsageScanSnapshot {
            workspace_revision: inner.workspace_revision,
            inputs,
            targets,
            strict_lexical_audit,
            resource_root: strict_lexical_audit.then(|| inner.root.clone()),
            ignored_identifier_ranges: query.ignored_identifier_ranges().to_vec(),
        })
    }

    /// Capture one file's exact usage-extraction inputs without cloning every
    /// other workspace file. Indexing treats an unknown extension as Java, so
    /// exact queries must make the identical language choice.
    fn current_usage_scan_input(
        inner: &IndexInner,
        file_id: FileId,
        file_path: &Path,
        strict_target_names: Option<&HashSet<&str>>,
    ) -> std::result::Result<CurrentUsageScanInput, CurrentUsageQueryError> {
        let published = inner.published_revisions.get(&file_id).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "{} has no published source revision",
                file_path.display()
            ))
        })?;
        let revision = published.token(file_id);
        let imports = inner.file_imports.get(&file_id).cloned().ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "{} has no import context",
                file_path.display()
            ))
        })?;
        Ok(CurrentUsageScanInput {
            revision,
            file_path: file_path.to_path_buf(),
            indexed_disk_hash: (published.origin == SourceRevisionOrigin::Disk)
                .then_some(published.content_hash),
            language: Language::from_path(file_path).unwrap_or(Language::Java),
            imports,
            imported_fqns: inner
                .file_imported_fqns
                .get(&file_id)
                .into_iter()
                .flatten()
                .map(|fqn| Arc::from(fqn.as_str()))
                .collect(),
            known_declaration_identifiers: strict_target_names.map_or_else(Vec::new, |names| {
                inner
                    .file_symbols
                    .get(&file_id)
                    .into_iter()
                    .flatten()
                    .filter(|symbol| names.contains(symbol.simple_name.as_ref()))
                    .map(|symbol| (Arc::clone(&symbol.simple_name), symbol.range))
                    .collect()
            }),
        })
    }

    fn snapshot_current_usage_file(
        &self,
        file_id: FileId,
    ) -> std::result::Result<(WorkspaceRevision, CurrentUsageScanInput), CurrentUsageQueryError>
    {
        let inner = self.inner.read();
        if inner.external_file_ids.contains(&file_id) {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "file {:?} is not a workspace source",
                file_id
            )));
        }
        let file_path = inner.file_path(file_id).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "file {:?} is not a current workspace source",
                file_id
            ))
        })?;
        let input = Self::current_usage_scan_input(&inner, file_id, file_path, None)?;
        Ok((inner.workspace_revision, input))
    }

    fn current_usage_file_is_current(
        &self,
        workspace_revision: WorkspaceRevision,
        input: &CurrentUsageScanInput,
    ) -> bool {
        let inner = self.inner.read();
        inner.workspace_bindings_are_current(workspace_revision)
            && inner.revision_token(input.revision.file_id()) == Some(input.revision)
            && inner.file_path(input.revision.file_id()) == Some(input.file_path.as_path())
    }

    fn resolve_current_usage_snapshot(
        &self,
        snapshot: &CurrentUsageScanSnapshot,
        is_cancelled: &(impl Fn() -> bool + Sync),
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        let resolved = if snapshot.strict_lexical_audit {
            snapshot
                .inputs
                .iter()
                .map(|input| self.resolve_current_usage_input(snapshot, input, is_cancelled))
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            snapshot
                .inputs
                .par_iter()
                .map(|input| self.resolve_current_usage_input(snapshot, input, is_cancelled))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        self.finish_current_usage_snapshot(snapshot, resolved, is_cancelled)
    }

    fn resolve_current_usage_snapshot_serial(
        &self,
        snapshot: &CurrentUsageScanSnapshot,
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        let resolved = snapshot
            .inputs
            .iter()
            .map(|input| self.resolve_current_usage_input(snapshot, input, is_cancelled))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.finish_current_usage_snapshot(snapshot, resolved, is_cancelled)
    }

    fn finish_current_usage_snapshot(
        &self,
        snapshot: &CurrentUsageScanSnapshot,
        resolved: Vec<Option<RevisionedFileLocations>>,
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<CurrentUsageLocations, CurrentUsageQueryError> {
        let files = resolved.into_iter().flatten().collect();
        let scanned_revisions = snapshot
            .inputs
            .iter()
            .map(|input| input.revision)
            .collect::<Vec<_>>();
        let resource_proof = snapshot
            .resource_root
            .as_ref()
            .map(|root| {
                crate::RefactoringResourceProof::capture(
                    root,
                    snapshot
                        .targets
                        .iter()
                        .filter_map(CurrentUsageTarget::lexical_identifier)
                        .map(Arc::<str>::from),
                    &snapshot
                        .inputs
                        .iter()
                        .map(
                            |input| crate::refactoring_resources::RefactoringSourceInput {
                                path: input.file_path.clone(),
                                indexed_disk_hash: input.indexed_disk_hash,
                            },
                        )
                        .collect::<Vec<_>>(),
                    is_cancelled,
                )
                .map(Arc::new)
            })
            .transpose()?;
        Ok(CurrentUsageLocations {
            workspace_revision: snapshot.workspace_revision,
            files,
            scanned_revisions: Arc::from(scanned_revisions),
            resource_proof,
        })
    }

    fn resolve_current_usage_input(
        &self,
        snapshot: &CurrentUsageScanSnapshot,
        input: &CurrentUsageScanInput,
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<Option<RevisionedFileLocations>, CurrentUsageQueryError> {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        let indexed = self
            .get_indexed_revision_content(input.revision)
            .ok_or_else(|| {
                CurrentUsageQueryError::Unavailable(format!(
                    "exact indexed bytes for {} are unavailable",
                    input.file_path.display()
                ))
            })?;
        if indexed.file_path != input.file_path {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "path changed while reading {}",
                input.file_path.display()
            )));
        }
        let usages: Arc<[crate::usage_extractor::Usage]> = if snapshot.strict_lexical_audit {
            if input.language == Language::Java {
                let root =
                    self.parse_strict_java_refactoring_input(input, &indexed, is_cancelled)?;
                let mut usages = crate::usage_extractor::UsageExtractor::extract_from_syntax(
                    &root,
                    input.revision.file_id(),
                    &input.imports,
                    self,
                )
                .map_err(|error| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "failed to extract {}: {error:#}",
                        input.file_path.display()
                    ))
                })?;
                // Imports and Javadoc references spell types too; a rename
                // must rewrite them, and the lexical audit must account them.
                usages.extend(
                    crate::usage_extractor::UsageExtractor::extract_import_and_doc_type_usages(
                        &root,
                        input.revision.file_id(),
                        &input.imports,
                        self,
                    ),
                );
                if is_cancelled() {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                self.audit_current_identifier_bindings(
                    snapshot,
                    input,
                    &indexed,
                    &root,
                    &usages,
                    is_cancelled,
                )?;
                usages.into()
            } else {
                let mut usages = self.extract_current_usages_uncached(input, &indexed)?;
                if input.language == Language::Kotlin {
                    // Kotlin imports spell types too; account and rewrite them.
                    let tokens = hyperion_kotlin::Lexer::new(&indexed.content).collect::<Vec<_>>();
                    let (green, errors) = hyperion_kotlin::Parser::parse(tokens);
                    if errors.is_empty() {
                        let root = hyperion_kotlin::SyntaxNode::new_root(green);
                        usages.extend(
                            crate::usage_extractor::UsageExtractor::extract_kotlin_import_type_usages(
                                &root,
                                input.revision.file_id(),
                                &input.imports,
                                self,
                            ),
                        );
                    }
                }
                if is_cancelled() {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                self.audit_non_java_identifier_bindings(
                    snapshot,
                    input,
                    &indexed,
                    &usages,
                    is_cancelled,
                )?;
                usages.into()
            }
        } else {
            self.extract_current_usages(snapshot.workspace_revision, input, &indexed)?
        };

        let mut locations = Vec::new();
        for usage in usages.iter() {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let classification = snapshot
                .targets
                .iter()
                .map(|target| target.classify(&usage.fqn, usage.location.kind))
                .max_by_key(|classification| match classification {
                    CurrentUsageMatch::No => 0,
                    CurrentUsageMatch::AmbiguousMethod => 1,
                    CurrentUsageMatch::Exact => 2,
                })
                .unwrap_or(CurrentUsageMatch::No);
            match classification {
                CurrentUsageMatch::No => continue,
                CurrentUsageMatch::Exact => {}
                CurrentUsageMatch::AmbiguousMethod => {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "{} contains an overload-ambiguous usage of {}",
                        input.file_path.display(),
                        usage.fqn
                    )));
                }
            }
            let mut location = usage.location.clone();
            if location.file_id != input.revision.file_id()
                || location.range.start > location.range.end
                || location.range.end as usize > indexed.content.len()
            {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "extractor produced an invalid range for {}",
                    input.file_path.display()
                )));
            }
            location.set_source_revision(input.revision.revision_id(), false);
            locations.push(location);
        }
        Ok((!locations.is_empty()).then_some(RevisionedFileLocations {
            revision: input.revision,
            locations,
        }))
    }

    fn current_usage_snapshot_is_current(&self, snapshot: &CurrentUsageScanSnapshot) -> bool {
        let inner = self.inner.read();
        inner.workspace_bindings_are_current(snapshot.workspace_revision)
            && snapshot.inputs.iter().all(|input| {
                inner.revision_token(input.revision.file_id()) == Some(input.revision)
                    && inner.file_path(input.revision.file_id()) == Some(input.file_path.as_path())
            })
    }

    fn extract_current_usages(
        &self,
        workspace_revision: WorkspaceRevision,
        input: &CurrentUsageScanInput,
        indexed: &crate::IndexedContent,
    ) -> std::result::Result<Arc<[crate::usage_extractor::Usage]>, CurrentUsageQueryError> {
        let key = (workspace_revision, input.revision);
        if let Some(usages) = self.current_usage_cache.lock().get(&key) {
            return Ok(usages);
        }
        let usages: Arc<[_]> = self.extract_current_usages_uncached(input, indexed)?.into();
        if self.current_workspace_revision() == workspace_revision {
            self.current_usage_cache
                .lock()
                .put(key, Arc::clone(&usages));
        }
        Ok(usages)
    }

    fn extract_current_usages_uncached(
        &self,
        input: &CurrentUsageScanInput,
        indexed: &crate::IndexedContent,
    ) -> std::result::Result<Vec<crate::usage_extractor::Usage>, CurrentUsageQueryError> {
        match input.language {
            Language::Java => self
                .cached_java_parse(input.revision.file_id(), &indexed.content)
                .map_or_else(
                    || {
                        crate::usage_extractor::UsageExtractor::extract(
                            &indexed.content,
                            input.revision.file_id(),
                            &input.imports,
                            self,
                        )
                    },
                    |green| {
                        let root = hyperion_parser::SyntaxNode::new_root(green.as_ref().clone());
                        crate::usage_extractor::UsageExtractor::extract_from_syntax(
                            &root,
                            input.revision.file_id(),
                            &input.imports,
                            self,
                        )
                    },
                ),
            Language::Kotlin => crate::kotlin_usage_extractor::KotlinUsageExtractor::extract(
                &indexed.content,
                input.revision.file_id(),
                &input.imports,
                self,
            ),
            Language::Groovy => crate::groovy_usage_extractor::GroovyUsageExtractor::extract(
                &indexed.content,
                input.revision.file_id(),
                &input.imports,
                self,
            ),
            Language::Scala => crate::scala_usage_extractor::ScalaUsageExtractor::extract(
                &indexed.content,
                input.revision.file_id(),
                &input.imports,
                self,
            ),
        }
        .map_err(|error| {
            CurrentUsageQueryError::Unavailable(format!(
                "failed to extract {}: {error:#}",
                input.file_path.display()
            ))
        })
    }

    /// Re-resolve one file's semantic dependency edges from its exact bytes.
    pub fn get_current_file_dependency_fqns(
        &self,
        file_id: FileId,
    ) -> std::result::Result<CurrentFileDependencies, CurrentUsageQueryError> {
        for _ in 0..CURRENT_USAGE_QUERY_ATTEMPTS {
            let (workspace_revision, input) = self.snapshot_current_usage_file(file_id)?;
            let Some(indexed) = self.get_indexed_revision_content(input.revision) else {
                if self.current_usage_file_is_current(workspace_revision, &input) {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "exact indexed bytes for {} are unavailable",
                        input.file_path.display()
                    )));
                }
                continue;
            };
            let usages = match self.extract_current_usages(workspace_revision, &input, &indexed) {
                Ok(usages) => usages,
                Err(error) if self.current_usage_file_is_current(workspace_revision, &input) => {
                    return Err(error);
                }
                Err(_) => continue,
            };
            let mut fqns = usages
                .iter()
                .map(|usage| Arc::<str>::from(usage.fqn.as_str()))
                .chain(input.imported_fqns.iter().cloned())
                .collect::<Vec<_>>();
            fqns.sort_unstable();
            fqns.dedup();
            let result = CurrentFileDependencies {
                workspace_revision,
                revision: input.revision,
                fqns,
            };
            if self.current_usage_file_is_current(result.workspace_revision, &input) {
                return Ok(result);
            }
        }
        Err(CurrentUsageQueryError::ConcurrentModification)
    }
}

#[cfg(test)]
mod tests;
