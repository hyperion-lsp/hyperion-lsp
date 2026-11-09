// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol lookup and query operations
//!
//! This module handles:
//! - Symbol lookups (find_symbol, get_symbol, find_usages)
//! - Method resolution (find_methods_by_name, find_method_by_args)
//! - Inheritance queries (find_methods_with_inheritance)
//! - Name-based queries (find_by_simple_name, get_fqns_by_simple_prefix)
//! - File path queries (get_file_id, get_file_path)

use super::refactoring::{method_definitions_by_name_in, overriding_declarations_in};
use super::*;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// Immutable source ordering for one requesting module/source set.
///
/// The roots come from a published build classpath. Workspace candidates
/// outside these roots cannot become an owner merely because their path sorts
/// first globally. A selected owner's members are always read from its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceScope {
    roots: Arc<[PathLookupKey]>,
    archives: Arc<[PathLookupKey]>,
}

impl SourceScope {
    #[must_use]
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self::with_external_archives(roots, std::iter::empty())
    }

    #[must_use]
    pub fn with_external_archives(
        roots: impl IntoIterator<Item = PathBuf>,
        archives: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        let mut ordered = Vec::new();
        for root in roots {
            let key = path_lookup_key(&root);
            if !ordered.contains(&key) {
                ordered.push(key);
            }
        }
        let mut external = Vec::new();
        for archive in archives {
            let key = path_lookup_key(&archive);
            if !external.contains(&key) {
                external.push(key);
            }
        }
        Self {
            roots: ordered.into(),
            archives: external.into(),
        }
    }

    fn rank(&self, inner: &IndexInner, file_id: FileId) -> Option<usize> {
        if let Some(source) = inner.external_file_id_to_source.get(&file_id) {
            let identity = source.key();
            return match identity.virtual_name() {
                Some("jdk") => Some(self.roots.len() + self.archives.len()),
                Some(_) => None,
                None => self
                    .archives
                    .iter()
                    .position(|archive| identity.archive_key() == Some(archive))
                    .map(|rank| self.roots.len() + rank)
                    .or_else(|| {
                        inner
                            .external_source_precedence
                            .get(&file_id)
                            .filter(|(_, _, owner)| owner.as_ref() == "jdk-classpath")
                            .map(|_| self.roots.len() + self.archives.len())
                    }),
            };
        }
        let key = inner.file_path_key(file_id)?;
        self.roots
            .iter()
            .position(|root| key.starts_with(root.as_path()))
    }
}

/// One read-locked owner election shared by member, hierarchy, and generic
/// projection queries. A scoped type and all its members derive from the same
/// source file; the unscoped form retains the established global election.
pub(crate) struct OwnerElection<'a> {
    pub(super) inner: &'a IndexInner,
    pub(super) scope: Option<&'a SourceScope>,
}

impl<'a> OwnerElection<'a> {
    pub(super) fn new(inner: &'a IndexInner, scope: Option<&'a SourceScope>) -> Self {
        Self { inner, scope }
    }

    pub(super) fn type_definition(&self, type_fqn: &str) -> Option<ResolvedDefinition> {
        let target = crate::SymbolTarget::new(type_fqn, crate::DeclarationNamespace::Type);
        self.scope.map_or_else(
            || {
                let definition = find_symbol_definition_in(self.inner, &target)?;
                self.inner
                    .symbols
                    .get(type_fqn)
                    .is_some_and(|active| Arc::ptr_eq(active, &definition.symbol))
                    .then_some(definition)
            },
            |scope| find_symbol_definition_in_scope(self.inner, &target, scope),
        )
    }

    pub(super) fn symbol(&self, fqn: &str) -> Option<Arc<crate::Symbol>> {
        self.scope.map_or_else(
            || self.inner.symbols.get(fqn).cloned(),
            |scope| {
                find_any_symbol_definition_in_scope(self.inner, fqn, scope)
                    .map(|definition| definition.symbol)
            },
        )
    }

    pub(super) fn member_definitions(
        &self,
        owner_fqn: &str,
    ) -> Option<Arc<Vec<ResolvedDefinition>>> {
        self.scope.map_or_else(
            || selected_owner_member_definitions_shared_in(self.inner, owner_fqn),
            |_| {
                let owner = self.type_definition(owner_fqn)?;
                selected_owner_member_definitions_shared_from_owner(self.inner, owner_fqn, owner)
            },
        )
    }
}

/// Compute the exclusive upper bound for a BTreeMap prefix range scan.
/// Given prefix "Foo.bar(", returns "Foo.bar)" — the next string after all
/// strings starting with the prefix. Returns None if the prefix ends with
/// the maximum byte value (extremely unlikely for FQN strings).
fn prefix_upper_bound(prefix: &str) -> Option<String> {
    let mut upper = prefix.as_bytes().to_vec();
    // Increment the last byte to get the exclusive upper bound
    // '(' is 0x28, ')' is 0x29 — this works for method prefix scans
    if let Some(last) = upper.last_mut() {
        if *last == u8::MAX {
            return None;
        }
        *last += 1;
        String::from_utf8(upper).ok()
    } else {
        None
    }
}

/// Iterate over all entries in a BTreeMap whose keys start with the given prefix.
/// Uses O(log N + K) range scan instead of O(N) full iteration.
pub(super) fn btree_prefix_scan<'a, V>(
    map: &'a BTreeMap<Arc<str>, V>,
    prefix: &str,
) -> Box<dyn Iterator<Item = (&'a Arc<str>, &'a V)> + 'a> {
    let prefix_key: Arc<str> = Arc::from(prefix);
    if let Some(upper) = prefix_upper_bound(prefix) {
        let upper_key: Arc<str> = Arc::from(upper.as_str());
        Box::new(map.range::<Arc<str>, _>(&prefix_key..&upper_key))
    } else {
        Box::new(map.range::<Arc<str>, _>(&prefix_key..))
    }
}

fn stamp_source_revision(inner: &IndexInner, location: &SymbolLocation) -> SymbolLocation {
    let mut location = location.clone();
    let external = inner.external_file_ids.contains(&location.file_id);
    let revision = if external {
        inner
            .published_external_source_generations
            .get(&location.file_id)
            .copied()
    } else {
        inner
            .revision_token(location.file_id)
            .map(FileRevisionToken::revision_id)
    };
    location.source_revision_id = None;
    if let Some(revision) = revision {
        location.set_source_revision(revision, external);
    }
    location
}

fn location_for_definition(
    inner: &IndexInner,
    file_id: FileId,
    symbol: &crate::Symbol,
    revision: Option<FileRevisionToken>,
) -> SymbolLocation {
    let external = revision.is_none();
    let revision_id = revision.map(FileRevisionToken::revision_id).or_else(|| {
        inner
            .published_external_source_generations
            .get(&file_id)
            .copied()
    });
    let mut location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: symbol.range,
        kind: symbol.kind,
        name: Arc::clone(&symbol.simple_name),
        fqn: Arc::clone(&symbol.fqn),
        container_name: symbol.container.clone(),
        documentation: symbol.documentation.clone(),
    };
    if let Some(revision_id) = revision_id {
        location.set_source_revision(revision_id, external);
    }
    location
}

/// Select one declaration namespace without consulting the legacy elected-FQN
/// maps. Metadata, range, and source authority are captured under the same
/// index read so equal textual FQNs cannot cross-wire them.
pub(super) fn find_symbol_definition_in(
    inner: &IndexInner,
    target: &crate::SymbolTarget,
) -> Option<ResolvedDefinition> {
    find_symbol_definition_excluding_in(inner, target, |_, _| false)
}

fn find_symbol_definition_excluding_in(
    inner: &IndexInner,
    target: &crate::SymbolTarget,
    excluded: impl Fn(FileId, &crate::Symbol) -> bool,
) -> Option<ResolvedDefinition> {
    let (file_id, symbol) = super::core::preferred_symbol_candidate_matching_excluding(
        inner,
        &target.fqn,
        |symbol| target.namespace.accepts(symbol.kind),
        excluded,
    )?;
    let is_external = inner.external_file_ids.contains(&file_id);
    let revision = if is_external {
        None
    } else {
        Some(inner.revision_token(file_id)?)
    };
    let source = revision.map_or(
        DefinitionSourceAuthority::External,
        DefinitionSourceAuthority::Workspace,
    );
    Some(ResolvedDefinition {
        target: target.clone(),
        location: location_for_definition(inner, file_id, &symbol, revision),
        symbol,
        workspace_revision: inner.workspace_revision,
        source,
    })
}

fn find_symbol_definition_in_source(
    inner: &IndexInner,
    file_id: FileId,
    target: &crate::SymbolTarget,
) -> Option<ResolvedDefinition> {
    let (_, symbol) =
        super::core::candidate_from_source_matching(inner, file_id, &target.fqn, |symbol| {
            target.namespace.accepts(symbol.kind)
        })?;
    let is_external = inner.external_file_ids.contains(&file_id);
    let revision = if is_external {
        None
    } else {
        Some(inner.revision_token(file_id)?)
    };
    Some(ResolvedDefinition {
        target: target.clone(),
        location: location_for_definition(inner, file_id, &symbol, revision),
        symbol,
        workspace_revision: inner.workspace_revision,
        source: revision.map_or(
            DefinitionSourceAuthority::External,
            DefinitionSourceAuthority::Workspace,
        ),
    })
}

fn find_symbol_definition_in_scope(
    inner: &IndexInner,
    target: &crate::SymbolTarget,
    scope: &SourceScope,
) -> Option<ResolvedDefinition> {
    let mut candidates = inner
        .symbol_definition_files
        .get(target.fqn.as_ref())?
        .iter()
        .filter_map(|candidate| {
            let rank = scope.rank(inner, candidate.file_id)?;
            let definition = find_symbol_definition_in_source(inner, candidate.file_id, target)?;
            Some((rank, definition))
        });
    let (rank, selected) = candidates.next()?;
    let mut best = (rank, selected);
    let mut ambiguous = false;
    for candidate in candidates {
        if candidate.0 < best.0 {
            best = candidate;
            ambiguous = false;
        } else if candidate.0 == best.0 && candidate.1.location.file_id != best.1.location.file_id {
            // Two declarations in one source root have no classpath order.
            ambiguous = true;
        }
    }
    if ambiguous {
        return None;
    }
    if target.namespace == crate::DeclarationNamespace::Type {
        if let Some(container) = best.1.symbol.container.as_deref() {
            let owner = crate::SymbolTarget::new(container, crate::DeclarationNamespace::Type);
            let owner = find_symbol_definition_in_scope(inner, &owner, scope)?;
            if owner.location.file_id != best.1.location.file_id {
                return None;
            }
        }
    }
    Some(best.1)
}

fn find_symbol_definition_coherent_in_scope(
    inner: &IndexInner,
    target: &crate::SymbolTarget,
    scope: &SourceScope,
) -> Option<ResolvedDefinition> {
    let Some(owner_fqn) = nominal_owner_for_target(target) else {
        return find_symbol_definition_in_scope(inner, target, scope);
    };
    let owner_target = crate::SymbolTarget::new(owner_fqn, crate::DeclarationNamespace::Type);
    let Some(owner) = find_symbol_definition_in_scope(inner, &owner_target, scope) else {
        return find_symbol_definition_in_scope(inner, target, scope);
    };
    let mut matches =
        selected_owner_member_definitions_from_owner(inner, owner_fqn, owner, |_, _| false)?
            .into_iter()
            .filter(|definition| target_matches_selected_member(target, definition));
    let selected = matches.next()?;
    matches.next().is_none().then_some(selected)
}

fn find_any_symbol_definition_in_scope(
    inner: &IndexInner,
    fqn: &str,
    scope: &SourceScope,
) -> Option<ResolvedDefinition> {
    [
        crate::DeclarationNamespace::Type,
        crate::DeclarationNamespace::Callable,
        crate::DeclarationNamespace::Value,
    ]
    .into_iter()
    .filter_map(|namespace| {
        let target = crate::SymbolTarget::new(fqn, namespace);
        find_symbol_definition_coherent_in_scope(inner, &target, scope)
    })
    .next()
}

/// Return the declarations owned by the source that supplied the active type.
/// Member FQNs are not independently elected: doing so can splice a shadowed
/// classpath or workspace definition into a different owner shape.
pub(crate) fn selected_owner_member_definitions_in(
    inner: &IndexInner,
    owner_fqn: &str,
) -> Option<Vec<ResolvedDefinition>> {
    selected_owner_member_definitions_shared_in(inner, owner_fqn)
        .map(|definitions| definitions.as_ref().clone())
}

pub(crate) fn selected_owner_member_definitions_excluding_in(
    inner: &IndexInner,
    owner_fqn: &str,
    excluded: impl Fn(&crate::SymbolTarget, FileId) -> bool,
) -> Option<Vec<ResolvedDefinition>> {
    let owner_target = crate::SymbolTarget::new(owner_fqn, crate::DeclarationNamespace::Type);
    let owner = find_symbol_definition_excluding_in(inner, &owner_target, |file_id, _| {
        excluded(&owner_target, file_id)
    })?;
    selected_owner_member_definitions_from_owner(inner, owner_fqn, owner, excluded)
}

fn selected_owner_member_definitions_from_owner(
    inner: &IndexInner,
    owner_fqn: &str,
    owner: ResolvedDefinition,
    excluded: impl Fn(&crate::SymbolTarget, FileId) -> bool,
) -> Option<Vec<ResolvedDefinition>> {
    let file_id = owner.location.file_id;
    let mut targets: HashSet<_> = inner
        .file_symbols
        .get(&file_id)?
        .iter()
        .filter(|symbol| symbol.container.as_deref() == Some(owner_fqn))
        .map(|symbol| crate::SymbolTarget::for_symbol(symbol))
        .collect();
    let mut definitions: Vec<_> = targets
        .drain()
        .filter(|target| !excluded(target, file_id))
        .filter_map(|target| find_symbol_definition_in_source(inner, file_id, &target))
        .filter(|definition| definition.symbol.container.as_deref() == Some(owner_fqn))
        .collect();
    definitions.sort_unstable_by(|left, right| {
        left.target
            .fqn
            .cmp(&right.target.fqn)
            .then((left.target.namespace as u8).cmp(&(right.target.namespace as u8)))
            .then(left.symbol.range.start.cmp(&right.symbol.range.start))
    });
    Some(definitions)
}

/// [`selected_owner_member_definitions_from_owner`] without exclusions,
/// memoized per owner for one published revision of its source file. Every
/// member lookup of a call site used to rebuild the whole list.
pub(crate) fn selected_owner_member_definitions_shared_from_owner(
    inner: &IndexInner,
    owner_fqn: &str,
    owner: ResolvedDefinition,
) -> Option<Arc<Vec<ResolvedDefinition>>> {
    const MAX_CACHED_OWNERS: usize = 2048;
    let file_id = owner.location.file_id;
    let revision_id = inner
        .published_revisions
        .get(&file_id)
        .map(|published| published.id);
    if let Some(revision_id) = revision_id {
        if let Some(entry) = inner.member_definitions_cache.lock().get(owner_fqn) {
            if entry.file_id == file_id
                && entry.revision_id == revision_id
                && entry.workspace_revision == inner.workspace_revision
            {
                return Some(Arc::clone(&entry.definitions));
            }
        }
    }
    let definitions = Arc::new(selected_owner_member_definitions_from_owner(
        inner,
        owner_fqn,
        owner,
        |_, _| false,
    )?);
    if let Some(revision_id) = revision_id {
        let mut cache = inner.member_definitions_cache.lock();
        if cache.len() >= MAX_CACHED_OWNERS {
            cache.clear();
        }
        cache.insert(
            owner_fqn.to_owned(),
            super::MemberDefinitionsEntry {
                file_id,
                revision_id,
                workspace_revision: inner.workspace_revision,
                definitions: Arc::clone(&definitions),
            },
        );
    }
    Some(definitions)
}

pub(crate) fn selected_owner_member_definitions_shared_in(
    inner: &IndexInner,
    owner_fqn: &str,
) -> Option<Arc<Vec<ResolvedDefinition>>> {
    let owner_target = crate::SymbolTarget::new(owner_fqn, crate::DeclarationNamespace::Type);
    let owner = find_symbol_definition_excluding_in(inner, &owner_target, |_, _| false)?;
    selected_owner_member_definitions_shared_from_owner(inner, owner_fqn, owner)
}

impl WorkspaceIndex {
    /// Resolve one type using a caller's ordered source roots. The chosen
    /// declaration, file, and revision come from a single index read.
    pub fn find_type_definition_in_scope(
        &self,
        type_fqn: &str,
        scope: &SourceScope,
    ) -> Option<ResolvedDefinition> {
        let inner = self.inner.read();
        let target = crate::SymbolTarget::new(type_fqn, crate::DeclarationNamespace::Type);
        find_symbol_definition_in_scope(&inner, &target, scope)
    }

    /// Read direct members from exactly the file elected as the type owner.
    pub fn find_member_definitions_in_scope(
        &self,
        type_fqn: &str,
        scope: &SourceScope,
    ) -> Option<Vec<ResolvedDefinition>> {
        let inner = self.inner.read();
        let target = crate::SymbolTarget::new(type_fqn, crate::DeclarationNamespace::Type);
        let owner = find_symbol_definition_in_scope(&inner, &target, scope)?;
        selected_owner_member_definitions_from_owner(&inner, type_fqn, owner, |_, _| false)
    }
}

fn nominal_owner_for_target(target: &crate::SymbolTarget) -> Option<&str> {
    if !matches!(
        target.namespace,
        crate::DeclarationNamespace::Constructor
            | crate::DeclarationNamespace::Callable
            | crate::DeclarationNamespace::Value
    ) {
        return None;
    }
    let descriptorless = target
        .fqn
        .split_once('(')
        .map_or(target.fqn.as_ref(), |(base, _)| base);
    descriptorless.rsplit_once('.').map(|(owner, _)| owner)
}

fn target_matches_selected_member(
    target: &crate::SymbolTarget,
    definition: &ResolvedDefinition,
) -> bool {
    if definition.target.namespace != target.namespace {
        return false;
    }
    if definition.target.fqn == target.fqn {
        return true;
    }
    target.namespace == crate::DeclarationNamespace::Callable
        && !target.fqn.contains('(')
        && definition
            .target
            .fqn
            .strip_prefix(target.fqn.as_ref())
            .is_some_and(|suffix| suffix.starts_with('('))
}

/// Resolve members from the same source authority that supplied their active
/// nominal owner. Equal member FQNs from shadowed dependencies must never be
/// spliced into a different owner shape. Package-owned JVM declarations retain
/// the ordinary source-precedence path because their textual prefix is not a
/// nominal type.
pub(super) fn find_symbol_definition_coherent_in(
    inner: &IndexInner,
    target: &crate::SymbolTarget,
) -> Option<ResolvedDefinition> {
    find_symbol_definition_coherent_excluding_in(inner, target, |_, _| false)
}

pub(crate) fn find_symbol_definition_coherent_excluding_in(
    inner: &IndexInner,
    target: &crate::SymbolTarget,
    excluded: impl Fn(&crate::SymbolTarget, FileId) -> bool,
) -> Option<ResolvedDefinition> {
    let Some(owner_fqn) = nominal_owner_for_target(target) else {
        return find_symbol_definition_excluding_in(inner, target, |file_id, _| {
            excluded(target, file_id)
        });
    };
    let owner_target = crate::SymbolTarget::new(owner_fqn, crate::DeclarationNamespace::Type);
    if find_symbol_definition_excluding_in(inner, &owner_target, |file_id, _| {
        excluded(&owner_target, file_id)
    })
    .is_none()
    {
        return find_symbol_definition_excluding_in(inner, target, |file_id, _| {
            excluded(target, file_id)
        });
    }
    let mut matches = selected_owner_member_definitions_excluding_in(inner, owner_fqn, &excluded)?
        .into_iter()
        .filter(|definition| target_matches_selected_member(target, definition));
    let definition = matches.next()?;
    matches.next().is_none().then_some(definition)
}

pub(crate) fn find_value_definition_in_file_at_range_in(
    inner: &IndexInner,
    file_id: FileId,
    simple_name: &str,
    start: u32,
    end: u32,
) -> Option<Arc<crate::Symbol>> {
    let mut candidates = inner.file_symbols.get(&file_id)?.iter().filter(|symbol| {
        if symbol.simple_name.as_ref() != simple_name
            || symbol.range.start != start
            || symbol.range.end != end
            || !crate::DeclarationNamespace::Value.accepts(symbol.kind)
        {
            return false;
        }
        let target =
            crate::SymbolTarget::new(Arc::clone(&symbol.fqn), crate::DeclarationNamespace::Value);
        find_symbol_definition_coherent_in(inner, &target).is_some_and(|definition| {
            definition.location.file_id == file_id && Arc::ptr_eq(&definition.symbol, symbol)
        })
    });
    let candidate = candidates.next().cloned();
    candidates.next().is_none().then_some(candidate).flatten()
}

fn find_symbol_in(inner: &IndexInner, fqn: &str) -> Option<SymbolLocation> {
    if let Some(location) = inner.symbol_locations.get(fqn) {
        if !inner.external_file_ids.contains(&location.file_id) {
            return Some(stamp_source_revision(inner, location));
        }
        if !fqn.contains('(') {
            let prefix = format!("{fqn}(");
            for (_, workspace_location) in btree_prefix_scan(&inner.symbol_locations, &prefix) {
                if !inner
                    .external_file_ids
                    .contains(&workspace_location.file_id)
                {
                    return Some(stamp_source_revision(inner, workspace_location));
                }
            }
        }
        return Some(stamp_source_revision(inner, location));
    }

    if let Some(paren_index) = fqn.find('(') {
        let base_fqn = &fqn[..paren_index];
        if let Some(location) = inner.symbol_locations.get(base_fqn) {
            return Some(stamp_source_revision(inner, location));
        }
        let prefix = format!("{base_fqn}(");
        let mut external_match = None;
        for (_, location) in btree_prefix_scan(&inner.symbol_locations, &prefix) {
            if !inner.external_file_ids.contains(&location.file_id) {
                return Some(stamp_source_revision(inner, location));
            }
            external_match.get_or_insert_with(|| stamp_source_revision(inner, location));
        }
        external_match
    } else {
        let prefix = format!("{fqn}(");
        let mut external_match = None;
        for (_, location) in btree_prefix_scan(&inner.symbol_locations, &prefix) {
            if !inner.external_file_ids.contains(&location.file_id) {
                return Some(stamp_source_revision(inner, location));
            }
            external_match.get_or_insert_with(|| stamp_source_revision(inner, location));
        }
        external_match
    }
}

fn find_usages_in(inner: &IndexInner, fqn: &str) -> Vec<SymbolLocation> {
    if let Some(paren_index) = fqn.find('(') {
        if let Some(usages) = inner.symbol_usages.get(fqn) {
            return usages.clone();
        }
        let base_fqn = &fqn[..paren_index];
        if let Some(usages) = inner.symbol_usages.get(base_fqn) {
            return usages.clone();
        }
        let prefix = format!("{base_fqn}(");
        let usages = btree_prefix_scan(&inner.symbol_usages, &prefix)
            .flat_map(|(_, usages)| usages.iter().cloned())
            .collect::<Vec<_>>();
        if !usages.is_empty() {
            return usages;
        }
    } else {
        let mut usages = inner.symbol_usages.get(fqn).cloned().unwrap_or_default();
        let prefix = format!("{fqn}(");
        usages.extend(
            btree_prefix_scan(&inner.symbol_usages, &prefix)
                .flat_map(|(_, usages)| usages.iter().cloned()),
        );
        if !usages.is_empty() {
            return usages;
        }
    }
    Vec::new()
}

fn group_revisioned_locations(
    inner: &IndexInner,
    locations: Vec<SymbolLocation>,
) -> RevisionedLocations {
    let mut files = Vec::<RevisionedFileLocations>::new();
    let mut group_by_file = HashMap::<FileId, usize>::new();
    for location in locations {
        let file_id = location.file_id;
        let Some(revision) = inner.revision_token(file_id) else {
            // External bytecode entries have no indexed source publication.
            continue;
        };
        let group_index = *group_by_file.entry(file_id).or_insert_with(|| {
            files.push(RevisionedFileLocations {
                revision,
                locations: Vec::new(),
            });
            files.len() - 1
        });
        files[group_index].locations.push(location);
    }
    RevisionedLocations { files }
}

fn search_locations_in(
    inner: &IndexInner,
    query: &str,
    is_cancelled: &impl Fn() -> bool,
) -> Vec<SymbolLocation> {
    const RESULT_LIMIT: usize = 100;
    let query_lower = query.to_lowercase();
    let mut workspace_results = Vec::new();
    let mut external_results = Vec::new();

    for (lower_fqn, fqns) in &inner.fqn_lower_index {
        if is_cancelled() {
            return Vec::new();
        }
        if !lower_fqn.contains(&query_lower) {
            continue;
        }
        for fqn in fqns {
            let Some(location) = inner.symbol_locations.get(fqn) else {
                continue;
            };
            if inner.external_file_ids.contains(&location.file_id) {
                if external_results.len() < RESULT_LIMIT {
                    external_results.push(stamp_source_revision(inner, location));
                }
            } else {
                workspace_results.push(stamp_source_revision(inner, location));
                if workspace_results.len() == RESULT_LIMIT {
                    return workspace_results;
                }
            }
        }
    }

    let remaining = RESULT_LIMIT - workspace_results.len();
    workspace_results.extend(external_results.into_iter().take(remaining));
    workspace_results
}

fn unique_source_type<'a>(
    inner: &'a IndexInner,
    file_id: FileId,
    fqn: &str,
) -> Option<&'a Arc<crate::Symbol>> {
    let mut matches = inner.file_symbols.get(&file_id)?.iter().filter(|symbol| {
        symbol.fqn.as_ref() == fqn && crate::is_type_declaration_kind(symbol.kind)
    });
    let symbol = matches.next()?;
    matches.next().is_none().then_some(symbol)
}

fn unique_source_declaration<'a>(
    inner: &'a IndexInner,
    file_id: FileId,
    fqn: &str,
) -> Option<&'a Arc<crate::Symbol>> {
    let mut matches = inner
        .file_symbols
        .get(&file_id)?
        .iter()
        .filter(|symbol| symbol.fqn.as_ref() == fqn);
    let symbol = matches.next()?;
    matches.next().is_none().then_some(symbol)
}

fn exact_source_type_resolution(
    election: &OwnerElection<'_>,
    file_id: FileId,
    imports: &ImportMap,
    lexical_owners: &[Arc<crate::Symbol>],
    written: &str,
    language: Option<Language>,
) -> Option<crate::TypeNameResolution> {
    let inner = election.inner;
    let written = written.trim();
    let (first, suffix) = written
        .split_once('.')
        .map_or((written, None), |(first, suffix)| (first, Some(suffix)));

    // A shadowed source's own lexical declarations must win over the active
    // duplicate's shape. Once a head is bound, a missing suffix stops
    // resolution instead of falling through to imports.
    for owner in lexical_owners {
        if owner.simple_name.as_ref() == first {
            let resolved = suffix
                .map(|suffix| format!("{}.{suffix}", owner.fqn))
                .unwrap_or_else(|| owner.fqn.to_string());
            return Some(
                unique_source_type(inner, file_id, &resolved)
                    .map(|_| crate::TypeNameResolution::Exact(resolved))
                    .unwrap_or(crate::TypeNameResolution::Missing),
            );
        }
        let binding = format!("{}.{}", owner.fqn, first);
        let direct = inner
            .file_symbols
            .get(&file_id)?
            .iter()
            .filter(|symbol| {
                symbol.fqn.as_ref() == binding && crate::is_type_declaration_kind(symbol.kind)
            })
            .count();
        if direct > 1 {
            return Some(crate::TypeNameResolution::Ambiguous(vec![binding]));
        }
        if direct == 1 {
            let resolved = suffix
                .map(|suffix| format!("{binding}.{suffix}"))
                .unwrap_or(binding);
            return Some(
                unique_source_type(inner, file_id, &resolved)
                    .map(|_| crate::TypeNameResolution::Exact(resolved))
                    .unwrap_or(crate::TypeNameResolution::Missing),
            );
        }
    }

    let all_owners_active = lexical_owners.iter().all(|owner| {
        election
            .symbol(owner.fqn.as_ref())
            .is_some_and(|active| Arc::ptr_eq(&active, owner))
    });
    if all_owners_active {
        let owners = lexical_owners
            .iter()
            .map(|owner| Arc::clone(&owner.fqn))
            .collect::<Vec<_>>();
        return Some(crate::type_name_resolution::resolve_written_type_name(
            election, imports, &owners, written, language,
        ));
    }

    // Inherited lexical member types require walking the shadowed owner's own
    // hierarchy. Refuse to guess rather than consulting another active shape.
    if lexical_owners
        .iter()
        .any(|owner| owner.parent_class.is_some() || !owner.interfaces.is_empty())
    {
        return None;
    }
    Some(crate::type_name_resolution::resolve_written_type_name(
        election,
        imports,
        &[] as &[Arc<str>],
        written,
        language,
    ))
}

fn hash_type_resolution(
    state: &crate::TypeNameResolution,
    hasher: &mut std::collections::hash_map::DefaultHasher,
) {
    match state {
        crate::TypeNameResolution::Exact(resolved) => {
            0_u8.hash(hasher);
            resolved.hash(hasher);
        }
        crate::TypeNameResolution::Missing => 1_u8.hash(hasher),
        crate::TypeNameResolution::Ambiguous(candidates) => {
            2_u8.hash(hasher);
            let mut candidates = candidates.clone();
            candidates.sort_unstable();
            candidates.dedup();
            candidates.hash(hasher);
        }
    }
}

fn exact_source_symbol_supertypes(
    election: &OwnerElection<'_>,
    file_id: FileId,
    symbol: &Arc<crate::Symbol>,
    imports: &ImportMap,
    language: Option<Language>,
) -> Option<crate::ResolvedDirectSupertypes> {
    let inner = election.inner;
    let mut lexical_owners = Vec::new();
    let mut container = symbol.container.clone();
    while let Some(owner) = container {
        let owner = Arc::clone(unique_source_declaration(inner, file_id, &owner)?);
        container = owner.container.clone();
        if crate::is_type_declaration_kind(owner.kind) {
            lexical_owners.push(owner);
        }
    }

    let exact_implicit = super::inheritance::exact_implicit_supertype(symbol, language);
    let mut raw_states = Vec::new();
    for recorded in symbol.parent_class.iter().chain(&symbol.interfaces) {
        if exact_implicit == Some(recorded.as_ref()) {
            continue;
        }
        raw_states.push(exact_source_type_resolution(
            election,
            file_id,
            imports,
            &lexical_owners,
            recorded,
            language,
        )?);
    }

    let parent_state_count = usize::from(
        symbol.parent_class.is_some() && symbol.parent_class.as_deref() != exact_implicit,
    );
    let bare_supertypes_are_interfaces = raw_states[parent_state_count..].iter().all(|state| {
        let crate::TypeNameResolution::Exact(resolved) = state else {
            return false;
        };
        unique_source_type(inner, file_id, resolved)
            .map(|target| target.kind)
            .or_else(|| election.symbol(resolved).map(|target| target.kind))
            == Some(SymbolKind::INTERFACE)
    });
    let mut superclass_state = raw_states
        .get(..parent_state_count)
        .and_then(|states| states.first())
        .cloned();
    let mut interface_states = raw_states[parent_state_count..].to_vec();
    // Some frontends record a single object-literal supertype in the
    // `parent_class` slot before the target's declaration kind is known.
    // Canonical resolution is the first layer that can recover its real role.
    let parent_was_reclassified = superclass_state.as_ref().is_some_and(|state| {
        let crate::TypeNameResolution::Exact(resolved) = state else {
            return false;
        };
        unique_source_type(inner, file_id, resolved)
            .map(|target| target.kind)
            .or_else(|| election.symbol(resolved).map(|target| target.kind))
            == Some(SymbolKind::INTERFACE)
    });
    if parent_was_reclassified {
        interface_states.insert(
            0,
            superclass_state
                .take()
                .expect("checked interface parent must be present"),
        );
    }
    if language == Some(Language::Kotlin)
        && symbol.kind != SymbolKind::INTERFACE
        && superclass_state.is_none()
    {
        let class_indices = interface_states
            .iter()
            .enumerate()
            .filter_map(|(index, state)| {
                let crate::TypeNameResolution::Exact(resolved) = state else {
                    return None;
                };
                unique_source_type(inner, file_id, resolved)
                    .map(|target| target.kind)
                    .or_else(|| election.symbol(resolved).map(|target| target.kind))
                    .is_some_and(|kind| {
                        kind != SymbolKind::INTERFACE && crate::is_type_declaration_kind(kind)
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if let [class_index] = class_indices.as_slice() {
            superclass_state = Some(interface_states.remove(*class_index));
        }
    }
    let implicit = crate::language::implicit_jvm_supertype(
        language,
        symbol.kind,
        &symbol.fqn,
        (!parent_was_reclassified)
            .then_some(symbol.parent_class.as_deref())
            .flatten(),
        bare_supertypes_are_interfaces,
    );
    if superclass_state.is_none() {
        if let Some(target) = implicit.filter(|target| *target != "java.lang.annotation.Annotation")
        {
            superclass_state = Some(if election.type_definition(target).is_some() {
                crate::TypeNameResolution::Exact(target.to_owned())
            } else {
                crate::TypeNameResolution::Missing
            });
        }
    }
    if let Some(target) = implicit.filter(|target| *target == "java.lang.annotation.Annotation") {
        interface_states.push(if election.type_definition(target).is_some() {
            crate::TypeNameResolution::Exact(target.to_owned())
        } else {
            crate::TypeNameResolution::Missing
        });
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    symbol.fqn.hash(&mut hasher);
    0_u8.hash(&mut hasher);
    if let Some(state) = &superclass_state {
        hash_type_resolution(state, &mut hasher);
    }
    1_u8.hash(&mut hasher);
    for state in &interface_states {
        hash_type_resolution(state, &mut hasher);
    }
    let superclass = superclass_state.and_then(|state| match state {
        crate::TypeNameResolution::Exact(resolved) => Some(resolved),
        crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
    });
    let mut interfaces = interface_states
        .into_iter()
        .filter_map(|state| match state {
            crate::TypeNameResolution::Exact(resolved) => Some(resolved),
            crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
        })
        .collect::<Vec<_>>();
    interfaces.sort_unstable();
    interfaces.dedup();
    Some(crate::ResolvedDirectSupertypes {
        superclass,
        interfaces,
        fingerprint: hasher.finish(),
    })
}

enum IndexedContentRead {
    Exact(crate::IndexedContent),
    DifferentDiskRevision,
    Missing,
}

impl IndexedContentRead {
    fn exact(self) -> Option<crate::IndexedContent> {
        match self {
            Self::Exact(indexed) => Some(indexed),
            Self::DifferentDiskRevision | Self::Missing => None,
        }
    }
}

impl WorkspaceIndex {
    /// Reuse the cached Java parse product only when it belongs to the exact
    /// caller-provided source revision.
    ///
    /// Both the tree and source are published atomically with the indexed
    /// revision. Comparing the retained source while holding the index read
    /// lock prevents a stale incremental tree from being paired with a newer
    /// editor buffer; the returned tree is an O(1) `Arc` clone.
    pub fn cached_java_parse(
        &self,
        file_id: FileId,
        expected_source: &str,
    ) -> Option<Arc<rowan::GreenNode>> {
        let inner = self.inner.read();
        let (tree, source) = inner.parse_tree_cache.peek(&file_id)?;
        (source.as_str() == expected_source).then(|| Arc::clone(tree))
    }

    /// Resolve a source-level type name in one indexed file.
    ///
    /// Callers provide enclosing type FQNs nearest-first. For an `extends`
    /// header, do not include the declaring type itself: its body members are
    /// not in scope in that header. Unlike legacy candidate-vector APIs, this
    /// preserves ambiguity between on-demand imports.
    pub fn resolve_type_name(
        &self,
        file_id: FileId,
        lexical_owners: &[String],
        written: &str,
    ) -> crate::TypeNameResolution {
        let inner = self.inner.read();
        let Some(imports) = inner.file_imports.get(&file_id) else {
            return crate::TypeNameResolution::Missing;
        };
        let lexical_owners = lexical_owners
            .iter()
            .map(|owner| Arc::<str>::from(owner.as_str()))
            .collect::<Vec<_>>();
        let language = inner.file_path(file_id).and_then(Language::from_path);
        if self.source_scope.is_some() {
            return crate::type_name_resolution::resolve_written_type_name(
                &OwnerElection::new(&inner, self.source_scope.as_ref()),
                imports,
                &lexical_owners,
                written,
                language,
            );
        }
        inner.resolve_type_name(imports, &lexical_owners, written, language)
    }

    /// The type arguments that the Kotlin typealias a type name written in
    /// `file_id` with `written_arguments` binds to gives its expanded class
    /// `["String"]` for `Names` under `typealias Names =
    /// java.util.ArrayList<String>`, and `["String", "Int"]` for `Inverse<Int,
    /// String>` under `typealias Inverse<K, V> = java.util.HashMap<V, K>`.
    ///
    /// The name resolves as [`Self::resolve_type_name`] resolves it, to the
    /// class the alias expands to. The arguments are the alias's own Kotlin
    /// spellings (see [`crate::KotlinTypeAlias::expanded_type_arguments`]),
    /// with the use's argument for each of the alias's type parameters. A use
    /// whose arguments do not match those parameters, in number or because
    /// one is empty, is [`crate::KotlinTypeAliasArguments::Unmatched`].
    pub fn resolve_kotlin_type_alias_arguments(
        &self,
        file_id: FileId,
        lexical_owners: &[String],
        written: &str,
        written_arguments: &[String],
    ) -> crate::KotlinTypeAliasArguments {
        let inner = self.inner.read();
        let Some(imports) = inner.file_imports.get(&file_id) else {
            return crate::KotlinTypeAliasArguments::NotAnAlias;
        };
        let language = inner.file_path(file_id).and_then(Language::from_path);
        if self.source_scope.is_some() {
            return crate::type_name_resolution::resolve_written_kotlin_type_alias_arguments(
                &OwnerElection::new(&inner, self.source_scope.as_ref()),
                imports,
                lexical_owners,
                written,
                written_arguments,
                language,
            );
        }
        crate::type_name_resolution::resolve_written_kotlin_type_alias_arguments(
            &*inner,
            imports,
            lexical_owners,
            written,
            written_arguments,
            language,
        )
    }

    /// Resolve a source-level type name against caller-provided syntax facts.
    ///
    /// This is the exact counterpart to [`Self::resolve_type_name`] for an
    /// unpublished editor revision. The workspace supplies only indexed type
    /// declarations; package/import bindings and language defaults come from
    /// the caller, so a stale retained `FileId` import map cannot affect the
    /// result.
    pub fn resolve_type_name_in_context(
        &self,
        imports: &ImportMap,
        lexical_owners: &[String],
        written: &str,
        language: Language,
    ) -> crate::TypeNameResolution {
        let inner = self.inner.read();
        let lexical_owners = lexical_owners
            .iter()
            .map(|owner| Arc::<str>::from(owner.as_str()))
            .collect::<Vec<_>>();
        if self.source_scope.is_some() {
            return crate::type_name_resolution::resolve_written_type_name(
                &OwnerElection::new(&inner, self.source_scope.as_ref()),
                imports,
                &lexical_owners,
                written,
                Some(language),
            );
        }
        inner.resolve_type_name(imports, &lexical_owners, written, Some(language))
    }

    /// Return the canonical, currently resolvable direct supertypes of a type.
    /// Missing or ambiguous source spellings do not fabricate hierarchy edges.
    pub fn direct_supertypes(&self, type_fqn: &str) -> Vec<String> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        if election.symbol(type_fqn).is_none() {
            return Vec::new();
        }
        let (parent, mut supertypes) =
            inner.resolved_supertype_roles_with_election(&election, type_fqn);
        supertypes.extend(parent);
        supertypes.sort_unstable();
        supertypes.dedup();
        supertypes
    }

    /// Canonical direct hierarchy with the superclass/interface role retained.
    /// Revision overlays use this for published ancestors so each declaration
    /// is resolved in its own file's import and language context.
    pub(crate) fn direct_supertype_roles(&self, type_fqn: &str) -> (Option<String>, Vec<String>) {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        inner.resolved_supertype_roles_with_election(&election, type_fqn)
    }

    /// Resolve hierarchy edges for one exact indexed source revision.
    ///
    /// Unlike [`Self::direct_supertypes`], this selects the declaration owned
    /// by `file_id` even when another file with the same FQN is active. The
    /// retained source must byte-for-byte match `expected_source`; cache
    /// eviction or any uncertainty about a shadowed lexical owner's inherited
    /// member types returns `None` so callers can fall back to syntax facts.
    pub fn direct_supertypes_for_revision(
        &self,
        file_id: FileId,
        type_fqn: &str,
        expected_source: &str,
    ) -> Option<crate::ResolvedDirectSupertypes> {
        self.file_supertypes_for_revision(file_id, expected_source)?
            .types
            .into_iter()
            .find(|resolved| resolved.type_fqn == type_fqn)
            .map(|resolved| crate::ResolvedDirectSupertypes {
                superclass: resolved.superclass,
                interfaces: resolved.interfaces,
                fingerprint: resolved.fingerprint,
            })
    }

    /// Resolve every nominal declaration owned by one exact indexed file
    /// revision under a single index read lock.
    ///
    /// The result carries both its exact file publication and the coherent
    /// workspace generation used for contextual resolution. The fingerprint
    /// includes each owner's Exact/Missing/Ambiguous edge state; it describes
    /// the computed value, while the workspace token validates lazy reuse.
    /// Duplicate nominal FQNs within one file make the batch unavailable.
    pub fn file_supertypes_for_revision(
        &self,
        file_id: FileId,
        expected_source: &str,
    ) -> Option<crate::ResolvedFileSupertypes> {
        let inner = self.inner.read();
        let published = inner.published_revisions.get(&file_id).copied()?;
        if published.content_hash != super::compute_content_hash(expected_source) {
            return None;
        }
        let file_revision = published.token(file_id);
        let workspace_revision = inner.workspace_revision;
        let content_matches = inner
            .file_content_cache
            .peek(&file_id)
            .is_some_and(|source| source.as_str() == expected_source)
            || inner
                .parse_tree_cache
                .peek(&file_id)
                .is_some_and(|(_, source)| source.as_str() == expected_source);
        if !content_matches {
            return None;
        }
        let imports = inner.file_imports.get(&file_id)?;
        let language = inner.file_path(file_id).and_then(Language::from_path);
        let mut symbols = inner
            .file_symbols
            .get(&file_id)?
            .iter()
            .filter(|symbol| crate::is_type_declaration_kind(symbol.kind))
            .cloned()
            .collect::<Vec<_>>();
        symbols.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        if symbols.windows(2).any(|pair| pair[0].fqn == pair[1].fqn) {
            return None;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut types = Vec::with_capacity(symbols.len());
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        for symbol in symbols {
            let resolved =
                exact_source_symbol_supertypes(&election, file_id, &symbol, imports, language)?;
            symbol.fqn.hash(&mut hasher);
            resolved.fingerprint.hash(&mut hasher);
            types.push(crate::ResolvedTypeSupertypes {
                type_fqn: symbol.fqn.to_string(),
                superclass: resolved.superclass,
                interfaces: resolved.interfaces,
                fingerprint: resolved.fingerprint,
            });
        }
        Some(crate::ResolvedFileSupertypes {
            types,
            fingerprint: hasher.finish(),
            file_revision,
            workspace_revision,
        })
    }

    /// Revalidate a lazily resolved hierarchy snapshot under one coherent read.
    pub fn resolved_file_supertypes_are_current(
        &self,
        resolved: &crate::ResolvedFileSupertypes,
    ) -> bool {
        let inner = self.inner.read();
        inner.revision_token(resolved.file_revision.file_id()) == Some(resolved.file_revision)
            && inner.workspace_revision == resolved.workspace_revision
    }

    /// Validate and consume a resolved hierarchy batch while retaining the
    /// coherent workspace read lock through the supplied installation.
    ///
    /// The LSP calls this from inside its semantic write transaction, which is
    /// the only place the index is mutated, so nothing can retire either token
    /// between validation and the installation closure. The retained read lock
    /// additionally protects callers that publish outside such a transaction.
    pub fn with_current_resolved_file_supertypes<R>(
        &self,
        resolved: &crate::ResolvedFileSupertypes,
        install: impl FnOnce() -> R,
    ) -> Option<R> {
        self.with_current_resolved_file_supertypes_and_absent_files(resolved, &[], install)
    }

    /// Hierarchy publication variant that also proves displaced FileIds remain
    /// absent. This prevents a delayed publisher from retiring semantic state
    /// for an identity already reintroduced by a newer workspace writer.
    pub fn with_current_resolved_file_supertypes_and_absent_files<R>(
        &self,
        resolved: &crate::ResolvedFileSupertypes,
        absent_file_ids: &[FileId],
        install: impl FnOnce() -> R,
    ) -> Option<R> {
        let inner = self.inner.read();
        (inner.revision_token(resolved.file_revision.file_id()) == Some(resolved.file_revision)
            && inner.workspace_revision == resolved.workspace_revision
            && absent_file_ids
                .iter()
                .all(|file_id| inner.revision_token(*file_id).is_none()))
        .then(install)
    }

    /// Validate exact indexed bytes and retain the coherent workspace read
    /// lock through publication into a caller-owned mirror.
    ///
    /// This is the non-Java counterpart to
    /// [`Self::with_current_resolved_file_supertypes`]. The retained cache
    /// comparison is intentional: a file token plus the compatibility hash is
    /// not a proof of byte equality.
    pub fn with_current_indexed_content<R>(
        &self,
        indexed: &crate::IndexedContent,
        install: impl FnOnce() -> R,
    ) -> Option<R> {
        self.with_current_indexed_content_and_absent_files(indexed, &[], install)
    }

    /// Exact-content publication variant that also validates displaced IDs.
    pub fn with_current_indexed_content_and_absent_files<R>(
        &self,
        indexed: &crate::IndexedContent,
        absent_file_ids: &[FileId],
        install: impl FnOnce() -> R,
    ) -> Option<R> {
        let file_id = indexed.revision.file_id();
        let inner = self.inner.read();
        let published = inner.published_revisions.get(&file_id)?;
        let retained_source_matches = inner
            .file_content_cache
            .peek(&file_id)
            .is_some_and(|source| source.as_str() == indexed.content.as_str())
            || inner
                .parse_tree_cache
                .peek(&file_id)
                .is_some_and(|(_, source)| source.as_str() == indexed.content.as_str());
        (published.token(file_id) == indexed.revision
            && published.content_hash == indexed.content_hash
            && inner.file_path(file_id) == Some(indexed.file_path.as_path())
            && retained_source_matches
            && absent_file_ids
                .iter()
                .all(|file_id| inner.revision_token(*file_id).is_none()))
        .then(install)
    }

    pub fn current_workspace_revision(&self) -> WorkspaceRevision {
        self.inner.read().workspace_revision
    }

    /// Whether every workspace-wide binding decision observable at `revision`
    /// is still valid.
    ///
    /// Interactive readers capture a generation, resolve, and then must prove
    /// they did not straddle a rebinding publication. Exact equality is too
    /// strong for that proof: restoring the cached JDK image publishes hundreds
    /// of owner-complete chunks, each advancing the generation, and a purely
    /// additive first publication cannot produce a view that no single
    /// generation could have produced. Answers therefore stay valid across it,
    /// while any republication, removal, or replacement still invalidates them.
    pub fn workspace_bindings_are_current(&self, revision: WorkspaceRevision) -> bool {
        self.inner.read().workspace_bindings_are_current(revision)
    }

    /// Whether two generations captured by one answer describe mutually
    /// coherent bindings that are all still valid.
    pub fn workspace_bindings_agree(
        &self,
        left: WorkspaceRevision,
        right: WorkspaceRevision,
    ) -> bool {
        let inner = self.inner.read();
        inner.workspace_bindings_are_current(left) && inner.workspace_bindings_are_current(right)
    }

    pub fn is_workspace_file(&self, file_id: FileId) -> bool {
        let inner = self.inner.read();
        inner
            .file_path_key(file_id)
            .is_some_and(|key| key.starts_with(&inner.root_key))
    }

    pub fn find_by_simple_name(&self, simple_name: &str) -> Vec<SymbolLocation> {
        let inner = self.inner.read();

        let mut locations = if let Some(fqns) = inner.simple_name_index.get(simple_name) {
            fqns.iter()
                .filter_map(|fqn| inner.symbol_locations.get(fqn.as_ref()))
                .map(|location| stamp_source_revision(&inner, location))
                .collect()
        } else {
            Vec::new()
        };
        locations.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        locations
    }

    /// Capture simple-name definitions and their owning source publications
    /// under one index read. External definitions are omitted because their
    /// byte ranges are owned by source providers rather than workspace files.
    pub fn find_by_simple_name_revisioned(&self, simple_name: &str) -> RevisionedLocations {
        let inner = self.inner.read();
        let mut locations = inner
            .simple_name_index
            .get(simple_name)
            .into_iter()
            .flatten()
            .filter_map(|fqn| inner.symbol_locations.get(fqn.as_ref()).cloned())
            .collect::<Vec<_>>();
        locations.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        group_revisioned_locations(&inner, locations)
    }

    /// Find only concrete type declarations with the given simple name.
    ///
    /// Unlike [`Self::find_by_simple_name`], this excludes methods, fields,
    /// constructors, and other symbols that share the spelling.
    pub fn find_type_declarations_by_simple_name(&self, simple_name: &str) -> Vec<SymbolLocation> {
        let inner = self.inner.read();
        let mut locations: Vec<_> = inner
            .simple_name_index
            .get(simple_name)
            .into_iter()
            .flatten()
            .filter_map(|fqn| {
                find_symbol_definition_coherent_in(
                    &inner,
                    &crate::SymbolTarget::new(Arc::clone(fqn), crate::DeclarationNamespace::Type),
                )
                .map(|definition| definition.location)
            })
            .collect();
        locations.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        locations
    }

    /// Resolve one declaration owned by `file_id` without falling back to an
    /// equal FQN currently owned by another source.
    ///
    /// The optional offset disambiguates overloads and nested declarations
    /// with the same simple name. Pointer identity against the active symbol is
    /// deliberate: shadowed sources and same-file declarations with a
    /// colliding textual FQN retain exact metadata in `file_symbols`. The
    /// requested namespace is elected independently, while duplicate sources
    /// still obey normal workspace/classpath precedence.
    pub fn find_symbol_in_file(
        &self,
        file_id: FileId,
        simple_name: &str,
        kind: SymbolKind,
        offset: Option<u32>,
    ) -> Option<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let namespace = crate::DeclarationNamespace::for_kind(kind);
        let mut candidates = inner.file_symbols.get(&file_id)?.iter().filter(|symbol| {
            if symbol.kind != kind || symbol.simple_name.as_ref() != simple_name {
                return false;
            }
            let target = crate::SymbolTarget::new(Arc::clone(&symbol.fqn), namespace);
            find_symbol_definition_coherent_in(&inner, &target).is_some_and(|definition| {
                definition.location.file_id == file_id && Arc::ptr_eq(&definition.symbol, symbol)
            })
        });

        if let Some(offset) = offset {
            return candidates
                .find(|symbol| symbol.range.start <= offset && offset < symbol.range.end)
                .cloned();
        }

        let candidate = candidates.next().cloned();
        candidates.next().is_none().then_some(candidate).flatten()
    }

    /// Resolve one currently elected value declaration by its exact source
    /// name range. Kotlin's scope graph reports declaration ranges for bare
    /// implicit-`this` property reads; this bridge preserves source-owner
    /// coherence instead of rebinding a duplicate FQN from another file.
    pub(crate) fn find_value_definition_in_file_at_range(
        &self,
        file_id: FileId,
        simple_name: &str,
        start: u32,
        end: u32,
    ) -> Option<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        find_value_definition_in_file_at_range_in(&inner, file_id, simple_name, start, end)
    }

    pub fn find_class_by_suffix(&self, suffix: &str) -> Option<String> {
        let inner = self.inner.read();
        let simple_name = suffix.rsplit('.').next().unwrap_or(suffix);

        if let Some(fqns) = inner.simple_name_index.get(simple_name) {
            let dot_suffix = format!(".{suffix}");
            return fqns
                .iter()
                .filter(|fqn| {
                    inner.symbols.get(fqn.as_ref()).is_some_and(|symbol| {
                        symbol.kind == SymbolKind::CLASS
                            && (fqn.as_ref() == suffix || fqn.ends_with(&dot_suffix))
                    })
                })
                .min()
                .map(ToString::to_string);
        }
        None
    }

    /// How many source declarations currently carry exactly this FQN.
    ///
    /// Kotlin overloads share one FQN and the by-FQN maps keep a single
    /// winner, so a caller that must know whether a function is overloaded has
    /// to count the per-file declaration tables. This scans every source file's
    /// symbols; use it for rare, decisive questions only.
    pub fn declaration_count(&self, fqn: &str) -> usize {
        let inner = self.inner.read();
        inner
            .file_symbols
            .values()
            .flatten()
            .filter(|symbol| symbol.fqn.as_ref() == fqn)
            .count()
    }

    /// Every declaration the source of `symbol` makes under its FQN in its
    /// namespace, `symbol` among them, in source order.
    ///
    /// Kotlin overloads share one FQN, and an FQN lookup elects one of them,
    /// so a caller that reads the elected declaration's metadata can learn
    /// here whether the source's other overloads say the same. Only the
    /// source of `symbol` is read, through the reverse declaration index, so
    /// a shadowed duplicate elsewhere is not counted. Empty when `symbol` is
    /// not a declaration the index holds.
    pub fn same_source_declarations(&self, symbol: &Arc<crate::Symbol>) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let Some(candidates) = inner.symbol_definition_files.get(symbol.fqn.as_ref()) else {
            return Vec::new();
        };
        let declaration = |candidate: &SourceSymbolCandidate| {
            inner
                .file_symbols
                .get(&candidate.file_id)?
                .get(candidate.ordinal as usize)
        };
        let Some(file_id) = candidates
            .iter()
            .find(|candidate| declaration(candidate).is_some_and(|own| Arc::ptr_eq(own, symbol)))
            .map(|candidate| candidate.file_id)
        else {
            return Vec::new();
        };
        let namespace = crate::DeclarationNamespace::for_kind(symbol.kind);
        candidates
            .iter()
            .filter(|candidate| candidate.file_id == file_id)
            .filter_map(declaration)
            .filter(|declared| namespace.accepts(declared.kind))
            .cloned()
            .collect()
    }

    /// Symbols that exist only in the JVM view of the Kotlin declaration
    /// `origin_fqn` (file-facade members, property accessors, `@JvmStatic`
    /// forwarders), in FQN order.
    pub fn jvm_aliases_of(&self, origin_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let mut aliases: Vec<_> = inner
            .symbols
            .values()
            .filter(|symbol| {
                symbol
                    .kotlin
                    .as_ref()
                    .and_then(|kotlin| kotlin.jvm_origin.as_deref())
                    == Some(origin_fqn)
            })
            .map(Arc::clone)
            .collect();
        aliases.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        aliases
    }

    /// The Kotlin declaration a JVM-view symbol derives from, if `fqn` is one.
    pub fn jvm_origin_of(&self, fqn: &str) -> Option<String> {
        let inner = self.inner.read();
        inner
            .symbols
            .get(fqn)
            .and_then(|symbol| symbol.kotlin.as_ref())
            .and_then(|kotlin| kotlin.jvm_origin.as_ref())
            .map(ToString::to_string)
    }

    /// Every declaration a rename or find-references request on `target` must
    /// consider one thing: the Kotlin declaration and its Java-visible aliases.
    ///
    /// `target` comes first. For a Kotlin property that adds `getX()`/`setX()`,
    /// for a top-level function the facade's static method, and so on; for a
    /// JVM-view symbol it adds the origin and the sibling aliases. A target
    /// without any JVM relationship yields just itself.
    pub fn jvm_reference_family(&self, target: &crate::SymbolTarget) -> Vec<crate::SymbolTarget> {
        let inner = self.inner.read();
        let origin_fqn = inner
            .symbols
            .get(target.fqn.as_ref())
            .and_then(|symbol| symbol.kotlin.as_ref())
            .and_then(|kotlin| kotlin.jvm_origin.as_ref())
            .map_or_else(|| target.fqn.to_string(), ToString::to_string);

        let mut family = vec![target.clone()];
        let mut push = |candidate: crate::SymbolTarget| {
            if !family.iter().any(|existing| {
                existing.fqn == candidate.fqn && existing.namespace == candidate.namespace
            }) {
                family.push(candidate);
            }
        };
        if let Some(origin) = inner.symbols.get(origin_fqn.as_str()) {
            push(crate::SymbolTarget::for_symbol(origin));
        }
        let mut aliases: Vec<_> = inner
            .symbols
            .values()
            .filter(|symbol| {
                symbol
                    .kotlin
                    .as_ref()
                    .and_then(|kotlin| kotlin.jvm_origin.as_deref())
                    == Some(origin_fqn.as_str())
            })
            .collect();
        aliases.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        for alias in aliases {
            push(crate::SymbolTarget::for_symbol(alias));
        }
        family
    }

    pub fn find_extension_by_name(
        &self,
        receiver_type: &str,
        method_name: &str,
    ) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();

        let mut extensions: Vec<_> = inner
            .symbols
            .values()
            .filter(|symbol| {
                // Only consider functions with matching name
                if symbol.kind != SymbolKind::FUNCTION || symbol.simple_name.as_ref() != method_name
                {
                    return false;
                }

                // Check if receiver_type matches
                if let Some(ref recv) = symbol.receiver_type {
                    let base_recv = recv.split('<').next().unwrap_or(recv.as_ref());
                    base_recv == receiver_type || recv.as_ref() == receiver_type
                } else {
                    false
                }
            })
            .map(Arc::clone)
            .collect();
        extensions.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        extensions
    }

    pub fn find_extensions_for_type(&self, receiver_type: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();

        let mut extensions: Vec<_> = inner
            .symbols
            .values()
            .filter(|symbol| {
                // Only consider functions
                if symbol.kind != SymbolKind::FUNCTION {
                    return false;
                }

                // Check if receiver_type matches
                if let Some(ref recv) = symbol.receiver_type {
                    // Match exact type or strip generic arguments for comparison
                    let base_recv = recv.split('<').next().unwrap_or(recv.as_ref());
                    base_recv == receiver_type || recv.as_ref() == receiver_type
                } else {
                    false
                }
            })
            .map(Arc::clone)
            .collect();
        extensions.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        extensions
    }

    /// Return the types that directly declare `target_fqn` as a supertype.
    ///
    /// This is the relationship required by the LSP type-hierarchy protocol:
    /// callers can expand each returned node to walk the hierarchy one level
    /// at a time.
    pub fn find_direct_subtypes(&self, target_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let mut subtypes: Vec<_> = inner
            .direct_subtypes(target_fqn)
            .into_iter()
            .filter_map(|fqn| inner.symbols.get(fqn.as_ref()).cloned())
            .collect();
        subtypes.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        subtypes
    }

    /// Return every concrete transitive implementation or subclass of
    /// `target_fqn`.
    ///
    /// Goto-implementation, code lenses, and CLI searches want the complete
    /// set: an abstract class or subinterface must not hide concrete types
    /// farther down the hierarchy, but neither is itself an implementation a
    /// user can instantiate or navigate to as the final implementation.
    pub fn find_implementations(&self, target_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let mut descendant_fqns = HashSet::new();
        inner.collect_transitive_subtypes(target_fqn, &mut descendant_fqns);

        let mut implementations: Vec<_> = descendant_fqns
            .into_iter()
            .filter_map(|fqn| inner.symbols.get(fqn.as_ref()).cloned())
            .filter(|symbol| symbol.kind != SymbolKind::INTERFACE && !symbol.modifiers.is_abstract)
            .collect();
        implementations.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        implementations
    }

    /// Like [`Self::find_implementations`], but matches the supertype by its
    /// simple name.
    ///
    /// This finds implementations of supertypes that are not themselves in
    /// the index (e.g. an interface from an unloaded dependency JAR): source
    /// declarations retain the written spelling, so matching its last segment
    /// remains a deliberately broad name-only fallback without an indexed
    /// declaration to anchor on.
    pub fn find_implementations_by_simple_name(
        &self,
        simple_name: &str,
    ) -> Vec<Arc<crate::Symbol>> {
        fn last_segment(fqn: &str) -> &str {
            fqn.rsplit('.').next().unwrap_or(fqn)
        }

        let inner = self.inner.read();
        let direct: Vec<_> = inner
            .symbols
            .values()
            .filter(|symbol| {
                if !crate::is_type_declaration_kind(symbol.kind) {
                    return false;
                }
                if symbol
                    .interfaces
                    .iter()
                    .any(|iface| last_segment(iface) == simple_name)
                {
                    return true;
                }
                symbol
                    .parent_class
                    .as_ref()
                    .is_some_and(|parent| last_segment(parent) == simple_name)
            })
            .map(Arc::clone)
            .collect();

        let mut fqns: HashSet<_> = direct
            .iter()
            .map(|symbol| Arc::clone(&symbol.fqn))
            .collect();
        for symbol in direct {
            inner.collect_transitive_subtypes(&symbol.fqn, &mut fqns);
        }

        let mut implementations: Vec<_> = fqns
            .into_iter()
            .filter_map(|fqn| inner.symbols.get(fqn.as_ref()).cloned())
            .filter(|symbol| symbol.kind != SymbolKind::INTERFACE && !symbol.modifiers.is_abstract)
            .collect();
        implementations.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        implementations
    }

    pub fn get_interface_implementations(&self, interface_fqn: &str) -> Vec<String> {
        let inner = self.inner.read();
        let mut implementations: Vec<_> = inner
            .direct_subtypes(interface_fqn)
            .into_iter()
            .map(|fqn| fqn.to_string())
            .collect();
        implementations.sort_unstable();
        implementations
    }

    pub fn find_overriding_method_locations(
        &self,
        class_fqn: &str,
        method_signature: &str,
    ) -> Vec<SymbolLocation> {
        let inner = self.inner.read();
        let target = crate::SymbolTarget::new(
            Arc::from(format!("{class_fqn}.{method_signature}")),
            crate::DeclarationNamespace::Callable,
        );
        let Some(root) = find_symbol_definition_coherent_in(&inner, &target) else {
            return Vec::new();
        };
        match overriding_declarations_in(&inner, root, &|| false) {
            Ok(definitions) => definitions
                .into_iter()
                .map(|definition| definition.location)
                .collect(),
            Err(error) => {
                // Reported as no overrides, so the failure is logged, not silent.
                tracing::warn!(
                    "overriding methods of {class_fqn}.{method_signature} are unavailable: {error}"
                );
                Vec::new()
            }
        }
    }

    pub fn find_interface_method_locations(
        &self,
        interface_fqn: &str,
        method_signature: &str,
    ) -> Vec<SymbolLocation> {
        self.find_overriding_method_locations(interface_fqn, method_signature)
    }

    pub fn find_method_by_args(
        &self,
        class_fqn: &str,
        method_name: &str,
        arg_count: usize,
    ) -> Option<Arc<crate::Symbol>> {
        let methods = self.find_methods_by_name(class_fqn, method_name);

        // First: exact match by argument count
        if let Some(method) = methods.iter().find(|m| m.parameters.len() == arg_count) {
            return Some(Arc::clone(method));
        }

        // Fallback: declared varargs support. An ordinary array parameter
        // still consumes exactly one argument; dependency symbols recover
        // `...` only when their class-file method carries ACC_VARARGS.
        methods
            .iter()
            .find(|m| {
                let has_varargs = m
                    .parameters
                    .last()
                    .map(|p| {
                        // Parameter format: "TypeName paramName" or "TypeName... paramName"
                        // Extract the type (first whitespace-separated token)
                        let type_part = p.split_whitespace().next().unwrap_or("");
                        type_part.contains("...")
                    })
                    .unwrap_or(false);
                has_varargs && arg_count >= m.parameters.len().saturating_sub(1)
            })
            .map(Arc::clone)
    }

    pub fn find_method_by_args_with_inheritance(
        &self,
        class_fqn: &str,
        method_name: &str,
        arg_count: usize,
    ) -> Option<Arc<crate::Symbol>> {
        let methods = self.find_effective_methods_by_name(class_fqn, method_name);
        let mut exact = methods
            .iter()
            .filter(|method| method.parameters.len() == arg_count);
        let selected = exact.next();
        if exact.next().is_some() {
            return None;
        }
        if let Some(method) = selected {
            return Some(Arc::clone(method));
        }
        let mut variadic = methods.iter().filter(|method| {
            let variadic = method.parameters.last().is_some_and(|parameter| {
                let parameter_type = parameter.split_whitespace().next().unwrap_or("");
                parameter_type.contains("...")
            });
            variadic && arg_count >= method.parameters.len().saturating_sub(1)
        });
        let selected = variadic.next();
        if variadic.next().is_some() {
            return None;
        }
        selected.map(Arc::clone)
    }

    pub fn find_method_location_by_args(
        &self,
        class_fqn: &str,
        method_name: &str,
        arg_count: usize,
    ) -> Option<SymbolLocation> {
        let method = self.find_method_by_args(class_fqn, method_name, arg_count)?;
        // Build the FQN with parameters to find the location
        let param_types: Vec<&str> = method
            .parameters
            .iter()
            .map(|p| {
                // Extract type from "Type name" format
                p.split_whitespace().next().unwrap_or("Object")
            })
            .collect();
        let fqn = format!("{}.{}({})", class_fqn, method_name, param_types.join(","));

        self.find_symbol_definition(&crate::SymbolTarget::new(
            fqn,
            crate::DeclarationNamespace::Callable,
        ))
        .map(|definition| definition.location)
    }

    pub fn find_method_locations_by_name(
        &self,
        class_fqn: &str,
        method_name: &str,
    ) -> Vec<SymbolLocation> {
        let inner = self.inner.read();
        method_definitions_by_name_in(&inner, class_fqn, method_name)
            .into_iter()
            .map(|definition| definition.location)
            .collect()
    }

    /// Resolve direct members from the source authority that supplied the
    /// active nominal owner, in one exact Java declaration namespace.
    ///
    /// This is the typed counterpart to legacy textual `Owner.name` lookup.
    /// In particular, a field, nested type, and method may all share that
    /// spelling, and a shadowed dependency must not contribute members to the
    /// elected workspace owner.
    pub fn find_member_definitions_by_name(
        &self,
        owner_fqn: &str,
        member_name: &str,
        namespace: crate::DeclarationNamespace,
    ) -> Vec<ResolvedDefinition> {
        let inner = self.inner.read();
        let members = self.source_scope.as_ref().map_or_else(
            || selected_owner_member_definitions_shared_in(&inner, owner_fqn),
            |scope| {
                let target = crate::SymbolTarget::new(owner_fqn, crate::DeclarationNamespace::Type);
                let owner = find_symbol_definition_in_scope(&inner, &target, scope)?;
                selected_owner_member_definitions_shared_from_owner(&inner, owner_fqn, owner)
            },
        );
        members
            .iter()
            .flat_map(|members| members.iter())
            .filter(|definition| {
                definition.target.namespace == namespace
                    && definition.symbol.simple_name.as_ref() == member_name
            })
            .cloned()
            .collect()
    }

    /// Resolve one effective inherited value declaration by name.
    ///
    /// This compatibility entry point delegates to the canonical field
    /// election. In particular, it does not filter an inaccessible nearer
    /// declaration and then resurrect a hidden ancestor, and it returns no
    /// definition when distinct inherited declarations make the name
    /// ambiguous.
    pub fn find_value_definition_with_inheritance(
        &self,
        receiver_fqn: &str,
        member_name: &str,
    ) -> Option<ResolvedDefinition> {
        self.find_effective_field_definition_by_name(receiver_fqn, member_name)
    }

    pub fn find_member_locations_by_name(
        &self,
        owner_fqn: &str,
        member_name: &str,
        namespace: crate::DeclarationNamespace,
    ) -> Vec<SymbolLocation> {
        self.find_member_definitions_by_name(owner_fqn, member_name, namespace)
            .into_iter()
            .map(|definition| definition.location)
            .collect()
    }

    pub fn find_member_symbols_by_name(
        &self,
        owner_fqn: &str,
        member_name: &str,
        namespace: crate::DeclarationNamespace,
    ) -> Vec<Arc<crate::Symbol>> {
        self.find_member_definitions_by_name(owner_fqn, member_name, namespace)
            .into_iter()
            .map(|definition| definition.symbol)
            .collect()
    }

    pub fn find_methods_by_name(
        &self,
        class_fqn: &str,
        method_name: &str,
    ) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let definitions = self.source_scope.as_ref().map_or_else(
            || method_definitions_by_name_in(&inner, class_fqn, method_name),
            |scope| {
                let target = crate::SymbolTarget::new(class_fqn, crate::DeclarationNamespace::Type);
                let Some(owner) = find_symbol_definition_in_scope(&inner, &target, scope) else {
                    return Vec::new();
                };
                selected_owner_member_definitions_from_owner(&inner, class_fqn, owner, |_, _| false)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|definition| {
                        definition.target.namespace == crate::DeclarationNamespace::Callable
                            && definition.symbol.simple_name.as_ref() == method_name
                    })
                    .collect()
            },
        );
        let mut methods: Vec<_> = definitions
            .into_iter()
            .map(|definition| (definition.target.fqn.to_string(), definition.symbol))
            .collect();

        // Sort deterministically, preferring symbols with type variable return types
        // (e.g., "V" or "T") over concrete types (e.g., "Object").
        // This ensures workspace stubs with generic info are preferred over JDK bytecode
        // which has erased types.
        methods.sort_by(|(fqn_a, sym_a), (fqn_b, sym_b)| {
            let is_type_var = |s: &Option<Arc<str>>| -> bool {
                s.as_ref().is_some_and(|t| {
                    t.len() == 1 && t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                })
            };
            let a_has_type_var = is_type_var(&sym_a.return_type);
            let b_has_type_var = is_type_var(&sym_b.return_type);
            match (a_has_type_var, b_has_type_var) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => fqn_a.cmp(fqn_b),
            }
        });

        methods.into_iter().map(|(_, sym)| sym).collect()
    }

    pub fn find_methods_by_name_with_inheritance(
        &self,
        class_fqn: &str,
        method_name: &str,
    ) -> Vec<Arc<crate::Symbol>> {
        // Preserve the historical API while using the canonical receiver
        // election. The former implementation took one workspace read per
        // owner and returned overridden declarations alongside the method a
        // Java receiver actually exposes.
        self.find_effective_methods_by_name(class_fqn, method_name)
    }

    /// Find a definition and capture its owning publication under the same
    /// index read lock. Definitions without indexed source fail closed.
    pub fn find_symbol_revisioned(&self, fqn: &str) -> Option<RevisionedLocation> {
        let inner = self.inner.read();
        let location = find_symbol_in(&inner, fqn)?;
        let revision = inner.revision_token(location.file_id)?;
        Some(RevisionedLocation { revision, location })
    }

    pub fn find_symbol(&self, fqn: &str) -> Option<SymbolLocation> {
        let inner = self.inner.read();
        if let Some(scope) = &self.source_scope {
            return find_any_symbol_definition_in_scope(&inner, fqn, scope)
                .map(|definition| definition.location);
        }
        find_symbol_in(&inner, fqn)
    }

    /// Resolve one semantic declaration together with the exact range and
    /// source-precedence authority that selected it.
    pub fn find_symbol_definition(
        &self,
        target: &crate::SymbolTarget,
    ) -> Option<ResolvedDefinition> {
        let inner = self.inner.read();
        if let Some(scope) = &self.source_scope {
            return find_symbol_definition_coherent_in_scope(&inner, target, scope);
        }
        find_symbol_definition_coherent_in(&inner, target)
    }

    /// Typed metadata convenience backed by [`Self::find_symbol_definition`].
    pub fn get_symbol_target(&self, target: &crate::SymbolTarget) -> Option<Arc<crate::Symbol>> {
        self.find_symbol_definition(target)
            .map(|definition| definition.symbol)
    }

    /// Recover the exact source-publication token carried by a queried
    /// location. This does not consult current index state or compare fields:
    /// an R1 location always reconstructs R1, even after an identical R2 is
    /// published, so exact content retrieval can reject the retired range.
    pub fn revisioned_location(&self, location: &SymbolLocation) -> Option<RevisionedLocation> {
        let revision_id = location.workspace_source_revision()?;
        Some(RevisionedLocation {
            revision: FileRevisionToken {
                file_id: location.file_id,
                revision_id,
            },
            location: location.clone(),
        })
    }

    /// Recover the exact external archive generation carried by a queried
    /// location without consulting current index state. A retained R1 location
    /// therefore remains R1 after a same-path R2 refresh reuses its `FileId`.
    pub fn external_source_revision(
        &self,
        location: &SymbolLocation,
    ) -> Option<ExternalSourceRevisionToken> {
        Some(ExternalSourceRevisionToken {
            file_id: location.file_id,
            generation: location.external_source_generation()?,
        })
    }

    /// Legacy resolved usage artifacts retained for source compatibility.
    ///
    /// These locations were selected when each file was indexed and do not
    /// carry a coherent workspace revision. Request and refactoring consumers
    /// must use [`Self::find_current_usages`].
    pub fn find_usages(&self, fqn: &str) -> Vec<SymbolLocation> {
        let inner = self.inner.read();
        find_usages_in(&inner, fqn)
    }

    /// Legacy indexed usages grouped by the exact source publication that owns
    /// each stored range. New request paths should prefer current re-extraction.
    pub fn find_usages_revisioned(&self, fqn: &str) -> RevisionedLocations {
        let inner = self.inner.read();
        group_revisioned_locations(&inner, find_usages_in(&inner, fqn))
    }

    /// Legacy dependency snapshot captured during indexing.
    ///
    /// This preserves the historical source-compatible API. Consumers that
    /// require current semantic edges and revision proofs must use
    /// [`Self::get_current_file_dependency_fqns`].
    pub fn get_file_dependency_fqns(&self, file_id: FileId) -> Vec<Arc<str>> {
        let inner = self.inner.read();
        let mut fqns: Vec<_> = inner
            .file_usage_counts
            .get(&file_id)
            .into_iter()
            .flat_map(|counts| counts.keys().cloned())
            .chain(
                inner
                    .file_imported_fqns
                    .get(&file_id)
                    .into_iter()
                    .flatten()
                    .map(|fqn| Arc::from(fqn.as_str())),
            )
            .collect();
        fqns.sort_unstable();
        fqns.dedup();
        fqns
    }

    pub fn get_class_members(&self, class_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let mut members: Vec<_> = selected_owner_member_definitions_in(&inner, class_fqn)
            .unwrap_or_default()
            .into_iter()
            .map(|definition| definition.symbol)
            .collect();
        members.sort_unstable_by(|left, right| {
            left.fqn
                .cmp(&right.fqn)
                .then(
                    (crate::DeclarationNamespace::for_kind(left.kind) as u8)
                        .cmp(&(crate::DeclarationNamespace::for_kind(right.kind) as u8)),
                )
                .then(left.range.start.cmp(&right.range.start))
        });
        members
    }

    /// Get all members defined on a class, its superclasses, and implemented/super interfaces.
    ///
    /// Returns members in inheritance order:
    /// - current class members first
    /// - then parent classes in linear order
    /// - then implemented and superinterfaces in a stable transitive order.
    pub fn get_class_members_with_inheritance(&self, class_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inheritance_chain = self.get_inheritance_chain(class_fqn);
        let mut members = Vec::new();
        for class_name in inheritance_chain {
            members.extend(self.get_class_members(&class_name));
        }
        members
    }

    pub fn get_constructors_for_class(&self, class_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let mut constructors: Vec<_> = selected_owner_member_definitions_in(&inner, class_fqn)
            .unwrap_or_default()
            .into_iter()
            .filter(|definition| {
                definition.target.namespace == crate::DeclarationNamespace::Constructor
            })
            .map(|definition| definition.symbol)
            .collect();
        constructors.sort_unstable_by(|left, right| {
            left.fqn
                .cmp(&right.fqn)
                .then(left.range.start.cmp(&right.range.start))
        });

        tracing::debug!(
            "get_constructors_for_class({}) returned {} constructors",
            class_fqn,
            constructors.len()
        );
        constructors
    }

    pub fn get_external_source_name(&self, file_id: FileId) -> Option<String> {
        self.inner
            .read()
            .external_file_id_to_source
            .get(&file_id)
            .map(RegisteredExternalSource::display_name)
    }

    /// Return the lossless identity of an external archive or virtual source.
    ///
    /// Source providers should prefer this over [`Self::get_external_source_name`],
    /// whose string result exists for compatibility and may be lossy for a
    /// filesystem path that is not valid UTF-8.
    pub fn get_external_source_identity(&self, file_id: FileId) -> Option<ExternalSourceIdentity> {
        self.inner
            .read()
            .external_file_id_to_source
            .get(&file_id)
            .map(|source| source.key().clone())
    }

    /// Return the retained user-facing archive spelling without using it as
    /// identity. Virtual sources do not have filesystem paths.
    pub fn get_external_source_path(&self, file_id: FileId) -> Option<PathBuf> {
        self.inner
            .read()
            .external_file_id_to_source
            .get(&file_id)
            .and_then(RegisteredExternalSource::display_path)
    }

    pub fn get_external_symbols(&self) -> Vec<crate::Symbol> {
        let inner = self.inner.read();
        inner
            .symbols
            .values()
            .filter(|sym| {
                // External symbols are those from JARs/JMODs (tracked in external_file_ids)
                if let Some(loc) = inner.symbol_locations.get(&sym.fqn) {
                    inner.external_file_ids.contains(&loc.file_id)
                } else {
                    false
                }
            })
            .map(|arc_sym| (**arc_sym).clone())
            .collect()
    }

    /// Snapshot the currently elected external symbols and all generic
    /// supertype metadata owned by external publications.
    ///
    /// Refresh-scoped production caches should use
    /// [`Self::get_external_index_data_by_source_for_refresh`] so ownership and
    /// revision proofs are retained. This compatibility snapshot supports
    /// isolated consumers, such as the integration-test JDK index, which own
    /// the complete external index.
    pub fn get_external_index_data(
        &self,
    ) -> (Vec<crate::Symbol>, Vec<crate::TypeGenericSupertypes>) {
        let inner = self.inner.read();
        let symbols = inner
            .symbols
            .values()
            .filter(|symbol| {
                inner
                    .symbol_locations
                    .get(&symbol.fqn)
                    .is_some_and(|location| inner.external_file_ids.contains(&location.file_id))
            })
            .map(|symbol| (**symbol).clone())
            .collect();
        let mut generic_supertypes = inner
            .external_file_ids
            .iter()
            .flat_map(|file_id| inner.file_generic_supertypes.get(file_id))
            .flat_map(|by_type| by_type.iter())
            .map(|(type_fqn, edges)| crate::TypeGenericSupertypes {
                type_fqn: type_fqn.to_string(),
                edges: edges.to_vec(),
            })
            .collect::<Vec<_>>();
        generic_supertypes.sort_unstable_by(|left, right| left.type_fqn.cmp(&right.type_fqn));
        (symbols, generic_supertypes)
    }

    /// Snapshot all symbols owned by one exact external-source refresh.
    ///
    /// A process can index the JDK, Gradle, and Maven classpaths concurrently.
    /// Cache writers must therefore never infer ownership from the global set of
    /// external file IDs. The refresh revisions are both the membership proof
    /// and the publication fence: if any source has since been superseded, no
    /// cache snapshot is returned. A merely reserved replacement is not a
    /// publication proof: failed extraction leaves last-good symbols under the
    /// previous generation.
    pub fn get_external_symbols_for_refresh(
        &self,
        refresh: &super::ExternalSourceRefresh,
    ) -> Option<Vec<crate::Symbol>> {
        let inner = self.inner.read();
        let file_ids: HashSet<_> = refresh
            .revisions
            .values()
            .map(|revision| {
                inner
                    .external_source_revision_is_published(*revision)
                    .then_some(revision.file_id())
            })
            .collect::<Option<_>>()?;

        Some(
            file_ids
                .into_iter()
                .flat_map(|file_id| inner.file_symbols.get(&file_id).into_iter().flatten())
                .map(|symbol| (**symbol).clone())
                .collect(),
        )
    }

    /// Snapshot symbols from one exact refresh without erasing archive
    /// ownership.
    ///
    /// Persistent caches which restore source navigation must retain these
    /// partitions. Assigning the complete set to one synthetic external file
    /// loses the archive revision needed to route split packages and duplicate
    /// declarations to the matching source provider.
    pub fn get_external_symbols_by_source_for_refresh(
        &self,
        refresh: &super::ExternalSourceRefresh,
    ) -> Option<Vec<(ExternalSourceIdentity, Vec<crate::Symbol>)>> {
        let inner = self.inner.read();
        refresh
            .revisions
            .iter()
            .map(|(identity, revision)| {
                if !inner.external_source_revision_is_published(*revision) {
                    return None;
                }
                let symbols = inner
                    .file_symbols
                    .get(&revision.file_id())
                    .into_iter()
                    .flatten()
                    .map(|symbol| (**symbol).clone())
                    .collect();
                Some((identity.clone(), symbols))
            })
            .collect()
    }

    /// Snapshot complete index data from one exact refresh without erasing
    /// archive ownership or generic hierarchy edges.
    pub fn get_external_index_data_by_source_for_refresh(
        &self,
        refresh: &super::ExternalSourceRefresh,
    ) -> Option<Vec<ExternalIndexDataBySource>> {
        let inner = self.inner.read();
        refresh
            .revisions
            .iter()
            .map(|(identity, revision)| {
                if !inner.external_source_revision_is_published(*revision) {
                    return None;
                }
                let symbols = inner
                    .file_symbols
                    .get(&revision.file_id())
                    .into_iter()
                    .flatten()
                    .map(|symbol| (**symbol).clone())
                    .collect();
                let mut generic_supertypes = inner
                    .file_generic_supertypes
                    .get(&revision.file_id())
                    .into_iter()
                    .flat_map(|by_type| by_type.iter())
                    .map(|(type_fqn, edges)| crate::TypeGenericSupertypes {
                        type_fqn: type_fqn.to_string(),
                        edges: edges.to_vec(),
                    })
                    .collect::<Vec<_>>();
                generic_supertypes
                    .sort_unstable_by(|left, right| left.type_fqn.cmp(&right.type_fqn));
                Some((identity.clone(), symbols, generic_supertypes))
            })
            .collect()
    }

    /// Return source text for one exact indexed revision.
    ///
    /// This is the canonical range-to-content bridge. It never publishes or
    /// re-indexes source state. A matching disk refill may populate only the
    /// bounded byte cache after rechecking the token and display path.
    pub fn get_indexed_revision_content(
        &self,
        revision: FileRevisionToken,
    ) -> Option<crate::IndexedContent> {
        self.get_indexed_revision_content_after_read(revision, || {})
            .exact()
    }

    /// Hash-based compatibility surface for callers not migrated to tokens.
    ///
    /// A hash cannot distinguish identical re-publications. New range consumers
    /// must obtain a token together with their query result instead.
    pub fn get_indexed_file_contents(
        &self,
        file_id: FileId,
        expected_hash: u64,
    ) -> Option<crate::IndexedContent> {
        let revision = {
            let inner = self.inner.read();
            let published = inner.published_revisions.get(&file_id)?;
            (published.content_hash == expected_hash).then(|| published.token(file_id))?
        };
        self.get_indexed_revision_content(revision)
    }

    /// Compatibility convenience for callers that need the current indexed
    /// revision but do not already hold its hash.
    ///
    /// This deliberately does not mean "whatever bytes are on disk". Range
    /// consumers therefore fail closed on an evicted, externally changed
    /// source instead of pairing old offsets with new text.
    pub fn get_file_contents(&self, file_id: FileId) -> Option<Arc<String>> {
        let revision = self.current_file_revision(file_id)?;
        self.get_indexed_revision_content(revision)
            .map(|indexed| indexed.content)
    }

    #[cfg(test)]
    pub(crate) fn get_indexed_file_contents_after_read_for_test(
        &self,
        file_id: FileId,
        expected_hash: u64,
        after_read: impl FnOnce(),
    ) -> Option<crate::IndexedContent> {
        let revision = {
            let inner = self.inner.read();
            let published = inner.published_revisions.get(&file_id)?;
            (published.content_hash == expected_hash).then(|| published.token(file_id))?
        };
        self.get_indexed_revision_content_after_read(revision, after_read)
            .exact()
    }

    #[cfg(test)]
    pub(crate) fn get_file_contents_after_read_for_test(
        &self,
        file_id: FileId,
        after_read: impl FnOnce(),
    ) -> Option<Arc<String>> {
        let revision = self.current_file_revision(file_id)?;
        self.get_indexed_revision_content_after_read(revision, after_read)
            .exact()
            .map(|indexed| indexed.content)
    }

    fn get_indexed_revision_content_after_read(
        &self,
        revision: FileRevisionToken,
        after_read: impl FnOnce(),
    ) -> IndexedContentRead {
        let file_id = revision.file_id;
        let (file_path, cached, expected_hash) = {
            let inner = self.inner.read();
            let Some(published) = inner.published_revisions.get(&file_id) else {
                return IndexedContentRead::Missing;
            };
            if published.id != revision.revision_id {
                return IndexedContentRead::Missing;
            }
            let Some(file_path) = inner.file_path(file_id).map(Path::to_path_buf) else {
                return IndexedContentRead::Missing;
            };

            // `file_content_cache` and `parse_tree_cache` are published in the
            // same write transaction as `published_revisions`. Raw disk reads
            // never enter either cache, so a retained source is exact without
            // rescanning the whole file to hash it on every range conversion.
            let cached = inner
                .file_content_cache
                .peek(&file_id)
                .map(Arc::clone)
                .or_else(|| {
                    inner
                        .parse_tree_cache
                        .peek(&file_id)
                        .map(|(_, source)| Arc::clone(source))
                });
            (file_path, cached, published.content_hash)
        };

        if let Some(content) = cached {
            after_read();
            let revision_matches = {
                let inner = self.inner.read();
                inner
                    .published_revisions
                    .get(&file_id)
                    .is_some_and(|published| published.id == revision.revision_id)
                    && inner.file_path(file_id) == Some(file_path.as_path())
            };
            if !revision_matches {
                return IndexedContentRead::Missing;
            }
            return IndexedContentRead::Exact(crate::IndexedContent {
                content,
                content_hash: expected_hash,
                revision,
                file_path,
            });
        }

        let contents = match crate::read_source_file(&file_path) {
            Ok(contents) => contents,
            Err(error) => {
                warn!("Failed to read indexed file {:?}: {}", file_path, error);
                return IndexedContentRead::Missing;
            }
        };
        let content = Arc::new(contents);
        let disk_hash = compute_content_hash(&content);
        after_read();

        if disk_hash != expected_hash {
            return IndexedContentRead::DifferentDiskRevision;
        }

        // Publication may have removed, relocated, or replaced this FileId
        // while disk I/O ran. Cache only bytes proven to belong to the same
        // path/hash identity. A concurrent exact publication wins naturally.
        let mut inner = self.write_inner();
        if inner
            .published_revisions
            .get(&file_id)
            .is_none_or(|published| published.id != revision.revision_id)
            || inner.file_path(file_id) != Some(file_path.as_path())
        {
            return IndexedContentRead::Missing;
        }
        let content = inner
            .file_content_cache
            .peek(&file_id)
            .map(Arc::clone)
            .or_else(|| {
                inner
                    .parse_tree_cache
                    .peek(&file_id)
                    .map(|(_, source)| Arc::clone(source))
            })
            .unwrap_or_else(|| {
                inner.file_content_cache.put(file_id, Arc::clone(&content));
                content
            });
        IndexedContentRead::Exact(crate::IndexedContent {
            content,
            content_hash: expected_hash,
            revision,
            file_path,
        })
    }

    /// Read the file currently present on disk without consulting or mutating
    /// the indexed-source cache.
    ///
    /// These bytes are intentionally unsafe to combine with stored index
    /// ranges until an explicit validated refresh publishes them.
    pub fn get_current_file_contents(&self, file_id: FileId) -> Option<Arc<String>> {
        self.get_current_file_contents_after_read(file_id, || {})
    }

    #[cfg(test)]
    pub(crate) fn get_current_file_contents_after_read_for_test(
        &self,
        file_id: FileId,
        after_read: impl FnOnce(),
    ) -> Option<Arc<String>> {
        self.get_current_file_contents_after_read(file_id, after_read)
    }

    fn get_current_file_contents_after_read(
        &self,
        file_id: FileId,
        after_read: impl FnOnce(),
    ) -> Option<Arc<String>> {
        let (file_path, file_path_key) = {
            let inner = self.inner.read();
            let registered = inner.registered_path(file_id)?;
            (
                registered.display_path().to_path_buf(),
                registered.lookup_key().clone(),
            )
        };
        if path_lookup_key(&file_path) != file_path_key {
            return None;
        }
        let content = Arc::new(crate::read_source_file(&file_path).ok()?);
        after_read();
        if path_lookup_key(&file_path) != file_path_key {
            return None;
        }
        let inner = self.inner.read();
        (inner.file_path_key(file_id) == Some(&file_path_key)
            && inner.file_path(file_id) == Some(file_path.as_path()))
        .then_some(content)
    }

    pub fn get_file_id(&self, path: &Path) -> Option<FileId> {
        let key = path_lookup_key(path);
        self.inner.read().file_id_for_key(&key)
    }

    /// Resolve the owner of an already-established filesystem identity without
    /// recomputing it after lock acquisition or disk I/O.
    pub fn get_file_id_for_key(&self, key: &PathLookupKey) -> Option<FileId> {
        self.inner.read().file_id_for_key(key)
    }

    /// Resolve an established identity, using exact stored display spelling
    /// only when the key itself has no owner and the path is proven absent.
    ///
    /// This is the deletion/rename path for a symlink alias that broke after
    /// key capture. Key ownership always wins, so stale display text cannot
    /// shadow a live filesystem identity.
    pub fn get_file_id_for_key_or_display(
        &self,
        key: &PathLookupKey,
        path: &Path,
    ) -> Option<FileId> {
        if let Some(file_id) = self.inner.read().file_id_for_key(key) {
            return Some(file_id);
        }
        if !matches!(path.try_exists(), Ok(false)) {
            return None;
        }
        let inner = self.inner.read();
        inner
            .file_id_for_key(key)
            .or_else(|| inner.file_id_for_unique_display(path))
    }

    pub fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
        let inner = self.inner.read();
        let result = inner.file_path(file_id).map(Path::to_path_buf);
        if result.is_none() {
            debug!(
                "WORKSPACE: FAIL get_file_path failed for file_id {:?}",
                file_id
            );
            debug!("WORKSPACE: Total file_ids in map: {}", inner.paths.len());
            if inner.paths.len() < 20 {
                debug!(
                    "WORKSPACE: Available file_ids: {:?}",
                    inner.paths.file_ids().collect::<Vec<_>>()
                );
            }
        }
        result
    }

    /// Atomically capture the display spelling and filesystem identity for one
    /// current file publication.
    pub fn get_registered_file_path(&self, file_id: FileId) -> Option<RegisteredFilePath> {
        self.inner.read().registered_path(file_id).cloned()
    }

    /// Check that an earlier atomic path snapshot still names this FileId.
    pub fn is_registered_file_path_current(
        &self,
        file_id: FileId,
        registered_path: &RegisteredFilePath,
    ) -> bool {
        self.inner.read().registered_path(file_id) == Some(registered_path)
    }

    /// Return the immutable path identity captured for the current file
    /// publication.
    pub fn get_file_path_key(&self, file_id: FileId) -> Option<PathLookupKey> {
        self.inner.read().file_path_key(file_id).cloned()
    }

    pub fn get_fqns_by_simple_name(&self, simple_name: &str) -> Vec<String> {
        let inner = self.inner.read();
        let mut fqns: Vec<String> = inner
            .simple_name_index
            .get(simple_name)
            .map(|set| set.iter().map(|fqn| fqn.to_string()).collect())
            .unwrap_or_default();
        fqns.sort_unstable();
        fqns
    }

    /// Member types declared directly inside `outer_fqn` (`java.util.Map` ->
    /// `Map.Entry`). Bytecode-indexed nested types carry no `container`, so
    /// this goes by the dotted name, which both source and bytecode symbols
    /// share.
    #[must_use]
    pub fn nested_type_symbols(&self, outer_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let prefix = format!("{outer_fqn}.");
        btree_prefix_scan(&inner.symbol_locations, &prefix)
            .filter(|(fqn, _)| !fqn[prefix.len()..].contains(['.', '(', '$']))
            .filter_map(|(fqn, _)| inner.symbols.get(fqn.as_ref()))
            .filter(|symbol| crate::is_type_declaration_kind(symbol.kind))
            .cloned()
            .collect()
    }

    /// Every indexed type declaration whose simple name matches `pattern` by
    /// prefix (case-insensitive) or by camel humps (`HM` -> `HashMap`), with
    /// whether it is external (dependency/JDK).
    ///
    /// Unlike [`Self::get_fqns_by_simple_prefix`] this is not capped: a cap
    /// applied before ranking drops `ArrayList` behind a dozen internal
    /// `*.Array` classes. Callers rank and truncate. Visibility and package
    /// exposure are the caller's policy; this returns declarations only.
    pub fn type_symbols_matching(&self, pattern: &str) -> Vec<(Arc<crate::Symbol>, bool)> {
        use crate::name_match::camel_hump_matches;
        if pattern.is_empty() {
            return Vec::new();
        }
        let inner = self.inner.read();
        let pattern_lower = pattern.to_lowercase();
        let mut fqns: Vec<&Arc<str>> = Vec::new();
        for (name_lower, set) in inner.simple_name_lower_index.range(pattern_lower.clone()..) {
            if !name_lower.starts_with(&pattern_lower) {
                break;
            }
            fqns.extend(set.iter());
        }
        // Camel humps cannot be range-scanned; they need the mixed-case name.
        // Only worth the full scan when the pattern can express humps.
        let has_hump_shape = pattern.chars().count() >= 2
            && pattern.chars().skip(1).any(char::is_uppercase)
            || pattern.chars().count() >= 2 && pattern.chars().all(char::is_uppercase);
        if has_hump_shape {
            for (name, set) in &inner.simple_name_index {
                if !name.to_lowercase().starts_with(&pattern_lower)
                    && camel_hump_matches(pattern, name)
                {
                    fqns.extend(set.iter());
                }
            }
        }
        fqns.into_iter()
            .filter_map(|fqn| inner.symbols.get(fqn.as_ref()))
            .filter(|symbol| crate::is_type_declaration_kind(symbol.kind))
            .map(|symbol| {
                // `true` when the declaration comes from a dependency or the
                // JDK rather than a source file of this workspace.
                let external = inner
                    .symbol_locations
                    .get(symbol.fqn.as_ref())
                    .is_some_and(|location| inner.external_file_ids.contains(&location.file_id));
                (Arc::clone(symbol), external)
            })
            .collect()
    }

    pub fn get_fqns_by_simple_prefix(&self, prefix: &str, limit: usize) -> Vec<String> {
        if prefix.is_empty() || limit == 0 {
            return Vec::new();
        }

        let prefix_lower = prefix.to_lowercase();
        let inner = self.inner.read();
        let mut results = Vec::new();

        for (simple_name_lower, fqns) in inner.simple_name_lower_index.range(prefix_lower.clone()..)
        {
            if !simple_name_lower.starts_with(&prefix_lower) {
                break;
            }
            let mut ordered_fqns: Vec<_> = fqns.iter().collect();
            ordered_fqns.sort_unstable();
            for fqn in ordered_fqns {
                results.push(fqn.to_string());
                if results.len() >= limit {
                    return results;
                }
            }
        }

        results
    }

    pub fn get_imports(&self, file_id: FileId) -> Option<ImportMap> {
        self.inner.read().file_imports.get(&file_id).cloned()
    }

    /// `java.lang` simple names as indexed, memoized against the workspace
    /// revision that produced them.
    ///
    /// Callers resolve names through a freshly constructed `ImportResolver`,
    /// so this must be amortized on the long-lived index rather than on the
    /// resolver. It stays revision-keyed on purpose: the set is legitimately
    /// empty until JDK indexing publishes, and a one-shot cache would pin that
    /// emptiness for the session.
    #[must_use]
    pub fn java_lang_class_names(&self) -> Arc<HashSet<String>> {
        let revision = self.current_workspace_revision();
        if let Some((cached_revision, names)) = self.java_lang_names.read().as_ref() {
            if *cached_revision == revision {
                return Arc::clone(names);
            }
        }

        let names = Arc::new(
            self.get_package_class_names("java.lang")
                .into_iter()
                .collect::<HashSet<_>>(),
        );

        // Re-check under the write lock: a concurrent publication may have
        // advanced the revision while this computed, and installing a set
        // under a revision it does not describe would outlive the mismatch.
        let mut slot = self.java_lang_names.write();
        if self.current_workspace_revision() == revision {
            *slot = Some((revision, Arc::clone(&names)));
        }
        names
    }

    /// Whether the package contains at least one indexed top-level type.
    ///
    /// Callers that only need an emptiness answer must not go through
    /// `get_package_class_names`: that builds and sorts a `Vec<String>` of
    /// every class in the package. The semantic walker asks this once per
    /// import declaration on every walk, where `java.util` alone would
    /// allocate and sort hundreds of names per import.
    #[must_use]
    pub fn package_has_class_names(&self, package: &str) -> bool {
        let inner = self.inner.read();
        let prefix = format!("{}.", package);

        inner
            .package_to_classes
            .get(package)
            .is_some_and(|class_fqns| {
                class_fqns.iter().any(|fqn| {
                    let Some(remainder) = fqn.strip_prefix(&prefix) else {
                        return false;
                    };
                    if remainder.contains('.') || remainder.contains('$') {
                        return false;
                    }
                    inner.symbols.get(fqn.as_str()).is_some_and(|symbol| {
                        matches!(
                            symbol.kind,
                            SymbolKind::CLASS
                                | SymbolKind::INTERFACE
                                | SymbolKind::ENUM
                                | SymbolKind::STRUCT
                        )
                    })
                })
            })
    }

    /// Names of the packages directly below `parent` (`java` -> `util`, `io`,
    /// ...; an empty `parent` yields the root segments). A segment is included
    /// when the package itself or any package below it declares a type.
    #[must_use]
    pub fn child_package_names(&self, parent: &str) -> Vec<String> {
        let inner = self.inner.read();
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}.")
        };
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for package in inner.package_to_classes.keys() {
            let Some(rest) = package.strip_prefix(prefix.as_str()) else {
                continue;
            };
            let segment = rest.split('.').next().unwrap_or(rest);
            if !segment.is_empty() {
                names.insert(segment.to_string());
            }
        }
        names.into_iter().collect()
    }

    pub fn get_package_class_names(&self, package: &str) -> Vec<String> {
        let inner = self.inner.read();
        let prefix = format!("{}.", package);

        let mut result: Vec<String> = match inner.package_to_classes.get(package) {
            Some(class_fqns) => class_fqns
                .iter()
                .filter_map(|fqn| {
                    let remainder = fqn.strip_prefix(&prefix)?;
                    // Exclude inner classes (containing . or $)
                    if remainder.contains('.') || remainder.contains('$') {
                        return None;
                    }
                    // Only include class-like symbols
                    let symbol = inner.symbols.get(fqn.as_str())?;
                    if matches!(
                        symbol.kind,
                        SymbolKind::CLASS
                            | SymbolKind::INTERFACE
                            | SymbolKind::ENUM
                            | SymbolKind::STRUCT
                    ) {
                        Some(remainder.to_string())
                    } else {
                        None
                    }
                })
                .collect(),
            None => Vec::new(),
        };

        // Sort for deterministic results
        result.sort();
        result
    }

    pub fn get_parent_class(&self, class_fqn: &str) -> Option<String> {
        let inner = self.inner.read();
        inner.symbols.get(class_fqn)?;
        inner.resolved_supertype_roles(class_fqn).0
    }

    pub fn get_sealed_subclasses(&self, sealed_fqn: &str) -> Vec<String> {
        let inner = self.inner.read();
        if !inner
            .symbols
            .get(sealed_fqn)
            .is_some_and(|symbol| symbol.modifiers.is_sealed)
        {
            return Vec::new();
        }

        let mut subclasses: Vec<_> = inner
            .direct_subtypes(sealed_fqn)
            .into_iter()
            .map(|fqn| fqn.to_string())
            .collect();
        subclasses.sort_unstable();
        subclasses
    }

    pub fn get_symbol(&self, fqn: &str) -> Option<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        if let Some(scope) = &self.source_scope {
            return find_any_symbol_definition_in_scope(&inner, fqn, scope)
                .map(|definition| definition.symbol);
        }

        // Direct lookup (new format with params or non-method lookups)
        if let Some(sym) = inner.symbols.get(fqn) {
            if !fqn.contains('(')
                && matches!(
                    sym.kind,
                    lsp_types::SymbolKind::METHOD | lsp_types::SymbolKind::FUNCTION
                )
                && btree_prefix_scan(&inner.symbol_locations, &format!("{fqn}("))
                    .next()
                    .is_some()
            {
                return None;
            }
            return Some(Arc::clone(sym));
        }

        // Backward compatibility for a genuinely descriptorless callable.
        // An overload family is not a declaration identity: choosing its first
        // BTree entry would make navigation depend on signature ordering.
        if !fqn.contains('(') {
            let prefix = format!("{}(", fqn);
            let mut match_symbol = None;
            for (key, _) in btree_prefix_scan(&inner.symbol_locations, &prefix) {
                let Some(symbol) = inner.symbols.get(key.as_ref()) else {
                    continue;
                };
                if match_symbol.is_some() {
                    return None;
                }
                match_symbol = Some(Arc::clone(symbol));
            }
            return match_symbol;
        }

        None
    }

    pub fn get_validated_file_contents(&self, file_id: FileId) -> Option<ValidatedContent> {
        self.get_validated_file_contents_after_read(file_id, || {})
    }

    #[cfg(test)]
    pub(crate) fn get_validated_file_contents_after_read_for_test(
        &self,
        file_id: FileId,
        after_read: impl FnOnce(),
    ) -> Option<ValidatedContent> {
        self.get_validated_file_contents_after_read(file_id, after_read)
    }

    fn get_validated_file_contents_after_read(
        &self,
        file_id: FileId,
        after_read: impl FnOnce(),
    ) -> Option<ValidatedContent> {
        let mut after_read = Some(after_read);
        loop {
            let revision = self.current_file_revision(file_id)?;
            let read = self.get_indexed_revision_content_after_read(revision, || {
                if let Some(after_read) = after_read.take() {
                    after_read();
                }
            });
            match read {
                IndexedContentRead::Exact(indexed) => {
                    return Some(ValidatedContent {
                        content: indexed.content,
                        file_path: indexed.file_path,
                    });
                }
                IndexedContentRead::DifferentDiskRevision => {
                    if self.current_file_revision(file_id) != Some(revision) {
                        continue;
                    }
                    return None;
                }
                IndexedContentRead::Missing
                    if self.current_file_revision(file_id) != Some(revision) =>
                {
                    continue;
                }
                IndexedContentRead::Missing => return None,
            }
        }
    }

    pub fn has_class(&self, fqn: &str) -> bool {
        let inner = self.inner.read();
        if let Some(scope) = self.source_scope.as_ref() {
            // A scoped view only sees the JARs on its own compile classpath:
            // main sources must not resolve test-only dependencies.
            return OwnerElection::new(&inner, Some(scope))
                .type_definition(fqn)
                .is_some();
        }
        inner
            .symbols
            .get(fqn)
            .is_some_and(|symbol| crate::is_type_declaration_kind(symbol.kind))
    }

    /// What a Kotlin typealias expands to (`kotlin.Comparator` to
    /// `java.util.Comparator`), when this view can see the facade declaring it.
    pub(crate) fn kotlin_type_alias_expansion(
        &self,
        alias_fqn: &str,
    ) -> Option<super::KotlinTypeAliasExpansion> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        inner.expand_kotlin_type_alias(alias_fqn, |owner| election.symbol(owner))
    }

    /// FQNs of the published facades that declare the Kotlin typealias
    /// `alias_fqn`, for a view that looks the facades up itself.
    pub(crate) fn kotlin_type_alias_facade_fqns(&self, alias_fqn: &str) -> Vec<Arc<str>> {
        self.inner
            .read()
            .kotlin_type_alias_facade_fqns(alias_fqn)
            .to_vec()
    }

    pub fn is_external_file(&self, file_id: FileId) -> bool {
        self.inner.read().external_file_ids.contains(&file_id)
    }

    pub fn is_sealed(&self, fqn: &str) -> bool {
        let inner = self.inner.read();
        inner
            .symbols
            .get(fqn)
            .is_some_and(|sym| sym.modifiers.is_sealed)
    }

    pub fn search(&self, query: &str) -> Vec<SymbolLocation> {
        self.search_cancellable(query, || false)
    }

    /// Search workspace symbols with exact per-file publication identities.
    pub fn search_revisioned(&self, query: &str) -> RevisionedLocations {
        self.search_revisioned_cancellable(query, || false)
    }

    pub fn search_revisioned_cancellable(
        &self,
        query: &str,
        is_cancelled: impl Fn() -> bool,
    ) -> RevisionedLocations {
        let inner = self.inner.read();
        let locations = search_locations_in(&inner, query, &is_cancelled);
        group_revisioned_locations(&inner, locations)
    }

    /// Search for symbols, stopping at a natural index-loop boundary when cancelled.
    pub fn search_cancellable(
        &self,
        query: &str,
        is_cancelled: impl Fn() -> bool,
    ) -> Vec<SymbolLocation> {
        let inner = self.inner.read();
        search_locations_in(&inner, query, &is_cancelled)
    }

    /// Get concrete workspace CLASS/ENUM locations with an early-exit limit.
    ///
    /// This avoids the O(N) lowercase+contains scan that `search("")` performs
    /// when constructor completion has no prefix. Eligibility is evaluated
    /// entirely from the coherent index snapshot: caller callbacks must not run
    /// while `inner` is locked because they can re-enter the index and deadlock
    /// behind a queued writer.
    pub fn get_workspace_instantiable_class_locations(
        &self,
        limit: usize,
        excluded_fqns: &HashSet<Arc<str>>,
    ) -> Vec<SymbolLocation> {
        let inner = self.inner.read();
        let mut results = Vec::new();

        for loc in inner.symbol_locations.values() {
            if results.len() >= limit {
                break;
            }
            if !matches!(loc.kind, SymbolKind::CLASS | SymbolKind::ENUM) {
                continue;
            }
            if inner.external_file_ids.contains(&loc.file_id) {
                continue;
            }
            if excluded_fqns.contains(&loc.fqn) {
                continue;
            }
            let is_instantiable = inner
                .symbols
                .get(loc.fqn.as_ref())
                .is_some_and(|symbol| !symbol.modifiers.is_abstract);
            if !is_instantiable {
                continue;
            }
            results.push(stamp_source_revision(&inner, loc));
        }

        results
    }

    /// Get all file IDs in the workspace (excluding external files like JDK).
    ///
    /// This is useful for cross-file analysis that needs to iterate over
    /// all workspace files.
    pub fn all_file_ids(&self) -> Vec<FileId> {
        let inner = self.inner.read();
        inner
            .paths
            .file_ids()
            .filter(|id| !inner.external_file_ids.contains(id))
            .copied()
            .collect()
    }

    /// Snapshot indexed workspace files below one directory, ordered by their
    /// authoritative display paths.
    ///
    /// File-watcher directory reconciliation cannot enumerate the old path
    /// after a rename. This query provides that missing side without exposing
    /// external archive identities or holding the index lock during I/O.
    pub fn workspace_files_under(&self, directory: &Path) -> Vec<(FileId, PathBuf)> {
        self.workspace_files_under_bounded(directory, usize::MAX).0
    }

    /// Return at most `limit` matching workspace files. The boolean reports
    /// whether more matches exist, allowing bounded callers to fail closed.
    pub fn workspace_files_under_bounded(
        &self,
        directory: &Path,
        limit: usize,
    ) -> (Vec<(FileId, PathBuf)>, bool) {
        let directory_key = path_lookup_key(directory);
        let inner = self.inner.read();
        let mut files = Vec::with_capacity(limit.min(inner.paths.len()));
        let mut truncated = false;
        for (file_id, registered) in inner.paths.iter() {
            if inner.external_file_ids.contains(file_id)
                || !registered.lookup_key().starts_with(&directory_key)
            {
                continue;
            }
            if files.len() == limit {
                truncated = true;
                break;
            }
            files.push((*file_id, registered.display_path().to_path_buf()));
        }
        files.sort_unstable_by(|left, right| left.1.cmp(&right.1));
        (files, truncated)
    }

    /// Return every workspace file that contributes a declaration for `fqn`.
    ///
    /// Most FQNs have one defining file, but languages such as Kotlin can
    /// declare overloads in separate files while the current symbol table
    /// still uses their shared base FQN. The reverse declaration index keeps
    /// this query proportional to the number of definitions returned.
    pub fn find_files_defining_symbol(&self, fqn: &str) -> Vec<FileId> {
        let inner = self.inner.read();
        let mut file_ids = inner
            .symbol_definition_files
            .get(fqn)
            .into_iter()
            .flatten()
            .map(|candidate| candidate.file_id)
            .filter(|file_id| !inner.external_file_ids.contains(file_id))
            .collect::<Vec<_>>();
        file_ids.sort_unstable_by_key(|file_id| file_id.as_u32());
        file_ids.dedup();
        file_ids
    }

    /// Return every workspace file contributing any declaration in `fqns`.
    /// Duplicate definitions remain visible even though `symbol_locations`
    /// selects one active declaration for ordinary navigation.
    pub fn find_files_defining_symbols(&self, fqns: &[Arc<str>]) -> Vec<FileId> {
        let inner = self.inner.read();
        let mut file_ids = HashSet::new();
        for fqn in fqns {
            if let Some(definitions) = inner.symbol_definition_files.get(fqn.as_ref()) {
                file_ids.extend(
                    definitions
                        .iter()
                        .map(|candidate| candidate.file_id)
                        .filter(|file_id| !inner.external_file_ids.contains(file_id)),
                );
            }
        }
        let mut file_ids: Vec<_> = file_ids.into_iter().collect();
        file_ids.sort_unstable_by_key(|file_id| file_id.as_u32());
        file_ids
    }
}
