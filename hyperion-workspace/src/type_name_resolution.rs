// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The single JLS-ordered type-name resolution engine.
//!
//! One written type spelling resolves to one identity through exactly one tier
//! walk. Two copies of this walk previously existed — one in
//! [`crate::import_resolver::ImportResolver`] and one in the index — and they
//! drifted: the same defect was fixed in one copy at a time, leaving the
//! others behind. The only remaining difference between callers is the
//! *view* of the workspace they resolve against, which is what
//! [`TypeNameScope`] abstracts.

use hyperion_types::DeclarationImports;

use crate::index::KotlinTypeAliasExpansion;
use crate::{KotlinTypeAliasArguments, Language, TypeNameResolution};

/// The workspace view one tier walk resolves against.
///
/// Implementors differ only in which declarations they can see (a published
/// index, or an index plus an unpublished revision overlay) and in whether a
/// core language binding counts as known before its declaration is indexed.
/// They must not reorder or reinterpret the tiers.
pub(crate) trait TypeNameScope {
    /// Whether a type declaration is actually indexed under `fqn`.
    fn indexed_type(&self, fqn: &str) -> bool;

    /// Whether `fqn` is a usable type identity.
    ///
    /// This is [`Self::indexed_type`] plus, for views that may rely on the
    /// language's unconditional bindings, the core `java.lang` types whose
    /// identity holds while the JDK is still being indexed. Views that must
    /// not guess leave this equal to [`Self::indexed_type`].
    fn bound_type(&self, fqn: &str) -> bool {
        self.indexed_type(fqn)
    }

    /// What a Kotlin typealias named `alias_fqn` expands to, as recorded on
    /// the declaring file's facade, library or source.
    fn kotlin_type_alias_expansion(&self, alias_fqn: &str) -> Option<KotlinTypeAliasExpansion>;

    /// Member types named `simple_name` inherited by `owner`, nearest branch
    /// first. Several results are a genuine ambiguity (JLS 8.5), not an
    /// ordered fallback.
    fn inherited_member_types(&self, owner: &str, simple_name: &str) -> Vec<String>;

    /// Member types named `simple_name` reachable through a static import of
    /// `owner`, honouring staticness and accessibility.
    fn static_member_types(
        &self,
        owner: &str,
        simple_name: &str,
        current_package: Option<&str>,
    ) -> Vec<String>;
}

/// Map a source-language spelling onto the JVM identity it denotes.
///
/// For Kotlin that is the built-in mapping table, then a typealias recorded
/// in the index: `kotlin.Comparator` denotes `java.util.Comparator`.
/// A declaration indexed under the spelling itself wins over an alias.
fn canonical_type_name<S: TypeNameScope + ?Sized>(
    scope: &S,
    name: String,
    language: Option<Language>,
) -> String {
    if language != Some(Language::Kotlin) {
        return name;
    }
    if let Some(mapped) = crate::kotlin_builtin_jvm_fqn(&name) {
        return mapped.to_owned();
    }
    kotlin_type_alias_identity(scope, &name).unwrap_or(name)
}

/// The JVM identity a Kotlin typealias denotes, unless a declaration is
/// indexed under the alias's own name.
fn kotlin_type_alias_identity<S: TypeNameScope + ?Sized>(scope: &S, name: &str) -> Option<String> {
    let expanded = kotlin_type_alias_binding(scope, name)?.class;
    Some(crate::kotlin_builtin_jvm_fqn(&expanded).map_or(expanded, str::to_owned))
}

/// The expansion of the Kotlin typealias `name`, unless a declaration is
/// indexed under the alias's own name.
fn kotlin_type_alias_binding<S: TypeNameScope + ?Sized>(
    scope: &S,
    name: &str,
) -> Option<KotlinTypeAliasExpansion> {
    // The alias lookup misses cheaply for nearly every name, so it goes first.
    let expanded = scope.kotlin_type_alias_expansion(name)?;
    (!scope.indexed_type(name)).then_some(expanded)
}

/// Whether `binding` is a Kotlin typealias whose expansion is bound.
fn is_bound_kotlin_type_alias<S: TypeNameScope + ?Sized>(
    scope: &S,
    binding: &str,
    language: Option<Language>,
) -> bool {
    language == Some(Language::Kotlin)
        && kotlin_type_alias_identity(scope, binding)
            .is_some_and(|identity| scope.bound_type(&identity))
}

/// One tier walk's resolution, with the bindings it was reached through.
struct Bound {
    resolution: TypeNameResolution,
    /// For an `Exact` resolution, the bound names before Kotlin's built-in
    /// and typealias mapping (`app.Names` for an alias that resolved to
    /// `java.util.ArrayList`); empty otherwise.
    sources: Vec<String>,
}

impl Bound {
    const fn unbound(resolution: TypeNameResolution) -> Self {
        Self {
            resolution,
            sources: Vec::new(),
        }
    }

    fn exact(identity: String, sources: Vec<String>) -> Self {
        Self {
            resolution: TypeNameResolution::Exact(identity),
            sources,
        }
    }
}

/// Reduce one tier's candidate bindings to a resolution.
///
/// A tier that produced candidates is decisive: if none of them survives
/// existence and suffix completion the name is `Missing` rather than falling
/// through to a weaker tier, which is what makes shadowing work.
fn finish_bound<S: TypeNameScope + ?Sized>(
    scope: &S,
    bindings: impl IntoIterator<Item = String>,
    suffix: Option<&str>,
    language: Option<Language>,
) -> Bound {
    let mut bindings = bindings
        .into_iter()
        .map(|source| {
            let identity = canonical_type_name(scope, source.clone(), language);
            (identity, source)
        })
        .collect::<Vec<_>>();
    bindings.sort_unstable();
    bindings.dedup();
    let mut identities = bindings
        .iter()
        .map(|(identity, _)| identity.clone())
        .collect::<Vec<_>>();
    identities.sort_unstable();
    identities.dedup();
    match identities.len() {
        0 => return Bound::unbound(TypeNameResolution::Missing),
        1 => {}
        _ => return Bound::unbound(TypeNameResolution::Ambiguous(identities)),
    }
    let identity = identities.pop().unwrap_or_default();
    let Some(suffix) = suffix else {
        return if scope.bound_type(&identity) {
            let sources = bindings.into_iter().map(|(_, source)| source).collect();
            Bound::exact(identity, sources)
        } else {
            Bound::unbound(TypeNameResolution::Missing)
        };
    };
    let completed = bindings
        .into_iter()
        .map(|(_, source)| {
            let written = format!("{source}.{suffix}");
            let mapped = canonical_type_name(scope, written.clone(), language);
            if mapped == written {
                let member = format!("{identity}.{suffix}");
                (member.clone(), member)
            } else {
                (mapped, written)
            }
        })
        .filter(|(candidate, _)| scope.bound_type(candidate))
        .collect::<Vec<_>>();
    let mut candidates = completed
        .iter()
        .map(|(candidate, _)| candidate.clone())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    if candidates.is_empty() {
        // `Child.Kind` where `Kind` is a member type inherited by `Child`
        // from a superclass or superinterface (JLS 8.5, 6.5.5.2).
        if let Some(inherited) = complete_through_inheritance(scope, &identity, suffix) {
            return Bound::exact(inherited.clone(), vec![inherited]);
        }
    }
    match candidates.len() {
        0 => Bound::unbound(TypeNameResolution::Missing),
        1 => {
            let candidate = candidates.pop().unwrap_or_default();
            let sources = completed
                .into_iter()
                .filter(|(completed, _)| *completed == candidate)
                .map(|(_, source)| source)
                .collect();
            Bound::exact(candidate, sources)
        }
        _ => Bound::unbound(TypeNameResolution::Ambiguous(candidates)),
    }
}

/// Follow `suffix` segment by segment from `owner`, taking each segment as a
/// direct member type or, failing that, as a uniquely inherited one.
fn complete_through_inheritance<S: TypeNameScope + ?Sized>(
    scope: &S,
    owner: &str,
    suffix: &str,
) -> Option<String> {
    let mut current = owner.to_owned();
    for segment in suffix.split('.') {
        let direct = format!("{current}.{segment}");
        if scope.indexed_type(&direct) {
            current = direct;
            continue;
        }
        let mut inherited = scope.inherited_member_types(&current, segment);
        inherited.sort_unstable();
        inherited.dedup();
        match inherited.as_slice() {
            [only] => current = only.clone(),
            _ => return None,
        }
    }
    Some(current)
}

/// Resolve one written type name through the JLS tier order.
///
/// Tiers, strongest first:
/// 1. lexical member types of each enclosing declaration, nearest first —
///    including inherited member types (JLS 6.5.5.1, 8.5)
/// 2. single-type and single-static-member imports (JLS 7.5.1, 7.5.3)
/// 3. the compilation unit's own package, when the type exists (JLS 6.5.5)
/// 4. the language's implicit single-type imports
/// 5. on-demand imports and the language's implicit on-demand packages
///    (`java.lang` among them) as one ambiguity set (JLS 7.5.2, 7.5.4)
/// 6. reclassification of the leading identifier as a package name (JLS 6.5.2)
///
/// `lexical_owners` are ordered nearest-first and must not include the type
/// whose own header is being resolved; its body members are not in scope there.
pub(crate) fn resolve_written_type_name<S, O>(
    scope: &S,
    imports: &DeclarationImports,
    lexical_owners: &[O],
    written: &str,
    language: Option<Language>,
) -> TypeNameResolution
where
    S: TypeNameScope + ?Sized,
    O: AsRef<str>,
{
    bind_written_type_name(scope, imports, lexical_owners, written, language).resolution
}

/// The type arguments that the Kotlin typealias a written type name binds to
/// gives the class it expands to, at a use that writes `written_arguments`
/// `["String"]` for `Names` under `typealias Names =
/// java.util.ArrayList<String>`, and `["String", "Int"]` for `Inverse<Int,
/// String>` under `typealias Inverse<K, V> = java.util.HashMap<V, K>`.
///
/// The name is resolved exactly as [`resolve_written_type_name`] resolves it.
/// It is an alias only when it resolves exactly and every binding of it is
/// the same alias; see [`kotlin_type_alias_arguments_at_use`] for how the
/// use's arguments are matched to the alias's type parameters.
pub(crate) fn resolve_written_kotlin_type_alias_arguments<S, O>(
    scope: &S,
    imports: &DeclarationImports,
    lexical_owners: &[O],
    written: &str,
    written_arguments: &[String],
    language: Option<Language>,
) -> KotlinTypeAliasArguments
where
    S: TypeNameScope + ?Sized,
    O: AsRef<str>,
{
    if language != Some(Language::Kotlin) {
        return KotlinTypeAliasArguments::NotAnAlias;
    }
    let bound = bind_written_type_name(scope, imports, lexical_owners, written, language);
    if !matches!(bound.resolution, TypeNameResolution::Exact(_)) {
        return KotlinTypeAliasArguments::NotAnAlias;
    }
    let mut uses = bound.sources.iter().map(|source| {
        // A built-in mapping (`kotlin.collections.List`) is not an alias.
        if crate::kotlin_builtin_jvm_fqn(source).is_some() {
            return None;
        }
        kotlin_type_alias_binding(scope, source)
            .map(|expansion| kotlin_type_alias_arguments_at_use(&expansion, written_arguments))
    });
    let Some(Some(first)) = uses.next() else {
        return KotlinTypeAliasArguments::NotAnAlias;
    };
    if uses.all(|other| other.as_ref() == Some(&first)) {
        first
    } else {
        KotlinTypeAliasArguments::NotAnAlias
    }
}

/// The type arguments `expansion` gives its class at a use of the alias that
/// writes `written_arguments`, each a Kotlin type spelling.
///
/// The use must give each of the alias's type parameters an argument, in
/// order: `Box<String>` for `typealias Box<T>`, and nothing for an alias
/// that declares none. Each parameter's name in the expansion's arguments is
/// replaced by the use's argument for it. A use with more or fewer arguments,
/// or an empty one, is [`KotlinTypeAliasArguments::Unmatched`] rather than
/// matched by position, and so is a projection (`*`, `out Number`) where the
/// parameter is not itself a type argument of the expansion.
fn kotlin_type_alias_arguments_at_use(
    expansion: &KotlinTypeAliasExpansion,
    written_arguments: &[String],
) -> KotlinTypeAliasArguments {
    if written_arguments.len() != expansion.type_parameters.len()
        || written_arguments
            .iter()
            .any(|argument| argument.trim().is_empty())
    {
        return KotlinTypeAliasArguments::Unmatched;
    }
    expansion
        .type_arguments
        .iter()
        .map(|argument| {
            substitute_type_parameters(argument, &expansion.type_parameters, written_arguments)
        })
        .collect::<Option<Vec<_>>>()
        .map_or(
            KotlinTypeAliasArguments::Unmatched,
            KotlinTypeAliasArguments::Expanded,
        )
}

/// A projected type argument: `*`, `out Number` or `in Number`.
fn is_projection(argument: &str) -> bool {
    argument == "*" || argument.starts_with("out ") || argument.starts_with("in ")
}

/// `spelling`, one type argument of an alias's expansion, with each of the
/// alias's `parameters` replaced by the use's argument for it. `None` when a
/// projected argument would land where a projection cannot stand, or would
/// conflict with the variance the expansion writes.
fn substitute_type_parameters(
    spelling: &str,
    parameters: &[String],
    arguments: &[String],
) -> Option<String> {
    let spelling = spelling.trim();
    let argument_for = |name: &str| {
        parameters
            .iter()
            .position(|parameter| parameter == name)
            .and_then(|index| arguments.get(index))
            .map(|argument| argument.trim())
    };

    // The parameter is the whole argument, as in `java.util.ArrayList<T>` or
    // `java.util.ArrayList<out T>`: any argument may take its place.
    let (variance, bare) = ["out ", "in "]
        .iter()
        .find_map(|variance| {
            spelling
                .strip_prefix(variance)
                .map(|rest| (*variance, rest.trim()))
        })
        .unwrap_or(("", spelling));
    if let Some(argument) = argument_for(bare) {
        if variance.is_empty() || argument == "*" || argument.starts_with(variance) {
            return Some(argument.to_owned());
        }
        if is_projection(argument) {
            return None;
        }
        return Some(format!("{variance}{argument}"));
    }

    // Elsewhere, each occurrence is a whole identifier, not a segment of a
    // qualified name, and a projection stands only as a type argument.
    let is_identifier = |c: char| c.is_alphanumeric() || c == '_';
    let chars = spelling.char_indices().collect::<Vec<_>>();
    let mut substituted = String::with_capacity(spelling.len());
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if !is_identifier(c) {
            substituted.push(c);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && is_identifier(chars[j].1) {
            j += 1;
        }
        let end = chars.get(j).map_or(spelling.len(), |(end, _)| *end);
        let identifier = &spelling[start..end];
        let before = spelling[..start].trim_end().chars().last();
        let after = spelling[end..].trim_start().chars().next();
        match argument_for(identifier).filter(|_| before != Some('.') && after != Some('.')) {
            Some(argument) => {
                let in_argument_position =
                    matches!(before, Some('<' | ',')) && matches!(after, Some('>' | ','));
                if is_projection(argument) && !in_argument_position {
                    return None;
                }
                substituted.push_str(argument);
                // `T?` with `T` bound to `String?` is `String?`.
                if argument.ends_with('?') && spelling[end..].starts_with('?') {
                    j += 1;
                }
            }
            None => substituted.push_str(identifier),
        }
        i = j;
    }
    Some(substituted)
}

fn bind_written_type_name<S, O>(
    scope: &S,
    imports: &DeclarationImports,
    lexical_owners: &[O],
    written: &str,
    language: Option<Language>,
) -> Bound
where
    S: TypeNameScope + ?Sized,
    O: AsRef<str>,
{
    let written = written.trim();
    if written.is_empty() {
        return Bound::unbound(TypeNameResolution::Missing);
    }
    let (first, suffix) = written
        .split_once('.')
        .map_or((written, None), |(first, suffix)| (first, Some(suffix)));

    // 1. Lexical member types shadow all imports. For each enclosing scope,
    // its direct and inherited member types form one tier before the next
    // outer scope.
    for owner in lexical_owners {
        let owner = owner.as_ref();
        if owner.rsplit('.').next() == Some(first) {
            return finish_bound(scope, [owner.to_owned()], suffix, language);
        }
        let direct = format!("{owner}.{first}");
        if scope.indexed_type(&direct) {
            return finish_bound(scope, [direct], suffix, language);
        }
        let inherited = scope.inherited_member_types(owner, first);
        if !inherited.is_empty() {
            return finish_bound(scope, inherited, suffix, language);
        }
    }

    // 2. Single-type and single-static member imports occupy the same
    // definitive tier. Invalid source can bind both; never let suffix
    // existence silently select one.
    let mut single_imports = imports
        .explicit
        .get(first)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    for binding in imports.static_imports_named(first) {
        if let Some((owner, member)) = binding.rsplit_once('.') {
            single_imports.extend(scope.static_member_types(
                owner,
                member,
                imports.current_package.as_deref(),
            ));
        }
    }
    if !single_imports.is_empty() {
        return finish_bound(scope, single_imports, suffix, language);
    }

    // 3. The compilation unit's own package shadows every on-demand import,
    // including java.lang — but only when the type actually exists there
    // (JLS 6.5.5).
    let current_binding = imports
        .current_package
        .as_ref()
        .map_or_else(|| first.to_owned(), |package| format!("{package}.{first}"));
    if scope.indexed_type(&current_binding)
        || is_bound_kotlin_type_alias(scope, &current_binding, language)
    {
        return finish_bound(scope, [current_binding], suffix, language);
    }

    // 4. Implicit single-type imports (Groovy's java.math bindings).
    if let Some(binding) = crate::implicit_single_type_import(language, first) {
        return finish_bound(scope, [binding.to_owned()], suffix, language);
    }

    // 5. On-demand imports. Java keeps written and implicit on-demand sources
    // in one ambiguity set; other languages let written imports shadow the
    // language defaults, and Scala's root contexts shadow left to right.
    let mut source_on_demand = imports
        .wildcards
        .iter()
        .map(|package| format!("{package}.{first}"))
        .filter(|binding| {
            scope.indexed_type(&canonical_type_name(scope, binding.clone(), language))
        })
        .collect::<Vec<_>>();
    source_on_demand.extend(imports.static_wildcards.iter().flat_map(|owner| {
        scope.static_member_types(owner, first, imports.current_package.as_deref())
    }));
    if language != Some(Language::Java) && !source_on_demand.is_empty() {
        return finish_bound(scope, source_on_demand, suffix, language);
    }

    let implicit_packages = crate::implicit_type_import_packages(language);
    if language == Some(Language::Scala) {
        for package in implicit_packages {
            let binding = format!("{package}.{first}");
            if scope.bound_type(&canonical_type_name(scope, binding.clone(), language)) {
                return finish_bound(scope, [binding], suffix, language);
            }
        }
    } else {
        let mut on_demand = source_on_demand;
        on_demand.extend(
            implicit_packages
                .iter()
                .map(|package| format!("{package}.{first}"))
                .filter(|binding| {
                    scope.bound_type(&canonical_type_name(scope, binding.clone(), language))
                }),
        );
        if !on_demand.is_empty() {
            return finish_bound(scope, on_demand, suffix, language);
        }
    }

    // 6. Package reclassification is the last qualified-name tier. An
    // on-demand type binding for the first segment obscures a same-named
    // package even when the requested member type is missing.
    let package_qualified = canonical_type_name(scope, written.to_owned(), language);
    if suffix.is_some() && scope.bound_type(&package_qualified) {
        return Bound::exact(package_qualified, vec![written.to_owned()]);
    }

    if imports.current_package.is_none() && scope.indexed_type(written) {
        Bound::exact(written.to_owned(), vec![written.to_owned()])
    } else {
        Bound::unbound(TypeNameResolution::Missing)
    }
}
