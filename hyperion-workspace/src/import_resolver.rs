// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Import resolver: resolves simple names to fully qualified names
//
// Exact type binding uses lexical/single-import/current-package/on-demand
// tiers and retains ambiguity as data. Language-specific implicit imports and
// Kotlin/JVM aliases share that path. Candidate vectors below are legacy
// heuristics for unindexed/UI lookups only.

use crate::{
    index::IndexInner, is_type_declaration_kind, DeclarationNamespace, FileId, ImportMap, Language,
    Symbol, TypeNameResolution, WorkspaceIndex,
};
use lsp_types::SymbolKind;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, OnceLock};
use tracing::debug;

type MembersByOwner = HashMap<Arc<str>, HashMap<Arc<str>, Vec<Arc<Symbol>>>>;

/// Symbols extracted from an unpublished source revision.
///
/// Usage extraction runs before publication so a failed parse cannot replace
/// the last good index revision. This overlay lets that extraction resolve the
/// new file against itself while masking declarations owned by the old file
/// revision. Other workspace and dependency symbols continue to come from the
/// published index.
pub(crate) struct RevisionSymbolOverlay {
    symbols: HashMap<Arc<str>, SmallVec<[Arc<Symbol>; 1]>>,
    members_by_owner: MembersByOwner,
    replaced_fqns: HashSet<Arc<str>>,
    masked_publications: HashSet<(crate::SymbolTarget, FileId)>,
}

impl RevisionSymbolOverlay {
    pub(crate) fn new(symbols: &[Symbol], replaced_fqns: HashSet<Arc<str>>) -> Self {
        let mut by_fqn = HashMap::with_capacity(symbols.len());
        let mut members_by_owner = MembersByOwner::new();

        for symbol in symbols {
            let symbol = Arc::new(symbol.clone());
            by_fqn
                .entry(Arc::clone(&symbol.fqn))
                .or_insert_with(SmallVec::new)
                .push(Arc::clone(&symbol));
            if let Some(owner) = &symbol.container {
                members_by_owner
                    .entry(Arc::clone(owner))
                    .or_default()
                    .entry(Arc::clone(&symbol.simple_name))
                    .or_default()
                    .push(Arc::clone(&symbol));
            }
        }

        Self {
            symbols: by_fqn,
            members_by_owner,
            replaced_fqns,
            masked_publications: HashSet::new(),
        }
    }

    pub(crate) fn from_published_file(symbols: &[Arc<Symbol>]) -> Self {
        let mut by_fqn = HashMap::with_capacity(symbols.len());
        let mut members_by_owner = MembersByOwner::new();
        let mut replaced_fqns = HashSet::with_capacity(symbols.len());
        for symbol in symbols {
            replaced_fqns.insert(Arc::clone(&symbol.fqn));
            by_fqn
                .entry(Arc::clone(&symbol.fqn))
                .or_insert_with(SmallVec::new)
                .push(Arc::clone(symbol));
            if let Some(owner) = &symbol.container {
                members_by_owner
                    .entry(Arc::clone(owner))
                    .or_default()
                    .entry(Arc::clone(&symbol.simple_name))
                    .or_default()
                    .push(Arc::clone(symbol));
            }
        }
        Self {
            symbols: by_fqn,
            members_by_owner,
            replaced_fqns,
            masked_publications: HashSet::new(),
        }
    }

    pub(crate) fn masking_publications(
        publications: impl IntoIterator<Item = (crate::SymbolTarget, FileId)>,
    ) -> Self {
        Self {
            symbols: HashMap::new(),
            members_by_owner: MembersByOwner::new(),
            replaced_fqns: HashSet::new(),
            masked_publications: publications.into_iter().collect(),
        }
    }

    fn exact_symbol(&self, fqn: &str) -> Option<Arc<Symbol>> {
        self.symbols
            .get(fqn)?
            .iter()
            .cloned()
            .min_by(|left, right| {
                // Match published-index collision policy: nominal declarations
                // win the textual key, then source order/range is deterministic.
                is_type_declaration_kind(right.kind)
                    .cmp(&is_type_declaration_kind(left.kind))
                    .then_with(|| left.range.start.cmp(&right.range.start))
                    .then_with(|| left.range.end.cmp(&right.range.end))
            })
    }

    fn exact_symbol_target(&self, target: &crate::SymbolTarget) -> Option<Arc<Symbol>> {
        self.symbols
            .get(target.fqn.as_ref())?
            .iter()
            .filter(|symbol| target.namespace.accepts(symbol.kind))
            .cloned()
            .min_by(|left, right| {
                left.range
                    .start
                    .cmp(&right.range.start)
                    .then_with(|| left.range.end.cmp(&right.range.end))
            })
    }

    fn definition_matches(&self, fqn: &str, start: u32, end: u32) -> bool {
        self.symbols.get(fqn).is_some_and(|symbols| {
            symbols
                .iter()
                .any(|symbol| symbol.range.start == start && symbol.range.end == end)
        })
    }

    fn value_definition_at(&self, name: &str, start: u32, end: u32) -> Option<Arc<Symbol>> {
        let mut candidates = self.symbols.values().flatten().filter(|symbol| {
            symbol.simple_name.as_ref() == name
                && symbol.range.start == start
                && symbol.range.end == end
                && DeclarationNamespace::Value.accepts(symbol.kind)
        });
        let candidate = candidates.next().cloned();
        candidates.next().is_none().then_some(candidate).flatten()
    }

    fn members_by_name(&self, owner: &str, name: &str) -> Option<&[Arc<Symbol>]> {
        self.members_by_owner
            .get(owner)?
            .get(name)
            .map(Vec::as_slice)
    }

    fn replaces(&self, fqn: &str) -> bool {
        self.replaced_fqns.contains(fqn)
    }

    /// This revision's facades that declare the Kotlin typealias `alias_fqn`.
    fn kotlin_type_alias_facades<'s>(
        &'s self,
        alias_fqn: &'s str,
    ) -> impl Iterator<Item = Arc<Symbol>> + 's {
        self.symbols
            .values()
            .flatten()
            .filter(move |symbol| {
                symbol.kotlin.as_ref().is_some_and(|kotlin| {
                    kotlin
                        .type_aliases
                        .iter()
                        .any(|alias| alias.fqn == alias_fqn)
                })
            })
            .cloned()
    }

    fn masks_publication(&self, target: &crate::SymbolTarget, file_id: FileId) -> bool {
        self.masked_publications
            .contains(&(target.clone(), file_id))
    }

    fn masks_symbol_publication(&self, symbol: &Symbol, file_id: FileId) -> bool {
        self.masks_publication(&crate::SymbolTarget::for_symbol(symbol), file_id)
    }
}

/// Resolves simple class names to fully qualified names using import information
pub struct ImportResolver<'a> {
    imports: &'a ImportMap,
    workspace: ResolverWorkspace<'a>,
    revision: Option<&'a RevisionSymbolOverlay>,
    language: Language,
    /// Lazy-initialized cache of java.lang class names
    java_lang_cache: OnceLock<HashSet<String>>,
}

#[derive(Clone, Copy)]
enum ResolverWorkspace<'a> {
    Shared(&'a WorkspaceIndex),
    Coherent(&'a IndexInner),
}

impl ResolverWorkspace<'_> {
    fn project_member_type(
        self,
        receiver_fqn: &str,
        receiver_type_arguments: &[String],
        declaring_owner_fqn: &str,
        written_type: &str,
        member_type_parameters: &[crate::TypeParameterInfo],
    ) -> Option<String> {
        match self {
            Self::Shared(index) => index.project_member_type_from_receiver(
                receiver_fqn,
                receiver_type_arguments,
                declaring_owner_fqn,
                written_type,
                member_type_parameters,
            ),
            Self::Coherent(inner) => crate::index::project_type_from_receiver_in(
                inner,
                receiver_fqn,
                receiver_type_arguments,
                declaring_owner_fqn,
                written_type,
                member_type_parameters,
            ),
        }
    }

    fn value_definition_at(
        self,
        file_id: FileId,
        name: &str,
        start: u32,
        end: u32,
    ) -> Option<Arc<Symbol>> {
        match self {
            Self::Shared(index) => {
                index.find_value_definition_in_file_at_range(file_id, name, start, end)
            }
            Self::Coherent(inner) => crate::index::find_value_definition_in_file_at_range_in(
                inner, file_id, name, start, end,
            ),
        }
    }

    fn find_symbol(self, fqn: &str) -> Option<crate::SymbolLocation> {
        match self {
            Self::Shared(index) => index.find_symbol(fqn),
            Self::Coherent(inner) => inner.symbol_locations.get(fqn).cloned(),
        }
    }

    fn get_imports(self, file_id: FileId) -> Option<ImportMap> {
        match self {
            Self::Shared(index) => index.get_imports(file_id),
            Self::Coherent(inner) => inner.file_imports.get(&file_id).cloned(),
        }
    }

    fn find_methods_by_name(self, owner: &str, name: &str) -> Vec<Arc<Symbol>> {
        match self {
            Self::Shared(index) => index.find_methods_by_name(owner, name),
            Self::Coherent(inner) => {
                let mut methods = crate::index::selected_owner_member_definitions_in(inner, owner)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|definition| {
                        definition.target.namespace == DeclarationNamespace::Callable
                            && definition.symbol.simple_name.as_ref() == name
                    })
                    .map(|definition| definition.symbol)
                    .collect::<Vec<_>>();
                methods.sort_unstable_by(|left, right| {
                    let is_type_variable = |symbol: &Symbol| {
                        symbol.return_type.as_ref().is_some_and(|return_type| {
                            return_type.len() == 1
                                && return_type
                                    .chars()
                                    .next()
                                    .is_some_and(|character| character.is_ascii_uppercase())
                        })
                    };
                    is_type_variable(right)
                        .cmp(&is_type_variable(left))
                        .then_with(|| left.fqn.cmp(&right.fqn))
                });
                methods
            }
        }
    }

    fn find_members_by_name(
        self,
        owner: &str,
        name: &str,
        namespace: DeclarationNamespace,
    ) -> Vec<Arc<Symbol>> {
        match self {
            Self::Shared(index) => index.find_member_symbols_by_name(owner, name, namespace),
            Self::Coherent(inner) => {
                crate::index::selected_owner_member_definitions_in(inner, owner)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|definition| {
                        definition.target.namespace == namespace
                            && definition.symbol.simple_name.as_ref() == name
                    })
                    .map(|definition| definition.symbol)
                    .collect()
            }
        }
    }

    fn get_inheritance_chain(self, class_fqn: &str) -> Vec<String> {
        match self {
            Self::Shared(index) => index.get_inheritance_chain(class_fqn),
            Self::Coherent(inner) => {
                let mut chain = Vec::new();
                let mut visited = HashSet::new();
                let mut interfaces = VecDeque::new();
                let mut class_name = Some(class_fqn.to_owned());
                while let Some(current) = class_name.take() {
                    if !visited.insert(current.clone()) {
                        break;
                    }
                    chain.push(current.clone());
                    if !inner.symbols.contains_key(current.as_str()) {
                        break;
                    }
                    let (parent, direct_interfaces) =
                        inner.resolved_supertype_roles(current.as_str());
                    interfaces.extend(direct_interfaces);
                    class_name = parent;
                }
                while let Some(interface) = interfaces.pop_front() {
                    if !visited.insert(interface.clone()) {
                        continue;
                    }
                    chain.push(interface.clone());
                    if !inner.symbols.contains_key(interface.as_str()) {
                        continue;
                    }
                    let (parent, direct_interfaces) =
                        inner.resolved_supertype_roles(interface.as_str());
                    interfaces.extend(parent);
                    interfaces.extend(direct_interfaces);
                }
                chain
            }
        }
    }

    fn direct_supertype_roles(self, fqn: &str) -> (Option<String>, Vec<String>) {
        match self {
            Self::Shared(index) => index.direct_supertype_roles(fqn),
            Self::Coherent(inner) => inner.resolved_supertype_roles(fqn),
        }
    }

    /// Whether the package has any indexed top-level type at all.
    ///
    /// Cheaper than [`Self::get_package_class_names`], which allocates and
    /// sorts every name; this is asked once per unresolved spelling.
    fn package_has_indexed_classes(self, package: &str) -> bool {
        match self {
            Self::Shared(index) => index.package_has_class_names(package),
            Self::Coherent(_) => !self.get_package_class_names(package).is_empty(),
        }
    }

    fn get_package_class_names(self, package: &str) -> Vec<String> {
        match self {
            Self::Shared(index) => index.get_package_class_names(package),
            Self::Coherent(inner) => {
                let prefix = format!("{package}.");
                let mut names = inner
                    .package_to_classes
                    .get(package)
                    .into_iter()
                    .flatten()
                    .filter_map(|fqn| {
                        let remainder = fqn.strip_prefix(&prefix)?;
                        (!remainder.contains('.') && !remainder.contains('$')).then_some(remainder)
                    })
                    .filter(|name| {
                        inner
                            .symbols
                            .get(format!("{package}.{name}").as_str())
                            .is_some_and(|symbol| is_type_declaration_kind(symbol.kind))
                    })
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                names.sort_unstable();
                names
            }
        }
    }

    /// `java.lang` simple names, memoized on the shared index.
    ///
    /// A coherent snapshot is already a short-lived immutable view, so it
    /// computes directly; only the long-lived shared index needs the memo.
    fn java_lang_class_names(self) -> Arc<HashSet<String>> {
        match self {
            Self::Shared(index) => index.java_lang_class_names(),
            Self::Coherent(_) => Arc::new(
                self.get_package_class_names("java.lang")
                    .into_iter()
                    .collect(),
            ),
        }
    }
}

impl<'a> ImportResolver<'a> {
    pub fn new(imports: &'a ImportMap, workspace_index: &'a WorkspaceIndex) -> Self {
        Self {
            imports,
            workspace: ResolverWorkspace::Shared(workspace_index),
            revision: None,
            language: Language::Java,
            java_lang_cache: OnceLock::new(),
        }
    }

    /// Create a resolver that applies one source language's import rules.
    ///
    /// Language is a parameter of the single resolution engine, not a reason
    /// for a caller to keep its own copy of the tier order.
    /// [`Self::new`] is the Java-shaped default.
    pub fn for_language(
        imports: &'a ImportMap,
        workspace_index: &'a WorkspaceIndex,
        language: Language,
    ) -> Self {
        Self {
            imports,
            workspace: ResolverWorkspace::Shared(workspace_index),
            revision: None,
            language,
            java_lang_cache: OnceLock::new(),
        }
    }

    pub(crate) fn for_revision(
        imports: &'a ImportMap,
        workspace_index: &'a WorkspaceIndex,
        revision: &'a RevisionSymbolOverlay,
        language: Language,
    ) -> Self {
        Self {
            imports,
            workspace: ResolverWorkspace::Shared(workspace_index),
            revision: Some(revision),
            language,
            java_lang_cache: OnceLock::new(),
        }
    }

    pub(crate) fn for_coherent_index(
        imports: &'a ImportMap,
        inner: &'a IndexInner,
        revision: &'a RevisionSymbolOverlay,
        language: Language,
    ) -> Self {
        Self {
            imports,
            workspace: ResolverWorkspace::Coherent(inner),
            revision: Some(revision),
            language,
            java_lang_cache: OnceLock::new(),
        }
    }

    pub fn imports(&self) -> &ImportMap {
        self.imports
    }

    fn rebind_imports<'b>(&'b self, imports: &'b ImportMap) -> ImportResolver<'b> {
        ImportResolver {
            imports,
            workspace: self.workspace,
            revision: self.revision,
            language: self.language,
            java_lang_cache: OnceLock::new(),
        }
    }

    pub(crate) fn declaration_imports(&self, fqn: &str) -> Option<ImportMap> {
        if self
            .revision
            .is_some_and(|revision| revision.exact_symbol(fqn).is_some())
        {
            return Some(self.imports.clone());
        }
        let location = self.workspace.find_symbol(fqn)?;
        self.workspace.get_imports(location.file_id)
    }

    /// Resolve a type spelling in the lexical/import context of the declaration
    /// that owns `symbol`.
    ///
    /// Member signatures are written where the member is declared, which may be
    /// in a different package from the runtime receiver or call site. Keeping
    /// this operation here also preserves the resolver's revision overlay and
    /// its ambiguity-aware type-binding tiers; callers must not reconstruct a
    /// [`hyperion_types::ResolutionContext`] and select the first wildcard.
    pub(crate) fn resolve_declaration_type_name(
        &self,
        symbol: &crate::Symbol,
        written: &str,
    ) -> TypeNameResolution {
        let Some(imports) = self.declaration_imports(&symbol.fqn) else {
            return TypeNameResolution::Missing;
        };
        let declaration_resolver = self.rebind_imports(&imports);

        let mut lexical_owners = Vec::new();
        let mut owner = symbol.container.as_ref().map(ToString::to_string);
        while let Some(owner_fqn) = owner {
            let Some(owner_symbol) = declaration_resolver.get_symbol(&owner_fqn) else {
                break;
            };
            owner = owner_symbol.container.as_ref().map(ToString::to_string);
            if is_type_declaration_kind(owner_symbol.kind) {
                lexical_owners.push(owner_fqn);
            }
        }

        let resolved = declaration_resolver
            .resolve_indexed_type_name_with_lexical_owners(written, &lexical_owners);
        if resolved != TypeNameResolution::Missing || written.contains('.') {
            return resolved;
        }

        // Single-type bindings are identities even when their class has not
        // been indexed yet (notably java.lang during cold workspace indexing).
        // On-demand imports are deliberately excluded: without target evidence
        // two wildcard packages remain indistinguishable.
        if let Some(explicit) = imports.explicit.get(written) {
            return TypeNameResolution::Exact(explicit.clone());
        }
        crate::implicit_single_type_import(Some(self.language), written)
            .map_or(TypeNameResolution::Missing, |implicit| {
                TypeNameResolution::Exact(implicit.to_owned())
            })
    }

    /// Look up a symbol in the coherent revision view used for extraction.
    /// New declarations shadow the published file, while removed declarations
    /// from that file are hidden rather than leaking out of the old revision.
    pub(crate) fn get_symbol(&self, fqn: &str) -> Option<Arc<Symbol>> {
        if let Some(revision) = self.revision {
            if let Some(symbol) = revision.exact_symbol(fqn) {
                return Some(symbol);
            }

            if !fqn.contains('(') {
                if let Some((owner, name)) = fqn.rsplit_once('.') {
                    if let Some(members) = revision.members_by_name(owner, name) {
                        let mut callables = members
                            .iter()
                            .filter(|symbol| DeclarationNamespace::Callable.accepts(symbol.kind));
                        let symbol = Arc::clone(callables.next()?);
                        return callables.next().is_none().then_some(symbol);
                    }
                }
            }
        }

        let symbol = match self.workspace {
            ResolverWorkspace::Coherent(inner) => {
                let namespaces = if fqn.contains('(') {
                    &[
                        DeclarationNamespace::Callable,
                        DeclarationNamespace::Constructor,
                    ][..]
                } else {
                    &[
                        DeclarationNamespace::Type,
                        DeclarationNamespace::Value,
                        DeclarationNamespace::Constructor,
                        DeclarationNamespace::Callable,
                    ][..]
                };
                namespaces.iter().find_map(|namespace| {
                    let target = crate::SymbolTarget::new(fqn, *namespace);
                    crate::index::find_symbol_definition_coherent_excluding_in(
                        inner,
                        &target,
                        |candidate, file_id| {
                            self.revision.is_some_and(|revision| {
                                revision.masks_publication(candidate, file_id)
                            })
                        },
                    )
                    .map(|definition| definition.symbol)
                })?
            }
            ResolverWorkspace::Shared(index) => {
                let symbol = index.get_symbol(fqn)?;
                if self.revision.is_some_and(|revision| {
                    index.find_symbol(&symbol.fqn).is_some_and(|location| {
                        revision.masks_symbol_publication(&symbol, location.file_id)
                    })
                }) {
                    return None;
                }
                symbol
            }
        };
        if self
            .revision
            .is_some_and(|revision| revision.replaces(&symbol.fqn))
        {
            return None;
        }
        Some(symbol)
    }

    pub(crate) fn get_symbol_target(&self, target: &crate::SymbolTarget) -> Option<Arc<Symbol>> {
        if let Some(revision) = self.revision {
            if let Some(symbol) = revision.exact_symbol_target(target) {
                return Some(symbol);
            }
        }
        let symbol = match self.workspace {
            ResolverWorkspace::Coherent(inner) => {
                crate::index::find_symbol_definition_coherent_excluding_in(
                    inner,
                    target,
                    |candidate, file_id| {
                        self.revision
                            .is_some_and(|revision| revision.masks_publication(candidate, file_id))
                    },
                )?
                .symbol
            }
            ResolverWorkspace::Shared(index) => {
                let symbol = index.get_symbol_target(target)?;
                if self.revision.is_some_and(|revision| {
                    index.find_symbol(&symbol.fqn).is_some_and(|location| {
                        revision.masks_publication(target, location.file_id)
                    })
                }) {
                    return None;
                }
                symbol
            }
        };
        if self
            .revision
            .is_some_and(|revision| revision.replaces(&symbol.fqn))
        {
            return None;
        }
        Some(symbol)
    }

    pub(crate) fn has_class(&self, fqn: &str) -> bool {
        self.get_symbol(fqn)
            .is_some_and(|symbol| is_type_declaration_kind(symbol.kind))
    }

    /// Whether a source type identity is known even if its declaration has not
    /// reached the index yet. The fallback is intentionally limited to the
    /// small java.lang set whose language binding is unconditional; arbitrary
    /// on-demand packages still require indexed target evidence.
    fn has_class_or_core_language_type(&self, fqn: &str) -> bool {
        self.has_class(fqn)
            || fqn.strip_prefix("java.lang.").is_some_and(|simple| {
                !simple.contains('.') && self.get_java_lang_classes().contains(simple)
            })
    }

    pub(crate) fn definition_matches(
        &self,
        fqn: &str,
        file_id: FileId,
        start: u32,
        end: u32,
    ) -> bool {
        if let Some(revision) = self.revision {
            if revision.definition_matches(fqn, start, end) {
                return true;
            }
            if revision.replaces(fqn) {
                return false;
            }
        }
        self.workspace.find_symbol(fqn).is_some_and(|location| {
            location.file_id == file_id
                && location.range.start == start
                && location.range.end == end
                && !self.revision.is_some_and(|revision| {
                    revision.masks_publication(
                        &crate::SymbolTarget::new(
                            fqn,
                            crate::DeclarationNamespace::for_kind(location.kind),
                        ),
                        location.file_id,
                    )
                })
        })
    }

    pub(crate) fn value_definition_at(
        &self,
        file_id: FileId,
        name: &str,
        start: u32,
        end: u32,
    ) -> Option<Arc<Symbol>> {
        if let Some(revision) = self.revision {
            if let Some(symbol) = revision.value_definition_at(name, start, end) {
                return Some(symbol);
            }

            // The published index may still contain the previous revision of
            // this file while usages for its replacement are being extracted.
            // Never resurrect a declaration that the overlay replaced or a
            // virtual-deletion proof explicitly masked merely because its old
            // source range happens to match the current scope graph.
            return self
                .workspace
                .value_definition_at(file_id, name, start, end)
                .filter(|symbol| {
                    !revision.replaces(&symbol.fqn)
                        && !revision.masks_symbol_publication(symbol, file_id)
                });
        }
        self.workspace
            .value_definition_at(file_id, name, start, end)
    }

    pub(crate) fn find_methods_by_name(&self, owner: &str, name: &str) -> Vec<Arc<Symbol>> {
        let mut methods = self
            .revision
            .and_then(|revision| revision.members_by_name(owner, name))
            .map_or_else(Vec::new, |members| {
                members
                    .iter()
                    .filter(|symbol| DeclarationNamespace::Callable.accepts(symbol.kind))
                    .cloned()
                    .collect()
            });
        let mut seen: HashSet<Arc<str>> = methods
            .iter()
            .map(|symbol| Arc::clone(&symbol.fqn))
            .collect();

        if !self
            .revision
            .is_some_and(|revision| revision.replaces(owner))
        {
            let published = match self.workspace {
                ResolverWorkspace::Coherent(inner) => {
                    crate::index::selected_owner_member_definitions_excluding_in(
                        inner,
                        owner,
                        |target, file_id| {
                            self.revision
                                .is_some_and(|revision| revision.masks_publication(target, file_id))
                        },
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|definition| {
                        definition.target.namespace == DeclarationNamespace::Callable
                            && definition.symbol.simple_name.as_ref() == name
                    })
                    .map(|definition| definition.symbol)
                    .collect()
                }
                ResolverWorkspace::Shared(_) => self.workspace.find_methods_by_name(owner, name),
            };
            methods.extend(published.into_iter().filter(|symbol| {
                !self.revision.is_some_and(|revision| {
                    revision.replaces(&symbol.fqn)
                        || matches!(self.workspace, ResolverWorkspace::Shared(_))
                            && self
                                .workspace
                                .find_symbol(&symbol.fqn)
                                .is_some_and(|location| {
                                    revision.masks_symbol_publication(symbol, location.file_id)
                                })
                }) && seen.insert(Arc::clone(&symbol.fqn))
            }));
        }
        methods
    }

    fn direct_members_by_name(
        &self,
        owner: &str,
        name: &str,
        namespace: DeclarationNamespace,
    ) -> Vec<Arc<Symbol>> {
        let mut members = self
            .revision
            .and_then(|revision| revision.members_by_name(owner, name))
            .map_or_else(Vec::new, |members| {
                members
                    .iter()
                    .filter(|symbol| namespace.accepts(symbol.kind))
                    .cloned()
                    .collect()
            });
        if self
            .revision
            .is_some_and(|revision| revision.replaces(owner))
        {
            return members;
        }
        let mut seen: HashSet<Arc<str>> = members
            .iter()
            .map(|symbol| Arc::clone(&symbol.fqn))
            .collect();
        let published = match self.workspace {
            ResolverWorkspace::Coherent(inner) => {
                crate::index::selected_owner_member_definitions_excluding_in(
                    inner,
                    owner,
                    |target, file_id| {
                        self.revision
                            .is_some_and(|revision| revision.masks_publication(target, file_id))
                    },
                )
                .unwrap_or_default()
                .into_iter()
                .filter(|definition| {
                    definition.target.namespace == namespace
                        && definition.symbol.simple_name.as_ref() == name
                })
                .map(|definition| definition.symbol)
                .collect()
            }
            ResolverWorkspace::Shared(_) => {
                self.workspace.find_members_by_name(owner, name, namespace)
            }
        };
        members.extend(published.into_iter().filter(|symbol| {
            !self.revision.is_some_and(|revision| {
                revision.replaces(&symbol.fqn)
                    || matches!(self.workspace, ResolverWorkspace::Shared(_))
                        && self
                            .workspace
                            .find_symbol(&symbol.fqn)
                            .is_some_and(|location| {
                                revision.masks_symbol_publication(symbol, location.file_id)
                            })
            }) && seen.insert(Arc::clone(&symbol.fqn))
        }));
        members
    }

    /// Compute inheritance through the same overlay-aware symbol view.
    pub(crate) fn get_inheritance_chain(&self, class_fqn: &str) -> Vec<String> {
        if self.revision.is_none() {
            return self.workspace.get_inheritance_chain(class_fqn);
        }

        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut interfaces = VecDeque::new();

        // Exhaust the concrete superclass chain before considering any
        // interface. Java class members dominate interface defaults even when
        // the declaration is inherited from a grandparent; a graph-wide BFS
        // incorrectly put Child's direct interface ahead of Grandparent.
        let mut class_name = Some(class_fqn.to_owned());
        while let Some(current) = class_name.take() {
            if !visited.insert(current.clone()) {
                break;
            }
            let Some(symbol) = self.get_symbol(&current) else {
                chain.push(current);
                break;
            };
            chain.push(current);
            let (parent, direct_interfaces) = self.resolved_supertype_roles(&symbol);
            interfaces.extend(direct_interfaces);
            class_name = parent;
        }

        while let Some(interface) = interfaces.pop_front() {
            if !visited.insert(interface.clone()) {
                continue;
            }
            let Some(symbol) = self.get_symbol(&interface) else {
                chain.push(interface);
                continue;
            };
            chain.push(interface);
            let (parent, direct_interfaces) = self.resolved_supertype_roles(&symbol);
            interfaces.extend(parent);
            interfaces.extend(direct_interfaces);
        }
        chain
    }

    /// Return the canonical direct superclass through the same revision-aware
    /// hierarchy view used for ordinary member lookup.
    pub(crate) fn get_direct_superclass(&self, class_fqn: &str) -> Option<String> {
        if self.revision.is_none() {
            return self.workspace.direct_supertype_roles(class_fqn).0;
        }

        let symbol = self.get_symbol(class_fqn)?;
        self.resolved_supertype_roles(&symbol).0
    }

    fn resolved_supertype_roles(&self, symbol: &Symbol) -> (Option<String>, Vec<String>) {
        if self
            .revision
            .is_none_or(|revision| revision.exact_symbol(&symbol.fqn).is_none())
        {
            // Only the unpublished file uses this resolver's imports. Every
            // published ancestor must be interpreted in its declaring file's
            // canonical context; resolving it through the client's imports
            // silently drops or misbinds raw source supertype spellings.
            return self.workspace.direct_supertype_roles(&symbol.fqn);
        }
        let mut lexical_owners = Vec::new();
        let mut container = symbol.container.clone();
        while let Some(owner) = container {
            let owner_symbol = self.get_symbol(&owner);
            if owner_symbol
                .as_ref()
                .is_some_and(|owner| is_type_declaration_kind(owner.kind))
            {
                lexical_owners.push(owner.to_string());
            }
            container = owner_symbol.and_then(|owner| owner.container.clone());
        }
        let implicit = crate::language::implicit_jvm_supertype(
            Some(self.language),
            symbol.kind,
            &symbol.fqn,
            symbol.parent_class.as_deref(),
            false,
        );
        let mut parent = implicit
            .filter(|target| *target != "java.lang.annotation.Annotation")
            .filter(|target| self.has_class(target))
            .map(str::to_owned);
        let mut interfaces = implicit
            .filter(|target| *target == "java.lang.annotation.Annotation")
            .filter(|target| self.has_class(target))
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>();
        let mut parent_was_reclassified = false;

        if let Some(recorded) = symbol.parent_class.as_deref() {
            if implicit != Some(recorded) {
                if let TypeNameResolution::Exact(resolved) =
                    self.resolve_indexed_type_name_with_lexical_owners(recorded, &lexical_owners)
                {
                    if self
                        .get_symbol(&resolved)
                        .is_some_and(|target| target.kind == SymbolKind::INTERFACE)
                    {
                        parent_was_reclassified = true;
                        parent = None;
                        interfaces.push(resolved);
                    } else {
                        parent = Some(resolved);
                    }
                }
            }
        }
        let mut all_bare_resolved = true;
        let mut bare = symbol
            .interfaces
            .iter()
            .filter_map(|recorded| {
                match self.resolve_indexed_type_name_with_lexical_owners(recorded, &lexical_owners)
                {
                    TypeNameResolution::Exact(resolved) => Some(resolved),
                    TypeNameResolution::Missing | TypeNameResolution::Ambiguous(_) => {
                        all_bare_resolved = false;
                        None
                    }
                }
            })
            .collect::<Vec<_>>();
        if self.language == Language::Kotlin
            && symbol.kind != SymbolKind::INTERFACE
            && parent.is_none()
        {
            let classes = bare
                .iter()
                .filter(|resolved| {
                    self.get_symbol(resolved).is_some_and(|target| {
                        target.kind != SymbolKind::INTERFACE
                            && is_type_declaration_kind(target.kind)
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if classes.len() == 1 {
                parent = classes.first().cloned();
                bare.retain(|resolved| Some(resolved) != parent.as_ref());
            }
        }
        let all_bare_interfaces = all_bare_resolved
            && bare.iter().all(|resolved| {
                self.get_symbol(resolved)
                    .is_some_and(|target| target.kind == SymbolKind::INTERFACE)
            });
        if parent.is_none() {
            let implicit_root = crate::language::implicit_jvm_supertype(
                Some(self.language),
                symbol.kind,
                &symbol.fqn,
                (!parent_was_reclassified)
                    .then_some(symbol.parent_class.as_deref())
                    .flatten(),
                all_bare_interfaces,
            );
            if let Some(root) = implicit_root.filter(|root| self.has_class(root)) {
                parent = Some(root.to_owned());
            }
        }
        interfaces.append(&mut bare);
        interfaces.sort_unstable();
        interfaces.dedup();
        (parent, interfaces)
    }

    /// Resolve one overlay inheritance name without guessing among indexed
    /// candidates. This mirrors the publication resolver's namespace tiers,
    /// but uses `get_symbol`/`has_class` so declarations in the unpublished
    /// revision participate and declarations replaced by it stay masked.
    pub(crate) fn resolve_indexed_type_name(&self, written: &str) -> TypeNameResolution {
        self.resolve_indexed_type_name_with_lexical_owners(written, &[])
    }

    /// Resolve one source type spelling to a usable identity without package
    /// or wildcard guesses. Explicit single-type imports remain identities
    /// while their dependency is still being indexed; on-demand and current-
    /// package candidates require declaration evidence.
    pub fn resolve_type_identity(&self, written: &str) -> TypeNameResolution {
        self.resolve_type_identity_in_owner(written, None)
    }

    /// Resolve a type spelling at a concrete lexical owner.
    ///
    /// Usage extraction previously resolved parameter and local types with an
    /// import-only context. That made a nested `Owner.State` look like a
    /// same-package `State`, which in turn corrupted every receiver-dependent
    /// usage derived from the variable. Keep lexical ownership in the same
    /// ambiguity-aware resolver used for declaration signatures.
    pub(crate) fn resolve_type_identity_in_owner(
        &self,
        written: &str,
        owner_fqn: Option<&str>,
    ) -> TypeNameResolution {
        let mut lexical_owners = Vec::new();
        let mut owner = owner_fqn.map(str::to_owned);
        while let Some(owner_fqn) = owner {
            let Some(owner_symbol) = self.get_symbol(&owner_fqn) else {
                break;
            };
            owner = owner_symbol.container.as_ref().map(ToString::to_string);
            if is_type_declaration_kind(owner_symbol.kind) {
                lexical_owners.push(owner_fqn);
            }
        }

        let resolved = self.resolve_indexed_type_name_with_lexical_owners(written, &lexical_owners);
        if resolved != TypeNameResolution::Missing || written.contains('.') {
            return resolved;
        }
        if let Some(explicit) = self.imports.explicit.get(written) {
            return TypeNameResolution::Exact(explicit.clone());
        }
        crate::implicit_single_type_import(Some(self.language), written)
            .map_or(TypeNameResolution::Missing, |implicit| {
                TypeNameResolution::Exact(implicit.to_owned())
            })
    }

    /// Resolve one unqualified non-type declaration from source import scope.
    /// Explicit imports are definitive, the current package precedes wildcard
    /// imports, and multiple on-demand owners remain ambiguous. This is used
    /// by Kotlin/Groovy top-level functions and properties, whose names do not
    /// belong in the nominal type resolver.
    pub(crate) fn resolve_unqualified_symbol(
        &self,
        name: &str,
        namespace: DeclarationNamespace,
    ) -> Option<Arc<Symbol>> {
        let lookup = |fqn: &str| self.get_symbol_target(&crate::SymbolTarget::new(fqn, namespace));
        if let Some(explicit) = self.imports.explicit.get(name) {
            return lookup(explicit);
        }

        let current = self
            .imports
            .current_package
            .as_ref()
            .map_or_else(|| name.to_owned(), |package| format!("{package}.{name}"));
        if let Some(symbol) = lookup(&current) {
            return Some(symbol);
        }

        let mut wildcard = self
            .imports
            .wildcards
            .iter()
            .filter_map(|package| lookup(&format!("{package}.{name}")))
            .collect::<Vec<_>>();
        wildcard.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        wildcard.dedup_by(|left, right| left.fqn == right.fqn);
        let [symbol] = wildcard.as_slice() else {
            return None;
        };
        Some(Arc::clone(symbol))
    }

    /// Resolve one written type name against this resolver's view of the
    /// workspace (published index plus any unpublished revision overlay).
    ///
    /// The tier walk lives in [`crate::type_name_resolution`] and is shared
    /// with the index; only the workspace view differs.
    fn resolve_indexed_type_name_with_lexical_owners(
        &self,
        written: &str,
        lexical_owners: &[String],
    ) -> TypeNameResolution {
        crate::type_name_resolution::resolve_written_type_name(
            self,
            self.imports,
            lexical_owners,
            written,
            Some(self.language),
        )
    }

    /// Get the set of java.lang class names, initializing the cache on first access
    ///
    /// The indexed half is memoized on the workspace index, not here: this
    /// resolver is constructed once per name resolution, so the `OnceLock`
    /// below only ever amortizes within a single resolution. Before that
    /// split, every resolution rebuilt and sorted a `Vec<String>` of the whole
    /// `java.lang` package and logged a warning when it was empty — 1219 of
    /// 1313 log lines on a 157-file project were that one warning.
    fn get_java_lang_classes(&self) -> &HashSet<String> {
        self.java_lang_cache.get_or_init(|| {
            let indexed = self.workspace.java_lang_class_names();
            if indexed.is_empty() {
                debug!("No java.lang classes found in workspace index, using minimal fallback");
            } else {
                debug!(
                    "Built java.lang cache with {} indexed classes plus the core fallback",
                    indexed.len()
                );
            }
            let mut classes = Self::minimal_java_lang_fallback();
            classes.extend(indexed.iter().cloned());
            classes
        })
    }

    /// Fallback set of `java.lang` classes for a workspace with no indexed JDK.
    ///
    /// The table itself lives in `hyperion_types` so the lookup-less
    /// resolution context and this resolver cannot drift apart again.
    /// The earlier fallback had the same defect.
    fn minimal_java_lang_fallback() -> HashSet<String> {
        hyperion_types::CORE_JAVA_LANG_TYPES
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    }

    /// Resolve a simple name to every fully qualified name it could denote,
    /// most likely first.
    ///
    /// This is the *guessing* entry point: unlike
    /// [`Self::resolve_type_identity`] it also emits candidates whose
    /// declaration is not indexed, because navigation and hover still need
    /// something to probe while a dependency is cold. Use
    /// `resolve_type_identity` whenever an answer must be an identity.
    ///
    /// Ordering has two levels:
    /// 1. JLS tier order — explicit import, own package, `java.lang`,
    ///    on-demand imports.
    /// 2. Within that, candidates whose type actually exists come before
    ///    guesses. A guess used to outrank an existing type purely because
    ///    its tier is stronger, so `import lib.*;` with an indexed
    ///    `lib.Helper` and no `app.Helper` handed callers `app.Helper` first,
    ///    and a fully qualified `java.util.List` handed them
    ///    `app.java.util.List`.
    pub fn resolve(&self, simple_name: &str) -> Vec<String> {
        // Trim whitespace - AST text extraction sometimes includes trailing spaces
        let simple_name = simple_name.trim();
        let mut candidates = Vec::new();

        // Handle qualified inner class references (e.g., "Outer.Inner")
        if simple_name.contains('.') {
            // Split into parts: Outer.Inner or com.example.Outer.Inner
            let parts: Vec<&str> = simple_name.split('.').collect();

            // Check if first part is an explicit import
            if let Some(outer_fqn) = self.imports.explicit.get(parts[0]) {
                // e.g., if "Outer" imports to "com.example.Outer",
                // then "Outer.Inner" resolves to "com.example.Outer.Inner"
                let inner_parts = &parts[1..];
                candidates.push(format!("{}.{}", outer_fqn, inner_parts.join(".")));
                return candidates;
            }

            // Try with current package
            if let Some(pkg) = &self.imports.current_package {
                candidates.push(format!("{}.{}", pkg, simple_name));
            }

            // Try with wildcard imports
            for wildcard_pkg in &self.imports.wildcards {
                // Qualify the first part with the wildcard package
                let qualified = format!("{}.{}", wildcard_pkg, simple_name);
                candidates.push(qualified);
            }

            // Also try as-is in case it's already fully qualified
            candidates.push(simple_name.to_string());

            return self.existing_candidates_first(candidates);
        }

        // 1. Check explicit imports first (highest priority)
        if let Some(fqn) = self.imports.explicit.get(simple_name) {
            candidates.push(fqn.clone());
            return candidates; // Explicit import is definitive
        }

        // 2. The compilation unit's own package shadows the implicit
        // java.lang import-on-demand (JLS §6.5.5).
        candidates.push(self.imports.current_package.as_ref().map_or_else(
            || simple_name.to_string(), // Default package
            |pkg| format!("{}.{}", pkg, simple_name),
        ));

        // 3. java.lang, emitted only for names the JDK (or the core fallback
        // table) actually has, so it is existence-verified by construction.
        if self.get_java_lang_classes().contains(simple_name) {
            candidates.push(format!("java.lang.{}", simple_name));
        }

        // 4. On-demand imports. JLS treats ambiguity between two on-demand
        // imports as a compile error, so an LSP only needs a deterministic
        // order; source order is that order.
        for wildcard_pkg in &self.imports.wildcards {
            candidates.push(format!("{}.{}", wildcard_pkg, simple_name));
        }

        let candidates = self.existing_candidates_first(candidates);
        debug!(
            "Resolved '{}' to {} candidates: {:?}",
            simple_name,
            candidates.len(),
            candidates
        );

        candidates
    }

    /// The single FQN a spelling can denote while its dependency is cold.
    ///
    /// A name with no indexed target is not necessarily wrong: the package it
    /// names may simply not be indexed yet. When import scope leaves exactly
    /// one possible reading and nothing at all from that reading's package is
    /// indexed, there is no evidence either way and reporting the name would
    /// cry wolf. Two or more readings are a genuine ambiguity, and
    /// an indexed package is evidence the spelling is wrong; both return
    /// `None` so the caller reports.
    ///
    /// This is a deliberate, named degradation of
    /// [`Self::resolve_type_identity`], not a second resolution order: it
    /// consumes [`Self::resolve`]'s candidates rather than re-deriving them.
    pub fn sole_reading_while_dependency_is_cold(&self, written: &str) -> Option<String> {
        let candidates = self.resolve(written);
        let [only] = candidates.as_slice() else {
            return None;
        };
        let (package, _) = only.rsplit_once('.')?;
        (!self.workspace.package_has_indexed_classes(package)).then(|| only.clone())
    }

    /// Stably move candidates with a known type declaration ahead of guesses.
    ///
    /// The relative order inside each group is the JLS tier order the caller
    /// built, so this only demotes candidates the workspace can prove absent.
    fn existing_candidates_first(&self, candidates: Vec<String>) -> Vec<String> {
        let (existing, guesses): (Vec<String>, Vec<String>) = candidates
            .into_iter()
            .partition(|candidate| self.has_class_or_core_language_type(candidate));
        existing.into_iter().chain(guesses).collect()
    }

    fn explicit_static_import_bindings(&self, imported_name: &str) -> Vec<(&str, &str)> {
        let mut bindings = Vec::new();
        for binding in &self.imports.static_imports {
            if binding.imported_name() != imported_name {
                continue;
            }
            let Some((owner, member)) = binding.fqn.rsplit_once('.') else {
                continue;
            };
            if !bindings.contains(&(owner, member)) {
                bindings.push((owner, member));
            }
        }
        bindings
    }

    fn on_demand_static_import_owners(&self) -> Vec<&str> {
        let mut owners = Vec::new();
        for owner in &self.imports.static_wildcards {
            if !owners.contains(&owner.as_str()) {
                owners.push(owner);
            }
        }
        owners
    }

    fn type_package(&self, type_fqn: &str) -> String {
        let mut top_level = type_fqn.to_owned();
        while let Some(container) = self
            .get_symbol(&top_level)
            .and_then(|symbol| symbol.container.clone())
        {
            top_level = container.to_string();
        }
        top_level
            .rsplit_once('.')
            .map_or("", |(package, _)| package)
            .to_owned()
    }

    fn declared_in_interface(&self, symbol: &Symbol) -> bool {
        symbol.container.as_deref().is_some_and(|owner| {
            self.get_symbol(owner)
                .is_some_and(|owner| owner.kind == SymbolKind::INTERFACE)
        })
    }

    fn static_import_may_access(&self, symbol: &Symbol) -> bool {
        if symbol.modifiers.is_private {
            return false;
        }
        if self.declared_in_interface(symbol)
            || symbol.modifiers.is_public
            || symbol.modifiers.is_internal
        {
            return true;
        }
        let Some(owner) = symbol.container.as_deref() else {
            return false;
        };
        self.type_package(owner) == self.imports.current_package.as_deref().unwrap_or("")
    }

    pub(crate) fn static_import_owner_is_accessible(&self, owner: &str) -> bool {
        let current_package = self.imports.current_package.as_deref().unwrap_or("");
        let owner_package = self.type_package(owner);
        let mut current = Some(owner.to_owned());
        while let Some(type_fqn) = current.take() {
            let Some(symbol) = self.get_symbol(&type_fqn) else {
                return false;
            };
            if symbol.modifiers.is_private {
                return false;
            }
            let declared_in_interface = self.declared_in_interface(&symbol);
            if !declared_in_interface
                && !symbol.modifiers.is_public
                && !symbol.modifiers.is_internal
                && owner_package != current_package
            {
                return false;
            }
            current = symbol.container.as_deref().and_then(|container| {
                self.get_symbol(container)
                    .filter(|container| is_type_declaration_kind(container.kind))
                    .map(|_| container.to_owned())
            });
        }
        true
    }

    pub(crate) fn member_is_inherited_by(&self, imported_owner: &str, member: &Symbol) -> bool {
        let Some(declared_owner) = member.container.as_deref() else {
            return false;
        };
        if declared_owner == imported_owner {
            return true;
        }
        if member.modifiers.is_private {
            return false;
        }
        if member.modifiers.is_public
            || member.modifiers.is_protected
            || member.modifiers.is_internal
            || self.declared_in_interface(member)
        {
            return true;
        }

        // A package-private declaration is inherited only along a path which
        // never leaves its package. Re-entering the package later does not
        // resurrect the member (JLS 8.2/8.4.8).
        let declaring_package = self.type_package(declared_owner);
        let mut pending = VecDeque::from([imported_owner.to_owned()]);
        let mut visited = HashSet::new();
        while let Some(owner) = pending.pop_front() {
            if !visited.insert(owner.clone()) || self.type_package(&owner) != declaring_package {
                continue;
            }
            if owner == declared_owner {
                return true;
            }
            let Some(symbol) = self.get_symbol(&owner) else {
                continue;
            };
            let (parent, interfaces) = self.resolved_supertype_roles(&symbol);
            pending.extend(parent);
            pending.extend(interfaces);
        }
        false
    }

    /// Whether the owner's effective member set contains a value with this name.
    pub fn has_value_member_named(&self, owner: &str, simple_name: &str) -> bool {
        self.effective_value_members(owner, simple_name).0
    }

    /// Resolve a bare or receiver-qualified field/property to the declaration
    /// selected by Java member lookup. A declaration on the nearest class
    /// hides the superclass tier even when access checks make the resulting
    /// expression invalid; unrelated interface declarations remain ambiguous.
    pub(crate) fn resolve_value_member(
        &self,
        owner: &str,
        simple_name: &str,
    ) -> Option<Arc<Symbol>> {
        let (_, members) = self.effective_value_members(owner, simple_name);
        (members.len() == 1).then(|| Arc::clone(&members[0]))
    }

    /// Project a member's declared type into the receiver's generic context.
    /// Missing or conflicting hierarchy metadata remains a fail-closed `None`.
    pub(crate) fn project_member_type(
        &self,
        receiver_fqn: &str,
        member: &Symbol,
    ) -> Option<String> {
        self.project_member_type_with_arguments(receiver_fqn, &[], member)
    }

    pub(crate) fn project_member_type_with_arguments(
        &self,
        receiver_fqn: &str,
        receiver_type_arguments: &[String],
        member: &Symbol,
    ) -> Option<String> {
        let owner = member.container.as_deref()?;
        let written = member.return_type.as_deref()?;
        self.workspace.project_member_type(
            receiver_fqn,
            receiver_type_arguments,
            owner,
            written,
            &member.type_parameters,
        )
    }

    fn effective_value_members(&self, owner: &str, simple_name: &str) -> (bool, Vec<Arc<Symbol>>) {
        let chain = self.get_inheritance_chain(owner);
        for class in chain.iter().filter(|candidate| {
            self.get_symbol(candidate)
                .is_some_and(|symbol| symbol.kind != SymbolKind::INTERFACE)
        }) {
            let declared =
                self.direct_members_by_name(class, simple_name, DeclarationNamespace::Value);
            if !declared.is_empty() {
                let inherited = declared
                    .into_iter()
                    .filter(|member| self.member_is_inherited_by(owner, member))
                    .collect();
                return (true, inherited);
            }
        }

        let mut declared = chain
            .iter()
            .filter(|candidate| {
                self.get_symbol(candidate)
                    .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE)
            })
            .flat_map(|interface| {
                self.direct_members_by_name(interface, simple_name, DeclarationNamespace::Value)
            })
            .collect::<Vec<_>>();
        let tier_present = !declared.is_empty();
        let declared_owners = declared
            .iter()
            .filter_map(|symbol| symbol.container.clone())
            .collect::<Vec<_>>();
        declared.retain(|candidate| {
            let Some(candidate_owner) = candidate.container.as_deref() else {
                return false;
            };
            self.member_is_inherited_by(owner, candidate)
                && !declared_owners.iter().any(|other_owner| {
                    let other_owner = other_owner.as_ref();
                    other_owner != candidate_owner
                        && self
                            .get_inheritance_chain(other_owner)
                            .iter()
                            .skip(1)
                            .any(|ancestor| ancestor == candidate_owner)
                })
        });
        declared.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        declared.dedup_by(|left, right| left.fqn == right.fqn);
        (tier_present, declared)
    }

    /// Whether the owner's effective member set contains a callable with this name.
    pub fn has_callable_member_named(&self, owner: &str, simple_name: &str) -> bool {
        !self
            .callable_members_from_owner(owner, simple_name)
            .is_empty()
    }

    fn callable_members_from_owner(&self, owner: &str, simple_name: &str) -> Vec<Arc<Symbol>> {
        let mut members = self
            .get_inheritance_chain(owner)
            .into_iter()
            .flat_map(|candidate_owner| {
                let inherited_interface = candidate_owner != owner
                    && self
                        .get_symbol(&candidate_owner)
                        .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE);
                self.direct_members_by_name(
                    &candidate_owner,
                    simple_name,
                    DeclarationNamespace::Callable,
                )
                .into_iter()
                .filter(move |method| {
                    !(inherited_interface && method.modifiers.is_static)
                        && self.member_is_inherited_by(owner, method)
                })
            })
            .collect::<Vec<_>>();
        members.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        members.dedup_by(|left, right| left.fqn == right.fqn);
        members
    }

    /// Whether Java's enclosing-type member search has at least one method
    /// which may accept these arguments. A proven absence permits the later
    /// static-import tier; ambiguity or incomplete reference-type facts retain
    /// the member tier and therefore fail closed.
    pub fn has_potentially_applicable_callable_member(
        &self,
        owner: &str,
        simple_name: &str,
        argument_types: &[crate::InvocationArgumentType],
    ) -> bool {
        self.callable_member_applicability(owner, simple_name, argument_types)
            != crate::usage_resolution::PotentialApplicability::No
    }

    pub(crate) fn callable_member_applicability(
        &self,
        owner: &str,
        simple_name: &str,
        argument_types: &[crate::InvocationArgumentType],
    ) -> crate::usage_resolution::PotentialApplicability {
        use crate::usage_resolution::PotentialApplicability;

        let mut result = PotentialApplicability::No;
        for method in self.callable_members_from_owner(owner, simple_name) {
            match crate::usage_resolution::method_potential_applicability_with(
                &method,
                argument_types,
                |from, to| self.reference_assignability(from, to),
            ) {
                PotentialApplicability::Yes => return PotentialApplicability::Yes,
                PotentialApplicability::Unknown => result = PotentialApplicability::Unknown,
                PotentialApplicability::No => {}
            }
        }
        result
    }

    pub(crate) fn callable_member_applicability_with_descriptor(
        &self,
        owner: &str,
        simple_name: &str,
        argument_descriptor: &str,
    ) -> crate::usage_resolution::PotentialApplicability {
        self.callable_member_applicability(
            owner,
            simple_name,
            &crate::usage_resolution::descriptor_types(argument_descriptor),
        )
    }

    fn reference_assignability(&self, from: &str, to: &str) -> Option<bool> {
        fn erased(raw: &str) -> &str {
            raw.trim()
                .split('<')
                .next()
                .unwrap_or(raw)
                .trim_end_matches("[]")
        }

        let from = erased(from);
        let to = erased(to);
        let resolve = |written: &str| match self.resolve_indexed_type_name(written) {
            TypeNameResolution::Exact(fqn) => Some(fqn),
            TypeNameResolution::Missing | TypeNameResolution::Ambiguous(_) => None,
        };
        let from = resolve(from)?;
        let to = resolve(to)?;
        if from == to || to == "java.lang.Object" {
            return Some(true);
        }
        if self
            .get_inheritance_chain(&from)
            .iter()
            .any(|ancestor| ancestor == &to)
        {
            return Some(true);
        }

        let from_symbol = self.get_symbol(&from)?;
        let to_symbol = self.get_symbol(&to)?;
        // The complete resolved chain above did not contain the target. A
        // final source type cannot acquire another superclass or interface,
        // so this is a proof of incompatibility even when the implicit Object
        // declaration itself is absent from the compact index.
        if from_symbol.modifiers.is_final {
            return Some(false);
        }
        if to_symbol.kind == SymbolKind::INTERFACE {
            return None;
        }

        // A class has exactly one superclass chain. If its declared parent is
        // unresolved, the compact index cannot prove that the target is absent;
        // otherwise walking to the implicit Object root is a complete proof.
        let mut current = from;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return None;
            }
            let symbol = self.get_symbol(&current)?;
            let (parent, _) = self.resolved_supertype_roles(&symbol);
            let Some(parent) = parent else {
                if symbol.parent_class.is_some() && current != "java.lang.Object" {
                    return None;
                }
                return Some(false);
            };
            if parent == to {
                return Some(true);
            }
            current = parent;
        }
    }

    fn value_is_effectively_static(&self, symbol: &Symbol) -> bool {
        symbol.modifiers.is_static
            || symbol.kind == SymbolKind::ENUM_MEMBER
            || self.declared_in_interface(symbol)
    }

    fn type_is_effectively_static(&self, symbol: &Symbol) -> bool {
        symbol.modifiers.is_static
            || self.declared_in_interface(symbol)
            || matches!(
                symbol.kind,
                SymbolKind::INTERFACE | SymbolKind::ENUM | SymbolKind::STRUCT
            )
    }

    fn static_values_from_owner(&self, owner: &str, simple_name: &str) -> Vec<Arc<Symbol>> {
        if !self.static_import_owner_is_accessible(owner) {
            return Vec::new();
        }
        let chain = self.get_inheritance_chain(owner);

        // A declaration in the nearest class hides every same-name field in
        // its superclass chain, even when that declaration is not importable.
        for class in chain.iter().filter(|candidate| {
            self.get_symbol(candidate)
                .is_some_and(|symbol| symbol.kind != SymbolKind::INTERFACE)
        }) {
            let declared =
                self.direct_members_by_name(class, simple_name, DeclarationNamespace::Value);
            if !declared.is_empty() {
                return declared
                    .into_iter()
                    .filter(|symbol| {
                        self.value_is_effectively_static(symbol)
                            && self.static_import_may_access(symbol)
                            && self.member_is_inherited_by(owner, symbol)
                    })
                    .collect();
            }
        }

        // Unrelated interface branches remain ambiguous. A declaration in a
        // subinterface hides the same name in its parent interface.
        let mut declared = chain
            .iter()
            .filter(|candidate| {
                self.get_symbol(candidate)
                    .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE)
            })
            .flat_map(|interface| {
                self.direct_members_by_name(interface, simple_name, DeclarationNamespace::Value)
            })
            .collect::<Vec<_>>();
        let declared_owners = declared
            .iter()
            .filter_map(|symbol| symbol.container.clone())
            .collect::<Vec<_>>();
        declared.retain(|candidate| {
            let Some(candidate_owner) = candidate.container.as_deref() else {
                return false;
            };
            !declared_owners.iter().any(|other_owner| {
                let other_owner = other_owner.as_ref();
                other_owner != candidate_owner
                    && self
                        .get_inheritance_chain(other_owner)
                        .iter()
                        .skip(1)
                        .any(|ancestor| ancestor == candidate_owner)
            })
        });
        declared
            .into_iter()
            .filter(|symbol| {
                self.value_is_effectively_static(symbol)
                    && self.static_import_may_access(symbol)
                    && self.member_is_inherited_by(owner, symbol)
            })
            .collect()
    }

    /// Return every field/constant binding contributed by the applicable
    /// static-import tier. More than one result is semantic ambiguity; callers
    /// must not elect one by source or index order.
    pub fn resolve_static_values(&self, simple_name: &str) -> Vec<Arc<Symbol>> {
        let mut candidates = self
            .explicit_static_import_bindings(simple_name)
            .into_iter()
            .flat_map(|(owner, member)| self.static_values_from_owner(owner, member))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates.extend(
                self.on_demand_static_import_owners()
                    .into_iter()
                    .flat_map(|owner| self.static_values_from_owner(owner, simple_name)),
            );
        }
        candidates.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        candidates.dedup_by(|left, right| left.fqn == right.fqn);
        candidates
    }

    /// Return the complete callable overload set contributed by static
    /// imports. Applicability is intentionally deferred until argument types
    /// are available; collapsing this to one descriptorless FQN loses valid
    /// overloads and makes wildcard-owner conflicts order-dependent.
    pub fn resolve_static_callables(&self, simple_name: &str) -> Vec<Arc<Symbol>> {
        fn erased_parameter_type(parameter: &str) -> String {
            let parameter = parameter
                .trim()
                .strip_prefix("vararg ")
                .unwrap_or(parameter);
            let parameter = parameter
                .rsplit_once(char::is_whitespace)
                .map_or(parameter, |(ty, _)| ty);
            let mut erased = String::with_capacity(parameter.len());
            let mut generic_depth = 0_u32;
            for character in parameter.chars() {
                match character {
                    '<' => generic_depth = generic_depth.saturating_add(1),
                    '>' => generic_depth = generic_depth.saturating_sub(1),
                    _ if generic_depth == 0 => erased.push(character),
                    _ => {}
                }
            }
            if erased.ends_with("...") {
                erased.truncate(erased.len() - 3);
                erased.push_str("[]");
            }
            erased
        }

        fn signature(method: &Symbol) -> Vec<String> {
            method
                .parameters
                .iter()
                .map(|parameter| erased_parameter_type(parameter))
                .collect()
        }

        let collect = |bindings: Vec<(&str, &str)>| {
            let mut candidates = Vec::new();
            for (imported_owner, declared_name) in bindings {
                if !self.static_import_owner_is_accessible(imported_owner) {
                    continue;
                }
                let chain = self.get_inheritance_chain(imported_owner);
                let mut hidden_signatures: HashSet<Vec<String>> = HashSet::new();
                for owner in chain {
                    let is_inherited_interface = owner != imported_owner
                        && self
                            .get_symbol(&owner)
                            .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE);
                    for method in self.direct_members_by_name(
                        &owner,
                        declared_name,
                        DeclarationNamespace::Callable,
                    ) {
                        let signature = signature(&method);
                        if is_inherited_interface {
                            // Static interface methods are never inherited.
                            continue;
                        }
                        if !hidden_signatures.insert(signature) {
                            continue;
                        }
                        if method.modifiers.is_static
                            && self.static_import_may_access(&method)
                            && self.member_is_inherited_by(imported_owner, &method)
                        {
                            candidates.push(method);
                        }
                    }
                }
            }
            candidates
        };

        let mut explicit = collect(self.explicit_static_import_bindings(simple_name));
        let explicit_signatures = explicit
            .iter()
            .map(|method| signature(method))
            .collect::<HashSet<_>>();
        let mut candidates = collect(
            self.on_demand_static_import_owners()
                .into_iter()
                .map(|owner| (owner, simple_name))
                .collect(),
        );
        candidates.retain(|method| !explicit_signatures.contains(&signature(method)));
        candidates.append(&mut explicit);
        candidates.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        candidates.dedup_by(|left, right| left.fqn == right.fqn);
        candidates
    }

    /// Select one applicable statically imported overload from actual call-site
    /// argument facts. Unknown or incomparable overloads deliberately remain
    /// unresolved.
    pub fn resolve_static_callable_invocation(
        &self,
        simple_name: &str,
        argument_types: &[crate::InvocationArgumentType],
    ) -> Option<Arc<Symbol>> {
        crate::usage_resolution::resolve_static_method_invocation(simple_name, argument_types, self)
    }

    fn static_type_bindings_from_owner(&self, owner: &str, simple_name: &str) -> Vec<String> {
        fn collect_branch(
            resolver: &ImportResolver<'_>,
            imported_owner: &str,
            owner: &str,
            simple_name: &str,
            visited: &mut HashSet<String>,
            candidates: &mut Vec<String>,
        ) {
            if !visited.insert(owner.to_owned()) {
                return;
            }
            let direct =
                resolver.direct_members_by_name(owner, simple_name, DeclarationNamespace::Type);
            if !direct.is_empty() {
                candidates.extend(direct.into_iter().filter_map(|symbol| {
                    (resolver.type_is_effectively_static(&symbol)
                        && resolver.static_import_may_access(&symbol)
                        && resolver.member_is_inherited_by(imported_owner, &symbol))
                    .then(|| symbol.fqn.to_string())
                }));
                return;
            }
            let Some(symbol) = resolver.get_symbol(owner) else {
                return;
            };
            let (parent, interfaces) = resolver.resolved_supertype_roles(&symbol);
            for ancestor in parent.into_iter().chain(interfaces) {
                collect_branch(
                    resolver,
                    imported_owner,
                    &ancestor,
                    simple_name,
                    &mut visited.clone(),
                    candidates,
                );
            }
        }

        if !self.static_import_owner_is_accessible(owner) {
            return Vec::new();
        }
        let mut candidates = Vec::new();
        collect_branch(
            self,
            owner,
            owner,
            simple_name,
            &mut HashSet::new(),
            &mut candidates,
        );
        candidates.sort_unstable();
        candidates.dedup();
        candidates
    }

    pub fn resolve_static_types(&self, simple_name: &str) -> Vec<Arc<Symbol>> {
        let mut candidates = self
            .explicit_static_import_bindings(simple_name)
            .into_iter()
            .flat_map(|(owner, member)| self.static_type_bindings_from_owner(owner, member))
            .filter_map(|fqn| {
                self.get_symbol_target(&crate::SymbolTarget::new(fqn, DeclarationNamespace::Type))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates.extend(
                self.on_demand_static_import_owners()
                    .into_iter()
                    .flat_map(|owner| self.static_type_bindings_from_owner(owner, simple_name))
                    .filter_map(|fqn| {
                        self.get_symbol_target(&crate::SymbolTarget::new(
                            fqn,
                            DeclarationNamespace::Type,
                        ))
                    }),
            );
        }
        candidates.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        candidates.dedup_by(|left, right| left.fqn == right.fqn);
        candidates
    }

    /// Legacy namespace-erased facade. New semantic callers must use one of
    /// the exact namespace APIs above. This remains conservative when two
    /// namespaces or overload declarations share a spelling.
    pub fn resolve_static(&self, simple_name: &str) -> Vec<String> {
        let mut candidates = self
            .resolve_static_values(simple_name)
            .into_iter()
            .map(|symbol| (DeclarationNamespace::Value, symbol.fqn.to_string()))
            .chain(
                self.resolve_static_callables(simple_name)
                    .into_iter()
                    .map(|symbol| (DeclarationNamespace::Callable, symbol.fqn.to_string())),
            )
            .chain(
                self.resolve_static_types(simple_name)
                    .into_iter()
                    .map(|symbol| (DeclarationNamespace::Type, symbol.fqn.to_string())),
            )
            .collect::<Vec<_>>();
        candidates.sort_unstable_by(|left, right| {
            (left.0 as u8, left.1.as_str()).cmp(&(right.0 as u8, right.1.as_str()))
        });
        candidates.dedup();
        if candidates.len() == 1 {
            let (namespace, mut fqn) = candidates.pop().expect("length checked");
            if namespace == DeclarationNamespace::Callable {
                fqn.truncate(fqn.find('(').unwrap_or(fqn.len()));
            }
            return vec![fqn];
        }
        Vec::new()
    }

    /// Check if a fully qualified name matches a simple name import
    pub fn is_imported(&self, fqn: &str, simple_name: &str) -> bool {
        // Check explicit import
        if let Some(imported_fqn) = self.imports.explicit.get(simple_name) {
            return imported_fqn == fqn;
        }

        // Check if it's from java.lang
        if fqn == format!("java.lang.{}", simple_name) {
            return true;
        }

        // Check if it's from current package
        if let Some(pkg) = &self.imports.current_package {
            if fqn == format!("{}.{}", pkg, simple_name) {
                return true;
            }
        }

        // Check wildcard imports
        for wildcard_pkg in &self.imports.wildcards {
            if fqn == format!("{}.{}", wildcard_pkg, simple_name) {
                return true;
            }
        }

        false
    }
}

/// A resolver's view adds the unpublished revision overlay and the core
/// `java.lang` bindings that hold before the JDK finishes indexing.
impl crate::type_name_resolution::TypeNameScope for ImportResolver<'_> {
    fn indexed_type(&self, fqn: &str) -> bool {
        self.has_class(fqn)
    }

    fn bound_type(&self, fqn: &str) -> bool {
        self.has_class_or_core_language_type(fqn)
    }

    // A source file's facade carries its typealiases, so an unpublished
    // revision of that file answers with its own facade, and the published
    // facade it replaces is hidden like any other replaced declaration.
    fn kotlin_type_alias_expansion(
        &self,
        alias_fqn: &str,
    ) -> Option<crate::index::KotlinTypeAliasExpansion> {
        let Some(revision) = self.revision else {
            return match self.workspace {
                ResolverWorkspace::Shared(index) => index.kotlin_type_alias_expansion(alias_fqn),
                ResolverWorkspace::Coherent(inner) => inner
                    .expand_kotlin_type_alias(alias_fqn, |owner| inner.symbols.get(owner).cloned()),
            };
        };
        let published = match self.workspace {
            ResolverWorkspace::Shared(index) => index.kotlin_type_alias_facade_fqns(alias_fqn),
            ResolverWorkspace::Coherent(inner) => {
                inner.kotlin_type_alias_facade_fqns(alias_fqn).to_vec()
            }
        };
        crate::index::kotlin_type_alias_expansion_from(
            alias_fqn,
            published
                .iter()
                .filter_map(|owner| self.get_symbol(owner))
                .chain(revision.kotlin_type_alias_facades(alias_fqn)),
        )
    }

    fn inherited_member_types(&self, owner: &str, simple_name: &str) -> Vec<String> {
        // Revision overlays do not own a second inheritance index. Reuse their
        // cycle-safe canonical chain and retain every inherited member-type
        // candidate; ambiguity is safer than selecting one by traversal order.
        // The active workspace resolver hides precisely once published.
        self.get_inheritance_chain(owner)
            .into_iter()
            .skip(1)
            .map(|ancestor| format!("{ancestor}.{simple_name}"))
            .filter(|candidate| self.has_class(candidate))
            .collect()
    }

    fn static_member_types(
        &self,
        owner: &str,
        simple_name: &str,
        _current_package: Option<&str>,
    ) -> Vec<String> {
        self.static_type_bindings_from_owner(owner, simple_name)
    }
}

impl hyperion_types::SymbolLookup for ImportResolver<'_> {
    fn symbol_exists(&self, fqn: &str) -> bool {
        self.get_symbol(fqn).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio;

    fn make_imports() -> ImportMap {
        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("List".to_string(), "java.util.List".to_string());
        imports.add_import("Map".to_string(), "java.util.Map".to_string());
        imports.add_wildcard("com.example.utils".to_string());
        imports.add_static_import("max".to_string(), "java.lang.Math.max".to_string());
        imports
    }

    async fn make_workspace_with_java_lang() -> (TempDir, WorkspaceIndex) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        // Create a minimal java.lang.String file
        let java_lang_dir = root.join("java").join("lang");
        std::fs::create_dir_all(&java_lang_dir).unwrap();
        std::fs::write(
            java_lang_dir.join("String.java"),
            "package java.lang; public class String {}",
        )
        .unwrap();
        std::fs::write(
            java_lang_dir.join("Math.java"),
            "package java.lang; public class Math { public static int max(int a, int b) { return a; } }",
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index
            .index_file(&java_lang_dir.join("String.java"))
            .await
            .unwrap();
        index
            .index_file(&java_lang_dir.join("Math.java"))
            .await
            .unwrap();

        (temp, index)
    }

    /// The fallback set is what `resolve` consults when the JDK is not indexed,
    /// and it is the only thing standing between an ordinary declaration and a
    /// spurious "cannot find symbol". It previously held 20 names, so
    /// `Runnable r = () -> {};` and `catch (IllegalArgumentException e)` were
    /// reported as unresolved on any workspace without a JDK index.
    #[test]
    fn minimal_java_lang_fallback_covers_everyday_declarations() {
        let fallback = ImportResolver::minimal_java_lang_fallback();

        for name in [
            "Runnable",
            "Comparable",
            "Iterable",
            "CharSequence",
            "Number",
            "StringBuilder",
            "AutoCloseable",
            "Override",
            "FunctionalInterface",
            "IllegalArgumentException",
            "IllegalStateException",
            "NullPointerException",
            "UnsupportedOperationException",
            "InterruptedException",
        ] {
            assert!(
                fallback.contains(name),
                "java.lang fallback is missing {name}"
            );
        }
    }

    #[test]
    fn java_lang_fallback_yields_a_candidate_for_runnable() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let imports = ImportMap::new();
        let resolver = ImportResolver::new(&imports, &index);

        assert!(
            resolver
                .resolve("Runnable")
                .contains(&"java.lang.Runnable".to_string()),
            "Runnable must resolve to java.lang.Runnable without an indexed JDK"
        );
    }

    #[test]
    fn revision_overlay_retains_same_fqn_type_and_field_with_type_precedence() {
        let extraction = crate::symbol_extractor::SymbolExtractor::extract(
            "package collision; class A { static class B {} int B; }",
        )
        .unwrap();
        let overlay = RevisionSymbolOverlay::new(&extraction.symbols, HashSet::new());
        let candidates = &overlay.symbols["collision.A.B"];
        assert_eq!(candidates.len(), 2);
        assert!(is_type_declaration_kind(
            overlay.exact_symbol("collision.A.B").unwrap().kind
        ));
        for candidate in candidates {
            assert!(overlay.definition_matches(
                "collision.A.B",
                candidate.range.start,
                candidate.range.end
            ));
        }
    }

    #[tokio::test]
    async fn revision_overlay_does_not_resurrect_a_removed_value_declaration() {
        let (temp, index) = make_workspace_with_java_lang().await;
        let path = temp.path().join("C.java");
        std::fs::write(&path, "package p; class C { int value; }").unwrap();
        index.index_file(&path).await.unwrap();

        let file_id = index.get_file_id(&path).unwrap();
        let old_location = index.find_symbol("p.C.value").unwrap();
        let mut imports = ImportMap::new();
        imports.current_package = Some("p".into());
        assert!(
            ImportResolver::for_language(&imports, &index, Language::Java)
                .value_definition_at(
                    file_id,
                    "value",
                    old_location.range.start,
                    old_location.range.end,
                )
                .is_some()
        );

        let overlay =
            RevisionSymbolOverlay::new(&[], HashSet::from([Arc::<str>::from("p.C.value")]));
        assert!(
            ImportResolver::for_revision(&imports, &index, &overlay, Language::Java)
                .value_definition_at(
                    file_id,
                    "value",
                    old_location.range.start,
                    old_location.range.end,
                )
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_resolve_explicit_import() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        let result = resolver.resolve("List");
        assert_eq!(result, vec!["java.util.List"]);
    }

    #[tokio::test]
    async fn test_resolve_java_lang() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        let result = resolver.resolve("String");
        assert!(result.contains(&"java.lang.String".into()));
    }

    #[tokio::test]
    async fn test_resolve_current_package() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        let result = resolver.resolve("MyClass");
        // Should include current package
        assert!(result.contains(&"com.example.MyClass".into()));
    }

    #[tokio::test]
    async fn test_resolve_wildcard() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        let result = resolver.resolve("Helper");
        // Should include wildcard import
        assert!(result.contains(&"com.example.utils.Helper".into()));
    }

    #[tokio::test]
    async fn test_resolve_multiple_candidates() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        let result = resolver.resolve("Foo");
        // Should have candidates from: current package, wildcard imports
        assert!(result.len() > 1);
        assert!(result.contains(&"com.example.Foo".into()));
        assert!(result.contains(&"com.example.utils.Foo".into()));
    }

    #[tokio::test]
    async fn test_resolve_static() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        let result = resolver.resolve_static("max");
        assert_eq!(result, vec!["java.lang.Math.max"]);
    }

    #[tokio::test]
    async fn static_wildcard_resolution_preserves_ambiguity() {
        let temp = TempDir::new().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        let first = first_dir.join("First.java");
        let second = second_dir.join("Second.java");
        std::fs::write(
            &first,
            "package first; public class First { public static int value; }",
        )
        .unwrap();
        std::fs::write(
            &second,
            "package second; public class Second { public static int value; }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&first).await.unwrap();
        index.index_file(&second).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_wildcard("first.First".into());
        imports.add_static_wildcard("second.Second".into());

        assert!(ImportResolver::new(&imports, &index)
            .resolve_static("value")
            .is_empty());
    }

    #[tokio::test]
    async fn static_resolution_preserves_namespaces_and_callable_overloads() {
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("First.java");
        let second = temp.path().join("Second.java");
        std::fs::write(
            &first,
            concat!(
                "package p; public class First { ",
                "public static int same; ",
                "public static void same(int value) {} ",
                "public static void same(String value) {} }",
            ),
        )
        .unwrap();
        std::fs::write(
            &second,
            "package p; public class Second { public static void same(long value) {} }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&first).await.unwrap();
        index.index_file(&second).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("q".into());
        imports.add_static_wildcard("p.First".into());
        imports.add_static_wildcard("p.Second".into());
        let resolver = ImportResolver::new(&imports, &index);

        assert_eq!(resolver.resolve_static_values("same").len(), 1);
        assert_eq!(resolver.resolve_static_callables("same").len(), 3);
        assert!(resolver.resolve_static_types("same").is_empty());
        assert!(resolver.resolve_static("same").is_empty());
    }

    #[tokio::test]
    async fn static_member_type_resolution_honors_inheritance_and_staticness() {
        let temp = TempDir::new().unwrap();
        let base = temp.path().join("Base.java");
        let child = temp.path().join("Child.java");
        std::fs::write(
            &base,
            concat!(
                "package p; public class Base { ",
                "public static class Nested {} ",
                "public class Inner {} }",
            ),
        )
        .unwrap();
        std::fs::write(&child, "package p; public class Child extends Base {}").unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&base).await.unwrap();
        index.index_file(&child).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("q".into());
        imports.add_static_wildcard("p.Child".into());
        let resolver = ImportResolver::new(&imports, &index);

        assert_eq!(
            resolver
                .resolve_static_types("Nested")
                .iter()
                .map(|symbol| symbol.fqn.as_ref())
                .collect::<Vec<_>>(),
            ["p.Base.Nested"]
        );
        assert!(resolver.resolve_static_types("Inner").is_empty());
    }

    #[tokio::test]
    async fn static_wildcard_resolution_ignores_provably_inaccessible_members() {
        let temp = TempDir::new().unwrap();
        let hidden_dir = temp.path().join("hidden");
        let visible_dir = temp.path().join("visible");
        std::fs::create_dir_all(&hidden_dir).unwrap();
        std::fs::create_dir_all(&visible_dir).unwrap();
        let hidden = hidden_dir.join("Hidden.java");
        let visible = visible_dir.join("Visible.java");
        std::fs::write(
            &hidden,
            "package hidden; public class Hidden { static int value; }",
        )
        .unwrap();
        std::fs::write(
            &visible,
            "package visible; public class Visible { public static int value; }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&hidden).await.unwrap();
        index.index_file(&visible).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_wildcard("hidden.Hidden".into());
        imports.add_static_wildcard("visible.Visible".into());

        assert_eq!(
            ImportResolver::new(&imports, &index).resolve_static("value"),
            ["visible.Visible.value"]
        );
    }

    #[tokio::test]
    async fn explicit_static_import_shadows_on_demand_imports() {
        let temp = TempDir::new().unwrap();
        let chosen = temp.path().join("Chosen.java");
        let other = temp.path().join("Other.java");
        std::fs::write(
            &chosen,
            "package chosen; public class Chosen { public static int value; }",
        )
        .unwrap();
        std::fs::write(
            &other,
            "package other; public class Other { public static int value; }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&chosen).await.unwrap();
        index.index_file(&other).await.unwrap();
        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_import("value".into(), "chosen.Chosen.value".into());
        imports.add_static_import("renamed".into(), "chosen.Chosen.value".into());
        imports.add_static_wildcard("other.Other".into());

        assert_eq!(
            ImportResolver::new(&imports, &index).resolve_static("value"),
            ["chosen.Chosen.value"]
        );
        assert_eq!(
            ImportResolver::new(&imports, &index)
                .resolve_static_values("renamed")
                .iter()
                .map(|symbol| symbol.fqn.as_ref())
                .collect::<Vec<_>>(),
            ["chosen.Chosen.value"]
        );
    }

    #[tokio::test]
    async fn static_import_shadowing_is_namespace_and_signature_specific() {
        let temp = TempDir::new().unwrap();
        let explicit = temp.path().join("Explicit.java");
        let second = temp.path().join("Second.java");
        let wildcard = temp.path().join("Wildcard.java");
        std::fs::write(
            &explicit,
            concat!(
                "package p; public class Explicit { ",
                "public static int same; ",
                "public static void choose(int value) {} }",
            ),
        )
        .unwrap();
        std::fs::write(
            &second,
            "package p; public class Second { public static int same; }",
        )
        .unwrap();
        std::fs::write(
            &wildcard,
            concat!(
                "package p; public class Wildcard { ",
                "public static int same; ",
                "public static void choose(int value) {} ",
                "public static void choose(String value) {} }",
            ),
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&explicit).await.unwrap();
        index.index_file(&second).await.unwrap();
        index.index_file(&wildcard).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_import("same".into(), "p.Explicit.same".into());
        imports.add_static_import("same".into(), "p.Second.same".into());
        imports.add_static_import("choose".into(), "p.Explicit.choose".into());
        imports.add_static_wildcard("p.Wildcard".into());
        let resolver = ImportResolver::new(&imports, &index);

        assert_eq!(
            resolver
                .resolve_static_values("same")
                .iter()
                .map(|symbol| symbol.fqn.as_ref())
                .collect::<Vec<_>>(),
            ["p.Explicit.same", "p.Second.same"]
        );
        assert_eq!(
            resolver
                .resolve_static_callables("choose")
                .iter()
                .map(|symbol| symbol.fqn.as_ref())
                .collect::<Vec<_>>(),
            [
                "p.Explicit.choose(int)",
                "p.Wildcard.choose(java.lang.String)",
            ]
        );
    }

    #[tokio::test]
    async fn explicit_callable_does_not_shadow_on_demand_value_namespace() {
        let temp = TempDir::new().unwrap();
        let methods = temp.path().join("Methods.java");
        let values = temp.path().join("Values.java");
        std::fs::write(
            &methods,
            "package p; public class Methods { public static void same() {} }",
        )
        .unwrap();
        std::fs::write(
            &values,
            "package p; public class Values { public static int same; }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&methods).await.unwrap();
        index.index_file(&values).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_import("same".into(), "p.Methods.same".into());
        imports.add_static_wildcard("p.Values".into());
        assert_eq!(
            ImportResolver::new(&imports, &index)
                .resolve_static_values("same")
                .iter()
                .map(|symbol| symbol.fqn.as_ref())
                .collect::<Vec<_>>(),
            ["p.Values.same"]
        );
    }

    #[tokio::test]
    async fn private_interface_members_and_inaccessible_owners_are_not_imported() {
        let temp = TempDir::new().unwrap();
        let contract = temp.path().join("Contract.java");
        let outer = temp.path().join("Outer.java");
        std::fs::write(
            &contract,
            concat!(
                "package p; public interface Contract { ",
                "private static void hidden() {} ",
                "static void visible() {} }",
            ),
        )
        .unwrap();
        std::fs::write(
            &outer,
            concat!(
                "package p; public class Outer { ",
                "private static class Hidden { public static int value; } }",
            ),
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&contract).await.unwrap();
        index.index_file(&outer).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_wildcard("p.Contract".into());
        imports.add_static_wildcard("p.Outer.Hidden".into());
        let resolver = ImportResolver::new(&imports, &index);
        assert!(resolver.resolve_static_callables("hidden").is_empty());
        assert_eq!(resolver.resolve_static_callables("visible").len(), 1);
        assert!(resolver.resolve_static_values("value").is_empty());
    }

    #[tokio::test]
    async fn package_private_member_is_not_resurrected_after_hierarchy_leaves_package() {
        let temp = TempDir::new().unwrap();
        let p = temp.path().join("p");
        let q = temp.path().join("q");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::create_dir_all(&q).unwrap();
        let base = p.join("Base.java");
        let middle = p.join("Middle.java");
        let child = q.join("Child.java");
        std::fs::write(
            &base,
            "package p; public class Base { static int local; public static int visible; }",
        )
        .unwrap();
        std::fs::write(&middle, "package p; public class Middle extends Base {}").unwrap();
        std::fs::write(&child, "package q; public class Child extends p.Middle {}").unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&base).await.unwrap();
        index.index_file(&middle).await.unwrap();
        index.index_file(&child).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("p".into());
        imports.add_static_wildcard("q.Child".into());
        let resolver = ImportResolver::new(&imports, &index);
        assert!(resolver.resolve_static_values("local").is_empty());
        assert_eq!(resolver.resolve_static_values("visible").len(), 1);
    }

    #[tokio::test]
    async fn static_invocation_uses_boxing_varargs_and_fails_closed_on_ambiguity() {
        let temp = TempDir::new().unwrap();
        let first = temp.path().join("First.java");
        let second = temp.path().join("Second.java");
        let local = temp.path().join("Local.java");
        std::fs::write(
            &first,
            concat!(
                "package p; public class First { ",
                "public static void pick(int value) {} ",
                "public static void pick(String... values) {} ",
                "public static void clash(Integer value) {} }",
            ),
        )
        .unwrap();
        std::fs::write(
            &second,
            concat!(
                "package p; public class Second { ",
                "public static void pick(Integer value) {} ",
                "public static void clash(Integer value) {} }",
            ),
        )
        .unwrap();
        std::fs::write(
            &local,
            concat!(
                "package consumer; public class Local { ",
                "void pick(String value) {} void clash(Object value) {} }",
            ),
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&first).await.unwrap();
        index.index_file(&second).await.unwrap();
        index.index_file(&local).await.unwrap();

        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".into());
        imports.add_static_import("pick".into(), "p.First.pick".into());
        imports.add_static_wildcard("p.Second".into());
        imports.add_static_import("clash".into(), "p.First.clash".into());
        imports.add_static_import("clash".into(), "p.Second.clash".into());
        let resolver = ImportResolver::new(&imports, &index);

        assert_eq!(
            resolver
                .resolve_static_callable_invocation(
                    "pick",
                    &[crate::InvocationArgumentType::Known("int".into())],
                )
                .map(|symbol| symbol.fqn.to_string()),
            Some("p.First.pick(int)".into())
        );
        assert_eq!(
            resolver
                .resolve_static_callable_invocation(
                    "pick",
                    &[crate::InvocationArgumentType::Known(
                        "java.lang.Integer".into(),
                    )],
                )
                .map(|symbol| symbol.fqn.to_string()),
            Some("p.Second.pick(java.lang.Integer)".into())
        );
        assert_eq!(
            resolver
                .resolve_static_callable_invocation(
                    "pick",
                    &[
                        crate::InvocationArgumentType::Known("java.lang.String".into()),
                        crate::InvocationArgumentType::Known("java.lang.String".into()),
                    ],
                )
                .map(|symbol| symbol.fqn.to_string()),
            Some("p.First.pick(java.lang.String...)".into())
        );
        assert!(resolver
            .resolve_static_callable_invocation(
                "clash",
                &[crate::InvocationArgumentType::Known("int".into())],
            )
            .is_none());
        assert!(resolver
            .resolve_static_callable_invocation("pick", &[crate::InvocationArgumentType::Unknown],)
            .is_none());
        assert!(!resolver.has_potentially_applicable_callable_member(
            "consumer.Local",
            "pick",
            &[crate::InvocationArgumentType::Known("int".into())],
        ));
        assert!(resolver.has_potentially_applicable_callable_member(
            "consumer.Local",
            "clash",
            &[crate::InvocationArgumentType::Known(
                "java.lang.String".into(),
            )],
        ));
    }

    #[tokio::test]
    async fn test_is_imported() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = make_imports();
        let resolver = ImportResolver::new(&imports, &index);

        assert!(resolver.is_imported("java.util.List", "List"));
        assert!(resolver.is_imported("java.lang.String", "String"));
        assert!(resolver.is_imported("com.example.MyClass", "MyClass"));
        assert!(!resolver.is_imported("org.other.Foo", "Foo"));
    }

    #[tokio::test]
    async fn test_default_package() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let mut imports = ImportMap::new();
        imports.current_package = None; // Default package

        let resolver = ImportResolver::new(&imports, &index);
        let result = resolver.resolve("MyClass");

        assert!(result.contains(&"MyClass".into()));
    }

    #[tokio::test]
    async fn test_java_lang_dynamic_lookup() {
        let (_temp, index) = make_workspace_with_java_lang().await;
        let imports = ImportMap::new();
        let resolver = ImportResolver::new(&imports, &index);

        // Get the java.lang cache
        let java_lang = resolver.get_java_lang_classes();

        // Should find String since we indexed it
        assert!(java_lang.contains("String"));
    }
}
