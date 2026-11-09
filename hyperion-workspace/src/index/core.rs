// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Core file indexing and management operations
//!
//! This module handles:
//! - File indexing (index_file_*, index_jar_file, index_jmod_file)
//! - External symbol management (add_external_symbol, add_external_symbols_batch)
//! - File removal (remove_file)

mod external_publication;

use super::*;
use tracing::{debug, info, warn};

/// Remove a value from a `HashMap<K, HashSet<String>>`, cleaning up the key if the set becomes empty.
fn remove_from_set_index<K, Q>(map: &mut HashMap<K, HashSet<String>>, key: &Q, value: &str)
where
    K: Eq + std::hash::Hash + std::borrow::Borrow<Q>,
    Q: Eq + std::hash::Hash + ?Sized,
{
    if let Some(entries) = map.get_mut(key) {
        entries.remove(value);
        if entries.is_empty() {
            map.remove(key);
        }
    }
}

/// Remove a shared string value from a secondary symbol index.
///
/// The FQN values in these indices share the allocation owned by `Symbol::fqn`;
/// `Arc<str>` supports borrowed `str` lookup, so removal remains allocation-free.
fn remove_from_arc_set_index<K, Q>(map: &mut HashMap<K, HashSet<Arc<str>>>, key: &Q, value: &str)
where
    K: Eq + std::hash::Hash + std::borrow::Borrow<Q>,
    Q: Eq + std::hash::Hash + ?Sized,
{
    if let Some(entries) = map.get_mut(key) {
        entries.remove(value);
        if entries.is_empty() {
            map.remove(key);
        }
    }
}

fn remove_from_btree_arc_set_index(
    map: &mut BTreeMap<String, HashSet<Arc<str>>>,
    key: &str,
    value: &str,
) {
    if let Some(entries) = map.get_mut(key) {
        entries.remove(value);
        if entries.is_empty() {
            map.remove(key);
        }
    }
}

fn remove_from_btree_smallvec_index(
    map: &mut BTreeMap<String, SmallVec<[Arc<str>; 1]>>,
    key: &str,
    value: &str,
) {
    if let Some(entries) = map.get_mut(key) {
        entries.retain(|entry| entry.as_ref() != value);
        if entries.is_empty() {
            map.remove(key);
        }
    }
}

fn remove_symbol_from_secondary_indices(
    inner: &mut IndexInner,
    removed_symbol: Option<&crate::Symbol>,
    fqn: &str,
) {
    let simple_name = removed_symbol
        .map(|s| s.simple_name.as_ref())
        .or_else(|| fqn.split('.').next_back())
        .unwrap_or(fqn);

    remove_from_arc_set_index(&mut inner.simple_name_index, simple_name, fqn);

    let simple_name_lower = simple_name.to_lowercase();
    remove_from_btree_arc_set_index(&mut inner.simple_name_lower_index, &simple_name_lower, fqn);

    if let Some(symbol) = removed_symbol {
        let location = inner.symbol_locations.get(fqn);
        let imports = location
            .and_then(|location| inner.file_imports.get(&location.file_id))
            .cloned();
        let language = location
            .and_then(|location| inner.file_path(location.file_id))
            .and_then(Language::from_path);
        // Constructor index: keyed by container FQN
        if symbol.kind == SymbolKind::CONSTRUCTOR {
            if let Some(container) = symbol
                .container
                .as_deref()
                .or_else(|| fqn.rsplit_once('.').map(|(c, _)| c))
            {
                remove_from_set_index(&mut inner.constructor_index, container, fqn);
            }
        }

        inner.inheritance.remove(symbol, imports.as_ref(), language);

        // Nested class index: keyed by outer class FQN
        if let Some(outer_class_fqn) = &symbol.container {
            remove_from_set_index(&mut inner.nested_class_index, outer_class_fqn.as_ref(), fqn);
        }

        // Package-to-classes: keyed by package name (top-level classes only)
        if symbol.container.is_none() && crate::is_type_declaration_kind(symbol.kind) {
            if let Some(package) = fqn.rsplit_once('.').map(|x| x.0) {
                remove_from_set_index(&mut inner.package_to_classes, package, fqn);
            }
        }

        // Kotlin typealiases: keyed by alias FQN, valued by the declaring facade
        for alias in symbol
            .kotlin
            .iter()
            .flat_map(|kotlin| kotlin.type_aliases.iter())
        {
            if let Some(owners) = inner.kotlin_type_alias_owners.get_mut(alias.fqn.as_str()) {
                owners.retain(|owner| owner.as_ref() != fqn);
                if owners.is_empty() {
                    inner.kotlin_type_alias_owners.remove(alias.fqn.as_str());
                }
            }
        }

        // `owned_declarations_index` is the union of active and shadowed source
        // declarations. Per-source registration owns its maintenance; active
        // declaration replacement must not erase another source's member key.
    }

    // Clean up the exact FQN without dropping case-distinct declarations that
    // share the same case-folded search key.
    remove_from_btree_smallvec_index(&mut inner.fqn_lower_index, &fqn.to_lowercase(), fqn);
}

/// Remove the previous indexed shape of a symbol before overwriting it.
///
/// Incoming reverse dependencies belong to users of the FQN, not to this
/// particular declaration revision, so replacements preserve them.
fn remove_symbol_for_replacement(inner: &mut IndexInner, fqn: &str) {
    // A declaration readers can already see is going away. Append-only
    // publication uses this count to detect that its inputs displaced
    // something after all: see `IndexInner::displaced_elections`.
    inner.displaced_elections = inner.displaced_elections.saturating_add(1);
    let removed_symbol = inner.symbols.remove(fqn);
    remove_symbol_from_secondary_indices(inner, removed_symbol.as_deref(), fqn);
    inner.symbol_locations.remove(fqn);
}

/// Compare the current classpath ranks of two external sources.
///
/// Directly allocated/legacy sources fall back to allocation order. Ranked
/// groups always precede unranked sources, and owner names break otherwise
/// equal ranks deterministically when a source belongs to multiple classpaths.
fn external_source_ordering(
    inner: &IndexInner,
    candidate: FileId,
    current: FileId,
) -> std::cmp::Ordering {
    match (
        inner.external_source_precedence.get(&candidate),
        inner.external_source_precedence.get(&current),
    ) {
        (Some(candidate), Some(current)) => candidate.cmp(current),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        // Higher legacy FileIds were allocated first and therefore precede.
        (None, None) => current.as_u32().cmp(&candidate.as_u32()),
    }
}

fn immediate_owner_fqn(symbol: &crate::Symbol) -> &str {
    symbol.container.as_deref().unwrap_or(symbol.fqn.as_ref())
}

fn remove_external_source_metadata(inner: &mut IndexInner, file_id: FileId) {
    inner
        .external_source_publications_in_progress
        .remove(&file_id);
    inner.published_external_source_generations.remove(&file_id);
    if !inner.external_file_ids.remove(&file_id) {
        return;
    }

    // Invalidate every extraction token before retiring the stable identity.
    // A task that already holds `file_id` must never be able to publish after
    // the source's last owner released it.
    inner.external_source_generations.remove(&file_id);

    let Some(source) = inner.external_file_id_to_source.remove(&file_id) else {
        return;
    };
    inner.external_source_ids.remove(source.key());
    for group in inner.external_source_groups.values_mut() {
        group
            .sources
            .retain(|candidate| candidate.key() != source.key());
    }
    inner
        .external_source_groups
        .retain(|_, group| !group.sources.is_empty());
}

/// Compare declaration authority for any two sources.
///
/// Workspace declarations always shadow dependencies. Duplicate workspace
/// declarations are ordered by their normalized path, then FileId. Path order
/// is stable across parallel indexing, cache HashMap iteration, and FileId
/// relocation. External declarations retain classpath rank semantics.
fn source_ordering(inner: &IndexInner, left: FileId, right: FileId) -> std::cmp::Ordering {
    match (
        inner.external_file_ids.contains(&left),
        inner.external_file_ids.contains(&right),
    ) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        (true, true) => external_source_ordering(inner, left, right),
        (false, false) => inner
            .file_path_key(left)
            .cmp(&inner.file_path_key(right))
            .then_with(|| left.as_u32().cmp(&right.as_u32())),
    }
}

fn declaration_ordering(left: &crate::Symbol, right: &crate::Symbol) -> std::cmp::Ordering {
    // Prefer nominal declarations when Java's distinct namespaces collapse to
    // the textual FQN key used by the current public index. This is a
    // conservative bridge until the index has a typed SymbolId: hierarchy and
    // nested-owner coherence are safer than publishing a same-named field.
    crate::is_type_declaration_kind(right.kind)
        .cmp(&crate::is_type_declaration_kind(left.kind))
        .then_with(|| left.range.start.cmp(&right.range.start))
        .then_with(|| left.range.end.cmp(&right.range.end))
        .then_with(|| left.container.cmp(&right.container))
        .then_with(|| left.simple_name.cmp(&right.simple_name))
        .then_with(|| left.return_type.cmp(&right.return_type))
}

fn source_candidate_symbol<'a>(
    inner: &'a IndexInner,
    candidate: &SourceSymbolCandidate,
) -> Option<&'a Arc<crate::Symbol>> {
    inner
        .file_symbols
        .get(&candidate.file_id)?
        .get(candidate.ordinal as usize)
}

pub(crate) fn candidate_from_source_matching(
    inner: &IndexInner,
    file_id: FileId,
    fqn: &str,
    predicate: impl Fn(&crate::Symbol) -> bool,
) -> Option<(FileId, Arc<crate::Symbol>)> {
    inner
        .symbol_definition_files
        .get(fqn)?
        .iter()
        .filter_map(|candidate| {
            (candidate.file_id == file_id)
                .then(|| source_candidate_symbol(inner, candidate))
                .flatten()
        })
        .filter(|symbol| predicate(symbol))
        .min_by(|left, right| declaration_ordering(left, right))
        .map(|symbol| (file_id, Arc::clone(symbol)))
}

pub(crate) fn preferred_symbol_candidate_matching(
    inner: &IndexInner,
    fqn: &str,
    predicate: impl Fn(&crate::Symbol) -> bool,
) -> Option<(FileId, Arc<crate::Symbol>)> {
    preferred_symbol_candidate_matching_excluding(inner, fqn, predicate, |_, _| false)
}

pub(crate) fn preferred_symbol_candidate_matching_excluding(
    inner: &IndexInner,
    fqn: &str,
    predicate: impl Fn(&crate::Symbol) -> bool,
    excluded: impl Fn(FileId, &crate::Symbol) -> bool,
) -> Option<(FileId, Arc<crate::Symbol>)> {
    inner
        .symbol_definition_files
        .get(fqn)?
        .iter()
        .filter_map(|candidate| {
            source_candidate_symbol(inner, candidate).map(|symbol| (candidate, symbol))
        })
        .filter(|(_, symbol)| predicate(symbol))
        .filter(|(candidate, symbol)| !excluded(candidate.file_id, symbol))
        .min_by(|left, right| {
            source_ordering(inner, left.0.file_id, right.0.file_id)
                .then_with(|| declaration_ordering(left.1, right.1))
        })
        .map(|(candidate, symbol)| (candidate.file_id, Arc::clone(symbol)))
}

fn publish_elected_symbol(
    inner: &mut IndexInner,
    fqn: &str,
    elected: Option<(FileId, Arc<crate::Symbol>)>,
) -> bool {
    let unchanged = elected.as_ref().is_some_and(|(file_id, symbol)| {
        inner
            .symbol_locations
            .get(fqn)
            .is_some_and(|location| location.file_id == *file_id)
            && inner
                .symbols
                .get(fqn)
                .is_some_and(|active| Arc::ptr_eq(active, symbol))
    });
    if unchanged {
        return false;
    }
    if inner.symbols.contains_key(fqn) {
        remove_symbol_for_replacement(inner, fqn);
    }
    if let Some((file_id, symbol)) = elected {
        insert_symbol_arc_into_indices(inner, symbol, file_id);
    }
    true
}

#[derive(Default)]
struct AffectedOwnerShape {
    members: HashMap<Arc<str>, HashSet<Arc<str>>>,
    declaration_parents: HashMap<Arc<str>, Arc<str>>,
}

fn candidate_parent(inner: &IndexInner, fqn: &str) -> Option<Arc<str>> {
    preferred_symbol_candidate_matching(inner, fqn, |_| true)
        .and_then(|(_, symbol)| symbol.container.clone())
}

fn root_owner_for_affected_symbol(
    inner: &IndexInner,
    affected: &AffectedOwnerShape,
    symbol: &crate::Symbol,
) -> Arc<str> {
    let mut owner: Arc<str> = Arc::from(immediate_owner_fqn(symbol));
    let mut visited = HashSet::new();
    while visited.insert(Arc::clone(&owner)) {
        let parent = affected
            .declaration_parents
            .get(owner.as_ref())
            .cloned()
            .or_else(|| candidate_parent(inner, &owner));
        let Some(parent) = parent else {
            break;
        };
        owner = parent;
    }
    owner
}

fn declaration_owns_candidates(
    inner: &IndexInner,
    fqn: &str,
    affected: &AffectedOwnerShape,
) -> bool {
    inner
        .owned_declarations_index
        .get(fqn)
        .is_some_and(|members| !members.is_empty())
        || affected
            .members
            .get(fqn)
            .is_some_and(|members| !members.is_empty())
}

fn clear_owner_descendants(
    inner: &mut IndexInner,
    owner_fqn: &str,
    affected: &AffectedOwnerShape,
    visiting: &mut HashSet<Arc<str>>,
) -> usize {
    let mut member_fqns: HashSet<_> = inner
        .owned_declarations_index
        .get(owner_fqn)
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    if let Some(additional) = affected.members.get(owner_fqn) {
        member_fqns.extend(additional.iter().cloned());
    }
    let mut changed = 0;
    for member_fqn in member_fqns {
        changed += usize::from(publish_elected_symbol(inner, &member_fqn, None));
        if declaration_owns_candidates(inner, &member_fqn, affected)
            && visiting.insert(Arc::clone(&member_fqn))
        {
            changed += clear_owner_descendants(inner, &member_fqn, affected, visiting);
            visiting.remove(member_fqn.as_ref());
        }
    }
    changed
}

/// Elect one declaration owner and recursively publish exactly the declaration
/// tree contributed by the same source. Nominal types, callables, properties,
/// and anonymous object literals can all own declarations in JVM source
/// languages; gating only type members leaks descendants from shadowed files.
fn reelect_owner_tree(
    inner: &mut IndexInner,
    owner_fqn: &str,
    required_source: Option<FileId>,
    required_container: Option<&str>,
    affected: &AffectedOwnerShape,
    visiting: &mut HashSet<Arc<str>>,
) -> usize {
    let owner = match required_source {
        Some(file_id) => candidate_from_source_matching(inner, file_id, owner_fqn, |symbol| {
            symbol.container.as_deref() == required_container
        }),
        None => preferred_symbol_candidate_matching(inner, owner_fqn, |symbol| {
            symbol.container.is_none()
        }),
    };
    let owner_source = owner.as_ref().map(|(file_id, _)| *file_id);
    let mut changed = usize::from(publish_elected_symbol(inner, owner_fqn, owner));
    let mut member_fqns: HashSet<_> = inner
        .owned_declarations_index
        .get(owner_fqn)
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    if let Some(additional) = affected.members.get(owner_fqn) {
        member_fqns.extend(additional.iter().cloned());
    }

    for member_fqn in member_fqns {
        let elected = if let Some(file_id) = owner_source {
            candidate_from_source_matching(inner, file_id, &member_fqn, |symbol| {
                symbol.container.as_deref() == Some(owner_fqn)
            })
        } else {
            // Some legacy dependency extractors publish members without an
            // owner declaration. Preserve those non-nominal declarations,
            // while never fabricating a nested/local type tree without its
            // exact source owner.
            preferred_symbol_candidate_matching(inner, &member_fqn, |symbol| {
                !crate::is_type_declaration_kind(symbol.kind)
                    && symbol.container.as_deref() == Some(owner_fqn)
            })
        };

        if elected.is_some() && declaration_owns_candidates(inner, &member_fqn, affected) {
            if visiting.insert(Arc::clone(&member_fqn)) {
                changed += reelect_owner_tree(
                    inner,
                    &member_fqn,
                    owner_source,
                    Some(owner_fqn),
                    affected,
                    visiting,
                );
                visiting.remove(member_fqn.as_ref());
            }
        } else {
            changed += usize::from(publish_elected_symbol(inner, &member_fqn, elected));
            if declaration_owns_candidates(inner, &member_fqn, affected)
                && visiting.insert(Arc::clone(&member_fqn))
            {
                changed += clear_owner_descendants(inner, &member_fqn, affected, visiting);
                visiting.remove(member_fqn.as_ref());
            }
        }
    }
    changed
}

pub(super) fn reelect_affected_symbols(
    inner: &mut IndexInner,
    affected_symbols: impl IntoIterator<Item = Arc<crate::Symbol>>,
) -> usize {
    reelect_affected_symbols_with_mode(inner, affected_symbols, ElectionAdvance::Rebinding)
}

/// Whether an election's generation advance may invalidate readers that
/// captured an earlier generation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ElectionAdvance {
    /// Declarations may have been retracted, replaced, or re-bound.
    Rebinding,
    /// Owner-complete declarations were appended without retracting anything.
    /// No previously visible declaration changed, so concurrent readers stay
    /// coherent: every intermediate state is one a legal publication order
    /// could have produced on its own.
    AppendOnly,
}

/// Append-only external election: see [`ElectionAdvance`].
pub(super) fn reelect_appended_symbols(
    inner: &mut IndexInner,
    affected_symbols: impl IntoIterator<Item = Arc<crate::Symbol>>,
) -> usize {
    reelect_affected_symbols_with_mode(inner, affected_symbols, ElectionAdvance::AppendOnly)
}

fn reelect_affected_symbols_with_mode(
    inner: &mut IndexInner,
    affected_symbols: impl IntoIterator<Item = Arc<crate::Symbol>>,
    advance: ElectionAdvance,
) -> usize {
    // Candidate, import, classpath, and removal mutations are staged before
    // election under this same write lock. Advance the coherent contextual
    // generation even if authority ultimately elects the same declaration:
    // ambiguity and lazy source-name resolution may still have changed.
    let appended_revision = match advance {
        ElectionAdvance::Rebinding => {
            inner.advance_workspace_revision();
            None
        }
        ElectionAdvance::AppendOnly => Some(inner.advance_workspace_revision_additively()),
    };
    let displaced_before = inner.displaced_elections;
    let affected_symbols = affected_symbols.into_iter().collect::<Vec<_>>();
    let mut affected = AffectedOwnerShape::default();
    for symbol in &affected_symbols {
        if let Some(container) = &symbol.container {
            affected
                .members
                .entry(Arc::clone(container))
                .or_default()
                .insert(Arc::clone(&symbol.fqn));
            affected
                .declaration_parents
                .insert(Arc::clone(&symbol.fqn), Arc::clone(container));
        }
    }
    let mut roots = affected_symbols
        .iter()
        .map(|symbol| root_owner_for_affected_symbol(inner, &affected, symbol))
        .collect::<Vec<_>>();
    roots.sort_unstable();
    roots.dedup();
    let mut visiting = HashSet::new();
    let changed = roots
        .into_iter()
        .map(|root| {
            visiting.insert(Arc::clone(&root));
            let changed = reelect_owner_tree(inner, &root, None, None, &affected, &mut visiting);
            visiting.remove(root.as_ref());
            changed
        })
        .sum();
    // Classification is an outcome, not an assumption: if this pass replaced or
    // retracted a declaration readers could already see, retire their captured
    // generations before the write lock is released.
    if let Some(revision) = appended_revision {
        if inner.displaced_elections != displaced_before {
            inner.promote_revision_to_rebinding(revision);
        }
    }
    changed
}

fn reelect_all_external_symbols(inner: &mut IndexInner) -> usize {
    let affected = inner
        .file_symbols
        .iter()
        .filter(|(file_id, _)| inner.external_file_ids.contains(file_id))
        .flat_map(|(_, symbols)| symbols.iter().cloned())
        .collect::<Vec<_>>();
    reelect_affected_symbols(inner, affected)
}

/// Remove one source's canonical candidates and atomically elect surviving
/// workspace declarations before considering dependency candidates.
pub(super) fn unregister_source_symbols_without_election(
    inner: &mut IndexInner,
    file_id: FileId,
) -> Vec<Arc<crate::Symbol>> {
    inner.take_file_generic_supertypes(file_id);
    inner.take_file_symbols(file_id)
}

fn remove_symbols_for_file_internal(inner: &mut IndexInner, file_id: FileId) -> Vec<Arc<str>> {
    let was_external = inner.external_file_ids.contains(&file_id);
    if was_external {
        // Declaration election must observe the surviving sources' new ranks.
        // Remove metadata before unregistering, rebuild after unregistering so
        // the retired source can never be momentarily re-elected as unranked.
        remove_external_source_metadata(inner, file_id);
    }
    let symbols = unregister_source_symbols_without_election(inner, file_id);
    let mut fqns: Vec<_> = symbols
        .iter()
        .map(|symbol| Arc::clone(&symbol.fqn))
        .collect();
    // Callers invalidate from these names, so add the typealiases the removed
    // revision declared.
    fqns.extend(super::inheritance::declared_kotlin_type_alias_fqns(
        symbols.iter().map(AsRef::as_ref),
    ));
    if was_external {
        inner.rebuild_external_source_precedence();
    }
    reelect_affected_symbols(inner, symbols);
    if was_external {
        reelect_all_external_symbols(inner);
    }
    fqns
}

pub(super) fn remove_symbols_for_file(inner: &mut IndexInner, file_id: FileId) -> Vec<Arc<str>> {
    remove_symbols_for_file_internal(inner, file_id)
}

/// Remove all usage locations contributed by a specific file.
///
/// Uses `file_usage_counts` for targeted cleanup. Falls back to a full scan when the map
/// has no entry (e.g., indexes created before this metadata was tracked).
fn remove_symbol_usages_for_file(
    inner: &mut IndexInner,
    file_id: FileId,
) -> HashMap<Arc<str>, u32> {
    inner.file_method_usages.remove(&file_id);
    if let Some(usage_counts) = inner.file_usage_counts.remove(&file_id) {
        for fqn in usage_counts.keys() {
            if let Some(usages) = inner.symbol_usages.get_mut(fqn.as_ref()) {
                usages.retain(|loc| loc.file_id != file_id);
                if usages.is_empty() {
                    inner.symbol_usages.remove(fqn.as_ref());
                }
            }
        }
        return usage_counts;
    }

    // Compatibility path for legacy state without file_usage_counts.
    let mut removed = HashMap::new();
    for usages in inner.symbol_usages.values_mut() {
        for usage in usages.iter().filter(|usage| usage.file_id == file_id) {
            *removed.entry(Arc::clone(&usage.fqn)).or_insert(0) += 1;
        }
        usages.retain(|loc| loc.file_id != file_id);
    }
    inner.symbol_usages.retain(|_, usages| !usages.is_empty());
    removed
}

fn count_usage_fqns(fqns: &[Arc<str>]) -> HashMap<Arc<str>, u32> {
    let mut counts = HashMap::new();
    for fqn in fqns {
        let count = counts.entry(Arc::clone(fqn)).or_insert(0_u32);
        *count = count.saturating_add(1);
    }
    counts
}

const LEXICAL_HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const LEXICAL_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;
const LEXICAL_BLOOM_WORDS: usize = 64;
const LEXICAL_BLOOM_BITS: usize = LEXICAL_BLOOM_WORDS * u64::BITS as usize;

#[derive(Clone)]
pub(crate) struct LexicalIdentifierBloom {
    words: [u64; LEXICAL_BLOOM_WORDS],
}

impl LexicalIdentifierBloom {
    fn new() -> Self {
        Self {
            words: [0; LEXICAL_BLOOM_WORDS],
        }
    }

    fn bit_indices(hash: u64) -> [usize; 3] {
        [
            hash as usize & (LEXICAL_BLOOM_BITS - 1),
            hash.rotate_left(21) as usize & (LEXICAL_BLOOM_BITS - 1),
            hash.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(42) as usize
                & (LEXICAL_BLOOM_BITS - 1),
        ]
    }

    fn insert(&mut self, hash: u64) {
        for bit in Self::bit_indices(hash) {
            self.words[bit / u64::BITS as usize] |= 1_u64 << (bit % u64::BITS as usize);
        }
    }

    pub(super) fn might_contain(&self, hash: u64) -> bool {
        Self::bit_indices(hash).into_iter().all(|bit| {
            self.words[bit / u64::BITS as usize] & (1_u64 << (bit % u64::BITS as usize)) != 0
        })
    }
}

pub(super) fn ascii_identifier_hash(identifier: &str) -> Option<u64> {
    let bytes = identifier.as_bytes();
    if bytes.is_empty()
        || !matches!(bytes[0], b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$')
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
    {
        return None;
    }
    Some(hash_identifier_bytes(bytes))
}

fn hash_identifier_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(LEXICAL_HASH_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(LEXICAL_HASH_PRIME)
    })
}

/// Build a compact, conservative lexical selector for one exact source.
///
/// A hash collision only admits an extra parse. Java Unicode escapes can form
/// identifier characters before lexing, so any source containing `\\u` is
/// left unfiltered rather than risking a false negative.
pub(super) fn lexical_identifier_hashes(source: &str) -> Option<Box<LexicalIdentifierBloom>> {
    if source.as_bytes().windows(2).any(|window| window == br"\u") {
        return None;
    }

    let bytes = source.as_bytes();
    let mut identifiers = LexicalIdentifierBloom::new();
    let mut start = 0;
    while start < bytes.len() {
        while start < bytes.len()
            && !(bytes[start].is_ascii_alphanumeric() || matches!(bytes[start], b'_' | b'$'))
        {
            start += 1;
        }
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'$'))
        {
            end += 1;
        }
        if end > start {
            let identifier = &bytes[start..end];
            if matches!(identifier[0], b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') {
                identifiers.insert(hash_identifier_bytes(identifier));
            }
        }
        start = end;
    }
    Some(Box::new(identifiers))
}

fn merge_usage_counts(destination: &mut HashMap<Arc<str>, u32>, source: HashMap<Arc<str>, u32>) {
    for (fqn, count) in source {
        let total = destination.entry(fqn).or_insert(0);
        *total = total.saturating_add(count);
    }
}

/// Insert a symbol into the core searchable indices (no inheritance tracking).
///
/// This is the single source of truth for symbol insertion into:
/// 1. `symbol_locations` — where the symbol is defined
/// 2. `symbols` — full symbol metadata
/// 3. `simple_name_index` — name → FQN lookup
/// 4. `simple_name_lower_index` — case-insensitive name → FQN lookup
/// 5. `constructor_index` — class FQN → constructor FQNs
///
/// All 4 insertion paths (workspace indexing, batch JAR/JMOD, single external,
/// batch external) use this function indirectly through
/// [`insert_symbol_into_indices`].
fn insert_symbol_to_core_indices(
    inner: &mut IndexInner,
    symbol: Arc<crate::Symbol>,
    file_id: FileId,
) {
    let fqn = Arc::clone(&symbol.fqn);
    // 1. Full symbol data. All secondary indices below reuse the FQN allocation
    // owned by the stored symbol rather than an equal caller-owned allocation.
    inner.symbols.insert(Arc::clone(&fqn), Arc::clone(&symbol));
    let canonical_fqn = Arc::clone(&symbol.fqn);

    // 2. Symbol location
    let location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: symbol.range,
        kind: symbol.kind,
        name: symbol.simple_name.clone(),
        fqn: canonical_fqn.clone(),
        container_name: symbol.container.clone(),
        documentation: symbol.documentation.clone(),
    };
    inner
        .symbol_locations
        .insert(canonical_fqn.clone(), location);

    // 3. Simple name index
    inner
        .simple_name_index
        .entry(symbol.simple_name.clone())
        .or_default()
        .insert(canonical_fqn.clone());

    // 4. Lowercase simple name index
    let simple_name_lower = symbol.simple_name.to_lowercase();
    inner
        .simple_name_lower_index
        .entry(simple_name_lower)
        .or_default()
        .insert(canonical_fqn.clone());

    // 5. Constructor index
    if symbol.kind == SymbolKind::CONSTRUCTOR {
        if let Some(container) = symbol
            .container
            .clone()
            .or_else(|| fqn.rsplit_once('.').map(|(c, _)| Arc::from(c)))
        {
            inner
                .constructor_index
                .entry(container)
                .or_default()
                .insert(canonical_fqn.to_string());
        }
    }

    // 6. `owned_declarations_index` is maintained by the canonical per-source
    // candidate registry, not by whichever declaration happens to be active.

    // Kotlin typealiases declared by an active file facade, library or source.
    for alias in symbol
        .kotlin
        .iter()
        .flat_map(|kotlin| kotlin.type_aliases.iter())
    {
        let owners = inner
            .kotlin_type_alias_owners
            .entry(Arc::from(alias.fqn.as_str()))
            .or_default();
        if !owners.contains(&canonical_fqn) {
            owners.push(Arc::clone(&canonical_fqn));
        }
    }

    // 7. Pre-lowercased FQN index for workspace symbol search
    let lowercase_fqn = canonical_fqn.to_lowercase();
    let case_variants = inner.fqn_lower_index.entry(lowercase_fqn).or_default();
    if let Err(position) =
        case_variants.binary_search_by(|existing| existing.as_ref().cmp(canonical_fqn.as_ref()))
    {
        case_variants.insert(position, canonical_fqn);
    }
}

/// Insert a symbol into every index atomically with respect to `IndexInner`.
///
/// Callers should use this instead of pairing core and structural insertion by
/// hand. On overwrite, the old declaration's secondary entries are removed
/// first so changing a parent, kind, or container cannot leave stale edges.
fn insert_symbol_arc_into_indices(
    inner: &mut IndexInner,
    symbol: Arc<crate::Symbol>,
    file_id: FileId,
) {
    let fqn = Arc::clone(&symbol.fqn);
    if inner.symbols.contains_key(fqn.as_ref()) {
        remove_symbol_for_replacement(inner, &fqn);
    }
    insert_symbol_to_core_indices(inner, Arc::clone(&symbol), file_id);
    populate_structural_indices(inner, &symbol, &fqn);
}

pub(super) fn register_source_symbols_without_election(
    inner: &mut IndexInner,
    symbols: impl IntoIterator<Item = Arc<crate::Symbol>>,
    file_id: FileId,
) -> Vec<Arc<crate::Symbol>> {
    let symbols: Vec<_> = symbols.into_iter().collect();
    inner.record_file_symbols(file_id, symbols.iter().cloned());
    symbols
}

/// Dispatch to the appropriate language-specific symbol extractor.
///
/// This is the SLOW part of indexing (parsing, semantic analysis). It runs
/// without any locks held so readers aren't blocked during extraction.
fn extract_symbols_for_language(
    lang: Option<Language>,
    source: &str,
    path: &Path,
    text_edit: Option<&hyperion_parser::TextEdit>,
    old_source: Option<&str>,
    cached_tree_and_source: Option<&(Arc<rowan::GreenNode>, Arc<String>)>,
) -> Result<crate::symbol_extractor::ExtractionResult> {
    match lang {
        Some(Language::Kotlin) => extract_kotlin_symbols(source, path),
        Some(Language::Groovy) => extract_groovy_symbols(source),
        Some(Language::Scala) => extract_scala_symbols(source),
        Some(Language::Java) | None => {
            extract_java_symbols(source, path, text_edit, old_source, cached_tree_and_source)
        }
    }
}

fn extract_kotlin_symbols(
    source: &str,
    path: &Path,
) -> Result<crate::symbol_extractor::ExtractionResult> {
    let file_stem = path.file_stem().and_then(|stem| stem.to_str());
    let kt_extraction = hyperion_kotlin::SymbolExtractor::extract_for_file(source, file_stem)
        .context("Failed to extract Kotlin symbols")?;

    let imports = kt_extraction.imports;
    let source_package = kt_extraction.package.clone();
    // Top-level typealiases ride on the file facade, as a library's do
    // A file without a facade (`@JvmMultifileClass`) drops them.
    let facade = kt_extraction.facade;
    let mut type_aliases = kt_extraction
        .type_aliases
        .into_iter()
        .map(|alias| crate::KotlinTypeAlias {
            fqn: alias.fqn,
            expanded_class: alias.expanded_class,
            type_parameters: alias.type_parameters,
            expanded_type_arguments: alias.expanded_type_arguments,
        })
        .collect::<Vec<_>>();

    let symbols: Vec<_> = kt_extraction
        .symbols
        .into_iter()
        .map(|kt_sym| {
            // Kotlin functions carry their declared parameter types directly in
            // `kt_sym.parameters` (as "TypeName paramName" strings, matching the
            // Java extractor's descriptor shape). Classes instead carry their
            // primary constructor's parameter types in `constructor_params`
            // (kept separate so synthetic data-class methods can be generated
            // from them); surface those into the same `parameters` slot Java
            // class entries use, so a Kotlin class's declared constructor
            // signature is represented in the index too.
            let parameters: Vec<Arc<str>> = if kt_sym.kind == SymbolKind::CLASS {
                kt_sym
                    .constructor_params
                    .iter()
                    .map(|p| Arc::<str>::from(format!("{} {}", p.param_type, p.name)))
                    .collect()
            } else {
                kt_sym.parameters.into_iter().map(Into::into).collect()
            };

            let carries_type_aliases = facade.as_deref() == Some(kt_sym.fqn.as_str())
                && kt_sym.jvm_origin.as_deref() == Some(kt_sym.fqn.as_str());
            let mut kotlin = kt_sym.jvm_origin.map(|origin| {
                Box::new(crate::KotlinSymbolMetadata {
                    jvm_origin: Some(origin.into()),
                    ..crate::KotlinSymbolMetadata::default()
                })
            });
            if carries_type_aliases && !type_aliases.is_empty() {
                kotlin.get_or_insert_with(Box::default).type_aliases =
                    std::mem::take(&mut type_aliases);
            }

            crate::Symbol {
                fqn: kt_sym.fqn.into(),
                simple_name: kt_sym.simple_name.into(),
                range: kt_sym.range,
                kind: kt_sym.kind,
                modifiers: kt_sym.modifiers,
                // The Kotlin extractor uses the package as the container for
                // top-level declarations. Workspace owner election reserves
                // `container` for actual nominal containment; retaining the
                // package here makes a top-level class look like a nested type
                // whose outer declaration does not exist.
                container: kt_sym
                    .container
                    .filter(|container| source_package.as_deref() != Some(container.as_str()))
                    .map(Into::into),
                return_type: kt_sym.return_type.map(Into::into),
                parameters,
                parent_class: kt_sym.parent_class.map(Into::into),
                interfaces: kt_sym.interfaces.into_iter().map(Into::into).collect(),
                documentation: kt_sym.documentation.map(Into::into),
                type_parameters: kt_sym.type_parameters,
                is_deprecated: false,
                receiver_type: kt_sym.receiver_type.map(Into::into),
                kotlin,
                throws: None,
            }
        })
        .collect();

    Ok(crate::symbol_extractor::ExtractionResult {
        package: kt_extraction.package,
        imports,
        symbols,
        generic_supertypes: Vec::new(),
        errors: Vec::new(),
    })
}

fn extract_groovy_symbols(source: &str) -> Result<crate::symbol_extractor::ExtractionResult> {
    let groovy_extraction = hyperion_groovy::LspSymbolExtractor::extract(source)
        .context("Failed to extract Groovy symbols")?;

    let imports = groovy_extraction.imports;
    let source_package = groovy_extraction.package.clone();

    let symbols: Vec<_> = groovy_extraction
        .symbols
        .into_iter()
        .map(|g_sym| crate::Symbol {
            fqn: g_sym.fqn.into(),
            simple_name: g_sym.simple_name.into(),
            range: g_sym.range,
            kind: g_sym.kind,
            modifiers: g_sym.modifiers,
            container: g_sym
                .container
                .filter(|container| source_package.as_deref() != Some(container.as_str()))
                .map(Into::into),
            return_type: g_sym.return_type.map(Into::into),
            parameters: g_sym.parameters.into_iter().map(Into::into).collect(),
            parent_class: g_sym.parent_class.map(Into::into),
            interfaces: g_sym.interfaces.into_iter().map(Into::into).collect(),
            documentation: g_sym.documentation.map(Into::into),
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        })
        .collect();

    Ok(crate::symbol_extractor::ExtractionResult {
        package: groovy_extraction.package,
        imports,
        symbols,
        generic_supertypes: Vec::new(),
        errors: Vec::new(),
    })
}

fn extract_scala_symbols(source: &str) -> Result<crate::symbol_extractor::ExtractionResult> {
    let scala_extraction = hyperion_scala::SymbolExtractor::extract(source)
        .context("Failed to extract Scala symbols")?;

    let imports = scala_extraction.imports;
    let source_package = scala_extraction.package.clone();
    let symbols: Vec<_> = scala_extraction
        .symbols
        .into_iter()
        .map(|scala_sym| crate::Symbol {
            fqn: scala_sym.fqn.into(),
            simple_name: scala_sym.simple_name.into(),
            range: scala_sym.range,
            kind: scala_sym.kind,
            modifiers: scala_sym.modifiers,
            container: scala_sym
                .container
                .filter(|container| source_package.as_deref() != Some(container.as_str()))
                .map(Into::into),
            return_type: scala_sym.return_type.map(Into::into),
            parameters: scala_sym.parameters.into_iter().map(Into::into).collect(),
            parent_class: scala_sym.parent_class.map(Into::into),
            interfaces: scala_sym.interfaces.into_iter().map(Into::into).collect(),
            documentation: None,
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        })
        .collect();

    Ok(crate::symbol_extractor::ExtractionResult {
        package: scala_extraction.package,
        imports,
        symbols,
        generic_supertypes: Vec::new(),
        errors: Vec::new(),
    })
}

fn extract_java_symbols(
    source: &str,
    _path: &Path,
    _text_edit: Option<&hyperion_parser::TextEdit>,
    _old_source: Option<&str>,
    _cached_tree_and_source: Option<&(Arc<rowan::GreenNode>, Arc<String>)>,
) -> Result<crate::symbol_extractor::ExtractionResult> {
    // PERF: We go straight to SymbolExtractor::extract which uses DeclarationsOnly
    // mode (8-10x faster than full parse). The previous implementation ran
    // incremental_parse here but discarded the result, wasting a full parse cycle.
    // The parse tree cache is updated separately in index_file_internal (Phase 5)
    // using a full-mode parse needed for incremental parsing support.
    SymbolExtractor::extract(source).context("Failed to extract Java symbols")
}

/// Remove every dependency edge contributed by `file_id`.
///
/// Normal state uses the tracked FQN set for O(imports) cleanup. Older cache
/// formats did not persist that set, so their one-time cleanup deliberately
/// scans the reverse map rather than leaving an untracked edge behind.
fn remove_imports_for_file(inner: &mut IndexInner, file_id: FileId) {
    let old_imports = inner.file_imports.remove(&file_id);
    if let Some(imported_fqns) = inner.file_imported_fqns.remove(&file_id) {
        for fqn in imported_fqns {
            if let Some(dependents) = inner.reverse_dependencies.get_mut(fqn.as_str()) {
                dependents.remove(&file_id);
                if dependents.is_empty() {
                    inner.reverse_dependencies.remove(fqn.as_str());
                }
            }
        }
    } else if old_imports.is_some() {
        for dependents in inner.reverse_dependencies.values_mut() {
            dependents.remove(&file_id);
        }
        inner
            .reverse_dependencies
            .retain(|_, dependents| !dependents.is_empty());
    }

    if let Some(old_imports) = old_imports {
        for package in old_imports.wildcards {
            if let Some(importers) = inner.package_index.get_mut(package.as_str()) {
                importers.remove(&file_id);
                if importers.is_empty() {
                    inner.package_index.remove(package.as_str());
                }
            }
        }
    }
}

/// Process import changes for a file: clean up old reverse dependencies,
/// store new imports, and register new reverse dependencies.
///
/// This uses O(m) cleanup (where m = number of imports) instead of O(n)
/// (where n = total reverse dependencies) to avoid pathological performance
/// on rapid file updates.
pub(super) fn process_imports_for_file(
    inner: &mut IndexInner,
    file_id: FileId,
    imports: &ImportMap,
) {
    remove_imports_for_file(inner, file_id);

    // Store the new import map (after cleanup, before adding new deps)
    inner.file_imports.insert(file_id, imports.clone());

    // Track which FQNs this file imports for efficient cleanup later
    let mut imported_fqns = HashSet::new();

    // Add reverse dependencies for new imports.
    // Skip external symbols (JDK, stdlib) — they never change during development
    // and tracking them causes unbounded memory growth.
    for fqn in imports.explicit.values() {
        if !is_external_symbol(fqn) {
            inner
                .reverse_dependencies
                .entry(fqn.clone().into())
                .or_default()
                .insert(file_id);
        }
        imported_fqns.insert(fqn.clone());
    }

    // Wildcard imports: import com.example.*;
    for wildcard in &imports.wildcards {
        let wildcard_key = format!("{}.*", wildcard);
        if !is_external_symbol(wildcard) {
            inner
                .reverse_dependencies
                .entry(wildcard_key.clone().into())
                .or_default()
                .insert(file_id);

            inner
                .package_index
                .entry(wildcard.clone().into())
                .or_default()
                .insert(file_id);
        }
        imported_fqns.insert(wildcard_key);
    }

    // Static imports: import static com.example.Utils.helper;
    for fqn in imports.static_import_fqns() {
        if !is_external_symbol(fqn) {
            inner
                .reverse_dependencies
                .entry(fqn.into())
                .or_default()
                .insert(file_id);
        }
        imported_fqns.insert(fqn.to_owned());
    }

    // Static wildcard imports: import static com.example.Utils.*;
    for wildcard in &imports.static_wildcards {
        let wildcard_key = format!("{}.*", wildcard);
        if !is_external_symbol(wildcard) {
            inner
                .reverse_dependencies
                .entry(wildcard_key.clone().into())
                .or_default()
                .insert(file_id);
        }
        imported_fqns.insert(wildcard_key);
    }

    inner.file_imported_fqns.insert(file_id, imported_fqns);
}

/// Populate the structural indices for a single symbol.
///
/// These indices are used for goto-implementation, type hierarchy, and invalidation:
/// 1. `nested_class_index` — outer class → nested classes
/// 2. `package_to_classes` — package → top-level classes
/// 3. `inheritance` — supertype → direct subtypes
///
/// Called from both `store_symbols_for_file` (fresh indexing) and
/// `restore_from_cache`/`restore_from_cache_batch` (cache restore).
pub(super) fn populate_structural_indices(
    inner: &mut IndexInner,
    symbol: &crate::Symbol,
    fqn: &Arc<str>,
) {
    let symbol_kind = symbol.kind;

    // Track nested nominal types in O(1) index.
    if crate::is_type_declaration_kind(symbol_kind) {
        if let Some(ref outer_class_fqn) = symbol.container {
            inner
                .nested_class_index
                .entry(outer_class_fqn.clone())
                .or_default()
                .insert(fqn.to_string());
        }
    }

    // Track top-level classes by package
    if crate::is_type_declaration_kind(symbol_kind) && symbol.container.is_none() {
        if let Some(package) = fqn.rsplit_once('.').map(|x| x.0) {
            inner
                .package_to_classes
                .entry(Arc::from(package))
                .or_default()
                .insert(fqn.to_string());
        }
    }

    // Fresh and cached source indexing install imports before symbols. External
    // declarations have no source import context and only contribute exact
    // inheritance edges.
    let imports = inner
        .symbol_locations
        .get(fqn.as_ref())
        .and_then(|location| inner.file_imports.get(&location.file_id));
    let language = inner
        .symbol_locations
        .get(fqn.as_ref())
        .and_then(|location| inner.file_path(location.file_id))
        .and_then(Language::from_path);
    inner.inheritance.insert(symbol, imports, language);
}

/// Store extracted symbols into all index maps and track inheritance/nested class relationships.
///
/// Returns `(symbols_added, symbol_fqns)` — the count and the FQN list for cache invalidation.
fn store_symbols_for_file_without_election(
    inner: &mut IndexInner,
    file_id: FileId,
    symbols: Vec<crate::Symbol>,
) -> (usize, Vec<Arc<str>>, Vec<Arc<crate::Symbol>>) {
    let symbols: Vec<_> = symbols.into_iter().map(Arc::new).collect();
    let symbol_fqns = symbols
        .iter()
        .map(|symbol| Arc::clone(&symbol.fqn))
        .collect();
    let symbols_added = symbols.len();
    let symbols = register_source_symbols_without_election(inner, symbols, file_id);
    (symbols_added, symbol_fqns, symbols)
}

/// Relocate a displaced file's index data when a FileId collision occurs.
///
/// This happens when FileIdRegistry and WorkspaceIndex allocate FileIds independently:
/// e.g., workspace scan assigns FileId(1) to FileA.java, then user opens FileB.java
/// and FileIdRegistry also assigns FileId(1). Instead of losing FileA's symbols,
/// we move them to a freshly allocated FileId.
fn relocate_displaced_file(
    inner: &mut IndexInner,
    file_id: FileId,
    new_path: &Path,
    new_key: &PathLookupKey,
) {
    let existing_path = match inner.registered_path(file_id) {
        Some(registered) if registered.lookup_key() != new_key => {
            registered.display_path().to_path_buf()
        }
        _ => return,
    };

    // Allocate a fresh FileId for the displaced file
    let new_file_id = inner.allocate_workspace_file_id();

    debug!(
        "FileId collision: {:?} was {:?}, relocating to {:?}. New file: {:?}",
        file_id,
        existing_path,
        new_file_id,
        new_path.display()
    );

    // 1. Update path mappings
    let existing_key = inner
        .remove_file_path(file_id)
        .expect("a displaced display path must retain its lookup key");
    inner.register_file_path(new_file_id, existing_path.clone(), existing_key);

    // 2. Relocate file_symbols and update symbol_locations
    let generic_supertypes = inner.take_file_generic_supertypes(file_id);
    let symbols = inner.take_file_symbols(file_id);
    if !symbols.is_empty() {
        inner.replace_file_symbols(new_file_id, symbols.clone());
        for symbol in &symbols {
            let fqn = &symbol.fqn;
            if let Some(loc) = inner.symbol_locations.get_mut(fqn.as_ref()) {
                if loc.file_id == file_id {
                    loc.file_id = new_file_id;
                }
            }
        }
    }
    if !generic_supertypes.is_empty() {
        inner
            .file_generic_supertypes
            .insert(new_file_id, generic_supertypes);
    }

    // 3. Relocate file_imports
    if let Some(imports) = inner.file_imports.remove(&file_id) {
        inner.file_imports.insert(new_file_id, imports);
    }

    // 4. Relocate tracked imported FQNs
    if let Some(imported_fqns) = inner.file_imported_fqns.remove(&file_id) {
        inner.file_imported_fqns.insert(new_file_id, imported_fqns);
    }

    // Reverse dependency and wildcard-package sets store importer FileIds as
    // values. Moving only the forward map leaves queries returning the now
    // unrelated file that reused `file_id`.
    for dependents in inner.reverse_dependencies.values_mut() {
        if dependents.remove(&file_id) {
            dependents.insert(new_file_id);
        }
    }
    for importers in inner.package_index.values_mut() {
        if importers.remove(&file_id) {
            importers.insert(new_file_id);
        }
    }

    // 5. Relocate symbol usages and usage tracking metadata
    if let Some(usage_counts) = inner.file_usage_counts.remove(&file_id) {
        for fqn in usage_counts.keys() {
            if let Some(usages) = inner.symbol_usages.get_mut(fqn.as_ref()) {
                for loc in usages.iter_mut() {
                    if loc.file_id == file_id {
                        loc.file_id = new_file_id;
                    }
                }
            }
        }
        inner.file_usage_counts.insert(new_file_id, usage_counts);
    } else {
        // Restored/legacy indexes may predate the forward count map. Preserve
        // the compatibility invariant used by removal: every reverse usage
        // must still follow its contributing file when the ID is relocated.
        for usages in inner.symbol_usages.values_mut() {
            for location in usages {
                if location.file_id == file_id {
                    location.file_id = new_file_id;
                }
            }
        }
    }
    if let Some(method_usages) = inner.file_method_usages.remove(&file_id) {
        inner.file_method_usages.insert(new_file_id, method_usages);
    }
    if let Some(identifiers) = inner.file_lexical_identifiers.remove(&file_id) {
        inner
            .file_lexical_identifiers
            .insert(new_file_id, identifiers);
    }

    // 6. Relocate cached content
    if let Some(content) = inner.file_content_cache.pop(&file_id) {
        inner.file_content_cache.put(new_file_id, content);
    }

    // 7. Relocate the exact published revision. The FileId embedded in a
    // token changes, so every token for the displaced identity fails closed.
    if let Some(revision) = inner.published_revisions.remove(&file_id) {
        inner.published_revisions.insert(new_file_id, revision);
    }

    // 8. Relocate the incremental parse product. Leaving it under `file_id`
    // would feed the newly-colliding file a tree and source from another path.
    if let Some((tree, source)) = inner.parse_tree_cache.remove(&file_id) {
        inner.parse_tree_cache.put(new_file_id, tree, source);
    }
}

struct SourceIndexRequest<'a> {
    path: &'a Path,
    path_key: PathLookupKey,
    move_from: Option<&'a Path>,
    move_from_key: Option<PathLookupKey>,
    source: &'a str,
    file_id: FileId,
    text_edit: Option<&'a hyperion_parser::TextEdit>,
    old_source: Option<&'a str>,
    extract_usages: bool,
    cache_parse_tree: bool,
    content_hash: Option<u64>,
    origin: SourceRevisionOrigin,
}

struct MovePathIdentity<'a> {
    old_path: &'a Path,
    old_key: &'a PathLookupKey,
    new_path: &'a Path,
    new_key: &'a PathLookupKey,
}

impl<'a> SourceIndexRequest<'a> {
    fn caller_owned(
        path: &'a Path,
        path_key: PathLookupKey,
        source: &'a str,
        file_id: FileId,
    ) -> Self {
        Self {
            path,
            path_key,
            move_from: None,
            move_from_key: None,
            source,
            file_id,
            text_edit: None,
            old_source: None,
            extract_usages: true,
            cache_parse_tree: true,
            content_hash: None,
            origin: SourceRevisionOrigin::EditorOrCaller,
        }
    }

    fn disk(path: &'a Path, path_key: PathLookupKey, source: &'a str, file_id: FileId) -> Self {
        Self {
            origin: SourceRevisionOrigin::Disk,
            ..Self::caller_owned(path, path_key, source, file_id)
        }
    }
}

/// The exact declaration revision awaiting the second cold-index pass.
///
/// A path alone is not sufficient: editors and file watchers may publish a
/// newer revision while a workspace build is in progress. The usage pass may
/// enrich this revision or replace it from a newly changed disk buffer, but it
/// must never attach stale work to a different published revision.
struct IndexedFileRevision {
    path: PathBuf,
    path_key: PathLookupKey,
    file_id: FileId,
    content_hash: u64,
    revision_id: NonZeroU64,
    retained_source: Option<Arc<String>>,
    origin: SourceRevisionOrigin,
}

const COLD_BUILD_IDLE: u8 = 0;
const COLD_BUILD_DECLARATIONS: u8 = 1;
const COLD_BUILD_USAGES: u8 = 2;

struct ColdBuildPhaseGuard<'a> {
    phase: &'a AtomicU8,
}

impl<'a> ColdBuildPhaseGuard<'a> {
    fn begin(phase: &'a AtomicU8) -> Self {
        phase.store(COLD_BUILD_DECLARATIONS, Ordering::Release);
        Self { phase }
    }

    fn begin_usage_pass(&self) {
        self.phase.store(COLD_BUILD_USAGES, Ordering::Release);
    }
}

impl Drop for ColdBuildPhaseGuard<'_> {
    fn drop(&mut self) {
        self.phase.store(COLD_BUILD_IDLE, Ordering::Release);
    }
}

impl WorkspaceIndex {
    /// Whether a complete workspace build has published no usage pass yet.
    ///
    /// A source revision published while this is true is visible before every
    /// cold-build usage extraction. Its captured dependents therefore do not
    /// need a second, concurrent transitive re-index pass.
    pub fn cold_declaration_pass_active(&self) -> bool {
        self.cold_build_phase.load(Ordering::Acquire) == COLD_BUILD_DECLARATIONS
    }

    /// Build the complete workspace index synchronously.
    ///
    /// This operation performs filesystem scanning and CPU-heavy parsing using
    /// Rayon. Async callers must run it on a blocking thread pool.
    pub fn build(&self) -> Result<IndexStats> {
        self.build_scoped(|publish| publish())
    }

    /// [`Self::build`], running every index-mutating step (the reservation and
    /// each chunk of files in either pass) inside `publish_scope`.
    ///
    /// The scope is called on the calling thread, once per bounded step. A host
    /// that serializes index writers opens its write transaction around each
    /// step, so a cold build of a large workspace interleaves with requests and
    /// editor notifications instead of holding the workspace for its whole
    /// duration.
    pub fn build_scoped(&self, publish_scope: impl Fn(&mut dyn FnMut())) -> Result<IndexStats> {
        let _build_guard = self
            .full_build_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let build_phase = ColdBuildPhaseGuard::begin(&self.cold_build_phase);

        let root = self.inner.read().root.clone();
        info!("Building workspace index at: {}", root.display());

        // Scan workspace for Java files
        let scanner = WorkspaceScanner::new(root);
        let java_files = scanner.scan().context("Failed to scan workspace")?;

        let total_files = java_files.len();
        info!(
            "Found {} Java files, pre-allocating HashMaps...",
            total_files
        );

        // PRE-ALLOCATE HASHMAPS based on file count
        // This eliminates 14 reallocations per HashMap (0→1→2→4→...→8192 for 10K files)
        // Expected savings: ~15ms for 10K files, eliminates 196 total reallocations
        publish_scope(&mut || {
            let mut inner = self.write_inner();

            // Use same multipliers as new_with_capacity()
            // L7: Check for overflow with clear error logging
            let estimated_symbols = total_files.checked_mul(13).unwrap_or_else(|| {
                tracing::error!(
                    "L7: HashMap capacity overflow in reserve: total_files={} exceeds usize::MAX/13",
                    total_files
                );
                total_files.saturating_mul(13)
            });
            let estimated_locations = total_files.checked_mul(4).unwrap_or_else(|| {
                tracing::error!(
                    "L7: HashMap capacity overflow in reserve: total_files={} exceeds usize::MAX/4",
                    total_files
                );
                total_files.saturating_mul(4)
            });
            let estimated_imports = total_files.checked_mul(7).unwrap_or_else(|| {
                tracing::error!(
                    "L7: HashMap capacity overflow in reserve: total_files={} exceeds usize::MAX/7",
                    total_files
                );
                total_files.saturating_mul(7)
            });
            let estimated_constructors = estimated_locations / 2;

            // Reserve additional capacity (won't shrink existing data)
            // Note: symbol_locations and symbol_usages are BTreeMaps (no reserve needed)
            inner.symbols.reserve(estimated_symbols);
            inner.simple_name_index.reserve(estimated_locations);
            inner.constructor_index.reserve(estimated_constructors);
            inner.file_symbols.reserve(total_files);
            inner.symbol_definition_files.reserve(estimated_symbols);
            inner.file_imports.reserve(total_files);
            inner.file_method_usages.reserve(total_files);
            inner.paths.reserve(total_files);
            inner.published_revisions.reserve(total_files);
            inner.reverse_dependencies.reserve(estimated_imports);
            inner.package_index.reserve(estimated_imports / 10);
            inner.inheritance.reserve(total_files / 2);
            inner.package_to_classes.reserve(total_files / 3);

            debug!(
                "Pre-allocated HashMaps: {} files → {} symbols, {} locations, {} imports",
                total_files, estimated_symbols, estimated_locations, estimated_imports
            );
        }); // Lock dropped here

        info!("HashMap pre-allocation complete, indexing in parallel...");

        // Atomic counters for progress tracking
        let files_indexed = Arc::new(AtomicUsize::new(0));
        let files_failed = Arc::new(AtomicUsize::new(0));
        let symbols_indexed = Arc::new(AtomicUsize::new(0));

        // Clone for progress reporting
        let files_indexed_clone = files_indexed.clone();
        let symbols_indexed_clone = symbols_indexed.clone();

        // Process files in small chunks to prevent memory exhaustion
        // With thousands of files, we need to limit batch size
        const CHUNK_SIZE: usize = 100;

        // Use system defaults for thread count (typically num_cpus)
        // Rayon will automatically scale based on available cores
        let pool = rayon::ThreadPoolBuilder::new()
            .build()
            .context("Failed to create thread pool")?;

        // Pass 1 indexes declarations before usage extraction. The old per-file
        // pipeline interleaved usage read-locks with symbol write-locks from
        // thousands of other workers, producing a severe lock convoy.
        info!("Index pass 1/2: declarations");
        let mut usage_files = Vec::with_capacity(total_files);

        // Process in chunks to avoid memory spikes from queuing too many files
        for chunk in java_files.chunks(CHUNK_SIZE) {
            let mut successful_chunk = Vec::new();
            publish_scope(&mut || {
                successful_chunk = pool.install(|| {
                    chunk
                        .par_iter()
                        .filter_map(|path| {
                            match self.index_file_sync_symbols_only(path) {
                                Ok((file_stats, revision)) => {
                                    let indexed = files_indexed.fetch_add(1, Ordering::Relaxed) + 1;
                                    symbols_indexed
                                        .fetch_add(file_stats.symbols_added, Ordering::Relaxed);

                                    // Progress reporting every 100 files
                                    if indexed.is_multiple_of(100) || indexed == total_files {
                                        let symbols = symbols_indexed_clone.load(Ordering::Relaxed);
                                        info!(
                                            "Progress: {}/{} files ({:.1}%), {} symbols",
                                            indexed,
                                            total_files,
                                            indexed as f64 / total_files as f64 * 100.0,
                                            symbols
                                        );
                                    }
                                    revision
                                }
                                Err(err) => {
                                    warn!("Failed to index {}: {}", path.display(), err);
                                    files_failed.fetch_add(1, Ordering::Relaxed);
                                    None
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                });
            });
            usage_files.extend(successful_chunk);
        }

        info!("Index pass 2/2: usages against complete symbol index");
        build_phase.begin_usage_pass();
        let usage_files_failed = AtomicUsize::new(0);
        for chunk in usage_files.chunks(CHUNK_SIZE) {
            publish_scope(&mut || {
                pool.install(|| {
                    chunk.par_iter().for_each(|revision| {
                        if let Err(err) = self.index_file_usages_sync(revision) {
                            warn!(
                                "Failed to index usages for {}: {}",
                                revision.path.display(),
                                err
                            );
                            usage_files_failed.fetch_add(1, Ordering::Relaxed);
                        }
                    })
                });
            });
        }

        let stats = IndexStats {
            files_scanned: total_files,
            files_indexed: files_indexed_clone.load(Ordering::Relaxed),
            files_failed: files_failed.load(Ordering::Relaxed)
                + usage_files_failed.load(Ordering::Relaxed),
            symbols_indexed: symbols_indexed_clone.load(Ordering::Relaxed),
        };

        info!(
            "Index built: {} files, {} symbols, {} failed",
            stats.files_indexed, stats.symbols_indexed, stats.files_failed
        );
        self.index_phase_snapshot().log("workspace build");

        Ok(stats)
    }

    /// Index multiple files in parallel using rayon.
    /// This is much faster than sequential indexing for large file sets.
    pub fn index_files_parallel(&self, files: &[PathBuf]) -> IndexStats {
        self.index_files_parallel_internal(files, false)
    }

    /// Index multiple files in parallel, skipping files already indexed with matching content hash.
    /// This is optimal for Gradle integration after cache restore - only indexes new/changed files.
    pub fn index_files_parallel_skip_cached(&self, files: &[PathBuf]) -> IndexStats {
        self.index_files_parallel_internal(files, true)
    }

    fn index_files_parallel_internal(&self, files: &[PathBuf], skip_cached: bool) -> IndexStats {
        use rayon::prelude::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let started = std::time::Instant::now();
        let total_files = files.len();
        if total_files == 0 {
            return IndexStats {
                files_scanned: 0,
                files_indexed: 0,
                files_failed: 0,
                symbols_indexed: 0,
            };
        }

        info!(
            "Parallel indexing {} files (skip_cached={})...",
            total_files, skip_cached
        );

        let files_indexed = Arc::new(AtomicUsize::new(0));
        let files_skipped = Arc::new(AtomicUsize::new(0));
        let files_failed = Arc::new(AtomicUsize::new(0));
        let symbols_indexed = Arc::new(AtomicUsize::new(0));

        let files_indexed_clone = files_indexed.clone();
        let files_skipped_clone = files_skipped.clone();
        let symbols_indexed_clone = symbols_indexed.clone();

        // PERFORMANCE: Use rayon's global thread pool directly instead of creating per-chunk pools.
        // This avoids pool setup/teardown overhead and keeps threads warm.
        files.par_iter().for_each(|path| {
            match self.index_file_from_disk(path, skip_cached, || {}) {
                Ok(Some(file_stats)) => {
                    let indexed = files_indexed.fetch_add(1, Ordering::Relaxed) + 1;
                    symbols_indexed.fetch_add(file_stats.symbols_added, Ordering::Relaxed);

                    // Progress every 500 files
                    let skipped = files_skipped_clone.load(Ordering::Relaxed);
                    let processed = indexed + skipped;
                    if processed.is_multiple_of(500) || processed == total_files {
                        let symbols = symbols_indexed_clone.load(Ordering::Relaxed);
                        info!(
                            "Parallel index progress: {}/{} files ({:.0}%), {} symbols, {} skipped",
                            processed,
                            total_files,
                            processed as f64 / total_files as f64 * 100.0,
                            symbols,
                            skipped
                        );
                    }
                }
                Ok(None) => {
                    files_skipped.fetch_add(1, Ordering::Relaxed);
                }
                Err(err) => {
                    debug!("Failed to index {}: {}", path.display(), err);
                    files_failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        let skipped = files_skipped_clone.load(Ordering::Relaxed);
        if skipped > 0 {
            info!("Skipped {} already-indexed files", skipped);
        }

        let stats = IndexStats {
            files_scanned: total_files,
            files_indexed: files_indexed_clone.load(Ordering::Relaxed),
            files_failed: files_failed.load(Ordering::Relaxed),
            symbols_indexed: symbols_indexed_clone.load(Ordering::Relaxed),
        };
        info!(
            "Parallel indexing finished in {:.2}s: {} indexed, {} skipped, {} failed",
            started.elapsed().as_secs_f64(),
            stats.files_indexed,
            skipped,
            stats.files_failed,
        );
        self.index_phase_snapshot()
            .log("parallel indexing aggregate");
        stats
    }

    pub async fn index_file(&self, path: &Path) -> Result<FileIndexStats> {
        let index = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || index.index_file_sync(&path))
            .await
            .context("File indexing task failed")?
    }

    pub fn index_file_sync_for_test(&self, path: &Path) -> Result<FileIndexStats> {
        self.index_file_sync(path)
    }

    #[cfg(test)]
    pub(crate) fn index_file_sync_after_read_for_test(
        &self,
        path: &Path,
        after_read: impl FnOnce(),
    ) -> Result<FileIndexStats> {
        self.index_file_from_disk(path, false, after_read)?
            .context("non-skipping disk index returned no revision")
    }

    /// Index a source buffer while the caller holds both the path lock and the
    /// file lock, acquired in that order.
    pub fn index_file_with_locks_held(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        let path_key = path_lookup_key(path);
        self.index_file_with_key_and_locks_held(path, &path_key, source, file_id)
    }

    /// Index a caller-owned buffer under the exact path lock represented by
    /// `path_key`.
    pub fn index_file_with_key_and_locks_held(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        self.index_file_internal(
            SourceIndexRequest::caller_owned(path, path_key.clone(), source, file_id),
            || Ok(()),
        )
    }

    /// Index a filesystem revision while the caller holds the path lock and
    /// then the file lock.
    ///
    /// This explicit provenance permits validated content retrieval to refresh
    /// the revision from disk later. Caller/editor publications deliberately
    /// use [`Self::index_file_with_locks_held`] and can never be overwritten by
    /// an incidental cache-miss read.
    pub fn index_disk_file_with_locks_held(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        let path_key = path_lookup_key(path);
        self.index_disk_file_with_key_and_locks_held(path, &path_key, source, file_id)
    }

    /// Index a disk buffer under one established path identity.
    ///
    /// Disk provenance causes the key to be revalidated immediately before
    /// publication. If the path was retargeted after the caller read `source`,
    /// publication fails without changing the prior revision.
    pub fn index_disk_file_with_key_and_locks_held(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        self.index_file_internal(
            SourceIndexRequest::disk(path, path_key.clone(), source, file_id),
            || Ok(()),
        )
    }

    /// Mark an already-identical indexed revision as disk-backed while the
    /// caller holds the path lock and then the file lock.
    ///
    /// Closing an editor document commonly finds that its final buffer already
    /// equals disk. Re-parsing solely to change provenance is wasteful, but
    /// leaving it caller-owned would permanently disable validated disk
    /// refresh after the document closes.
    pub fn adopt_disk_revision_with_locks_held(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
    ) -> bool {
        let path_key = path_lookup_key(path);
        self.adopt_disk_revision_with_key_and_locks_held(path, &path_key, source, file_id)
    }

    pub fn adopt_disk_revision_with_key_and_locks_held(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
    ) -> bool {
        self.adopt_revision_origin_with_key_and_locks_held(
            path,
            path_key,
            source,
            file_id,
            SourceRevisionOrigin::Disk,
        )
    }

    /// Mark an already-identical indexed revision as caller/editor-owned while
    /// the caller holds the path lock and then the file lock.
    ///
    /// `didOpen` commonly receives the same bytes that were cold-indexed from
    /// disk. The bytes and ranges do not need to be rebuilt, but ownership does
    /// change: after the editor opens the document, a cache miss must never let
    /// an incidental disk read replace that authoritative in-memory revision.
    pub fn adopt_caller_owned_revision_with_locks_held(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
    ) -> bool {
        let path_key = path_lookup_key(path);
        self.adopt_caller_owned_revision_with_key_and_locks_held(path, &path_key, source, file_id)
    }

    pub fn adopt_caller_owned_revision_with_key_and_locks_held(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
    ) -> bool {
        self.adopt_revision_origin_with_key_and_locks_held(
            path,
            path_key,
            source,
            file_id,
            SourceRevisionOrigin::EditorOrCaller,
        )
    }

    fn adopt_revision_origin_with_key_and_locks_held(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
        origin: SourceRevisionOrigin,
    ) -> bool {
        if origin == SourceRevisionOrigin::Disk && path_lookup_key(path) != *path_key {
            return false;
        }
        let content_hash = compute_content_hash(source);
        let mut inner = self.write_inner();
        let bytes_match = inner
            .file_content_cache
            .peek(&file_id)
            .is_some_and(|retained| retained.as_str() == source)
            || inner
                .parse_tree_cache
                .peek(&file_id)
                .is_some_and(|(_, retained)| retained.as_str() == source);
        let revision_matches = bytes_match
            && inner
                .published_revisions
                .get(&file_id)
                .is_some_and(|revision| revision.content_hash == content_hash)
            && inner.file_path_key(file_id) == Some(path_key);
        if revision_matches {
            // Authority is revision identity even when bytes and ranges are
            // unchanged. Pre-adoption request snapshots must not survive.
            inner.allocate_revision(file_id, content_hash, origin);
        }
        revision_matches
    }

    /// Move and replace one exact indexed revision while the caller holds both
    /// path locks and every affected file lock.
    ///
    /// Unlike ordinary publication, this deliberately reuses `file_id` at a
    /// different path instead of treating that reuse as an allocator collision.
    /// The old revision, any destination revision, and the new source become one
    /// reader-visible workspace transaction. A stale `old_path` fails without
    /// mutating the last published revision.
    pub fn index_moved_file_with_locks_held(
        &self,
        old_path: &Path,
        new_path: &Path,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        let old_key = path_lookup_key(old_path);
        let new_key = path_lookup_key(new_path);
        self.index_moved_file_with_keys_and_locks_held(
            old_path, &old_key, new_path, &new_key, source, file_id,
        )
    }

    pub fn index_moved_file_with_keys_and_locks_held(
        &self,
        old_path: &Path,
        old_key: &PathLookupKey,
        new_path: &Path,
        new_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        self.index_moved_revision_with_keys_and_locks_held(
            MovePathIdentity {
                old_path,
                old_key,
                new_path,
                new_key,
            },
            source,
            file_id,
            SourceRevisionOrigin::EditorOrCaller,
        )
    }

    /// Move and replace an exact filesystem revision while the caller holds
    /// both path locks and every affected file lock.
    ///
    /// File-watcher renames read `source` from `new_path`; keeping that origin
    /// explicit allows later validated cache misses to observe newer disk
    /// revisions. Editor-driven moves use
    /// [`Self::index_moved_file_with_locks_held`] instead.
    pub fn index_moved_disk_file_with_locks_held(
        &self,
        old_path: &Path,
        new_path: &Path,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        let old_key = path_lookup_key(old_path);
        let new_key = path_lookup_key(new_path);
        self.index_moved_disk_file_with_keys_and_locks_held(
            old_path, &old_key, new_path, &new_key, source, file_id,
        )
    }

    pub fn index_moved_disk_file_with_keys_and_locks_held(
        &self,
        old_path: &Path,
        old_key: &PathLookupKey,
        new_path: &Path,
        new_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        self.index_moved_revision_with_keys_and_locks_held(
            MovePathIdentity {
                old_path,
                old_key,
                new_path,
                new_key,
            },
            source,
            file_id,
            SourceRevisionOrigin::Disk,
        )
    }

    fn index_moved_revision_with_keys_and_locks_held(
        &self,
        paths: MovePathIdentity<'_>,
        source: &str,
        file_id: FileId,
        origin: SourceRevisionOrigin,
    ) -> Result<FileIndexStats> {
        let MovePathIdentity {
            old_path,
            old_key,
            new_path,
            new_key,
        } = paths;
        self.index_file_internal(
            SourceIndexRequest {
                move_from: Some(old_path),
                move_from_key: Some(old_key.clone()),
                origin,
                ..SourceIndexRequest::caller_owned(new_path, new_key.clone(), source, file_id)
            },
            || Ok(()),
        )
    }

    pub fn index_file_with_source(&self, path: &Path, source: &str) -> Result<FileIndexStats> {
        // Serialize by path before choosing an ID. During startup, bulk indexing and
        // didOpen can otherwise reserve different IDs and bypass the FileId lock.
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let _path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (file_id, result) = self.index_file_with_source_under_path_lock(
            path,
            &path_key,
            source,
            SourceRevisionOrigin::EditorOrCaller,
        );
        drop(_path_guard);
        drop(path_lock);
        if result.is_err() {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result
    }

    /// Index a caller-owned buffer while the normalized path lock is held.
    ///
    /// Disk-backed entry points acquire that lock before reading so an old
    /// buffer cannot wait behind and then overwrite a newer source revision.
    fn index_file_with_source_under_path_lock(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        origin: SourceRevisionOrigin,
    ) -> (FileId, Result<FileIndexStats>) {
        self.with_current_automatic_file_id(
            path_key,
            |_| {},
            |file_id| match origin {
                SourceRevisionOrigin::EditorOrCaller => self.index_file_internal(
                    SourceIndexRequest::caller_owned(path, path_key.clone(), source, file_id),
                    || Ok(()),
                ),
                SourceRevisionOrigin::Disk => self.index_file_internal(
                    SourceIndexRequest::disk(path, path_key.clone(), source, file_id),
                    || Ok(()),
                ),
            },
        )
    }

    /// Run an automatic publication while holding the FileId lock that still
    /// owns `path_key` after the lock wait.
    ///
    /// A caller-owned publication for another path can reserve the same ID and
    /// relocate this path while an automatic indexer waits. ID selection is
    /// therefore optimistic: after acquiring the selected lock, re-resolve the
    /// identity and retry if either side changed. The caller must hold the path
    /// lock for `path_key` for the whole operation.
    fn with_current_automatic_file_id<T>(
        &self,
        path_key: &PathLookupKey,
        after_initial_selection: impl FnOnce(FileId),
        operation: impl FnOnce(FileId) -> T,
    ) -> (FileId, T) {
        let mut after_initial_selection = Some(after_initial_selection);
        let mut operation = Some(operation);

        loop {
            let file_id = {
                let mut inner = self.write_inner();
                inner
                    .file_id_for_key(path_key)
                    .unwrap_or_else(|| inner.allocate_workspace_file_id())
            };
            if let Some(after_selection) = after_initial_selection.take() {
                after_selection(file_id);
            }

            let file_lock = self.get_file_lock(file_id);
            let file_guard = file_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let still_owned = {
                let inner = self.inner.read();
                match inner.file_id_for_key(path_key) {
                    Some(current_id) => current_id == file_id,
                    None => {
                        !inner.paths.contains_file_id(file_id)
                            && !inner.external_file_ids.contains(&file_id)
                    }
                }
            };
            if !still_owned {
                drop(file_guard);
                drop(file_lock);
                self.remove_file_lock_if_unused(file_id);
                continue;
            }

            let result = operation.take().expect("automatic operation ran once")(file_id);
            drop(file_guard);
            drop(file_lock);
            return (file_id, result);
        }
    }

    #[cfg(test)]
    pub(crate) fn index_file_with_source_after_id_selection_for_test(
        &self,
        path: &Path,
        source: &str,
        after_initial_selection: impl FnOnce(FileId),
    ) -> Result<FileIndexStats> {
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (file_id, result) =
            self.with_current_automatic_file_id(&path_key, after_initial_selection, |file_id| {
                self.index_file_internal(
                    SourceIndexRequest::caller_owned(path, path_key.clone(), source, file_id),
                    || Ok(()),
                )
            });
        drop(path_guard);
        drop(path_lock);
        if result.is_err() {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn index_file_with_source_before_publish_for_test<F>(
        &self,
        path: &Path,
        source: &str,
        before_publish: F,
    ) -> Result<FileIndexStats>
    where
        F: FnOnce() -> Result<()>,
    {
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let _path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file_id = {
            let mut inner = self.write_inner();
            inner
                .file_id_for_key(&path_key)
                .unwrap_or_else(|| inner.allocate_workspace_file_id())
        };
        let file_lock = self.get_file_lock(file_id);
        let _file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = self.index_file_internal(
            SourceIndexRequest::caller_owned(path, path_key.clone(), source, file_id),
            before_publish,
        );
        drop(_file_guard);
        drop(file_lock);
        drop(_path_guard);
        drop(path_lock);
        if result.is_err() {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result
    }

    pub fn index_file_with_source_and_edit(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
        text_edit: Option<&hyperion_parser::TextEdit>,
        old_source: Option<&str>,
    ) -> Result<FileIndexStats> {
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let _path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file_lock = self.get_file_lock(file_id);
        let _per_file_guard = file_lock.lock().unwrap_or_else(|poisoned| {
            // BUG FIX H15: File lock poisoned - a thread panicked while indexing this file
            // Index state may be inconsistent (symbol in one map but not others)
            // Force-clean all traces before proceeding
            warn!(
                "File lock for {:?} was poisoned (thread panicked during indexing). \
                 Clearing stale index state before re-indexing...",
                file_id
            );

            let guard = poisoned.into_inner();

            // Clean up all index state for this file to prevent inconsistencies
            let mut inner = self.write_inner();
            let removed_fqns = self.force_clean_file_state_internal(&mut inner, file_id);
            self.invalidate_caches_for_fqns_in_state(&inner, &removed_fqns);

            guard
        });

        let result = self.index_file_internal(
            SourceIndexRequest {
                path,
                path_key: path_key.clone(),
                move_from: None,
                move_from_key: None,
                source,
                file_id,
                text_edit,
                old_source,
                extract_usages: true,
                cache_parse_tree: true,
                content_hash: None,
                origin: SourceRevisionOrigin::EditorOrCaller,
            },
            || Ok(()),
        );
        drop(_per_file_guard);
        drop(file_lock);
        drop(_path_guard);
        drop(path_lock);
        if result.is_err() {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result
    }

    /// Index an incremental source revision while the caller holds the path
    /// lock and then the file lock.
    pub fn index_file_with_edit_and_locks_held(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
        text_edit: Option<&hyperion_parser::TextEdit>,
        old_source: Option<&str>,
    ) -> Result<FileIndexStats> {
        let path_key = path_lookup_key(path);
        self.index_file_with_edit_and_key_and_locks_held(
            path, &path_key, source, file_id, text_edit, old_source,
        )
    }

    pub fn index_file_with_edit_and_key_and_locks_held(
        &self,
        path: &Path,
        path_key: &PathLookupKey,
        source: &str,
        file_id: FileId,
        text_edit: Option<&hyperion_parser::TextEdit>,
        old_source: Option<&str>,
    ) -> Result<FileIndexStats> {
        self.index_file_internal(
            SourceIndexRequest {
                path,
                path_key: path_key.clone(),
                move_from: None,
                move_from_key: None,
                source,
                file_id,
                text_edit,
                old_source,
                extract_usages: true,
                cache_parse_tree: true,
                content_hash: None,
                origin: SourceRevisionOrigin::EditorOrCaller,
            },
            || Ok(()),
        )
    }

    pub fn index_file_with_source_and_id(
        &self,
        path: &Path,
        source: &str,
        file_id: FileId,
    ) -> Result<FileIndexStats> {
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let _path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Extraction is staged outside the global index lock. Serialize the
        // complete attempt so two revisions cannot finish extraction out of
        // order and publish stale data last. Different files remain concurrent.
        let file_lock = self.get_file_lock(file_id);
        let _per_file_guard = file_lock.lock().unwrap_or_else(|poisoned| {
            // BUG FIX H15: File lock poisoned - a thread panicked while indexing this file
            // Index state may be inconsistent (symbol in one map but not others)
            // Force-clean all traces before proceeding
            warn!(
                "File lock for {:?} was poisoned (thread panicked during indexing). \
                 Clearing stale index state before re-indexing...",
                file_id
            );

            let guard = poisoned.into_inner();

            // Clean up all index state for this file to prevent inconsistencies
            let mut inner = self.write_inner();
            let removed_fqns = self.force_clean_file_state_internal(&mut inner, file_id);
            self.invalidate_caches_for_fqns_in_state(&inner, &removed_fqns);

            guard
        });

        let result = self.index_file_internal(
            SourceIndexRequest::caller_owned(path, path_key.clone(), source, file_id),
            || Ok(()),
        );
        drop(_per_file_guard);
        drop(file_lock);
        drop(_path_guard);
        drop(path_lock);
        if result.is_err() {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result
    }

    pub fn remove_file(&self, file_id: FileId) {
        self.remove_file_after_key_selection(file_id, |_| {});
    }

    #[cfg(test)]
    pub(crate) fn remove_file_after_key_selection_for_test(
        &self,
        file_id: FileId,
        after_key_selection: impl FnMut(Option<&PathLookupKey>),
    ) {
        self.remove_file_after_key_selection(file_id, after_key_selection);
    }

    fn remove_file_after_key_selection(
        &self,
        file_id: FileId,
        mut after_key_selection: impl FnMut(Option<&PathLookupKey>),
    ) {
        // Follow the same path → file lock order as indexing. Deletion can race
        // a watcher update or didOpen; without these locks, a stale indexer can
        // publish the file again after it has been removed.
        loop {
            let known_key = self.inner.read().file_path_key(file_id).cloned();
            after_key_selection(known_key.as_ref());
            let path_lock = known_key
                .as_ref()
                .map(|key| self.get_path_lock_for_key(key));
            let path_guard = path_lock.as_ref().map(|lock| {
                lock.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            });
            let file_lock = self.get_file_lock(file_id);
            let file_guard = file_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let key_is_current = self.inner.read().file_path_key(file_id) == known_key.as_ref();
            if !key_is_current {
                drop(file_guard);
                drop(file_lock);
                drop(path_guard);
                drop(path_lock);
                if let Some(known_key) = known_key.as_ref() {
                    self.remove_path_lock_key_if_unused(known_key);
                }
                continue;
            }

            let removed_key = self.remove_file_with_locks_held(file_id);

            // Drop all guards before conditionally reclaiming their lock entries.
            drop(file_guard);
            drop(file_lock);
            drop(path_guard);
            drop(path_lock);

            self.remove_file_lock_if_unused(file_id);
            if let Some(removed_key) = removed_key.as_ref() {
                self.remove_path_lock_key_if_unused(removed_key);
            }
            break;
        }

        debug!("Removed file {:?} from workspace index", file_id);
    }

    fn reclaim_unregistered_path_locks(&self, keys: &[PathLookupKey]) {
        let unregistered = {
            let inner = self.inner.read();
            keys.iter()
                .filter(|key| inner.file_id_for_key(key).is_none())
                .cloned()
                .collect::<Vec<_>>()
        };
        for key in unregistered {
            self.remove_path_lock_key_if_unused(&key);
        }
    }

    /// Remove the revision currently published at `path`, then run the caller's
    /// cross-layer cleanup while the path and file locks are still held.
    ///
    /// A delayed watcher event may carry a `FileId` that has since been
    /// replaced or relocated. In that case this reports `RevisionChanged`
    /// without invoking either callback. `Retained` means `should_remove` ran
    /// under both locks and rejected removal. On success, dependent files are
    /// captured before the declaration and reverse edges are retired.
    pub fn remove_file_revision<P, F>(
        &self,
        path: &Path,
        file_id: FileId,
        should_remove: P,
        after_removal: F,
    ) -> FileRevisionRemovalOutcome
    where
        P: FnOnce() -> bool,
        F: FnOnce(),
    {
        self.remove_file_revision_after_key_selection(
            path,
            file_id,
            |_| {},
            should_remove,
            after_removal,
        )
    }

    #[cfg(test)]
    pub(crate) fn remove_file_revision_after_key_selection_for_test<P, F>(
        &self,
        path: &Path,
        file_id: FileId,
        after_key_selection: impl FnOnce(&[PathLookupKey]),
        should_remove: P,
        after_removal: F,
    ) -> FileRevisionRemovalOutcome
    where
        P: FnOnce() -> bool,
        F: FnOnce(),
    {
        self.remove_file_revision_after_key_selection(
            path,
            file_id,
            after_key_selection,
            should_remove,
            after_removal,
        )
    }

    fn remove_file_revision_after_key_selection<P, F>(
        &self,
        path: &Path,
        file_id: FileId,
        after_key_selection: impl FnOnce(&[PathLookupKey]),
        should_remove: P,
        after_removal: F,
    ) -> FileRevisionRemovalOutcome
    where
        P: FnOnce() -> bool,
        F: FnOnce(),
    {
        let requested_key = path_lookup_key(path);
        let stored_key = {
            let inner = self.inner.read();
            inner
                .registered_path(file_id)
                .filter(|registered| registered.display_path() == path)
                .map(|registered| registered.lookup_key().clone())
        };
        let mut locked_keys = vec![requested_key.clone()];
        if let Some(stored_key) = stored_key.as_ref() {
            locked_keys.push(stored_key.clone());
        }
        locked_keys.sort_unstable();
        locked_keys.dedup();
        after_key_selection(&locked_keys);
        let path_locks = locked_keys
            .iter()
            .map(|key| self.get_path_lock_for_key(key))
            .collect::<Vec<_>>();
        let path_guards = path_locks
            .iter()
            .map(|lock| {
                lock.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            })
            .collect::<Vec<_>>();
        let file_lock = self.get_file_lock(file_id);
        let file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let (exact_key_matches, absent_display_matches, file_is_published) = {
            let inner = self.inner.read();
            let requested_owner = inner.file_id_for_key(&requested_key);
            let exact_key_matches = requested_owner == Some(file_id)
                && inner.file_path_key(file_id) == Some(&requested_key);
            let absent_display_matches = requested_owner.is_none()
                && stored_key.as_ref().is_some_and(|stored_key| {
                    inner.file_path_key(file_id) == Some(stored_key)
                        && inner.file_id_for_key(stored_key) == Some(file_id)
                        && inner.file_path(file_id) == Some(path)
                        && inner.file_id_for_unique_display(path) == Some(file_id)
                });
            (
                exact_key_matches,
                absent_display_matches,
                inner.paths.contains_file_id(file_id),
            )
        };
        let revision_matches =
            exact_key_matches || (absent_display_matches && matches!(path.try_exists(), Ok(false)));
        if !revision_matches {
            drop(file_guard);
            drop(file_lock);
            drop(path_guards);
            drop(path_locks);
            if !file_is_published {
                self.remove_file_lock_if_unused(file_id);
            }
            self.reclaim_unregistered_path_locks(&locked_keys);
            return FileRevisionRemovalOutcome::RevisionChanged;
        }
        if !should_remove() {
            drop(file_guard);
            drop(file_lock);
            drop(path_guards);
            drop(path_locks);
            self.reclaim_unregistered_path_locks(&locked_keys);
            return FileRevisionRemovalOutcome::Retained;
        }
        // `should_remove` can wait on or inspect other state. A create may land
        // while it runs even for the ordinary exact-key case, whose identity
        // remains stable across delete/recreate. Never let that stale absence
        // observation retire a now-live source.
        if !matches!(path.try_exists(), Ok(false)) {
            drop(file_guard);
            drop(file_lock);
            drop(path_guards);
            drop(path_locks);
            self.reclaim_unregistered_path_locks(&locked_keys);
            return FileRevisionRemovalOutcome::RevisionChanged;
        }

        let dependent_files = self.get_dependent_files_for_file(file_id);
        let removed_key = self.remove_file_with_locks_held(file_id);
        after_removal();

        drop(file_guard);
        drop(file_lock);
        drop(path_guards);
        drop(path_locks);

        self.remove_file_lock_if_unused(file_id);
        if let Some(removed_key) = removed_key.as_ref() {
            if !locked_keys.contains(removed_key) {
                locked_keys.push(removed_key.clone());
            }
        }
        self.reclaim_unregistered_path_locks(&locked_keys);

        debug!(
            "Removed file {:?} from workspace index at {}",
            file_id,
            path.display()
        );
        FileRevisionRemovalOutcome::Removed { dependent_files }
    }

    /// Remove every index-owned component for `file_id` while its publication
    /// locks are held by the caller. Returns the immutable lookup key needed to
    /// reclaim its lock after deletion; recomputing from the former display
    /// path could produce another identity once a symlink is broken.
    pub(super) fn remove_file_with_locks_held(&self, file_id: FileId) -> Option<PathLookupKey> {
        let mut inner = self.write_inner();

        // Remove symbols and track removed FQNs for cache invalidation.
        let removed_fqns = remove_symbols_for_file(&mut inner, file_id);

        remove_imports_for_file(&mut inner, file_id);

        // Clean up symbol usages referencing this file.
        let removed_usage_counts = remove_symbol_usages_for_file(&mut inner, file_id);
        self.replace_indexed_usage_counts(&removed_usage_counts, &HashMap::new());

        // Remove both sides of the path mapping. Keep the immutable key so its
        // lock can be reclaimed after all guards are released.
        let removed_key = inner.remove_file_path(file_id);
        inner.file_lexical_identifiers.remove(&file_id);

        // External metadata and precedence were retired atomically inside
        // `remove_symbols_for_file` before fallback election.
        remove_external_source_metadata(&mut inner, file_id);
        inner.file_content_cache.pop(&file_id);
        inner.published_revisions.remove(&file_id);

        // Deleted files must not retain their source/tree until incidental LRU
        // eviction, and FileIds are deliberately never reused.
        inner.parse_tree_cache.remove(&file_id);

        // Publish removal and cache invalidation under the same index write
        // lock, matching source-revision publication.
        self.invalidate_caches_for_fqns_in_state(&inner, &removed_fqns);
        // Removing one duplicate owner may promote fallback-only descendants
        // whose FQNs were absent from the removed source. Until election
        // returns an exact changed-identity set, a full resolution-cache clear
        // is the only complete invalidation boundary for source removal.
        self.clear_resolution_caches();
        removed_key
    }

    // ===== Private helper methods =====

    fn index_file_sync(&self, path: &Path) -> Result<FileIndexStats> {
        self.index_file_from_disk(path, false, || {})?
            .context("non-skipping disk index returned no revision")
    }

    fn index_file_from_disk(
        &self,
        path: &Path,
        skip_cached: bool,
        after_read: impl FnOnce(),
    ) -> Result<Option<FileIndexStats>> {
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let read_started = std::time::Instant::now();
        let source = match crate::read_source_file(path).context("Failed to read file") {
            Ok(source) => source,
            Err(error) => {
                drop(path_guard);
                drop(path_lock);
                self.remove_path_lock_key_if_unused(&path_key);
                return Err(error);
            }
        };
        IndexPhaseProfile::add_duration(&self.index_profile.read_ns, read_started.elapsed());
        after_read();

        if skip_cached {
            let current_hash = compute_content_hash(&source);
            // Resolve the path and published hash under one guard. Calling
            // `get_content_hash` from an `and_then` on `inner.read()` keeps
            // the first guard alive while recursively acquiring the same
            // lock. If a writer queues between those reads, parking_lot's
            // writer fairness blocks the recursive read behind that writer,
            // while the writer waits for the first read: a permanent cycle.
            let unchanged = {
                let inner = self.inner.read();
                inner
                    .file_id_for_key(&path_key)
                    .and_then(|file_id| inner.published_revisions.get(&file_id))
                    .is_some_and(|revision| revision.content_hash == current_hash)
            };
            if unchanged {
                return Ok(None);
            }
        }

        let (file_id, result) = self.index_file_with_source_under_path_lock(
            path,
            &path_key,
            &source,
            SourceRevisionOrigin::Disk,
        );
        drop(path_guard);
        drop(path_lock);
        if result.is_err() {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result.map(Some)
    }

    fn index_file_sync_symbols_only(
        &self,
        path: &Path,
    ) -> Result<(FileIndexStats, Option<IndexedFileRevision>)> {
        let path_key = path_lookup_key(path);
        let path_lock = self.get_path_lock_for_key(&path_key);
        let _path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // An editor/caller publication that won this path before the cold
        // declaration pass is authoritative. Re-reading disk here used to
        // overwrite unsaved text during startup. Preserve that exact revision
        // and, when its bounded source is still retained, run its usage
        // extraction after the complete declaration graph becomes visible.
        if self.cold_declaration_pass_active() {
            let retained = {
                let inner = self.inner.read();
                inner.file_id_for_key(&path_key).and_then(|file_id| {
                    let revision = inner.published_revisions.get(&file_id)?;
                    (revision.origin == SourceRevisionOrigin::EditorOrCaller).then(|| {
                        let source =
                            inner
                                .file_content_cache
                                .peek(&file_id)
                                .cloned()
                                .or_else(|| {
                                    inner
                                        .parse_tree_cache
                                        .peek(&file_id)
                                        .map(|(_, source)| Arc::clone(source))
                                });
                        let symbols_added = inner.file_symbols.get(&file_id).map_or(0, Vec::len);
                        (file_id, *revision, source, symbols_added)
                    })
                })
            };
            if let Some((file_id, revision, retained_source, symbols_added)) = retained {
                let usage_revision = retained_source.map(|source| IndexedFileRevision {
                    path: path.to_path_buf(),
                    path_key,
                    file_id,
                    content_hash: revision.content_hash,
                    revision_id: revision.id,
                    retained_source: Some(source),
                    origin: revision.origin,
                });
                return Ok((
                    FileIndexStats {
                        symbols_added,
                        ..FileIndexStats::default()
                    },
                    usage_revision,
                ));
            }
        }

        let read_started = std::time::Instant::now();
        let source = match crate::read_source_file(path).context("Failed to read file") {
            Ok(source) => source,
            Err(error) => {
                drop(_path_guard);
                drop(path_lock);
                self.remove_path_lock_key_if_unused(&path_key);
                return Err(error);
            }
        };
        IndexPhaseProfile::add_duration(&self.index_profile.read_ns, read_started.elapsed());

        let content_hash = compute_content_hash(&source);
        let (file_id, result) = self.with_current_automatic_file_id(
            &path_key,
            |_| {},
            |file_id| {
                self.index_file_internal(
                    SourceIndexRequest {
                        extract_usages: false,
                        cache_parse_tree: false,
                        content_hash: Some(content_hash),
                        ..SourceIndexRequest::disk(path, path_key.clone(), &source, file_id)
                    },
                    || Ok(()),
                )
            },
        );
        let publication_failed = result.is_err();
        let result = result.map(|stats| {
            let revision_id = self
                .inner
                .read()
                .published_revisions
                .get(&file_id)
                .expect("successful declaration publication has a revision")
                .id;
            (
                stats,
                Some(IndexedFileRevision {
                    path: path.to_path_buf(),
                    path_key: path_key.clone(),
                    file_id,
                    content_hash,
                    revision_id,
                    retained_source: None,
                    origin: SourceRevisionOrigin::Disk,
                }),
            )
        });
        drop(_path_guard);
        drop(path_lock);
        if publication_failed {
            self.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);
        }
        result
    }

    fn index_file_usages_sync(&self, revision: &IndexedFileRevision) -> Result<()> {
        let path = &revision.path;
        let source = if let Some(source) = &revision.retained_source {
            Arc::clone(source)
        } else {
            let read_started = std::time::Instant::now();
            let source = Arc::new(crate::read_source_file(path).context("Failed to read file")?);
            IndexPhaseProfile::add_duration(&self.index_profile.read_ns, read_started.elapsed());
            source
        };
        let content_hash = compute_content_hash(&source);

        let path_lock = self.get_path_lock_for_key(&revision.path_key);
        let path_guard = path_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if self.inner.read().file_id_for_key(&revision.path_key) != Some(revision.file_id) {
            // Another indexer replaced or relocated this path after pass 1.
            // Its complete publication is authoritative, so stale pass-2 work
            // must not be attached to the new identity.
            return Ok(());
        }

        let file_lock = self.get_file_lock(revision.file_id);
        let file_guard = file_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let imports = {
            let inner = self.inner.read();
            if inner.file_id_for_key(&revision.path_key) != Some(revision.file_id)
                || inner
                    .published_revisions
                    .get(&revision.file_id)
                    .is_none_or(|published| {
                        published.id != revision.revision_id
                            || published.content_hash != revision.content_hash
                    })
            {
                // A newer editor/file-watcher revision won while this pass was
                // waiting for the file lock. It already indexed its own usages.
                return Ok(());
            }
            inner
                .file_imports
                .get(&revision.file_id)
                .cloned()
                .unwrap_or_default()
        };

        if content_hash != revision.content_hash {
            // Disk changed between the declaration and usage passes. Publish
            // the newly read buffer as one complete revision instead of mixing
            // its usages with the old declarations and imports.
            self.index_file_internal(
                SourceIndexRequest::disk(
                    path,
                    revision.path_key.clone(),
                    &source,
                    revision.file_id,
                ),
                || Ok(()),
            )?;
            return Ok(());
        }

        let lang = Language::from_path(path);
        let language_index = IndexPhaseProfile::language_index(lang);

        let usage_started = std::time::Instant::now();
        let usages = match lang {
            Some(Language::Kotlin) => crate::kotlin_usage_extractor::KotlinUsageExtractor::extract(
                &source,
                revision.file_id,
                &imports,
                self,
            )
            .context("Failed to extract Kotlin usages")?,
            Some(Language::Scala) => crate::scala_usage_extractor::ScalaUsageExtractor::extract(
                &source,
                revision.file_id,
                &imports,
                self,
            )
            .context("Failed to extract Scala usages")?,
            Some(Language::Groovy) => crate::groovy_usage_extractor::GroovyUsageExtractor::extract(
                &source,
                revision.file_id,
                &imports,
                self,
            )
            .context("Failed to extract Groovy usages")?,
            Some(Language::Java) | None => crate::usage_extractor::UsageExtractor::extract(
                &source,
                revision.file_id,
                &imports,
                self,
            )
            .context("Failed to extract usages")?,
        };
        IndexPhaseProfile::add_duration(
            &self.index_profile.usage_extract_ns[language_index],
            usage_started.elapsed(),
        );

        let usage_fqns: Vec<Arc<str>> = usages
            .iter()
            .map(|usage| Arc::from(usage.fqn.as_str()))
            .collect();
        let usage_counts = count_usage_fqns(&usage_fqns);
        let store_started = std::time::Instant::now();
        {
            let mut inner = self.write_inner();
            inner.allocate_revision(revision.file_id, revision.content_hash, revision.origin);
            // Fresh disk declarations have no usages yet, so cleanup there
            // would make a cold build quadratic. A caller-owned revision was
            // deliberately preserved from before the build and already has a
            // complete usage set; replace that set instead of appending a
            // duplicate after the declaration graph becomes complete.
            let removed_usage_counts = if revision.origin == SourceRevisionOrigin::EditorOrCaller {
                remove_symbol_usages_for_file(&mut inner, revision.file_id)
            } else {
                HashMap::new()
            };
            if matches!(lang, Some(Language::Java) | None) {
                inner.replace_file_method_usages(
                    revision.file_id,
                    usages.iter().map(|usage| &usage.location),
                );
            } else {
                inner.file_method_usages.remove(&revision.file_id);
            }
            for usage in usages {
                inner
                    .symbol_usages
                    .entry(usage.fqn.clone().into())
                    .or_default()
                    .push(usage.location);
            }

            if !usage_counts.is_empty() {
                inner
                    .file_usage_counts
                    .insert(revision.file_id, usage_counts.clone());
            }
            self.replace_indexed_usage_counts(&removed_usage_counts, &usage_counts);
        }
        IndexPhaseProfile::add_duration(&self.index_profile.store_lock_ns, store_started.elapsed());

        drop(file_guard);
        drop(file_lock);
        drop(path_guard);
        drop(path_lock);
        Ok(())
    }

    fn index_file_internal<F>(
        &self,
        request: SourceIndexRequest<'_>,
        before_publish: F,
    ) -> Result<FileIndexStats>
    where
        F: FnOnce() -> Result<()>,
    {
        let SourceIndexRequest {
            path,
            path_key,
            move_from,
            move_from_key,
            source,
            file_id,
            text_edit,
            old_source,
            extract_usages,
            cache_parse_tree,
            content_hash,
            origin,
        } = request;
        // PERFORMANCE FIX: Split lock acquisition into short critical sections
        // Symbol extraction is SLOW (parsing, analysis) and doesn't need the lock
        // Only acquire lock for actual HashMap mutations

        // Compute content hash before acquiring lock (avoid holding lock during hashing)
        let hash_started = std::time::Instant::now();
        let content_hash = content_hash.unwrap_or_else(|| compute_content_hash(source));
        let lexical_identifiers = lexical_identifier_hashes(source);
        IndexPhaseProfile::add_duration(&self.index_profile.hash_ns, hash_started.elapsed());

        // Snapshot the only prior-revision input needed for incremental parsing.
        // Do not mutate published state until every fallible extraction step has
        // completed: callers must keep seeing the last good revision if parsing
        // or analysis of the replacement fails.
        let setup_started = std::time::Instant::now();
        let (cached_tree_and_source, replaced_fqns) = {
            let inner = self.inner.read();
            let previous_file_id = if move_from.is_some() {
                Some(file_id)
            } else {
                inner.file_id_for_key(&path_key)
            };
            let mut replaced_fqns: HashSet<_> = previous_file_id
                .and_then(|previous_file_id| inner.file_symbols.get(&previous_file_id))
                .into_iter()
                .flatten()
                .map(|symbol| Arc::clone(&symbol.fqn))
                .collect();
            if move_from.is_some() {
                if let Some(destination_id) = inner
                    .file_id_for_key(&path_key)
                    .filter(|destination_id| *destination_id != file_id)
                {
                    replaced_fqns.extend(
                        inner
                            .file_symbols
                            .get(&destination_id)
                            .into_iter()
                            .flatten()
                            .map(|symbol| Arc::clone(&symbol.fqn)),
                    );
                }
            }
            (
                previous_file_id
                    .and_then(|previous_file_id| inner.parse_tree_cache.peek(&previous_file_id))
                    .cloned(),
                replaced_fqns,
            )
        };
        IndexPhaseProfile::add_duration(&self.index_profile.setup_lock_ns, setup_started.elapsed());

        // Pure computation phase (NO LOCK) - symbol extraction, usage extraction, parse tree
        // This is the SLOW part (parsing, semantic analysis) - don't block readers!
        let lang = Language::from_path(path);
        let language_index = IndexPhaseProfile::language_index(lang);
        self.index_profile.files[language_index].fetch_add(1, Ordering::Relaxed);
        let symbol_started = std::time::Instant::now();
        let extraction = extract_symbols_for_language(
            lang,
            source,
            path,
            text_edit,
            old_source,
            cached_tree_and_source.as_ref(),
        )?;
        IndexPhaseProfile::add_duration(
            &self.index_profile.symbol_extract_ns[language_index],
            symbol_started.elapsed(),
        );

        // Prepare imports for storage (NO LOCK - just data manipulation)
        let mut imports = extraction.imports;
        imports.current_package = extraction.package.clone();
        let revision_symbols =
            crate::import_resolver::RevisionSymbolOverlay::new(&extraction.symbols, replaced_fqns);

        // Compute one canonical full Java parse product for both usage
        // extraction and incremental parsing. A declarations-only cold pass
        // requests neither and therefore avoids populating a cache that would
        // immediately evict most workspace files.
        let parse_cache_started = std::time::Instant::now();
        let new_parse_tree = if (cache_parse_tree || extract_usages)
            && (lang == Some(Language::Java) || lang.is_none())
        {
            let tree = if let (Some(edit), Some(old_src), Some((cached_tree, cached_src))) =
                (text_edit, old_source, cached_tree_and_source.as_ref())
            {
                if cached_src.as_str() == old_src {
                    let (new_tree, _errors) =
                        hyperion_parser::incremental_parse(cached_tree, old_src, edit);
                    debug!(
                        "Parse tree cache updated via incremental parse for {}",
                        path.display()
                    );
                    new_tree
                } else {
                    let tokens = hyperion_parser::Lexer::new(source).collect();
                    let (tree, _errors) = hyperion_parser::Parser::parse(tokens);
                    tree
                }
            } else {
                let tokens = hyperion_parser::Lexer::new(source).collect();
                let (tree, _errors) = hyperion_parser::Parser::parse(tokens);
                tree
            };
            Some(Arc::new(tree))
        } else {
            None
        };
        IndexPhaseProfile::add_duration(
            &self.index_profile.parse_cache_ns,
            parse_cache_started.elapsed(),
        );

        // Extract usages (NO LOCK - resolves imports against index via read path).
        // Java consumes the canonical tree above rather than parsing the same
        // revision again. Kotlin has its own grammar-specific extractor; the
        // Kotlin, Scala, and Groovy use grammar-specific extractors.
        let usages = if extract_usages {
            let usage_started = std::time::Instant::now();
            let usages = match lang {
                Some(Language::Kotlin) => {
                    crate::kotlin_usage_extractor::KotlinUsageExtractor::extract_for_revision(
                        source,
                        file_id,
                        &imports,
                        self,
                        &revision_symbols,
                    )
                }
                Some(Language::Scala) => {
                    crate::scala_usage_extractor::ScalaUsageExtractor::extract_for_revision(
                        source,
                        file_id,
                        &imports,
                        self,
                        &revision_symbols,
                    )
                    .context("Failed to extract Scala usages")?
                }
                Some(Language::Groovy) => {
                    crate::groovy_usage_extractor::GroovyUsageExtractor::extract_for_revision(
                        source,
                        file_id,
                        &imports,
                        self,
                        &revision_symbols,
                    )
                    .context("Failed to extract Groovy usages")?
                }
                Some(Language::Java) | None => {
                    let root = hyperion_parser::SyntaxNode::new_root(
                        new_parse_tree
                            .as_ref()
                            .expect("Java usage extraction requires a full parse")
                            .as_ref()
                            .clone(),
                    );
                    crate::usage_extractor::UsageExtractor::extract_from_syntax_for_revision(
                        &root,
                        file_id,
                        &imports,
                        self,
                        &revision_symbols,
                    )
                }
            };
            IndexPhaseProfile::add_duration(
                &self.index_profile.usage_extract_ns[language_index],
                usage_started.elapsed(),
            );
            usages
        } else {
            Vec::new()
        };
        let usage_fqns: Vec<Arc<str>> = usages.iter().map(|u| Arc::from(u.fqn.as_str())).collect();
        let usage_counts = count_usage_fqns(&usage_fqns);

        // Keep the exact indexed buffer available to navigation and range
        // validation. For open documents this may differ from the on-disk file;
        // falling back to disk here would mark a correct unsaved index as stale
        // and overwrite it during goto-definition.
        let indexed_source = Arc::new(source.to_string());
        // Tests use this boundary to prove readers never observe a half-removed
        // revision. Production callers pass a zero-cost no-op closure.
        before_publish()?;
        if (origin == SourceRevisionOrigin::Disk || move_from.is_some())
            && path_lookup_key(path) != path_key
        {
            anyhow::bail!(
                "discarded revision for {} because its established path identity changed",
                path.display()
            );
        }

        // Publish the complete replacement under one write lock. Identity
        // migration, old-revision removal, and new-revision insertion are one
        // transaction from every reader's point of view.
        let store_started = std::time::Instant::now();
        let (symbols_added, affected_fqns, dependent_files, retired_file_ids) = {
            let mut inner = self.write_inner();
            if let Some(old_path) = move_from {
                let old_key = move_from_key
                    .as_ref()
                    .expect("move requests carry the old path key");
                let revision_matches = inner.file_id_for_key(old_key) == Some(file_id)
                    && inner.file_path_key(file_id) == Some(old_key)
                    && inner.file_path(file_id) == Some(old_path);
                if !revision_matches {
                    anyhow::bail!(
                        "cannot move stale revision {:?} from {} to {}",
                        file_id,
                        old_path.display(),
                        path.display()
                    );
                }
            }
            let mut removed_fqns = Vec::new();
            let mut affected_symbols = Vec::new();
            let mut retired_file_ids = Vec::new();
            let mut dependent_files = HashSet::new();
            let mut removed_usage_counts = HashMap::new();

            // Capture incoming edges before deletion removes historical FQN
            // keys. Post-publication code cannot reconstruct these for a class
            // that was renamed or deleted by this revision.
            if let Some(previous_file_id) = inner.file_id_for_key(&path_key) {
                let previous_fqns = inner
                    .file_symbols
                    .get(&previous_file_id)
                    .into_iter()
                    .flatten()
                    .map(|symbol| symbol.fqn.as_ref());
                super::cache::collect_dependent_files_for_fqns_from_inner(
                    &inner,
                    previous_fqns,
                    &mut dependent_files,
                );
            }

            if let Some(old_file_id) = inner.file_id_for_key(&path_key) {
                if old_file_id != file_id {
                    debug!(
                        "FileId mismatch: {:?} had {:?}, now {:?}",
                        path.display(),
                        old_file_id,
                        file_id
                    );

                    let removed =
                        unregister_source_symbols_without_election(&mut inner, old_file_id);
                    removed_fqns.extend(removed.iter().map(|symbol| Arc::clone(&symbol.fqn)));
                    affected_symbols.extend(removed);
                    remove_imports_for_file(&mut inner, old_file_id);
                    merge_usage_counts(
                        &mut removed_usage_counts,
                        remove_symbol_usages_for_file(&mut inner, old_file_id),
                    );
                    inner.remove_file_path(old_file_id);
                    inner.file_lexical_identifiers.remove(&old_file_id);
                    inner.file_content_cache.pop(&old_file_id);
                    inner.published_revisions.remove(&old_file_id);
                    inner.parse_tree_cache.remove(&old_file_id);
                    retired_file_ids.push(old_file_id);
                }
            }

            // Relocation must precede storing this revision's hash and source:
            // otherwise those new values would be moved to the displaced file.
            if move_from.is_none() {
                relocate_displaced_file(&mut inner, file_id, path, &path_key);
            }
            inner.register_file_path(file_id, path.to_path_buf(), path_key);

            let removed = unregister_source_symbols_without_election(&mut inner, file_id);
            removed_fqns.extend(removed.iter().map(|symbol| Arc::clone(&symbol.fqn)));
            affected_symbols.extend(removed);
            merge_usage_counts(
                &mut removed_usage_counts,
                remove_symbol_usages_for_file(&mut inner, file_id),
            );
            inner.file_content_cache.pop(&file_id);
            inner.allocate_revision(file_id, content_hash, origin);

            process_imports_for_file(&mut inner, file_id, &imports);

            inner.replace_file_generic_supertypes(file_id, extraction.generic_supertypes);
            let (symbols_added, symbol_fqns, added_symbols) =
                store_symbols_for_file_without_election(&mut inner, file_id, extraction.symbols);
            affected_symbols.extend(added_symbols);
            // The typealiases either revision declares, for cache invalidation.
            let type_alias_fqns = super::inheritance::declared_kotlin_type_alias_fqns(
                affected_symbols.iter().map(AsRef::as_ref),
            );
            reelect_affected_symbols(&mut inner, affected_symbols);

            if matches!(lang, Some(Language::Java) | None) {
                inner.replace_file_method_usages(
                    file_id,
                    usages.iter().map(|usage| &usage.location),
                );
            } else {
                inner.file_method_usages.remove(&file_id);
            }
            for usage in usages {
                inner
                    .symbol_usages
                    .entry(usage.fqn.clone().into())
                    .or_default()
                    .push(usage.location);
            }

            if usage_counts.is_empty() {
                inner.file_usage_counts.remove(&file_id);
            } else {
                inner
                    .file_usage_counts
                    .insert(file_id, usage_counts.clone());
            }
            self.replace_indexed_usage_counts(&removed_usage_counts, &usage_counts);

            match (cache_parse_tree, new_parse_tree) {
                (true, Some(tree)) => {
                    inner
                        .parse_tree_cache
                        .put(file_id, tree, Arc::clone(&indexed_source));
                    debug!(
                        "Cached parse tree for {} (cache entries: {}, approx bytes: {})",
                        path.display(),
                        inner.parse_tree_cache.len(),
                        inner.parse_tree_cache.total_bytes(),
                    );
                }
                _ => {
                    // Every published cache entry must describe this exact
                    // source revision. A declarations-only rebuild keeps no
                    // full tree, so discard the previous revision's tree.
                    inner.parse_tree_cache.remove(&file_id);
                }
            }

            inner
                .file_content_cache
                .put(file_id, Arc::clone(&indexed_source));
            if let Some(identifiers) = lexical_identifiers {
                inner.file_lexical_identifiers.insert(file_id, identifiers);
            } else {
                inner.file_lexical_identifiers.remove(&file_id);
            }

            removed_fqns.extend(symbol_fqns);
            removed_fqns.sort_unstable();
            removed_fqns.dedup();

            super::cache::collect_dependent_files_for_fqns_from_inner(
                &inner,
                removed_fqns.iter().map(AsRef::as_ref),
                &mut dependent_files,
            );
            dependent_files.remove(&file_id);
            for retired_file_id in &retired_file_ids {
                dependent_files.remove(retired_file_id);
            }
            let mut dependent_files: Vec<_> = dependent_files.into_iter().collect();
            dependent_files.sort_unstable_by_key(|file_id| file_id.as_u32());

            if type_alias_fqns.is_empty() {
                self.invalidate_caches_for_fqns_in_state(&inner, &removed_fqns);
            } else {
                // A class written against an alias name lives in another file,
                // so only the alias names reach it.
                let mut invalidated_fqns = removed_fqns.clone();
                invalidated_fqns.extend(type_alias_fqns);
                self.invalidate_caches_for_fqns_in_state(&inner, &invalidated_fqns);
            }

            (
                symbols_added,
                removed_fqns,
                dependent_files,
                retired_file_ids,
            )
        };
        IndexPhaseProfile::add_duration(&self.index_profile.store_lock_ns, store_started.elapsed());

        for retired_file_id in &retired_file_ids {
            self.remove_file_lock_if_unused(*retired_file_id);
        }

        Ok(FileIndexStats {
            symbols_added,
            affected_fqns,
            dependent_files,
            retired_file_ids,
        })
    }

    /// Add an external symbol to the index (e.g., from JDK bytecode)
    ///
    /// External symbols don't have a real file or range, but are still searchable.
    fn force_clean_file_state_internal(
        &self,
        inner: &mut IndexInner,
        file_id: FileId,
    ) -> Vec<Arc<str>> {
        let was_external = inner.external_file_ids.contains(&file_id);
        let removed_fqns = remove_symbols_for_file(inner, file_id);

        remove_imports_for_file(inner, file_id);

        // Clean up symbol usages
        let removed_usage_counts = remove_symbol_usages_for_file(inner, file_id);
        self.replace_indexed_usage_counts(&removed_usage_counts, &HashMap::new());

        // Remove path mapping
        inner.remove_file_path(file_id);
        inner.file_lexical_identifiers.remove(&file_id);

        // Clean up external file mappings
        remove_external_source_metadata(inner, file_id);
        if was_external {
            self.clear_resolution_caches();
        }

        // Invalidate file content cache
        inner.file_content_cache.pop(&file_id);

        // Remove content hash
        inner.published_revisions.remove(&file_id);

        // NOTE: We do NOT remove parse_tree_cache here because caller will re-index
        // NOTE: We do NOT remove per-file lock here because caller holds it

        removed_fqns
    }
}

/// Check if a fully qualified name belongs to an external (immutable) package.
///
/// External symbols come from the JDK, standard library, or major frameworks.
/// These symbols are immutable during development - we never need to track
/// reverse dependencies for cache invalidation.
///
/// # Why this matters
///
/// Without this filter, `reverse_dependencies` would track every file that
/// imports `java.lang.String`, causing unbounded memory growth:
/// - 10,000 files × 50 imports = 500K entries in reverse_dependencies
/// - Most entries are never invalidated (JDK doesn't change)
///
/// # Examples
///
/// ```rust,ignore
/// assert!(is_external_symbol("java.lang.String"));        // JDK core
/// assert!(is_external_symbol("javax.swing.JFrame"));      // JDK swing
/// assert!(is_external_symbol("jakarta.servlet.http.HttpServlet")); // Jakarta EE
/// assert!(is_external_symbol("kotlin.collections.List")); // Kotlin stdlib
/// assert!(!is_external_symbol("com.example.User"));       // Workspace code
/// assert!(!is_external_symbol("com.sunny.energy.Panel")); // Not com.sun.java.*
/// ```
fn is_external_symbol(fqn: &str) -> bool {
    const EXTERNAL_PREFIXES: &[&str] = &[
        // Java SE (standard library)
        "java.",
        "javax.",
        "jakarta.", // Jakarta EE (Java EE 9+)
        "jdk.",
        "sun.",          // Internal JDK classes (sun.misc.Unsafe, sun.reflect.*, etc.)
        "com.sun.java.", // More specific to avoid matching company names like com.sunny
        // Kotlin standard library
        "kotlin.",
        "kotlinx.",
        // Groovy standard library
        "groovy.",
        "org.codehaus.groovy.",
        // Scala standard library
        "scala.",
        // Android framework
        "android.",
        "androidx.",
        "com.google.android.",
        // Note: We intentionally do NOT include:
        // - "com.sun." (too broad - matches company names like com.sunny)
        // - Framework packages like "org.springframework." (user code may shadow these)
        //
        // Add frameworks conservatively based on real memory usage data.
    ];

    EXTERNAL_PREFIXES
        .iter()
        .any(|prefix| fqn.starts_with(prefix))
}

#[cfg(test)]
mod cold_index_revision_tests {
    use super::*;

    #[test]
    fn cold_build_phase_guard_clears_on_every_exit_path() {
        let phase = AtomicU8::new(COLD_BUILD_IDLE);
        {
            let guard = ColdBuildPhaseGuard::begin(&phase);
            assert_eq!(phase.load(Ordering::Acquire), COLD_BUILD_DECLARATIONS);
            guard.begin_usage_pass();
            assert_eq!(phase.load(Ordering::Acquire), COLD_BUILD_USAGES);
        }
        assert_eq!(phase.load(Ordering::Acquire), COLD_BUILD_IDLE);
    }

    #[test]
    fn disk_read_is_inside_the_path_serialization_boundary() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("SerializedRead.java");
        std::fs::write(&path, "package app; class SerializedRead {}").unwrap();
        let index = WorkspaceIndex::new(workspace.path().to_path_buf());

        index
            .index_file_sync_after_read_for_test(&path, || {
                let same_path_lock = index.get_path_lock(&path);
                assert!(
                    same_path_lock.try_lock().is_err(),
                    "a source buffer read from disk must not be publishable after a newer path update"
                );
            })
            .unwrap();
        assert!(index.get_symbol("app.SerializedRead").is_some());
    }

    #[test]
    fn usage_pass_replaces_a_disk_revision_changed_after_declarations() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("Changing.java");
        let before = "package app; class Before { int oldField; }";
        let after = "package app; class After { int newField; }";
        std::fs::write(&path, before).unwrap();

        let index = WorkspaceIndex::new(workspace.path().to_path_buf());
        index.index_file_with_source(&path, before).unwrap();
        let initial_file_id = index.get_file_id(&path).unwrap();
        assert!(
            index
                .inner
                .read()
                .parse_tree_cache
                .peek(&initial_file_id)
                .is_some(),
            "a complete incremental index should retain its parse product"
        );
        let (_, revision) = index.index_file_sync_symbols_only(&path).unwrap();
        let revision = revision.expect("disk declaration revision");
        let file_id = revision.file_id;
        assert!(index.get_symbol("app.Before").is_some());
        assert!(index.inner.read().parse_tree_cache.peek(&file_id).is_none());

        std::fs::write(&path, after).unwrap();
        index.index_file_usages_sync(&revision).unwrap();

        assert!(index.get_symbol("app.Before").is_none());
        assert!(index.get_symbol("app.After").is_some());
        assert_eq!(
            index
                .get_file_contents(file_id)
                .as_deref()
                .map(String::as_str),
            Some(after)
        );
        let inner = index.inner.read();
        let (_, cached_source) = inner
            .parse_tree_cache
            .peek(&file_id)
            .expect("the complete replacement should retain its parse product");
        assert_eq!(cached_source.as_str(), after);
    }

    #[test]
    fn usage_pass_does_not_overwrite_a_newer_in_memory_revision() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("EditorWins.java");
        let disk = "package app; class DiskRevision {}";
        let editor = "package app; class EditorRevision {}";
        std::fs::write(&path, disk).unwrap();

        let index = WorkspaceIndex::new(workspace.path().to_path_buf());
        let (_, cold_revision) = index.index_file_sync_symbols_only(&path).unwrap();
        let cold_revision = cold_revision.expect("disk declaration revision");
        let file_id = cold_revision.file_id;

        index
            .index_file_with_source_and_id(&path, editor, file_id)
            .unwrap();
        index.index_file_usages_sync(&cold_revision).unwrap();

        assert!(index.get_symbol("app.DiskRevision").is_none());
        assert!(index.get_symbol("app.EditorRevision").is_some());
        assert_eq!(
            index
                .get_file_contents(file_id)
                .as_deref()
                .map(String::as_str),
            Some(editor)
        );
    }
}
