// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Canonical reverse inheritance graph.
//!
//! A symbol already owns the written forward edges in `parent_class` and
//! `interfaces`. Source spellings deliberately remain contextual so a later
//! same-package declaration or on-demand-import collision can relink them.
//! This module stores exactly one reverse entry for every declared edge (plus
//! exact compiler-synthesized Java roots):
//!
//! ```text
//! recorded supertype spelling -> direct subtype FQNs
//! ```
//!
//! Interface implementations, ordinary subclasses, and sealed subclasses are
//! views of the same relationship. Keeping separate maps for those views made
//! them sensitive to insertion order and allowed individual indexing paths to
//! update one map without updating the others.
//!
//! Every source edge is also keyed by its first and final simple segments so
//! late arrivals/removals can be resolved canonically without scanning every
//! type. The first segment matters for contextual qualified names: adding a
//! same-package `Outer` can make a former `Outer.Inner` wildcard binding
//! disappear even though `Inner` itself did not change. Exact external
//! bytecode and implicit Java edges do not pay that auxiliary memory cost.
//!
//! A Kotlin supertype written through a typealias sits in the alias's
//! simple-name bucket, not the expanded class's. The index therefore also
//! records which aliases expand to each class, so a subtype search for that
//! class reads the aliases' buckets too. The edge is still bound by
//! type-name resolution when it is read, so an alias added or removed in
//! another file takes effect without rebuilding any edge.

use super::{IndexInner, OwnerElection};
use crate::{is_type_declaration_kind, ImportMap, Language, Symbol, TypeNameResolution};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct InheritanceIndex {
    direct_subtypes: HashMap<Arc<str>, HashSet<Arc<str>>>,
    contextual_subtypes_by_simple_name: HashMap<Arc<str>, HashSet<Arc<str>>>,
    /// JVM class a Kotlin typealias expands to -> the aliases declaring it,
    /// one entry per facade so removal stays symmetric.
    kotlin_type_aliases_by_expansion: HashMap<Arc<str>, HashSet<KotlinTypeAliasDeclaration>>,
}

/// `(alias FQN, declaring facade FQN)`.
type KotlinTypeAliasDeclaration = (Arc<str>, Arc<str>);

impl InheritanceIndex {
    pub(super) fn with_capacity(estimated_supertypes: usize) -> Self {
        Self {
            direct_subtypes: HashMap::with_capacity(estimated_supertypes),
            contextual_subtypes_by_simple_name: HashMap::with_capacity(estimated_supertypes / 8),
            kotlin_type_aliases_by_expansion: HashMap::new(),
        }
    }

    pub(super) fn reserve(&mut self, additional_supertypes: usize) {
        self.direct_subtypes.reserve(additional_supertypes);
        self.contextual_subtypes_by_simple_name
            .reserve(additional_supertypes / 8);
    }

    /// Add every direct inheritance edge declared by `symbol`.
    pub(super) fn insert(
        &mut self,
        symbol: &Symbol,
        imports: Option<&ImportMap>,
        language: Option<Language>,
    ) {
        for alias in kotlin_type_aliases(symbol) {
            self.kotlin_type_aliases_by_expansion
                .entry(kotlin_type_alias_target(alias).into())
                .or_default()
                .insert((alias.fqn.as_str().into(), Arc::clone(&symbol.fqn)));
        }

        if !is_type_declaration_kind(symbol.kind) {
            return;
        }

        let implicit = exact_implicit_supertype(symbol, language);
        if let Some(supertype) = potential_implicit_supertype(symbol, language) {
            self.direct_subtypes
                .entry(supertype.into())
                .or_default()
                .insert(Arc::clone(&symbol.fqn));
        }

        for supertype in symbol.parent_class.iter().chain(symbol.interfaces.iter()) {
            if implicit == Some(supertype.as_ref()) {
                continue;
            }
            self.direct_subtypes
                .entry(Arc::clone(supertype))
                .or_default()
                .insert(Arc::clone(&symbol.fqn));
            if imports.is_some() {
                for key in contextual_keys(supertype, imports, language) {
                    self.contextual_subtypes_by_simple_name
                        .entry(key.into())
                        .or_default()
                        .insert(Arc::clone(&symbol.fqn));
                }
            }
        }
    }

    /// Remove every direct inheritance edge declared by `symbol`.
    pub(super) fn remove(
        &mut self,
        symbol: &Symbol,
        imports: Option<&ImportMap>,
        language: Option<Language>,
    ) {
        for alias in kotlin_type_aliases(symbol) {
            let target = kotlin_type_alias_target(alias);
            let remove_key = self
                .kotlin_type_aliases_by_expansion
                .get_mut(target)
                .is_some_and(|aliases| {
                    aliases.retain(|(alias_fqn, facade)| {
                        alias_fqn.as_ref() != alias.fqn || facade != &symbol.fqn
                    });
                    aliases.is_empty()
                });
            if remove_key {
                self.kotlin_type_aliases_by_expansion.remove(target);
            }
        }

        if !is_type_declaration_kind(symbol.kind) {
            return;
        }

        let implicit = exact_implicit_supertype(symbol, language);
        if let Some(supertype) = potential_implicit_supertype(symbol, language) {
            let remove_key = self
                .direct_subtypes
                .get_mut(supertype)
                .is_some_and(|subtypes| {
                    subtypes.remove(symbol.fqn.as_ref());
                    subtypes.is_empty()
                });
            if remove_key {
                self.direct_subtypes.remove(supertype);
            }
        }

        for supertype in symbol.parent_class.iter().chain(symbol.interfaces.iter()) {
            if implicit == Some(supertype.as_ref()) {
                continue;
            }
            let remove_key = self
                .direct_subtypes
                .get_mut(supertype.as_ref())
                .is_some_and(|subtypes| {
                    subtypes.remove(symbol.fqn.as_ref());
                    subtypes.is_empty()
                });
            if remove_key {
                self.direct_subtypes.remove(supertype.as_ref());
            }

            if imports.is_some() {
                for key in contextual_keys(supertype, imports, language) {
                    let remove_simple_key = self
                        .contextual_subtypes_by_simple_name
                        .get_mut(key.as_str())
                        .is_some_and(|subtypes| {
                            subtypes.remove(symbol.fqn.as_ref());
                            subtypes.is_empty()
                        });
                    if remove_simple_key {
                        self.contextual_subtypes_by_simple_name.remove(key.as_str());
                    }
                }
            }
        }
    }

    /// Simple-name buckets whose source declarations may name `supertype_fqn`
    /// as a direct supertype: its own simple name, and the simple name of
    /// every Kotlin typealias that expands to it.
    fn candidate_simple_names<'a>(&'a self, supertype_fqn: &'a str) -> Vec<&'a str> {
        let mut names = std::iter::once(simple_name(supertype_fqn))
            .chain(
                self.kotlin_type_aliases_by_expansion
                    .get(supertype_fqn)
                    .into_iter()
                    .flatten()
                    .map(|(alias_fqn, _)| simple_name(alias_fqn)),
            )
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Declarations that may name `supertype_fqn` as a direct supertype: the
    /// exact spelling, then the contextual buckets. Callers bind each one by
    /// type-name resolution; this only bounds the search.
    fn subtype_candidates<'a>(
        &'a self,
        supertype_fqn: &'a str,
        candidate_simple_names: &'a [&'a str],
    ) -> impl Iterator<Item = &'a Arc<str>> + 'a {
        self.direct_subtypes
            .get(supertype_fqn)
            .into_iter()
            .flatten()
            .chain(
                candidate_simple_names
                    .iter()
                    .filter_map(|name| self.contextual_subtypes_by_simple_name.get(*name))
                    .flatten(),
            )
    }
}

/// FQNs of the Kotlin typealiases `symbols` declare.
///
/// An alias is not a symbol, but a class whose supertype is written through
/// its name binds differently once the alias is added, changed or removed.
/// Publication passes the old and new revisions of the declarations it
/// changed, so cache invalidation also starts from these names: the
/// conservative subtype view finds the classes written against them.
pub(super) fn declared_kotlin_type_alias_fqns<'a>(
    symbols: impl IntoIterator<Item = &'a Symbol>,
) -> Vec<Arc<str>> {
    symbols
        .into_iter()
        .flat_map(kotlin_type_aliases)
        .map(|alias| Arc::from(alias.fqn.as_str()))
        .collect()
}

fn kotlin_type_aliases(symbol: &Symbol) -> impl Iterator<Item = &crate::KotlinTypeAlias> {
    symbol
        .kotlin
        .iter()
        .flat_map(|kotlin| kotlin.type_aliases.iter())
}

/// The JVM class `alias` denotes, mapped as type-name resolution maps an
/// alias's expansion.
fn kotlin_type_alias_target(alias: &crate::KotlinTypeAlias) -> &str {
    crate::kotlin_builtin_jvm_fqn(&alias.expanded_class).unwrap_or(&alias.expanded_class)
}

fn contextual_keys(
    supertype: &str,
    imports: Option<&ImportMap>,
    language: Option<Language>,
) -> Vec<String> {
    let mut keys = vec![
        first_segment(supertype).to_owned(),
        simple_name(supertype).to_owned(),
    ];
    if let Some(imports) = imports {
        let (head, suffix) = supertype
            .split_once('.')
            .map_or((supertype, None), |(head, suffix)| (head, Some(suffix)));
        for binding in imports
            .explicit
            .get(head)
            .map(String::as_str)
            .into_iter()
            .chain(imports.static_imports_named(head))
        {
            let bound = suffix
                .map(|suffix| format!("{binding}.{suffix}"))
                .unwrap_or_else(|| binding.to_owned());
            keys.push(first_segment(&bound).to_owned());
            keys.push(simple_name(&bound).to_owned());
        }
    }
    if language == Some(Language::Kotlin) {
        for source_name in std::iter::once(supertype.to_owned()).chain(
            crate::implicit_type_import_packages(language)
                .iter()
                .flat_map(|package| {
                    [
                        format!("{package}.{}", first_segment(supertype)),
                        format!("{package}.{supertype}"),
                    ]
                }),
        ) {
            if let Some(mapped) = crate::kotlin_builtin_jvm_fqn(&source_name) {
                keys.push(simple_name(mapped).to_owned());
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Exact JVM edge synthesized by Java rather than written in source.
///
/// Annotation declarations use the parent slot as a compact kind marker; the
/// returned target still has interface role in `resolved_supertype_roles`.
pub(super) fn exact_implicit_supertype(
    symbol: &Symbol,
    language: Option<Language>,
) -> Option<&'static str> {
    crate::language::implicit_jvm_supertype(
        language,
        symbol.kind,
        &symbol.fqn,
        symbol.parent_class.as_deref(),
        false,
    )
}

/// Reverse-graph candidate for a language-mandated JVM root.
///
/// For Kotlin a bare supertype may later resolve to a class, so Object is only
/// a candidate here; `resolved_supertype_roles` validates that all bare entries
/// resolved as interfaces before publishing the edge.
pub(super) fn potential_implicit_supertype(
    symbol: &Symbol,
    language: Option<Language>,
) -> Option<&'static str> {
    crate::language::implicit_jvm_supertype(
        language,
        symbol.kind,
        &symbol.fqn,
        symbol.parent_class.as_deref(),
        true,
    )
}

impl IndexInner {
    /// FQNs of the active facades that declare the Kotlin typealias `alias_fqn`.
    pub(crate) fn kotlin_type_alias_facade_fqns(&self, alias_fqn: &str) -> &[Arc<str>] {
        self.kotlin_type_alias_owners
            .get(alias_fqn)
            .map_or(&[], |owners| owners.as_slice())
    }

    /// What the Kotlin typealias `alias_fqn` expands to, read from the
    /// declaring facades that `facade` can see.
    pub(crate) fn expand_kotlin_type_alias(
        &self,
        alias_fqn: &str,
        facade: impl Fn(&str) -> Option<Arc<Symbol>>,
    ) -> Option<KotlinTypeAliasExpansion> {
        kotlin_type_alias_expansion_from(
            alias_fqn,
            self.kotlin_type_alias_facade_fqns(alias_fqn)
                .iter()
                .filter_map(|owner| facade(owner)),
        )
    }
}

/// What a Kotlin typealias expands to: its class, the alias's own type
/// parameters, and the type arguments the expansion gives that class in
/// terms of them (see [`crate::KotlinTypeAlias`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KotlinTypeAliasExpansion {
    pub(crate) class: String,
    pub(crate) type_parameters: Vec<String>,
    pub(crate) type_arguments: Vec<String>,
}

/// What the Kotlin typealias `alias_fqn` expands to according to the facade
/// symbols `facades`. Facades that disagree about the class yield nothing
/// rather than one picked by order; facades that agree about the class but
/// not about its type parameters or arguments yield the class with neither.
pub(crate) fn kotlin_type_alias_expansion_from(
    alias_fqn: &str,
    facades: impl IntoIterator<Item = Arc<Symbol>>,
) -> Option<KotlinTypeAliasExpansion> {
    let mut expansions = facades
        .into_iter()
        .filter_map(|symbol| {
            symbol
                .kotlin
                .as_ref()?
                .type_aliases
                .iter()
                .find(|alias| alias.fqn == alias_fqn)
                .map(|alias| {
                    (
                        alias.expanded_class.clone(),
                        alias.type_parameters.clone(),
                        alias.expanded_type_arguments.clone(),
                    )
                })
        })
        .collect::<Vec<_>>();
    expansions.sort_unstable();
    expansions.dedup();
    let (class, type_parameters, type_arguments) = expansions.first()?.clone();
    if expansions.iter().any(|(other, _, _)| *other != class) {
        return None;
    }
    let (type_parameters, type_arguments) = if expansions.len() == 1 {
        (type_parameters, type_arguments)
    } else {
        (Vec::new(), Vec::new())
    };
    Some(KotlinTypeAliasExpansion {
        class,
        type_parameters,
        type_arguments,
    })
}

/// The published index resolves against indexed declarations only: it must
/// never assume a binding whose declaration it has not seen.
impl crate::type_name_resolution::TypeNameScope for IndexInner {
    fn indexed_type(&self, fqn: &str) -> bool {
        Self::indexed_type(self, fqn)
    }

    fn kotlin_type_alias_expansion(&self, alias_fqn: &str) -> Option<KotlinTypeAliasExpansion> {
        self.expand_kotlin_type_alias(alias_fqn, |owner| self.symbols.get(owner).cloned())
    }

    fn inherited_member_types(&self, owner: &str, simple_name: &str) -> Vec<String> {
        self.inherited_member_type_bindings(owner, simple_name)
    }

    fn static_member_types(
        &self,
        owner: &str,
        simple_name: &str,
        current_package: Option<&str>,
    ) -> Vec<String> {
        self.static_imported_member_type_bindings(owner, simple_name, current_package)
    }
}

impl crate::type_name_resolution::TypeNameScope for OwnerElection<'_> {
    fn indexed_type(&self, fqn: &str) -> bool {
        self.type_definition(fqn).is_some()
    }

    fn kotlin_type_alias_expansion(&self, alias_fqn: &str) -> Option<KotlinTypeAliasExpansion> {
        self.inner
            .expand_kotlin_type_alias(alias_fqn, |owner| self.symbol(owner))
    }

    fn inherited_member_types(&self, owner: &str, simple_name: &str) -> Vec<String> {
        self.inherited_member_type_bindings(owner, simple_name)
    }

    fn static_member_types(
        &self,
        owner: &str,
        simple_name: &str,
        current_package: Option<&str>,
    ) -> Vec<String> {
        self.static_imported_member_type_bindings(owner, simple_name, current_package)
    }
}

impl OwnerElection<'_> {
    fn type_package(&self, fqn: &str) -> String {
        let mut top_level = fqn.to_owned();
        while let Some(container) = self
            .symbol(&top_level)
            .and_then(|symbol| symbol.container.clone())
            .filter(|container| {
                self.symbol(container)
                    .is_some_and(|symbol| is_type_declaration_kind(symbol.kind))
            })
        {
            top_level = container.to_string();
        }
        top_level
            .rsplit_once('.')
            .map_or("", |(package, _)| package)
            .to_owned()
    }

    fn inherited_member_type_bindings(&self, owner: &str, simple_name: &str) -> Vec<String> {
        fn collect(
            election: &OwnerElection<'_>,
            ancestor: String,
            simple_name: &str,
            path: &mut Vec<String>,
            visited: &mut HashSet<String>,
            bindings: &mut Vec<String>,
        ) {
            if !visited.insert(ancestor.clone()) {
                return;
            }
            let candidate = format!("{ancestor}.{simple_name}");
            if let Some(member) = election.type_definition(&candidate) {
                let symbol = &member.symbol;
                let declared_in_interface = symbol.container.as_deref().is_some_and(|container| {
                    election
                        .symbol(container)
                        .is_some_and(|owner| owner.kind == lsp_types::SymbolKind::INTERFACE)
                });
                if !symbol.modifiers.is_private
                    && (declared_in_interface
                        || symbol.modifiers.is_public
                        || symbol.modifiers.is_protected
                        || path.iter().all(|descendant| {
                            election.type_package(descendant) == election.type_package(&candidate)
                        }))
                {
                    bindings.push(candidate);
                }
                return;
            }
            path.push(ancestor.clone());
            let (parent, interfaces) = election
                .inner
                .resolved_supertype_roles_with_election(election, &ancestor);
            for supertype in parent.into_iter().chain(interfaces) {
                collect(election, supertype, simple_name, path, visited, bindings);
            }
            path.pop();
        }

        let (parent, interfaces) = self
            .inner
            .resolved_supertype_roles_with_election(self, owner);
        let mut bindings = Vec::new();
        for supertype in parent.into_iter().chain(interfaces) {
            collect(
                self,
                supertype,
                simple_name,
                &mut vec![owner.to_owned()],
                &mut HashSet::new(),
                &mut bindings,
            );
        }
        bindings.sort_unstable();
        bindings.dedup();
        bindings
    }

    fn static_imported_member_type_bindings(
        &self,
        owner: &str,
        simple_name: &str,
        current_package: Option<&str>,
    ) -> Vec<String> {
        let source_package = current_package.unwrap_or("");
        let owner_package = self.type_package(owner);
        let mut current = Some(owner.to_owned());
        while let Some(type_fqn) = current {
            let Some(declaration) = self.type_definition(&type_fqn) else {
                return Vec::new();
            };
            let symbol = declaration.symbol;
            if symbol.modifiers.is_private
                || (!symbol.modifiers.is_public
                    && !symbol.modifiers.is_internal
                    && source_package != owner_package)
            {
                return Vec::new();
            }
            current = symbol.container.as_deref().and_then(|container| {
                self.symbol(container)
                    .filter(|symbol| is_type_declaration_kind(symbol.kind))
                    .map(|_| container.to_owned())
            });
        }

        let direct = format!("{owner}.{simple_name}");
        let bindings = if self.type_definition(&direct).is_some() {
            vec![direct]
        } else {
            self.inherited_member_type_bindings(owner, simple_name)
        };
        bindings
            .into_iter()
            .filter(|binding| {
                self.type_definition(binding).is_some_and(|declaration| {
                    let symbol = &declaration.symbol;
                    let declared_in_interface =
                        symbol.container.as_deref().is_some_and(|container| {
                            self.symbol(container)
                                .is_some_and(|owner| owner.kind == lsp_types::SymbolKind::INTERFACE)
                        });
                    let effectively_static = symbol.modifiers.is_static
                        || declared_in_interface
                        || matches!(
                            symbol.kind,
                            lsp_types::SymbolKind::INTERFACE
                                | lsp_types::SymbolKind::ENUM
                                | lsp_types::SymbolKind::STRUCT
                        );
                    let accessible = !symbol.modifiers.is_private
                        && (declared_in_interface
                            || symbol.modifiers.is_public
                            || symbol.modifiers.is_internal
                            || source_package == self.type_package(binding));
                    effectively_static && accessible
                })
            })
            .collect()
    }
}

fn simple_name(fqn: &str) -> &str {
    fqn.rsplit('.').next().unwrap_or(fqn)
}

fn first_segment(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

impl IndexInner {
    pub(super) fn symbol_language(&self, fqn: &str) -> Option<Language> {
        self.symbol_locations
            .get(fqn)
            .and_then(|location| self.file_path(location.file_id))
            .and_then(Language::from_path)
    }

    pub(super) fn implicit_exact_supertype(&self, symbol: &Symbol) -> Option<&'static str> {
        exact_implicit_supertype(symbol, self.symbol_language(&symbol.fqn))
    }

    pub(super) fn indexed_type(&self, fqn: &str) -> bool {
        self.symbols
            .get(fqn)
            .is_some_and(|symbol| is_type_declaration_kind(symbol.kind))
    }

    fn type_package(&self, fqn: &str) -> String {
        let mut top_level = fqn;
        while let Some(container) = self
            .symbols
            .get(top_level)
            .and_then(|symbol| symbol.container.as_deref())
        {
            top_level = container;
        }
        top_level
            .rsplit_once('.')
            .map_or("", |(package, _)| package)
            .to_owned()
    }

    fn inherited_member_type_is_accessible(
        &self,
        member: &Symbol,
        inheritance_path: &[String],
    ) -> bool {
        if member.modifiers.is_private {
            return false;
        }
        let declared_in_interface = member.container.as_deref().is_some_and(|container| {
            self.symbols
                .get(container)
                .is_some_and(|owner| owner.kind == lsp_types::SymbolKind::INTERFACE)
        });
        declared_in_interface
            || member.modifiers.is_public
            || member.modifiers.is_protected
            || inheritance_path
                .iter()
                .all(|descendant| self.type_package(descendant) == self.type_package(&member.fqn))
    }

    fn inherited_member_type_bindings(&self, receiver: &str, simple_name: &str) -> Vec<String> {
        fn collect_branch(
            inner: &IndexInner,
            ancestor: String,
            simple_name: &str,
            visited: &mut HashSet<String>,
            inheritance_path: &mut Vec<String>,
            bindings: &mut Vec<String>,
        ) {
            if !visited.insert(ancestor.clone()) {
                return;
            }
            let candidate = format!("{ancestor}.{simple_name}");
            if let Some(member) = super::query::find_symbol_definition_coherent_in(
                inner,
                &crate::SymbolTarget::new(candidate.clone(), crate::DeclarationNamespace::Type),
            ) {
                if inner.inherited_member_type_is_accessible(&member.symbol, inheritance_path) {
                    bindings.push(candidate);
                }
                // A declaration hides the same-name declaration deeper in
                // this branch even when it is not itself inherited.
                return;
            }
            let Some(symbol) = inner.symbols.get(ancestor.as_str()) else {
                return;
            };
            inheritance_path.push(ancestor.clone());
            for recorded in symbol.parent_class.iter().chain(&symbol.interfaces) {
                if let TypeNameResolution::Exact(parent) =
                    inner.resolve_recorded_supertype(&ancestor, recorded)
                {
                    collect_branch(
                        inner,
                        parent,
                        simple_name,
                        visited,
                        inheritance_path,
                        bindings,
                    );
                }
            }
            inheritance_path.pop();
        }

        let Some(symbol) = self.symbols.get(receiver) else {
            return Vec::new();
        };
        let mut bindings = Vec::new();
        for recorded in symbol.parent_class.iter().chain(&symbol.interfaces) {
            if let TypeNameResolution::Exact(parent) =
                self.resolve_recorded_supertype(receiver, recorded)
            {
                // Hiding is branch-local. A declaration in Parent suppresses
                // Grandparent's same-name declaration, while a distinct
                // interface branch can still make the inherited name ambiguous.
                collect_branch(
                    self,
                    parent,
                    simple_name,
                    &mut HashSet::new(),
                    &mut vec![receiver.to_owned()],
                    &mut bindings,
                );
            }
        }
        bindings.sort_unstable();
        bindings.dedup();
        bindings
    }

    fn member_type_is_effectively_static(&self, member: &Symbol) -> bool {
        if member.modifiers.is_static {
            return true;
        }
        if matches!(
            member.kind,
            lsp_types::SymbolKind::INTERFACE
                | lsp_types::SymbolKind::ENUM
                | lsp_types::SymbolKind::STRUCT
        ) {
            return true;
        }
        member.container.as_deref().is_some_and(|container| {
            self.symbols
                .get(container)
                .is_some_and(|owner| owner.kind == lsp_types::SymbolKind::INTERFACE)
        })
    }

    fn static_import_member_is_accessible(
        &self,
        member: &Symbol,
        current_package: Option<&str>,
    ) -> bool {
        if member.modifiers.is_private {
            return false;
        }
        let declared_in_interface = member.container.as_deref().is_some_and(|container| {
            self.symbols
                .get(container)
                .is_some_and(|owner| owner.kind == lsp_types::SymbolKind::INTERFACE)
        });
        if declared_in_interface || member.modifiers.is_public || member.modifiers.is_internal {
            return true;
        }
        current_package.unwrap_or("") == self.type_package(&member.fqn)
    }

    fn static_import_owner_is_accessible(
        &self,
        owner_fqn: &str,
        current_package: Option<&str>,
    ) -> bool {
        let source_package = current_package.unwrap_or("");
        let owner_package = self.type_package(owner_fqn);
        let mut current = Some(owner_fqn.to_owned());
        while let Some(type_fqn) = current.take() {
            let Some(definition) = super::query::find_symbol_definition_coherent_in(
                self,
                &crate::SymbolTarget::new(type_fqn, crate::DeclarationNamespace::Type),
            ) else {
                return false;
            };
            let symbol = definition.symbol;
            if symbol.modifiers.is_private {
                return false;
            }
            let declared_in_interface = symbol.container.as_deref().is_some_and(|container| {
                self.symbols
                    .get(container)
                    .is_some_and(|owner| owner.kind == lsp_types::SymbolKind::INTERFACE)
            });
            if !declared_in_interface
                && !symbol.modifiers.is_public
                && !symbol.modifiers.is_internal
                && source_package != owner_package
            {
                return false;
            }
            current = symbol.container.as_deref().and_then(|container| {
                self.symbols
                    .get(container)
                    .filter(|container| is_type_declaration_kind(container.kind))
                    .map(|_| container.to_owned())
            });
        }
        true
    }

    /// Resolve static-importable member types through one exact elected owner.
    /// Direct member types hide inherited declarations even when the direct
    /// declaration is not importable; inherited interface branches retain
    /// ambiguity as distinct bindings.
    pub(crate) fn static_imported_member_type_bindings(
        &self,
        owner_fqn: &str,
        simple_name: &str,
        current_package: Option<&str>,
    ) -> Vec<String> {
        if !self.static_import_owner_is_accessible(owner_fqn, current_package) {
            return Vec::new();
        }
        let direct = format!("{owner_fqn}.{simple_name}");
        let bindings = if super::query::find_symbol_definition_coherent_in(
            self,
            &crate::SymbolTarget::new(direct.clone(), crate::DeclarationNamespace::Type),
        )
        .is_some()
        {
            vec![direct]
        } else {
            self.inherited_member_type_bindings(owner_fqn, simple_name)
        };
        bindings
            .into_iter()
            .filter(|binding| {
                super::query::find_symbol_definition_coherent_in(
                    self,
                    &crate::SymbolTarget::new(binding.clone(), crate::DeclarationNamespace::Type),
                )
                .is_some_and(|definition| {
                    self.member_type_is_effectively_static(&definition.symbol)
                        && self
                            .static_import_member_is_accessible(&definition.symbol, current_package)
                })
            })
            .collect()
    }

    /// Resolve one written type name against the published index.
    ///
    /// The tier walk itself lives in [`crate::type_name_resolution`]; this is
    /// only the index's view of it. `lexical_owners` are ordered
    /// nearest-first and must not include the type whose `extends` header is
    /// being resolved; declarations in that type's body are not in scope in
    /// its own header.
    pub(super) fn resolve_type_name(
        &self,
        imports: &ImportMap,
        lexical_owners: &[Arc<str>],
        written: &str,
        language: Option<Language>,
    ) -> TypeNameResolution {
        crate::type_name_resolution::resolve_written_type_name(
            self,
            imports,
            lexical_owners,
            written,
            language,
        )
    }

    pub(super) fn resolve_recorded_supertype(
        &self,
        subtype_fqn: &str,
        recorded_supertype: &str,
    ) -> TypeNameResolution {
        self.resolve_recorded_supertype_with_election(
            &OwnerElection::new(self, None),
            subtype_fqn,
            recorded_supertype,
        )
    }

    pub(crate) fn resolve_recorded_supertype_with_election(
        &self,
        election: &OwnerElection<'_>,
        subtype_fqn: &str,
        recorded_supertype: &str,
    ) -> TypeNameResolution {
        if self.implicit_exact_supertype_for_election(election, subtype_fqn)
            == Some(recorded_supertype)
        {
            return if election.type_definition(recorded_supertype).is_some() {
                TypeNameResolution::Exact(recorded_supertype.to_owned())
            } else {
                TypeNameResolution::Missing
            };
        }
        let Some(declaration) = election.type_definition(subtype_fqn) else {
            return if election.type_definition(recorded_supertype).is_some() {
                TypeNameResolution::Exact(recorded_supertype.to_owned())
            } else {
                TypeNameResolution::Missing
            };
        };
        let file_id = declaration.location.file_id;
        if self.external_file_ids.contains(&file_id) {
            return if election.type_definition(recorded_supertype).is_some() {
                TypeNameResolution::Exact(recorded_supertype.to_owned())
            } else {
                TypeNameResolution::Missing
            };
        }
        let Some(imports) = self.file_imports.get(&file_id) else {
            return TypeNameResolution::Missing;
        };
        let language = self.file_path(file_id).and_then(Language::from_path);
        crate::type_name_resolution::resolve_written_type_name(
            election,
            imports,
            &self.lexical_owners_with_election(election, subtype_fqn),
            recorded_supertype,
            language,
        )
    }

    fn implicit_exact_supertype_for_election(
        &self,
        election: &OwnerElection<'_>,
        subtype_fqn: &str,
    ) -> Option<&'static str> {
        election
            .symbol(subtype_fqn)
            .as_deref()
            .and_then(|symbol| self.implicit_exact_supertype(symbol))
    }

    fn lexical_owners_with_election(
        &self,
        election: &OwnerElection<'_>,
        subtype_fqn: &str,
    ) -> Vec<Arc<str>> {
        let mut owners = Vec::new();
        let mut current = election
            .symbol(subtype_fqn)
            .and_then(|symbol| symbol.container.clone());
        while let Some(owner) = current {
            let owner_symbol = election.symbol(&owner);
            current = owner_symbol
                .as_ref()
                .and_then(|symbol| symbol.container.clone());
            if owner_symbol.is_some_and(|symbol| is_type_declaration_kind(symbol.kind)) {
                owners.push(owner);
            }
        }
        owners
    }

    /// Resolve direct edges and preserve the class/interface role needed by
    /// member lookup. Kotlin bare header entries have no syntactic role: when
    /// their declaration is indexed, its `SymbolKind` supplies that role;
    /// until then the edge remains unresolved rather than being promoted to a
    /// false superclass.
    pub(crate) fn resolved_supertype_roles(&self, type_fqn: &str) -> (Option<String>, Vec<String>) {
        self.resolved_supertype_roles_with_election(&OwnerElection::new(self, None), type_fqn)
    }

    pub(crate) fn resolved_supertype_roles_with_election(
        &self,
        election: &OwnerElection<'_>,
        type_fqn: &str,
    ) -> (Option<String>, Vec<String>) {
        let Some(declaration) = election.type_definition(type_fqn) else {
            return (None, Vec::new());
        };
        let symbol = &declaration.symbol;
        let language = self
            .file_path(declaration.location.file_id)
            .and_then(Language::from_path);
        let implicit = self.implicit_exact_supertype(symbol);
        let mut parent = implicit
            .filter(|target| *target != "java.lang.annotation.Annotation")
            .filter(|target| election.type_definition(target).is_some())
            .map(str::to_owned);
        let mut interfaces = implicit
            .filter(|target| *target == "java.lang.annotation.Annotation")
            .filter(|target| election.type_definition(target).is_some())
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>();
        let mut parent_was_reclassified = false;

        if let Some(recorded) = symbol.parent_class.as_deref() {
            if implicit != Some(recorded) {
                if let TypeNameResolution::Exact(resolved) =
                    self.resolve_recorded_supertype_with_election(election, type_fqn, recorded)
                {
                    if election
                        .symbol(resolved.as_str())
                        .is_some_and(|target| target.kind == lsp_types::SymbolKind::INTERFACE)
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
        let mut resolved_bare = symbol
            .interfaces
            .iter()
            .filter_map(|recorded| {
                match self.resolve_recorded_supertype_with_election(election, type_fqn, recorded) {
                    TypeNameResolution::Exact(resolved) => Some(resolved),
                    TypeNameResolution::Missing | TypeNameResolution::Ambiguous(_) => {
                        all_bare_resolved = false;
                        None
                    }
                }
            })
            .collect::<Vec<_>>();

        if language == Some(Language::Kotlin)
            && symbol.kind != lsp_types::SymbolKind::INTERFACE
            && parent.is_none()
        {
            let class_candidates = resolved_bare
                .iter()
                .filter(|resolved| {
                    election.symbol(resolved.as_str()).is_some_and(|target| {
                        target.kind != lsp_types::SymbolKind::INTERFACE
                            && is_type_declaration_kind(target.kind)
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if class_candidates.len() == 1 {
                parent = class_candidates.first().cloned();
                resolved_bare.retain(|resolved| Some(resolved) != parent.as_ref());
            }
        }
        let all_bare_interfaces = all_bare_resolved
            && resolved_bare.iter().all(|resolved| {
                election
                    .symbol(resolved.as_str())
                    .is_some_and(|target| target.kind == lsp_types::SymbolKind::INTERFACE)
            });
        if parent.is_none() {
            let implicit_root = crate::language::implicit_jvm_supertype(
                language,
                symbol.kind,
                &symbol.fqn,
                (!parent_was_reclassified)
                    .then_some(symbol.parent_class.as_deref())
                    .flatten(),
                all_bare_interfaces,
            );
            if let Some(root) =
                implicit_root.filter(|root| election.type_definition(root).is_some())
            {
                parent = Some(root.to_owned());
            }
        }
        interfaces.append(&mut resolved_bare);
        interfaces.sort_unstable();
        interfaces.dedup();
        (parent, interfaces)
    }

    /// Resolve the active direct subtype edges for `supertype_fqn`.
    ///
    /// Source symbols retain contextual written names when publication cannot
    /// resolve them exactly. The simple-name candidate index lets this view
    /// resolve those edges lazily without scanning the workspace or making the
    /// answer depend on load order.
    pub(super) fn direct_subtypes(&self, supertype_fqn: &str) -> Vec<Arc<str>> {
        let candidate_simple_names = self.inheritance.candidate_simple_names(supertype_fqn);
        let mut subtypes: Vec<_> = self
            .inheritance
            .subtype_candidates(supertype_fqn, &candidate_simple_names)
            .filter_map(|subtype_fqn| {
                let (parent, interfaces) = self.resolved_supertype_roles(subtype_fqn);
                (parent.as_deref() == Some(supertype_fqn)
                    || interfaces
                        .iter()
                        .any(|interface| interface == supertype_fqn))
                .then(|| Arc::clone(subtype_fqn))
            })
            .collect();
        subtypes.sort_unstable();
        subtypes.dedup();
        subtypes
    }

    /// Add all transitive descendants of `supertype_fqn` to `result`.
    ///
    /// `result` doubles as the visited set, so malformed cyclic hierarchies
    /// terminate. The root is never returned, even when a cycle points to it.
    pub(super) fn collect_transitive_subtypes(
        &self,
        supertype_fqn: &str,
        result: &mut HashSet<Arc<str>>,
    ) {
        let mut pending = self.direct_subtypes(supertype_fqn);

        while let Some(subtype) = pending.pop() {
            if subtype.as_ref() == supertype_fqn || !result.insert(Arc::clone(&subtype)) {
                continue;
            }

            pending.extend(self.direct_subtypes(&subtype));
        }
    }

    /// Collect a conservative descendant closure for cache invalidation.
    /// Potential contextual targets are included even after the target symbol
    /// has been removed, because that removal itself changes resolution.
    pub(super) fn collect_potential_transitive_subtypes(
        &self,
        supertype_fqn: &str,
        result: &mut HashSet<Arc<str>>,
    ) {
        let mut pending = self.potential_direct_subtypes(supertype_fqn);

        while let Some(subtype) = pending.pop() {
            if subtype.as_ref() == supertype_fqn || !result.insert(Arc::clone(&subtype)) {
                continue;
            }
            pending.extend(self.potential_direct_subtypes(&subtype));
        }
    }

    /// Return the bounded source candidates whose declared direct supertype
    /// could bind to `supertype_fqn` in either the current or next coherent
    /// workspace revision.
    ///
    /// Besides destructive-refactoring completeness, publication uses this
    /// view to invalidate declarations whose contextual (same-package,
    /// imported, or qualified) supertype binding may have changed.
    pub(super) fn potential_direct_subtypes(&self, supertype_fqn: &str) -> Vec<Arc<str>> {
        let candidate_simple_names = self.inheritance.candidate_simple_names(supertype_fqn);
        let mut subtypes: Vec<_> = self
            .inheritance
            .subtype_candidates(supertype_fqn, &candidate_simple_names)
            .filter_map(|subtype_fqn| {
                let subtype = self.symbols.get(subtype_fqn.as_ref())?;
                let resolves_exactly = {
                    let (parent, interfaces) = self.resolved_supertype_roles(subtype_fqn);
                    parent.as_deref() == Some(supertype_fqn)
                        || interfaces
                            .iter()
                            .any(|interface| interface == supertype_fqn)
                };
                (resolves_exactly
                    || subtype
                        .parent_class
                        .iter()
                        .chain(subtype.interfaces.iter())
                        .any(|recorded| {
                            recorded.as_ref() == supertype_fqn
                                || self.is_potential_contextual_supertype(
                                    &subtype.fqn,
                                    recorded,
                                    &candidate_simple_names,
                                )
                        }))
                .then(|| Arc::clone(subtype_fqn))
            })
            .collect();
        subtypes.sort_unstable();
        subtypes.dedup();
        subtypes
    }

    /// Whether every registered declaration that could name `supertype_fqn`
    /// has an exact current hierarchy binding.
    ///
    /// Navigation may ignore unresolved contextual candidates, but destructive
    /// refactorings cannot claim a complete subtype/override closure while one
    /// of those edges is missing or ambiguous.
    pub(super) fn direct_subtype_resolution_is_complete(&self, supertype_fqn: &str) -> bool {
        self.potential_direct_subtypes(supertype_fqn) == self.direct_subtypes(supertype_fqn)
    }

    fn is_potential_contextual_supertype(
        &self,
        subtype_fqn: &str,
        recorded_supertype: &str,
        candidate_simple_names: &[&str],
    ) -> bool {
        let is_source = self
            .symbol_locations
            .get(subtype_fqn)
            .is_some_and(|location| self.file_imports.contains_key(&location.file_id));
        // This is the conservative invalidation view, not binding. The
        // auxiliary index already bounded candidates to the target's final
        // simple segment, or that of a Kotlin typealias expanding to it.
        // Treat every source candidate in those buckets as potential so
        // removal of lexical/inherited/imported member types can never strand
        // a stale descendant edge. False positives are bounded by those
        // simple-name buckets and are preferable to missed reindexing.
        is_source
            && contextual_keys(
                recorded_supertype,
                self.symbol_locations
                    .get(subtype_fqn)
                    .and_then(|location| self.file_imports.get(&location.file_id)),
                self.symbol_language(subtype_fqn),
            )
            .iter()
            .any(|key| candidate_simple_names.contains(&key.as_str()))
    }
}
