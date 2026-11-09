// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Generic type-argument composition across nominal inheritance edges.
//!
//! The ordinary symbol graph stores erased superclass/interface identities.
//! Parameterized edges are sparse side metadata, so this module resolves and
//! composes them under the same index read as the active declarations.

use super::*;
use crate::TypeNameResolution;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) type TypeBindings = SmallVec<[(Arc<str>, Option<String>); 4]>;

pub(super) fn binding<'a>(bindings: &'a TypeBindings, name: &str) -> Option<&'a Option<String>> {
    bindings
        .iter()
        .rev()
        .find_map(|(parameter, value)| (parameter.as_ref() == name).then_some(value))
}

fn enclosing_type_fqn(inner: &IndexInner, type_fqn: &str) -> Option<String> {
    let declaration = inner.symbols.get(type_fqn)?;
    if declaration.modifiers.is_static
        || matches!(
            declaration.kind,
            SymbolKind::INTERFACE | SymbolKind::ENUM | SymbolKind::STRUCT
        )
    {
        return None;
    }
    if let Some(container) = declaration.container.as_deref() {
        if let Some(container_symbol) = inner
            .symbols
            .get(container)
            .filter(|symbol| crate::is_type_declaration_kind(symbol.kind))
        {
            return (container_symbol.kind != SymbolKind::INTERFACE).then(|| container.to_owned());
        }
    }

    type_fqn.match_indices('.').rev().find_map(|(offset, _)| {
        let candidate = &type_fqn[..offset];
        inner
            .symbols
            .get(candidate)
            .is_some_and(|symbol| crate::is_type_declaration_kind(symbol.kind))
            .then(|| candidate.to_owned())
    })
}

fn enclosing_type_fqn_with_election(
    election: &OwnerElection<'_>,
    type_fqn: &str,
) -> Option<String> {
    let declaration = election.symbol(type_fqn)?;
    if declaration.modifiers.is_static
        || matches!(
            declaration.kind,
            SymbolKind::INTERFACE | SymbolKind::ENUM | SymbolKind::STRUCT
        )
    {
        return None;
    }
    if let Some(container) = declaration.container.as_deref() {
        if let Some(container_symbol) = election
            .symbol(container)
            .filter(|symbol| crate::is_type_declaration_kind(symbol.kind))
        {
            return (container_symbol.kind != SymbolKind::INTERFACE).then(|| container.to_owned());
        }
    }
    type_fqn.match_indices('.').rev().find_map(|(offset, _)| {
        let candidate = &type_fqn[..offset];
        election
            .symbol(candidate)
            .is_some_and(|symbol| crate::is_type_declaration_kind(symbol.kind))
            .then(|| candidate.to_owned())
    })
}

pub(crate) fn effective_type_parameters_with_election(
    election: &OwnerElection<'_>,
    type_fqn: &str,
) -> Option<Vec<crate::TypeParameterInfo>> {
    let mut owners = Vec::new();
    let mut current = Some(type_fqn.to_owned());
    while let Some(owner) = current {
        if owners.contains(&owner) {
            return None;
        }
        current = enclosing_type_fqn_with_election(election, &owner);
        owners.push(owner);
    }
    owners.reverse();
    Some(
        owners
            .iter()
            .filter_map(|owner| election.symbol(owner))
            .flat_map(|owner| owner.type_parameters.clone())
            .collect(),
    )
}

pub(super) fn effective_type_parameters<'a>(
    inner: &'a IndexInner,
    type_fqn: &str,
) -> Option<Vec<&'a crate::TypeParameterInfo>> {
    let mut owners = Vec::new();
    let mut current = Some(type_fqn.to_owned());
    while let Some(owner) = current {
        if owners.iter().any(|existing| existing == &owner) {
            return None;
        }
        current = enclosing_type_fqn(inner, &owner);
        owners.push(owner);
    }
    owners.reverse();
    Some(
        owners
            .iter()
            .filter_map(|owner| inner.symbols.get(owner.as_str()))
            .flat_map(|owner| owner.type_parameters.iter())
            .collect(),
    )
}

pub(super) fn substitute_member_type_parameters(
    written: &str,
    substitutions: &TypeBindings,
    shadowing_parameters: &[crate::TypeParameterInfo],
) -> Option<String> {
    let mut output = String::with_capacity(written.len());
    let mut identifier = String::new();
    let mut unresolved = false;
    let flush = |identifier: &mut String, output: &mut String, unresolved: &mut bool| {
        if identifier.is_empty() {
            return;
        }
        if shadowing_parameters
            .iter()
            .any(|parameter| parameter.name.as_ref() == identifier)
        {
            output.push_str(identifier);
        } else {
            match binding(substitutions, identifier.as_str()) {
                Some(Some(replacement)) => output.push_str(replacement),
                Some(None) => *unresolved = true,
                None => output.push_str(identifier),
            }
        }
        identifier.clear();
    };

    for character in written.trim().chars() {
        // Dotted nominal identities are one token. A type parameter `T` must
        // not rewrite the final segment of the unrelated class `pkg.T`.
        if character.is_alphanumeric() || matches!(character, '_' | '$' | '.') {
            identifier.push(character);
        } else {
            flush(&mut identifier, &mut output, &mut unresolved);
            output.push(character);
        }
    }
    flush(&mut identifier, &mut output, &mut unresolved);
    if unresolved {
        return None;
    }
    if output.ends_with("...") {
        output.truncate(output.len() - 3);
        output.push_str("[]");
    }
    Some(output)
}

pub(super) fn qualify_type_with_bindings_in(
    election: &OwnerElection<'_>,
    subtype_fqn: &str,
    written: &str,
    substitutions: &TypeBindings,
) -> Option<String> {
    let mut output = String::with_capacity(written.len());
    let mut token = String::new();
    let flush = |token: &mut String, output: &mut String| -> Option<()> {
        if token.is_empty() {
            return Some(());
        }
        if let Some(binding) = binding(substitutions, token.as_str()) {
            output.push_str(binding.as_deref()?);
        } else if matches!(
            token.as_str(),
            "byte"
                | "short"
                | "int"
                | "long"
                | "float"
                | "double"
                | "boolean"
                | "char"
                | "void"
                | "extends"
                | "super"
        ) {
            output.push_str(token);
        } else {
            match election.inner.resolve_recorded_supertype_with_election(
                election,
                subtype_fqn,
                token,
            ) {
                TypeNameResolution::Exact(resolved) => output.push_str(&resolved),
                // A qualified identity is already contextual. Missing simple
                // names are retained for incomplete classpaths; ambiguity is
                // the unsafe case and fails closed below.
                TypeNameResolution::Missing => output.push_str(token),
                TypeNameResolution::Ambiguous(_) => return None,
            }
        }
        token.clear();
        Some(())
    };

    for character in written.trim().chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '$' | '.') {
            token.push(character);
        } else {
            flush(&mut token, &mut output)?;
            output.push(character);
        }
    }
    flush(&mut token, &mut output)?;
    Some(output)
}

fn declaration_bindings(
    election: &OwnerElection<'_>,
    declaration: &crate::Symbol,
    supplied_arguments: &[String],
) -> Option<TypeBindings> {
    let parameters = effective_type_parameters_with_election(election, &declaration.fqn)?;
    let own_offset = parameters
        .len()
        .saturating_sub(declaration.type_parameters.len());
    if !supplied_arguments.is_empty()
        && supplied_arguments.len() != parameters.len()
        && supplied_arguments.len() != declaration.type_parameters.len()
    {
        return None;
    }
    Some(
        parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let supplied_position = if supplied_arguments.len() == parameters.len() {
                    Some(index)
                } else if index >= own_offset {
                    Some(index - own_offset)
                } else {
                    None
                };
                (
                    Arc::clone(&parameter.name),
                    supplied_position
                        .and_then(|position| supplied_arguments.get(position).cloned()),
                )
            })
            .collect(),
    )
}

fn active_generic_edges<'a>(
    election: &'a OwnerElection<'_>,
    subtype_fqn: &str,
) -> Option<&'a [crate::GenericSupertype]> {
    let file_id = election.type_definition(subtype_fqn)?.location.file_id;
    election
        .inner
        .file_generic_supertypes
        .get(&file_id)?
        .get(subtype_fqn)
        .map(AsRef::as_ref)
}

fn bindings_for_direct_supertype(
    election: &OwnerElection<'_>,
    subtype_fqn: &str,
    subtype_bindings: &TypeBindings,
    supertype_fqn: &str,
) -> Option<TypeBindings> {
    let supertype_parameters = effective_type_parameters_with_election(election, supertype_fqn)?;
    if supertype_parameters.is_empty() {
        return Some(TypeBindings::new());
    }

    let mut matching_edges = active_generic_edges(election, subtype_fqn)
        .into_iter()
        .flatten()
        .filter(|edge| {
            matches!(
                election.inner.resolve_recorded_supertype_with_election(election, subtype_fqn, &edge.written_target),
                TypeNameResolution::Exact(ref resolved) if resolved == supertype_fqn
            )
        });

    // No explicit edge is a raw generic supertype. Preserve the hierarchy but
    // mark every parameter unknown. Duplicate exact edges are malformed or
    // ambiguous metadata and fail closed.
    let Some(edge) = matching_edges.next() else {
        return Some(
            supertype_parameters
                .iter()
                .map(|parameter| (Arc::clone(&parameter.name), None))
                .collect(),
        );
    };
    if matching_edges.next().is_some() {
        return None;
    }
    if edge.arguments.len() != supertype_parameters.len() {
        return None;
    }

    supertype_parameters
        .iter()
        .enumerate()
        .map(|(position, parameter)| {
            let written = edge.arguments.get(position)?;
            Some((
                Arc::clone(&parameter.name),
                qualify_type_with_bindings_in(election, subtype_fqn, written, subtype_bindings),
            ))
        })
        .collect()
}

fn hierarchy_nodes_reaching_owner(
    election: &OwnerElection<'_>,
    receiver_fqn: &str,
    declaring_owner_fqn: &str,
) -> Option<HashSet<String>> {
    let mut visited = HashSet::from([receiver_fqn.to_owned()]);
    let mut pending = VecDeque::from([receiver_fqn.to_owned()]);
    let mut predecessors = HashMap::<String, SmallVec<[String; 2]>>::new();
    while let Some(current) = pending.pop_front() {
        let (parent, interfaces) = election
            .inner
            .resolved_supertype_roles_with_election(election, &current);
        for target in parent.into_iter().chain(interfaces) {
            predecessors
                .entry(target.clone())
                .or_default()
                .push(current.clone());
            if visited.insert(target.clone()) {
                pending.push_back(target);
            }
        }
    }
    if !visited.contains(declaring_owner_fqn) {
        return None;
    }

    let mut relevant = HashSet::from([declaring_owner_fqn.to_owned()]);
    let mut pending = VecDeque::from([declaring_owner_fqn.to_owned()]);
    while let Some(current) = pending.pop_front() {
        for predecessor in predecessors.get(&current).into_iter().flatten() {
            if relevant.insert(predecessor.clone()) {
                pending.push_back(predecessor.clone());
            }
        }
    }
    Some(relevant)
}

pub(super) fn owner_type_bindings_in(
    inner: &IndexInner,
    receiver_fqn: &str,
    receiver_type_arguments: &[String],
    declaring_owner_fqn: &str,
) -> Option<TypeBindings> {
    owner_type_bindings_with_election(
        &OwnerElection::new(inner, None),
        receiver_fqn,
        receiver_type_arguments,
        declaring_owner_fqn,
    )
}

pub(crate) fn owner_type_bindings_with_election(
    election: &OwnerElection<'_>,
    receiver_fqn: &str,
    receiver_type_arguments: &[String],
    declaring_owner_fqn: &str,
) -> Option<TypeBindings> {
    let receiver = election.symbol(receiver_fqn)?;
    election.symbol(declaring_owner_fqn)?;
    let receiver_bindings = declaration_bindings(election, &receiver, receiver_type_arguments)?;
    let relevant = hierarchy_nodes_reaching_owner(election, receiver_fqn, declaring_owner_fqn)?;

    let mut discovered = HashMap::<String, TypeBindings>::new();
    discovered.insert(receiver_fqn.to_owned(), receiver_bindings.clone());
    let mut pending = VecDeque::from([(receiver_fqn.to_owned(), receiver_bindings)]);

    while let Some((current, bindings)) = pending.pop_front() {
        if current == declaring_owner_fqn {
            continue;
        }
        let (parent, interfaces) = election
            .inner
            .resolved_supertype_roles_with_election(election, &current);
        for target in parent
            .into_iter()
            .chain(interfaces)
            .filter(|target| relevant.contains(target))
        {
            let target_bindings =
                bindings_for_direct_supertype(election, &current, &bindings, &target)?;
            match discovered.get(&target) {
                Some(existing) if existing != &target_bindings => {
                    // Java rejects inheriting one generic declaration through
                    // incompatible instantiations. If an incomplete index does
                    // contain such a diamond, exposing either projection would
                    // be unsound.
                    return None;
                }
                Some(_) => {}
                None => {
                    discovered.insert(target.clone(), target_bindings.clone());
                    pending.push_back((target, target_bindings));
                }
            }
        }
    }

    discovered.remove(declaring_owner_fqn)
}

pub(crate) fn project_type_from_receiver_in(
    inner: &IndexInner,
    receiver_fqn: &str,
    receiver_type_arguments: &[String],
    declaring_owner_fqn: &str,
    written_type: &str,
    shadowing_parameters: &[crate::TypeParameterInfo],
) -> Option<String> {
    let bindings = owner_type_bindings_in(
        inner,
        receiver_fqn,
        receiver_type_arguments,
        declaring_owner_fqn,
    )?;
    substitute_member_type_parameters(written_type, &bindings, shadowing_parameters)
}

impl WorkspaceIndex {
    /// Project a type written in an inherited declaration into a receiver's
    /// generic context.
    ///
    /// The hierarchy, selected declarations, and sparse edge metadata are all
    /// observed through one index read. Raw edges, conflicting diamonds, and
    /// malformed arity fail closed when the requested type depends on an
    /// unknown owner parameter.
    pub fn project_type_from_receiver(
        &self,
        receiver_fqn: &str,
        receiver_type_arguments: &[String],
        declaring_owner_fqn: &str,
        written_type: &str,
    ) -> Option<String> {
        project_type_from_receiver_in(
            &self.inner.read(),
            receiver_fqn,
            receiver_type_arguments,
            declaring_owner_fqn,
            written_type,
            &[],
        )
    }

    /// Project an inherited member type while respecting member-level type
    /// parameters that shadow equally named owner parameters.
    pub fn project_member_type_from_receiver(
        &self,
        receiver_fqn: &str,
        receiver_type_arguments: &[String],
        declaring_owner_fqn: &str,
        written_type: &str,
        member_type_parameters: &[crate::TypeParameterInfo],
    ) -> Option<String> {
        project_type_from_receiver_in(
            &self.inner.read(),
            receiver_fqn,
            receiver_type_arguments,
            declaring_owner_fqn,
            written_type,
            member_type_parameters,
        )
    }

    /// Return the declaring owner's parameters expressed in the receiver's
    /// context. `None` entries are raw/unknown arguments; an outer `None`
    /// means the hierarchy or its metadata was conflicting or malformed.
    pub fn project_owner_type_arguments(
        &self,
        receiver_fqn: &str,
        receiver_type_arguments: &[String],
        declaring_owner_fqn: &str,
    ) -> Option<Vec<(crate::TypeParameterInfo, Option<String>)>> {
        let inner = self.inner.read();
        let bindings = owner_type_bindings_in(
            &inner,
            receiver_fqn,
            receiver_type_arguments,
            declaring_owner_fqn,
        )?;
        effective_type_parameters(&inner, declaring_owner_fqn)?
            .into_iter()
            .map(|parameter| {
                Some((
                    parameter.clone(),
                    binding(&bindings, parameter.name.as_ref())?.clone(),
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    fn index_java(index: &WorkspaceIndex, root: &std::path::Path, name: &str, source: &str) {
        let path = root.join(name);
        std::fs::write(&path, source).unwrap();
        index.index_file_sync_for_test(&path).unwrap();
    }

    fn scoped_duplicate_sources(
        shadow_base: &str,
        visible_base: &str,
        visible_child: &str,
    ) -> (TempDir, WorkspaceIndex) {
        let temp = TempDir::new().unwrap();
        let shadow_root = temp.path().join("a-shadow/src/main/java/p");
        let visible_root = temp.path().join("z-visible/src/main/java/p");
        std::fs::create_dir_all(&shadow_root).unwrap();
        std::fs::create_dir_all(&visible_root).unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(&index, &shadow_root, "Base.java", shadow_base);
        index_java(&index, &visible_root, "Base.java", visible_base);
        index_java(
            &index,
            &visible_root,
            "Text.java",
            "package p; public class Text {}",
        );
        index_java(&index, &visible_root, "Child.java", visible_child);
        let scoped = index.with_source_scope(super::SourceScope::new([visible_root]));
        (temp, scoped)
    }

    #[test]
    fn scoped_generic_projection_uses_visible_owner_parameters() {
        let (_temp, index) = scoped_duplicate_sources(
            "package p; public class Base<X, Y> { public X value; }",
            "package p; public class Base<T> { public T value; }",
            "package p; public class Child extends Base<Text> {}",
        );

        let field = index
            .find_projected_effective_field_by_name("p.Child", &[], "value")
            .expect("visible generic field");
        assert_eq!(field.projected_type.as_deref(), Some("p.Text"));
    }

    #[test]
    fn scoped_inheritance_finds_visible_static_base_method() {
        let (_temp, index) = scoped_duplicate_sources(
            "package p; public class Base { public static boolean other() { return true; } }",
            "package p; public class Base { public static int seed() { return 1; } }",
            "package p; public class Child extends Base {}",
        );

        let methods = index.find_projected_effective_methods_by_name("p.Child", &[], "seed");
        let [method] = methods.as_slice() else {
            panic!("one visible inherited seed method expected: {methods:#?}")
        };
        assert!(
            index
                .get_file_path(method.definition.location.file_id)
                .is_some_and(|path| path.to_string_lossy().contains("z-visible")),
            "inherited method must come from visible Base"
        );
    }

    #[test]
    fn scoped_field_projection_uses_visible_duplicate_owner() {
        let (_temp, index) = scoped_duplicate_sources(
            "package p; public class Base<T> { public boolean value; }",
            "package p; public class Base<T> { public T value; }",
            "package p; public class Child extends Base<Text> {}",
        );

        let field = index
            .find_projected_effective_field_by_name("p.Child", &[], "value")
            .expect("visible inherited field");
        assert_eq!(field.projected_type.as_deref(), Some("p.Text"));
        assert!(
            index
                .get_file_path(field.definition.location.file_id)
                .is_some_and(|path| path.to_string_lossy().contains("z-visible")),
            "projected field must come from visible Base"
        );
    }

    #[test]
    fn scoped_type_name_ignores_excluded_same_package_type() {
        let temp = TempDir::new().unwrap();
        let shadow_root = temp.path().join("a-shadow/src/main/java");
        let visible_root = temp.path().join("z-visible/src/main/java");
        std::fs::create_dir_all(shadow_root.join("client")).unwrap();
        std::fs::create_dir_all(visible_root.join("api")).unwrap();
        std::fs::create_dir_all(visible_root.join("client")).unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            &shadow_root.join("client"),
            "Factory.java",
            "package client; public class Factory {}",
        );
        index_java(
            &index,
            &visible_root.join("api"),
            "Factory.java",
            "package api; public class Factory {}",
        );
        index_java(
            &index,
            &visible_root.join("client"),
            "Use.java",
            "package client; import api.*; class Use { Factory value; }",
        );

        let file_id = index
            .get_file_id(&visible_root.join("client/Use.java"))
            .expect("visible client indexed");
        let scoped = index.with_source_scope(super::SourceScope::new([visible_root]));
        assert_eq!(
            scoped.resolve_type_name(file_id, &[], "Factory"),
            TypeNameResolution::Exact("api.Factory".to_string())
        );
    }

    fn external_type(
        fqn: &str,
        parent: Option<&str>,
        type_parameters: Vec<crate::TypeParameterInfo>,
    ) -> crate::Symbol {
        crate::Symbol {
            fqn: fqn.into(),
            simple_name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
            range: crate::TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: crate::Modifiers::default(),
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: parent.map(Into::into),
            interfaces: Vec::new(),
            documentation: None,
            type_parameters,
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        }
    }

    #[test]
    fn composes_fixed_reordered_and_nested_source_edges() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Base.java",
            "package p; class Base<A, B> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Middle.java",
            "package p; class Middle<X, Y> extends Base<java.util.List<Y>, X> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Child.java",
            "package p; class Child extends Middle<String, Integer> {}",
        );

        assert_eq!(
            index.project_type_from_receiver("p.Child", &[], "p.Base", "A"),
            Some("java.util.List<Integer>".to_string())
        );
        assert_eq!(
            index.project_type_from_receiver("p.Child", &[], "p.Base", "B[]"),
            Some("String[]".to_string())
        );
    }

    #[test]
    fn raw_source_edge_only_blocks_types_that_need_unknown_arguments() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Base.java",
            "package p; class Base<T> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Raw.java",
            "package p; class Raw extends Base {}",
        );

        assert_eq!(
            index.project_type_from_receiver("p.Raw", &[], "p.Base", "String"),
            Some("String".to_string())
        );
        assert_eq!(
            index.project_type_from_receiver("p.Raw", &[], "p.Base", "T"),
            None
        );
    }

    #[test]
    fn resolves_source_edge_arguments_in_the_declaring_file_context() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Base.java",
            "package base; class Base<T> {}",
        );
        index_java(
            &index,
            temp.path(),
            "AlphaString.java",
            "package alpha; public class String {}",
        );
        index_java(
            &index,
            temp.path(),
            "BetaString.java",
            "package beta; public class String {}",
        );
        index_java(
            &index,
            temp.path(),
            "Child.java",
            "package child; import base.Base; import beta.String; class Child extends Base<java.util.List<? extends String[]>> {}",
        );

        assert_eq!(
            index.project_type_from_receiver("child.Child", &[], "base.Base", "T"),
            Some("java.util.List<? extends beta.String[]>".to_string())
        );
        assert_eq!(
            index.project_type_from_receiver("child.Child", &[], "base.Base", "other.T"),
            Some("other.T".to_string()),
            "a type variable must not rewrite the last segment of a qualified nominal type"
        );
    }

    #[test]
    fn ambiguous_source_edge_argument_fails_closed() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Base.java",
            "package base; class Base<T> {}",
        );
        index_java(
            &index,
            temp.path(),
            "AlphaString.java",
            "package alpha; public class String {}",
        );
        index_java(
            &index,
            temp.path(),
            "BetaString.java",
            "package beta; public class String {}",
        );
        index_java(
            &index,
            temp.path(),
            "Child.java",
            "package child; import base.Base; import alpha.*; import beta.*; class Child extends Base<String> {}",
        );

        assert_eq!(
            index.project_type_from_receiver("child.Child", &[], "base.Base", "T"),
            None
        );
    }

    #[test]
    fn malformed_arity_and_conflicting_diamonds_fail_closed() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Root.java",
            "package p; interface Root<T> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Left.java",
            "package p; interface Left extends Root<String> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Right.java",
            "package p; interface Right extends Root<Integer> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Conflict.java",
            "package p; interface Conflict extends Left, Right {}",
        );
        assert_eq!(
            index.project_type_from_receiver("p.Conflict", &[], "p.Root", "T"),
            None
        );

        let malformed_path = temp.path().join("Malformed.java");
        std::fs::write(
            &malformed_path,
            "package p; class Malformed implements Root<String> {}",
        )
        .unwrap();
        index.index_file_sync_for_test(&malformed_path).unwrap();
        let file_id = index.get_file_id(&malformed_path).unwrap();
        index.inner.write().replace_file_generic_supertypes(
            file_id,
            vec![crate::TypeGenericSupertypes {
                type_fqn: "p.Malformed".to_string(),
                edges: vec![crate::GenericSupertype {
                    written_target: "Root".to_string(),
                    arguments: vec!["String".to_string(), "Integer".to_string()],
                }],
            }],
        );
        assert_eq!(
            index.project_type_from_receiver("p.Malformed", &[], "p.Root", "T"),
            None
        );

        index_java(
            &index,
            temp.path(),
            "Base.java",
            "package p; class Base<Q> {} class Good extends Base<String> {}",
        );
        index_java(
            &index,
            temp.path(),
            "Noise.java",
            "package p; interface Noise<N> {} interface Bad extends Noise<Integer> {} class Receiver extends Good implements Bad {}",
        );
        let noise_file = index.get_file_id(&temp.path().join("Noise.java")).unwrap();
        index.inner.write().replace_file_generic_supertypes(
            noise_file,
            vec![crate::TypeGenericSupertypes {
                type_fqn: "p.Bad".to_string(),
                edges: vec![crate::GenericSupertype {
                    written_target: "Noise".to_string(),
                    arguments: vec!["Integer".to_string(), "String".to_string()],
                }],
            }],
        );
        assert_eq!(
            index.project_type_from_receiver("p.Receiver", &[], "p.Base", "Q"),
            Some("String".to_string()),
            "malformed metadata on an unrelated interface path must not poison the superclass"
        );
    }

    #[test]
    fn static_nested_types_and_interfaces_do_not_capture_outer_parameters() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Types.java",
            concat!(
                "package p; class Outer<T> { ",
                "static class Static<U> {} interface Contract<V> {} class Inner<W> {} } ",
                "interface InterfaceOuter<X> { class Nested<Y> {} } ",
                "class StaticChild extends Outer.Static<String> {} ",
                "class ContractChild implements Outer.Contract<Integer> {} ",
                "class InterfaceNestedChild extends InterfaceOuter.Nested<String> {} ",
                "class InnerChild extends Outer<String>.Inner<Integer> {}"
            ),
        );

        let static_bindings = index
            .project_owner_type_arguments("p.StaticChild", &[], "p.Outer.Static")
            .unwrap();
        assert_eq!(static_bindings.len(), 1);
        assert_eq!(static_bindings[0].0.name.as_ref(), "U");
        assert_eq!(static_bindings[0].1.as_deref(), Some("String"));
        let interface_bindings = index
            .project_owner_type_arguments("p.ContractChild", &[], "p.Outer.Contract")
            .unwrap();
        assert_eq!(interface_bindings.len(), 1);
        assert_eq!(interface_bindings[0].0.name.as_ref(), "V");
        assert_eq!(interface_bindings[0].1.as_deref(), Some("Integer"));
        let interface_nested_bindings = index
            .project_owner_type_arguments("p.InterfaceNestedChild", &[], "p.InterfaceOuter.Nested")
            .unwrap();
        assert_eq!(interface_nested_bindings.len(), 1);
        assert_eq!(interface_nested_bindings[0].0.name.as_ref(), "Y");
        assert_eq!(interface_nested_bindings[0].1.as_deref(), Some("String"));
        let inner_bindings = index
            .project_owner_type_arguments("p.InnerChild", &[], "p.Outer.Inner")
            .unwrap();
        assert_eq!(
            inner_bindings
                .iter()
                .map(|(parameter, argument)| (
                    parameter.name.as_ref(),
                    argument.as_deref().unwrap()
                ))
                .collect::<Vec<_>>(),
            [("T", "String"), ("W", "Integer")]
        );
    }

    #[test]
    fn duplicate_type_uses_metadata_from_the_elected_declaration() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_java(
            &index,
            temp.path(),
            "Base.java",
            "package p; class Base<T> {}",
        );
        let first = temp.path().join("First.java");
        let second = temp.path().join("Second.java");
        std::fs::write(&first, "package p; class Duplicate extends Base<String> {}").unwrap();
        std::fs::write(
            &second,
            "package p; class Duplicate extends Base<Integer> {}",
        )
        .unwrap();
        index.index_file_sync_for_test(&first).unwrap();
        index.index_file_sync_for_test(&second).unwrap();

        let first_id = index.get_file_id(&first).unwrap();
        let selected_id = index.inner.read().symbol_locations["p.Duplicate"].file_id;
        let expected = if selected_id == first_id {
            "String"
        } else {
            "Integer"
        };
        assert_eq!(
            index.project_type_from_receiver("p.Duplicate", &[], "p.Base", "T"),
            Some(expected.to_string())
        );
    }

    #[test]
    fn composes_external_class_signature_edges() {
        let temp = TempDir::new().unwrap();
        let source_dir = temp.path().join("src/ext");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let source = source_dir.join("Child.java");
        std::fs::write(
            &source,
            r#"
                package ext;
                class Base<A, B> {
                    public A convert(B value) { return null; }
                }
                class Middle<X> extends Base<java.util.List<X>, X> {}
                class Outer<T> {
                    class Inner<U> {}
                    static class Static<V> {}
                    interface Contract<W> {}
                }
                class NestedChild extends Outer<String>.Inner<Integer> {
                    NestedChild(Outer<String> outer) { outer.super(); }
                }
                class StaticChild extends Outer.Static<Long> {}
                class ContractChild implements Outer.Contract<Double> {}
                interface InterfaceOuter<X> { class Nested<Y> {} }
                class InterfaceNestedChild extends InterfaceOuter.Nested<Float> {}
                public class Child extends Middle<String> {
                    @Override public java.util.List<String> convert(String value) { return null; }
                }
            "#,
        )
        .unwrap();
        let output = Command::new("javac")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&source)
            .output()
            .expect("run javac for generic hierarchy fixture");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let jar_path = temp.path().join("generic-hierarchy.jar");
        let file = std::fs::File::create(&jar_path).unwrap();
        let mut jar = zip::ZipWriter::new(file);
        for class in [
            "Base",
            "Middle",
            "Outer",
            "Outer$Inner",
            "Outer$Static",
            "Outer$Contract",
            "NestedChild",
            "StaticChild",
            "ContractChild",
            "InterfaceOuter",
            "InterfaceOuter$Nested",
            "InterfaceNestedChild",
            "Child",
        ] {
            jar.start_file(
                format!("ext/{class}.class"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            jar.write_all(&std::fs::read(classes_dir.join(format!("ext/{class}.class"))).unwrap())
                .unwrap();
        }
        jar.finish().unwrap();

        let index = WorkspaceIndex::new(temp.path().join("workspace"));
        index.index_jar_file(&jar_path).unwrap();
        assert_eq!(
            index.project_type_from_receiver("ext.Child", &[], "ext.Base", "A"),
            Some("java.util.List<java.lang.String>".to_string())
        );
        assert_eq!(
            index.project_type_from_receiver("ext.Child", &[], "ext.Base", "B"),
            Some("java.lang.String".to_string())
        );
        let methods = index.find_effective_methods_by_name("ext.Child", "convert");
        assert_eq!(
            methods.len(),
            1,
            "a dependency bridge is filtered, so generic projection must still elect the source override: {methods:#?}"
        );
        assert_eq!(methods[0].container.as_deref(), Some("ext.Child"));
        assert_eq!(
            index.project_type_from_receiver("ext.NestedChild", &[], "ext.Outer.Inner", "U"),
            Some("java.lang.Integer".to_string())
        );
        assert_eq!(
            index.project_type_from_receiver("ext.NestedChild", &[], "ext.Outer.Inner", "T"),
            Some("java.lang.String".to_string())
        );
        let static_bindings = index
            .project_owner_type_arguments("ext.StaticChild", &[], "ext.Outer.Static")
            .unwrap();
        assert_eq!(static_bindings.len(), 1);
        assert_eq!(static_bindings[0].0.name.as_ref(), "V");
        assert_eq!(static_bindings[0].1.as_deref(), Some("java.lang.Long"));
        let interface_bindings = index
            .project_owner_type_arguments("ext.ContractChild", &[], "ext.Outer.Contract")
            .unwrap();
        assert_eq!(interface_bindings.len(), 1);
        assert_eq!(interface_bindings[0].0.name.as_ref(), "W");
        assert_eq!(interface_bindings[0].1.as_deref(), Some("java.lang.Double"));
        let interface_nested_bindings = index
            .project_owner_type_arguments(
                "ext.InterfaceNestedChild",
                &[],
                "ext.InterfaceOuter.Nested",
            )
            .unwrap();
        assert_eq!(interface_nested_bindings.len(), 1);
        assert_eq!(interface_nested_bindings[0].0.name.as_ref(), "Y");
        assert_eq!(
            interface_nested_bindings[0].1.as_deref(),
            Some("java.lang.Float")
        );
    }

    #[test]
    fn superseding_chunked_external_publication_retracts_generic_metadata() {
        let index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
        let archive = std::path::PathBuf::from("/deps/generic-partial.jar");
        let refresh = index.replace_external_sources_revisioned(
            "generic-test:/workspace",
            std::slice::from_ref(&archive),
        );
        let revision = refresh.revision_for(&archive).unwrap();

        let mut symbols = vec![external_type(
            "dep.GenericBase",
            None,
            vec![crate::TypeParameterInfo::new("T", 0)],
        )];
        symbols.extend((0..150).map(|number| {
            external_type(
                &format!("dep.Partial{number:03}"),
                Some("dep.GenericBase"),
                Vec::new(),
            )
        }));
        let metadata = vec![crate::TypeGenericSupertypes {
            type_fqn: "dep.Partial000".to_string(),
            edges: vec![crate::GenericSupertype {
                written_target: "dep.GenericBase".to_string(),
                arguments: vec!["java.lang.String".to_string()],
            }],
        }];
        let first_chunk_visible = Arc::new(std::sync::Barrier::new(2));
        let resume_old = Arc::new(std::sync::Barrier::new(2));
        let old_index = Arc::clone(&index);
        let old_visible = Arc::clone(&first_chunk_visible);
        let old_resume = Arc::clone(&resume_old);
        let publication = std::thread::spawn(move || {
            old_index.insert_symbols_and_generic_supertypes_after_first_chunk_for_test(
                symbols,
                metadata,
                revision,
                || {
                    old_visible.wait();
                    old_resume.wait();
                },
            )
        });

        first_chunk_visible.wait();
        assert!(index
            .inner
            .read()
            .file_generic_supertypes
            .contains_key(&revision.file_id()));

        let replacement = index.replace_external_sources_revisioned(
            "generic-test:/workspace",
            std::slice::from_ref(&archive),
        );
        assert_ne!(replacement.revision_for(&archive).unwrap(), revision);
        assert!(!index
            .inner
            .read()
            .file_generic_supertypes
            .contains_key(&revision.file_id()));

        resume_old.wait();
        assert!(!publication.join().unwrap());
        assert!(!index
            .inner
            .read()
            .file_generic_supertypes
            .contains_key(&revision.file_id()));
    }
}
