// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Exact callable-family and structural dependency proofs for refactorings.

use super::generic_supertypes::{binding, effective_type_parameters, owner_type_bindings_in};
use super::query::{
    btree_prefix_scan, find_symbol_definition_coherent_in, find_symbol_definition_in,
    selected_owner_member_definitions_shared_in,
};
use super::*;
use std::collections::{HashSet, VecDeque};

#[derive(Debug)]
pub(super) struct CallableOverload {
    pub(super) fqn: Arc<str>,
    pub(super) parameter_count: usize,
    pub(super) variadic: bool,
}

impl CallableOverload {
    fn for_symbol(symbol: &crate::Symbol) -> Self {
        let variadic = symbol.parameters.last().is_some_and(|parameter| {
            let parameter_type = parameter.split_whitespace().next().unwrap_or("");
            parameter_type.contains("...")
        });
        Self {
            fqn: Arc::clone(&symbol.fqn),
            parameter_count: symbol.parameters.len(),
            variadic,
        }
    }

    pub(super) fn accepts_arity(&self, arity: usize) -> bool {
        arity == self.parameter_count
            || (self.variadic && arity >= self.parameter_count.saturating_sub(1))
    }
}

pub(super) fn method_definitions_by_name_in(
    inner: &IndexInner,
    owner_fqn: &str,
    method_name: &str,
) -> Vec<ResolvedDefinition> {
    if let Some(definitions) = selected_owner_member_definitions_shared_in(inner, owner_fqn) {
        return definitions
            .iter()
            .filter(|definition| {
                definition.target.namespace == crate::DeclarationNamespace::Callable
                    && definition.symbol.simple_name.as_ref() == method_name
            })
            .cloned()
            .collect();
    }

    // JVM languages may publish package-owned top-level callables without a
    // nominal type owner. Retain source-precedence election for that shape.
    let method_prefix = format!("{owner_fqn}.{method_name}(");
    let candidate_fqns: Vec<_> = inner
        .owned_declarations_index
        .get(owner_fqn)
        .map(|members| {
            members
                .iter()
                .filter(|fqn| fqn.starts_with(&method_prefix))
                .cloned()
                .collect()
        })
        .unwrap_or_else(|| {
            btree_prefix_scan(&inner.symbol_locations, &method_prefix)
                .map(|(fqn, _)| Arc::clone(fqn))
                .collect()
        });
    candidate_fqns
        .into_iter()
        .filter_map(|fqn| {
            find_symbol_definition_in(
                inner,
                &crate::SymbolTarget::new(fqn, crate::DeclarationNamespace::Callable),
            )
        })
        .filter(|definition| {
            definition.symbol.container.as_deref() == Some(owner_fqn)
                && definition.symbol.simple_name.as_ref() == method_name
        })
        .collect()
}
pub(super) fn callable_overloads_for_target_in(
    inner: &IndexInner,
    fqn: &str,
) -> Vec<CallableOverload> {
    let descriptorless = fqn.split_once('(').map_or(fqn, |(base, _)| base);
    let Some((owner_fqn, method_name)) = descriptorless.rsplit_once('.') else {
        return Vec::new();
    };
    method_definitions_by_name_in(inner, owner_fqn, method_name)
        .into_iter()
        .map(|definition| CallableOverload::for_symbol(&definition.symbol))
        .collect()
}

pub(super) fn inherited_method_reference_aliases_in(
    inner: &IndexInner,
    fqn: &str,
) -> Vec<Arc<str>> {
    let Some(definition) = find_symbol_definition_coherent_in(
        inner,
        &crate::SymbolTarget::new(fqn, crate::DeclarationNamespace::Callable),
    ) else {
        return Vec::new();
    };
    if !method_is_virtual(&definition.symbol) {
        return Vec::new();
    }
    let (Some(owner), method_name) = (
        definition.symbol.container.as_deref(),
        definition.symbol.simple_name.as_ref(),
    ) else {
        return Vec::new();
    };
    let mut pending = VecDeque::from(inner.direct_subtypes(owner));
    let mut visited = HashSet::new();
    let mut aliases = Vec::new();
    while let Some(subtype) = pending.pop_front() {
        if !visited.insert(Arc::clone(&subtype)) {
            continue;
        }
        aliases.push(Arc::from(format!("{subtype}.{method_name}")));
        pending.extend(inner.direct_subtypes(&subtype));
    }
    aliases.sort_unstable();
    aliases.dedup();
    aliases
}

fn callable_owner_hierarchies_are_complete_in<'a, F, I>(
    inner: &IndexInner,
    owners: I,
    is_cancelled: &F,
) -> std::result::Result<bool, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
    I: IntoIterator<Item = &'a str>,
{
    let owners = owners.into_iter().map(Arc::<str>::from).collect::<Vec<_>>();

    let exact_supertypes = |type_fqn: &str| -> Option<Vec<String>> {
        let type_symbol = inner.symbols.get(type_fqn)?;
        let implicit_root_is_indexed = inner
            .implicit_exact_supertype(type_symbol)
            .is_none_or(|root| inner.indexed_type(root));
        let written_edges_are_exact = type_symbol
            .parent_class
            .iter()
            .chain(type_symbol.interfaces.iter())
            .all(|recorded| {
                matches!(
                    inner.resolve_recorded_supertype(type_fqn, recorded),
                    crate::TypeNameResolution::Exact(_)
                )
            });
        (implicit_root_is_indexed && written_edges_are_exact)
            .then(|| direct_supertypes_in(inner, type_fqn))
    };

    // A family can be disconnected by an unresolved edge above an
    // intermediate non-declaring type. Walk the complete ancestor closure of
    // every elected family owner, not just the owners that happened to make it
    // into the family.
    let mut pending = VecDeque::from(owners.clone());
    let mut visited_ancestors = HashSet::new();
    while let Some(type_fqn) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !visited_ancestors.insert(Arc::clone(&type_fqn)) {
            continue;
        }
        let Some(supertypes) = exact_supertypes(&type_fqn) else {
            return Ok(false);
        };
        pending.extend(supertypes.into_iter().map(Arc::<str>::from));
    }

    // Descendants are a separate traversal: walking down from ancestors would
    // drag unrelated sibling hierarchies into the proof, but every descendant
    // of an actual family owner can contain (or inherit through) an override.
    let mut pending = VecDeque::from(owners);
    let mut visited_descendants = HashSet::new();
    while let Some(type_fqn) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !visited_descendants.insert(Arc::clone(&type_fqn)) {
            continue;
        }
        if exact_supertypes(&type_fqn).is_none()
            || !inner.direct_subtype_resolution_is_complete(&type_fqn)
        {
            return Ok(false);
        }
        pending.extend(inner.direct_subtypes(&type_fqn));
    }
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CallableOverrideSignature {
    name: Arc<str>,
    declared_parameters: Vec<String>,
    erased_parameters: Vec<String>,
}

pub(super) fn split_callable_descriptor(fqn: &str) -> Option<Vec<String>> {
    let open = fqn.find('(')?;
    let parameters = fqn.get(open + 1..)?.strip_suffix(')')?;
    if parameters.is_empty() {
        return Some(Vec::new());
    }

    let mut result = Vec::new();
    let mut start = 0;
    let mut generic_depth = 0_u32;
    for (offset, character) in parameters.char_indices() {
        match character {
            '<' => generic_depth = generic_depth.checked_add(1)?,
            '>' => generic_depth = generic_depth.checked_sub(1)?,
            ',' if generic_depth == 0 => {
                result.push(parameters.get(start..offset)?.trim().to_owned());
                start = offset + character.len_utf8();
            }
            _ => {}
        }
    }
    if generic_depth != 0 {
        return None;
    }
    result.push(parameters.get(start..)?.trim().to_owned());
    Some(result)
}

pub(super) fn erase_descriptor_type_arguments(parameter: &str) -> Option<String> {
    let mut erased = String::with_capacity(parameter.len());
    let mut depth = 0_u32;
    for character in parameter.trim().chars() {
        match character {
            '<' => depth = depth.checked_add(1)?,
            '>' => depth = depth.checked_sub(1)?,
            _ if depth == 0 => erased.push(character),
            _ => {}
        }
    }
    (depth == 0).then_some(erased)
}

pub(super) fn normalize_override_parameter(parameter: &str) -> Option<String> {
    let parameter = parameter.trim().strip_prefix("vararg ").map_or_else(
        || parameter.trim().to_owned(),
        |parameter| format!("{}...", parameter.trim()),
    );
    let mut normalized = parameter
        .trim_end_matches('?')
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if normalized.ends_with("...") {
        normalized.truncate(normalized.len() - 3);
        normalized.push_str("[]");
    }
    Some(normalized)
}

fn declared_parameter_type(parameter: &str) -> &str {
    let parameter = parameter
        .trim()
        .strip_prefix("vararg ")
        .unwrap_or(parameter);
    parameter
        .rsplit_once(char::is_whitespace)
        .map_or(parameter, |(parameter_type, _)| parameter_type)
}

fn mentions_declared_type_parameter(parameter: &str, declared: &HashSet<&str>) -> bool {
    parameter
        .split(|character: char| !(character.is_alphanumeric() || matches!(character, '_' | '$')))
        .any(|component| !component.is_empty() && declared.contains(component))
}

fn callable_override_signature_in(
    inner: &IndexInner,
    symbol: &crate::Symbol,
) -> Option<CallableOverrideSignature> {
    let descriptor_parameters = split_callable_descriptor(&symbol.fqn)?;
    let owner_type_parameters = symbol
        .container
        .as_deref()
        .and_then(|owner| inner.symbols.get(owner))
        .map(|owner| owner.type_parameters.as_slice())
        .unwrap_or_default();

    let declared_type_parameters = symbol
        .type_parameters
        .iter()
        .chain(owner_type_parameters)
        .map(|type_parameter| type_parameter.name.as_ref())
        .collect::<HashSet<_>>();
    let has_unsubstituted_type_parameter =
        descriptor_parameters.iter().any(|parameter| {
            mentions_declared_type_parameter(parameter, &declared_type_parameters)
        }) || symbol.parameters.iter().any(|parameter| {
            mentions_declared_type_parameter(
                declared_parameter_type(parameter),
                &declared_type_parameters,
            )
        }) || symbol.return_type.as_deref().is_some_and(|return_type| {
            mentions_declared_type_parameter(return_type, &declared_type_parameters)
        });
    if has_unsubstituted_type_parameter {
        return None;
    }

    let descriptor_parameters = descriptor_parameters
        .into_iter()
        .map(|parameter| normalize_override_parameter(&parameter))
        .collect::<Option<Vec<_>>>()?;
    let declared_parameters = if symbol.parameters.len() == descriptor_parameters.len() {
        symbol
            .parameters
            .iter()
            .zip(&descriptor_parameters)
            .map(|(parameter, erased_descriptor)| {
                let written = normalize_override_parameter(declared_parameter_type(parameter))?;
                if !written.contains('<') {
                    // The callable descriptor is the canonical source of raw
                    // parameter identity. Bytecode symbols deliberately keep
                    // simple display types, whereas source symbols usually
                    // retain qualification.
                    return Some(erased_descriptor.clone());
                }
                // Generic source shape is needed for directional Java
                // subsignatures, but it is useful only when its erased owner
                // agrees with the canonical callable descriptor.
                (erase_descriptor_type_arguments(&written)?.as_str() == erased_descriptor)
                    .then_some(written)
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        descriptor_parameters
    };
    let erased_parameters = declared_parameters
        .iter()
        .map(|parameter| erase_descriptor_type_arguments(parameter))
        .collect::<Option<Vec<_>>>()?;

    Some(CallableOverrideSignature {
        name: Arc::clone(&symbol.simple_name),
        declared_parameters,
        erased_parameters,
    })
}

pub(super) fn exact_erased_callable_signature_in(
    inner: &IndexInner,
    symbol: &crate::Symbol,
) -> Option<(Arc<str>, Vec<String>)> {
    callable_override_signature_in(inner, symbol)
        .map(|signature| (signature.name, signature.erased_parameters))
}

fn signature_is_subsignature(
    child: &CallableOverrideSignature,
    parent: &CallableOverrideSignature,
) -> bool {
    child.name == parent.name
        && (child.declared_parameters == parent.declared_parameters
            || child.declared_parameters == parent.erased_parameters)
}

fn signatures_are_override_related(
    left: &CallableOverrideSignature,
    right: &CallableOverrideSignature,
) -> bool {
    signature_is_subsignature(left, right) || signature_is_subsignature(right, left)
}

pub(super) fn type_package_in(inner: &IndexInner, type_fqn: &str) -> String {
    let mut top_level = type_fqn;
    while let Some(container) = inner
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

pub(super) fn owner_is_interface(inner: &IndexInner, owner: &str) -> bool {
    inner
        .symbols
        .get(owner)
        .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE)
}

pub(super) fn method_is_virtual(symbol: &crate::Symbol) -> bool {
    matches!(symbol.kind, SymbolKind::METHOD | SymbolKind::FUNCTION)
        && !symbol.modifiers.is_private
        && !symbol.modifiers.is_static
}

pub(super) fn method_visibility_rank(inner: &IndexInner, symbol: &crate::Symbol) -> u8 {
    if symbol.modifiers.is_public
        || symbol
            .container
            .as_deref()
            .is_some_and(|owner| owner_is_interface(inner, owner))
    {
        3
    } else if symbol.modifiers.is_protected {
        2
    } else if symbol.modifiers.is_internal {
        1
    } else if symbol.modifiers.is_private {
        0
    } else {
        1
    }
}

fn is_primitive_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "boolean" | "byte" | "short" | "char" | "int" | "long" | "float" | "double" | "void"
    )
}

fn split_parameterized_type(type_name: &str) -> Option<(&str, Vec<&str>)> {
    let open = type_name.find('<')?;
    let arguments = type_name.get(open + 1..)?.strip_suffix('>')?;
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0_u32;
    for (offset, character) in arguments.char_indices() {
        match character {
            '<' => depth = depth.checked_add(1)?,
            '>' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                result.push(arguments.get(start..offset)?);
                start = offset + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    result.push(arguments.get(start..)?);
    Some((type_name.get(..open)?, result))
}

pub(super) fn nominal_type_is_covariant_in(inner: &IndexInner, child: &str, parent: &str) -> bool {
    if child == parent {
        return true;
    }
    let mut pending = direct_supertypes_in(inner, child);
    let mut seen = HashSet::new();
    while let Some(candidate) = pending.pop() {
        if candidate == parent {
            return true;
        }
        if seen.insert(candidate.clone()) {
            pending.extend(direct_supertypes_in(inner, &candidate));
        }
    }
    false
}

fn generic_argument_is_contained_in(inner: &IndexInner, child: &str, parent: &str) -> Option<bool> {
    if child == parent || parent == "?" {
        return Some(true);
    }
    if let Some(parent_bound) = parent.strip_prefix("?extends") {
        if let Some(child_bound) = child.strip_prefix("?extends") {
            return return_type_is_covariant_in(inner, child_bound, parent_bound);
        }
        return if child.starts_with('?') {
            Some(false)
        } else {
            return_type_is_covariant_in(inner, child, parent_bound)
        };
    }
    if let Some(parent_bound) = parent.strip_prefix("?super") {
        if let Some(child_bound) = child.strip_prefix("?super") {
            return return_type_is_covariant_in(inner, parent_bound, child_bound);
        }
        return if child.starts_with('?') {
            Some(false)
        } else {
            return_type_is_covariant_in(inner, parent_bound, child)
        };
    }
    Some(false)
}

/// `None` means the erased hierarchy proves a nominal relationship but lacks
/// the generic-supertype substitution needed to decide compatibility.
pub(super) fn return_type_is_covariant_in(
    inner: &IndexInner,
    child: &str,
    parent: &str,
) -> Option<bool> {
    let normalized_child = child
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let normalized_parent = parent
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let (child, parent) = (normalized_child.as_str(), normalized_parent.as_str());
    if child == parent {
        return Some(true);
    }
    if is_primitive_type(child) || is_primitive_type(parent) {
        return Some(false);
    }
    if matches!(parent, "Object" | "java.lang.Object") {
        return Some(true);
    }

    let child_component = child.strip_suffix("[]");
    let parent_component = parent.strip_suffix("[]");
    match (child_component, parent_component) {
        (Some(child_component), Some(parent_component)) => {
            return return_type_is_covariant_in(inner, child_component, parent_component);
        }
        (Some(_), None)
            if matches!(
                parent,
                "Cloneable" | "java.lang.Cloneable" | "Serializable" | "java.io.Serializable"
            ) =>
        {
            return Some(true);
        }
        (Some(_), None) | (None, Some(_)) => return Some(false),
        (None, None) => {}
    }

    let child_parameterized = split_parameterized_type(child);
    let parent_parameterized = split_parameterized_type(parent);
    let child_raw = child_parameterized.as_ref().map_or(child, |(raw, _)| *raw);
    let parent_raw = parent_parameterized
        .as_ref()
        .map_or(parent, |(raw, _)| *raw);
    if !nominal_type_is_covariant_in(inner, child_raw, parent_raw) {
        return Some(false);
    }
    match (child_parameterized, parent_parameterized) {
        (Some((_, child_arguments)), Some((_, parent_arguments))) => {
            if child_raw != parent_raw {
                let supplied_arguments = child_arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect::<Vec<_>>();
                let bindings =
                    owner_type_bindings_in(inner, child_raw, &supplied_arguments, parent_raw)?;
                let effective_parameters = effective_type_parameters(inner, parent_raw)?;
                let own_parameter_count = inner.symbols.get(parent_raw)?.type_parameters.len();
                let compared_parameters = if parent_arguments.len() == effective_parameters.len() {
                    effective_parameters.as_slice()
                } else if parent_arguments.len() == own_parameter_count {
                    effective_parameters.get(
                        effective_parameters
                            .len()
                            .checked_sub(own_parameter_count)?..,
                    )?
                } else {
                    return Some(false);
                };
                for (parameter, parent_argument) in compared_parameters.iter().zip(parent_arguments)
                {
                    if let Some(projected) = binding(&bindings, parameter.name.as_ref())?.as_deref()
                    {
                        if !generic_argument_is_contained_in(inner, projected, parent_argument)? {
                            return Some(false);
                        }
                    }
                }
                return Some(true);
            }
            if child_arguments.len() != parent_arguments.len() {
                return Some(false);
            }
            for (child, parent) in child_arguments.iter().zip(parent_arguments) {
                if !generic_argument_is_contained_in(inner, child, parent)? {
                    return Some(false);
                }
            }
            Some(true)
        }
        // A raw child return is return-type-substitutable via Java's unchecked
        // conversion rule; a parameterized child is always a subtype of its
        // raw generic supertype.
        _ => Some(true),
    }
}

pub(super) fn override_return_is_compatible_in(
    inner: &IndexInner,
    child: &crate::Symbol,
    parent: &crate::Symbol,
) -> std::result::Result<bool, CurrentUsageQueryError> {
    let compatible = match (child.return_type.as_deref(), parent.return_type.as_deref()) {
        (Some(child), Some(parent)) => return_type_is_covariant_in(inner, child, parent),
        (None, None) => Some(true),
        _ => Some(false),
    };
    compatible.ok_or_else(|| {
        CurrentUsageQueryError::Unavailable(format!(
            "generic return substitution is unavailable between '{}' and '{}'",
            child.fqn, parent.fqn
        ))
    })
}

pub(super) fn owner_prevents_overrides(inner: &IndexInner, owner: &str) -> bool {
    inner
        .symbols
        .get(owner)
        .is_some_and(|symbol| symbol.modifiers.is_final)
}

fn legal_override_edge_in(
    inner: &IndexInner,
    child: &ResolvedDefinition,
    parent: &ResolvedDefinition,
) -> std::result::Result<bool, CurrentUsageQueryError> {
    let Some(parent_owner) = parent.symbol.container.as_deref() else {
        return Ok(false);
    };
    let structurally_legal = method_is_virtual(&child.symbol)
        && method_is_virtual(&parent.symbol)
        && !parent.symbol.modifiers.is_final
        && !owner_prevents_overrides(inner, parent_owner)
        && method_visibility_rank(inner, &child.symbol)
            >= method_visibility_rank(inner, &parent.symbol);
    if !structurally_legal {
        return Ok(false);
    }
    override_return_is_compatible_in(inner, &child.symbol, &parent.symbol)
}

fn declared_related_method_in(
    inner: &IndexInner,
    owner: &str,
    signature: &CallableOverrideSignature,
) -> std::result::Result<Option<ResolvedDefinition>, CurrentUsageQueryError> {
    let mut matched = None;
    for definition in method_definitions_by_name_in(inner, owner, &signature.name) {
        let Some(candidate) = callable_override_signature_in(inner, &definition.symbol) else {
            if definition.symbol.parameters.len() == signature.declared_parameters.len() {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "callable '{}' has no exact override descriptor",
                    definition.target.fqn
                )));
            }
            continue;
        };
        if !signatures_are_override_related(&candidate, signature) {
            continue;
        }
        if matched.is_some() {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "type '{owner}' has duplicate declarations for '{}({})'",
                signature.name,
                signature.declared_parameters.join(",")
            )));
        }
        matched = Some(definition);
    }
    Ok(matched)
}

fn method_is_inheritable_with_package_state(
    inner: &IndexInner,
    method: &crate::Symbol,
    declaring_owner: &str,
    uniform_descendant_package: Option<&str>,
) -> bool {
    if !method_is_virtual(method) {
        return false;
    }
    if inner
        .symbols
        .get(declaring_owner)
        .is_some_and(|owner| owner.kind == SymbolKind::INTERFACE)
        || method.modifiers.is_public
        || method.modifiers.is_protected
        || method.modifiers.is_internal
    {
        return true;
    }

    let language = inner
        .symbol_locations
        .get(declaring_owner)
        .and_then(|location| inner.file_path(location.file_id))
        .and_then(Language::from_path);
    if !matches!(language, Some(Language::Java) | None) {
        return true;
    }

    let package = type_package_in(inner, declaring_owner);
    uniform_descendant_package == Some(package.as_str())
}

fn direct_supertypes_in(inner: &IndexInner, type_fqn: &str) -> Vec<String> {
    let (parent, mut interfaces) = inner.resolved_supertype_roles(type_fqn);
    interfaces.extend(parent);
    interfaces.sort_unstable();
    interfaces.dedup();
    interfaces
}

pub(super) fn extend_uniform_package(
    inner: &IndexInner,
    uniform_package: Option<String>,
    next_type: &str,
) -> Option<String> {
    uniform_package.filter(|package| type_package_in(inner, next_type) == *package)
}

fn method_is_inheritable_downward(
    inner: &IndexInner,
    method: &crate::Symbol,
    declaring_owner: &str,
    same_declaring_package: bool,
) -> bool {
    method_is_inheritable_with_package_state(
        inner,
        method,
        declaring_owner,
        same_declaring_package
            .then(|| type_package_in(inner, declaring_owner))
            .as_deref(),
    )
}

fn inherited_related_declarations_at_type_in<F>(
    inner: &IndexInner,
    type_fqn: &str,
    reference_signature: &CallableOverrideSignature,
    is_cancelled: &F,
) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
{
    let type_package = type_package_in(inner, type_fqn);
    let mut pending = direct_supertypes_in(inner, type_fqn)
        .into_iter()
        .map(|supertype| (supertype, Some(type_package.clone())))
        .collect::<VecDeque<_>>();
    let mut visited = HashSet::new();
    let mut inherited = Vec::new();

    while let Some((supertype, uniform_package)) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !visited.insert((supertype.clone(), uniform_package.clone())) {
            continue;
        }
        if let Some(candidate) = declared_related_method_in(inner, &supertype, reference_signature)?
        {
            if method_is_inheritable_with_package_state(
                inner,
                &candidate.symbol,
                &supertype,
                uniform_package.as_deref(),
            ) {
                inherited.push(candidate);
                continue;
            }
        }

        let next_package = extend_uniform_package(inner, uniform_package, &supertype);
        for ancestor in direct_supertypes_in(inner, &supertype) {
            pending.push_back((ancestor, next_package.clone()));
        }
    }
    inherited.sort_unstable_by(|left, right| left.target.fqn.cmp(&right.target.fqn));
    inherited.dedup_by(|left, right| left.target == right.target);
    Ok(inherited)
}

fn declarations_share_dispatch_at_join_in(
    inner: &IndexInner,
    left: &ResolvedDefinition,
    right: &ResolvedDefinition,
) -> std::result::Result<bool, CurrentUsageQueryError> {
    let (Some(left_owner), Some(right_owner)) = (
        left.symbol.container.as_deref(),
        right.symbol.container.as_deref(),
    ) else {
        return Ok(false);
    };
    let (Some(left_signature), Some(right_signature)) = (
        callable_override_signature_in(inner, &left.symbol),
        callable_override_signature_in(inner, &right.symbol),
    ) else {
        return Ok(false);
    };
    let left_interface = owner_is_interface(inner, left_owner);
    let right_interface = owner_is_interface(inner, right_owner);

    let direction_is_compatible = |child_signature: &CallableOverrideSignature,
                                   child: &crate::Symbol,
                                   parent_signature: &CallableOverrideSignature,
                                   parent: &crate::Symbol| {
        if signature_is_subsignature(child_signature, parent_signature) {
            override_return_is_compatible_in(inner, child, parent)
        } else {
            Ok(false)
        }
    };

    match (left_interface, right_interface) {
        (true, true) => {
            let left_to_right = direction_is_compatible(
                &left_signature,
                &left.symbol,
                &right_signature,
                &right.symbol,
            );
            let right_to_left = direction_is_compatible(
                &right_signature,
                &right.symbol,
                &left_signature,
                &left.symbol,
            );
            match (left_to_right, right_to_left) {
                (Ok(true), _) | (_, Ok(true)) => Ok(true),
                (Err(error), _) | (_, Err(error)) => Err(error),
                _ => Ok(false),
            }
        }
        (false, true) => {
            if !left.symbol.modifiers.is_public {
                return Ok(false);
            }
            direction_is_compatible(
                &left_signature,
                &left.symbol,
                &right_signature,
                &right.symbol,
            )
        }
        (true, false) => {
            if !right.symbol.modifiers.is_public {
                return Ok(false);
            }
            direction_is_compatible(
                &right_signature,
                &right.symbol,
                &left_signature,
                &left.symbol,
            )
        }
        (false, false) => Ok(false),
    }
}

fn nearest_overridden_declarations_in<F>(
    inner: &IndexInner,
    declaration: &ResolvedDefinition,
    is_cancelled: &F,
) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
{
    if !method_is_virtual(&declaration.symbol) {
        return Ok(Vec::new());
    }
    let Some(owner) = declaration.symbol.container.as_deref() else {
        return Ok(Vec::new());
    };
    let signature =
        callable_override_signature_in(inner, &declaration.symbol).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "callable '{}' has no exact override descriptor",
                declaration.target.fqn
            ))
        })?;

    let mut pending = VecDeque::new();
    for supertype in direct_supertypes_in(inner, owner) {
        pending.push_back((supertype, Some(type_package_in(inner, owner))));
    }
    let mut visited = HashSet::new();
    let mut overridden = Vec::new();
    while let Some((supertype, uniform_package)) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !visited.insert((supertype.clone(), uniform_package.clone())) {
            continue;
        }
        if let Some(candidate) = declared_related_method_in(inner, &supertype, &signature)? {
            let Some(candidate_signature) =
                callable_override_signature_in(inner, &candidate.symbol)
            else {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "callable '{}' has no exact override descriptor",
                    candidate.target.fqn
                )));
            };
            if signature_is_subsignature(&signature, &candidate_signature)
                && method_is_inheritable_with_package_state(
                    inner,
                    &candidate.symbol,
                    &supertype,
                    uniform_package.as_deref(),
                )
                && legal_override_edge_in(inner, declaration, &candidate)?
            {
                overridden.push(candidate);
            }
            continue;
        }

        let next_package = extend_uniform_package(inner, uniform_package, &supertype);
        for ancestor in direct_supertypes_in(inner, &supertype) {
            pending.push_back((ancestor, next_package.clone()));
        }
    }
    Ok(overridden)
}

fn nearest_overriding_declarations_in<F>(
    inner: &IndexInner,
    declaration: &ResolvedDefinition,
    is_cancelled: &F,
) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
{
    if !method_is_virtual(&declaration.symbol) {
        return Ok(Vec::new());
    }
    let Some(owner) = declaration.symbol.container.as_deref() else {
        return Ok(Vec::new());
    };
    if owner_prevents_overrides(inner, owner) {
        return Ok(Vec::new());
    }
    let signature =
        callable_override_signature_in(inner, &declaration.symbol).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "callable '{}' has no exact override descriptor",
                declaration.target.fqn
            ))
        })?;

    let mut pending = VecDeque::new();
    for subtype in inner.direct_subtypes(owner) {
        let subtype = subtype.to_string();
        pending.push_back((
            subtype.clone(),
            type_package_in(inner, &subtype) == type_package_in(inner, owner),
        ));
    }
    let mut visited = HashSet::new();
    let mut overriding = Vec::new();
    while let Some((subtype, same_declaring_package)) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !visited.insert((subtype.clone(), same_declaring_package))
            || !method_is_inheritable_downward(
                inner,
                &declaration.symbol,
                owner,
                same_declaring_package,
            )
        {
            continue;
        }
        if let Some(candidate) = declared_related_method_in(inner, &subtype, &signature)? {
            let Some(candidate_signature) =
                callable_override_signature_in(inner, &candidate.symbol)
            else {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "callable '{}' has no exact override descriptor",
                    candidate.target.fqn
                )));
            };
            if !declaration.symbol.modifiers.is_final
                && signature_is_subsignature(&candidate_signature, &signature)
                && legal_override_edge_in(inner, &candidate, declaration)?
            {
                overriding.push(candidate);
            }
            continue;
        }

        for inherited in
            inherited_related_declarations_at_type_in(inner, &subtype, &signature, is_cancelled)?
        {
            if inherited.target != declaration.target
                && declarations_share_dispatch_at_join_in(inner, declaration, &inherited)?
            {
                overriding.push(inherited);
            }
        }

        for descendant in inner.direct_subtypes(&subtype) {
            let descendant = descendant.to_string();
            pending.push_back((
                descendant.clone(),
                same_declaring_package
                    && type_package_in(inner, &descendant) == type_package_in(inner, owner),
            ));
        }
    }
    overriding.sort_unstable_by(|left, right| left.target.fqn.cmp(&right.target.fqn));
    overriding.dedup_by(|left, right| left.target == right.target);
    Ok(overriding)
}

fn callable_override_family_in<F>(
    inner: &IndexInner,
    root: ResolvedDefinition,
    is_cancelled: &F,
) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
{
    callable_override_signature_in(inner, &root.symbol).ok_or_else(|| {
        CurrentUsageQueryError::Unavailable(format!(
            "callable '{}' has no exact override descriptor",
            root.target.fqn
        ))
    })?;
    let mut pending = VecDeque::from([root]);
    let mut seen = HashSet::new();
    let mut family = Vec::new();

    while let Some(declaration) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !seen.insert(declaration.target.clone()) {
            continue;
        }
        pending.extend(nearest_overridden_declarations_in(
            inner,
            &declaration,
            is_cancelled,
        )?);
        pending.extend(nearest_overriding_declarations_in(
            inner,
            &declaration,
            is_cancelled,
        )?);
        family.push(declaration);
    }

    family.sort_unstable_by(|left, right| {
        left.target
            .fqn
            .cmp(&right.target.fqn)
            .then(
                left.location
                    .file_id
                    .as_u32()
                    .cmp(&right.location.file_id.as_u32()),
            )
            .then(left.location.range.start.cmp(&right.location.range.start))
    });
    Ok(family)
}

pub(super) fn overriding_declarations_in<F>(
    inner: &IndexInner,
    root: ResolvedDefinition,
    is_cancelled: &F,
) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
where
    F: Fn() -> bool + ?Sized,
{
    let root_target = root.target.clone();
    let mut pending = VecDeque::from([root]);
    let mut seen = HashSet::from([root_target]);
    let mut overriding = Vec::new();
    while let Some(declaration) = pending.pop_front() {
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        for candidate in nearest_overriding_declarations_in(inner, &declaration, is_cancelled)? {
            if seen.insert(candidate.target.clone()) {
                pending.push_back(candidate.clone());
                overriding.push(candidate);
            }
        }
    }
    overriding.sort_unstable_by(|left, right| left.target.fqn.cmp(&right.target.fqn));
    Ok(overriding)
}
impl WorkspaceIndex {
    /// Resolve one exact instance callable to its complete override-connected
    /// declaration component under a single workspace graph revision.
    ///
    /// Descriptorless and unsubstituted generic identities fail closed. Callers
    /// that require a complete family (for example refactorings) must propagate
    /// `Unavailable`; ordinary reference navigation may deliberately fall back
    /// to the exact declaration target.
    pub fn find_current_callable_override_family(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
    ) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError> {
        self.find_current_callable_override_family_cancellable(
            target,
            expected_workspace_revision,
            || false,
        )
    }

    /// Prove that the selected callable owner's written hierarchy and complete
    /// descendant graph are exact in this workspace generation.
    ///
    /// Ordinary navigation can fall back when hierarchy data is partial;
    /// destructive refactorings must suppress themselves instead.
    pub fn current_callable_owner_hierarchy_is_complete(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
    ) -> std::result::Result<bool, CurrentUsageQueryError> {
        self.current_callable_owner_hierarchy_is_complete_cancellable(
            target,
            expected_workspace_revision,
            || false,
        )
    }

    pub fn current_callable_owner_hierarchy_is_complete_cancellable<F>(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
        is_cancelled: F,
    ) -> std::result::Result<bool, CurrentUsageQueryError>
    where
        F: Fn() -> bool,
    {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let definition = find_symbol_definition_coherent_in(&inner, target).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "callable '{}' has no elected definition",
                target.fqn
            ))
        })?;
        let Some(owner) = definition.symbol.container.as_deref() else {
            return Ok(false);
        };
        callable_owner_hierarchies_are_complete_in(&inner, std::iter::once(owner), &is_cancelled)
    }

    pub fn find_current_callable_override_family_cancellable<F>(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
        is_cancelled: F,
    ) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
    where
        F: Fn() -> bool,
    {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let Some(root) = find_symbol_definition_coherent_in(&inner, target) else {
            return Ok(Vec::new());
        };
        callable_override_family_in(&inner, root, &is_cancelled)
    }

    /// Every declaration the callable `target` overrides or implements,
    /// nearest first and transitively (interface and superclass methods a
    /// call could name while dispatching to `target`). Empty when the target
    /// is not virtual or its override descriptor is unavailable.
    pub fn find_current_overridden_declarations_cancellable<F>(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
        is_cancelled: F,
    ) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
    where
        F: Fn() -> bool,
    {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let Some(root) = find_symbol_definition_coherent_in(&inner, target) else {
            return Ok(Vec::new());
        };
        if callable_override_signature_in(&inner, &root.symbol).is_none() {
            return Ok(Vec::new());
        }
        let mut pending = VecDeque::from([root.clone()]);
        let mut seen = HashSet::new();
        let mut ancestors = Vec::new();
        while let Some(declaration) = pending.pop_front() {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if !seen.insert(declaration.target.clone()) {
                continue;
            }
            pending.extend(nearest_overridden_declarations_in(
                &inner,
                &declaration,
                &is_cancelled,
            )?);
            if declaration.target != root.target {
                ancestors.push(declaration);
            }
        }
        Ok(ancestors)
    }

    /// Resolve a callable family and prove the complete hierarchy it depends on
    /// under one index read and one union traversal.
    pub fn find_current_callable_override_family_for_refactoring_cancellable<F>(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
        is_cancelled: F,
    ) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError>
    where
        F: Fn() -> bool,
    {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let Some(root) = find_symbol_definition_coherent_in(&inner, target) else {
            return Ok(Vec::new());
        };
        let family = callable_override_family_in(&inner, root, &is_cancelled)?;
        let complete = callable_owner_hierarchies_are_complete_in(
            &inner,
            family
                .iter()
                .filter_map(|definition| definition.symbol.container.as_deref()),
            &is_cancelled,
        )?;
        if !complete {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "callable family for '{}' has an incomplete owner hierarchy",
                target.fqn
            )));
        }
        Ok(family)
    }

    /// Check whether renaming an exact virtual callable family would collide
    /// with, newly override, or newly hide another callable in its relevant
    /// hierarchy.
    ///
    /// The proof and lookup share one index read. It deliberately treats any
    /// same-named overload as a conflict: changing an overload set is a
    /// semantic operation in its own right and cannot be justified by a plain
    /// rename request.
    pub fn current_callable_family_conflicts_with_name_for_refactoring<F>(
        &self,
        family_targets: &[crate::SymbolTarget],
        new_name: &str,
        expected_workspace_revision: WorkspaceRevision,
        is_cancelled: F,
    ) -> std::result::Result<bool, CurrentUsageQueryError>
    where
        F: Fn() -> bool,
    {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        if family_targets.is_empty() {
            return Err(CurrentUsageQueryError::Unavailable(
                "callable family is empty".to_string(),
            ));
        }

        let mut owners = Vec::with_capacity(family_targets.len());
        for target in family_targets {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if target.namespace != crate::DeclarationNamespace::Callable {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "'{}' is not a callable target",
                    target.fqn
                )));
            }
            let definition =
                find_symbol_definition_coherent_in(&inner, target).ok_or_else(|| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "callable '{}' has no elected definition",
                        target.fqn
                    ))
                })?;
            let owner = definition.symbol.container.clone().ok_or_else(|| {
                CurrentUsageQueryError::Unavailable(format!(
                    "callable '{}' has no nominal owner",
                    target.fqn
                ))
            })?;
            owners.push(owner);
        }
        owners.sort_unstable();
        owners.dedup();

        if !callable_owner_hierarchies_are_complete_in(
            &inner,
            owners.iter().map(AsRef::as_ref),
            &is_cancelled,
        )? {
            return Err(CurrentUsageQueryError::Unavailable(
                "callable family has an incomplete owner hierarchy".to_string(),
            ));
        }

        let mut relevant = HashSet::new();
        let mut ancestors = VecDeque::from(owners.clone());
        while let Some(type_fqn) = ancestors.pop_front() {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if !relevant.insert(Arc::clone(&type_fqn)) {
                continue;
            }
            ancestors.extend(
                direct_supertypes_in(&inner, &type_fqn)
                    .into_iter()
                    .map(Arc::<str>::from),
            );
        }

        let mut descendants = VecDeque::from(owners);
        let mut visited_descendants = HashSet::new();
        while let Some(type_fqn) = descendants.pop_front() {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if !visited_descendants.insert(Arc::clone(&type_fqn)) {
                continue;
            }
            relevant.insert(Arc::clone(&type_fqn));
            descendants.extend(inner.direct_subtypes(&type_fqn));
        }

        for owner in relevant {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if !method_definitions_by_name_in(&inner, &owner, new_name).is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Resolve exact overriding declarations below one callable owner.
    ///
    /// The target and all returned definitions belong to
    /// `expected_workspace_revision`; unsupported callable identities return
    /// `Unavailable` instead of falling back to name or arity matching.
    pub fn find_current_overriding_method_definitions(
        &self,
        target: &crate::SymbolTarget,
        expected_workspace_revision: WorkspaceRevision,
    ) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError> {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let Some(root) = find_symbol_definition_coherent_in(&inner, target) else {
            return Ok(Vec::new());
        };
        overriding_declarations_in(&inner, root, &|| false)
    }

    /// Resolve the declaration at one exact source range and require it to be
    /// the currently elected definition in its semantic namespace.
    ///
    /// This is the range-to-identity bridge for request features that begin at
    /// syntax (for example call hierarchy). It does not pass through the
    /// legacy one-location-per-textual-FQN maps, so a constructor and an
    /// ordinary same-named method remain distinct.
    pub fn find_symbol_definition_at_revision_range(
        &self,
        revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
        range: crate::TextRange,
        namespace: crate::DeclarationNamespace,
    ) -> Option<ResolvedDefinition> {
        let inner = self.inner.read();
        if inner.workspace_revision != workspace_revision
            || inner.revision_token(revision.file_id()) != Some(revision)
            || inner.external_file_ids.contains(&revision.file_id())
        {
            return None;
        }

        let mut candidates = inner
            .file_symbols
            .get(&revision.file_id())?
            .iter()
            // JVM-view aliases (`getNote()` of Kotlin `note`) share the range of
            // the Kotlin declaration they derive from; the declaration is the
            // identity, its aliases are views of it.
            .filter(|symbol| {
                symbol.range == range
                    && namespace.accepts(symbol.kind)
                    && symbol
                        .kotlin
                        .as_ref()
                        .is_none_or(|kotlin| kotlin.jvm_origin.is_none())
            });
        let symbol = Arc::clone(candidates.next()?);
        if candidates.next().is_some() {
            return None;
        }

        let target = crate::SymbolTarget::for_symbol(&symbol);
        let definition = find_symbol_definition_coherent_in(&inner, &target)?;
        (definition.location.file_id == revision.file_id()
            && Arc::ptr_eq(&definition.symbol, &symbol))
        .then_some(definition)
    }

    /// The Kotlin declaration behind the JVM-view aliases (`getNote()`,
    /// `SquareKt.f()`) recorded at `range`, when every alias there derives from
    /// the same origin. A Java call site navigates to the range of that
    /// declaration but has looked the alias up by a callable namespace the
    /// declaration itself (a property) is not in.
    pub fn find_jvm_origin_definition_at_revision_range(
        &self,
        revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
        range: crate::TextRange,
    ) -> Option<ResolvedDefinition> {
        let inner = self.inner.read();
        if inner.workspace_revision != workspace_revision
            || inner.revision_token(revision.file_id()) != Some(revision)
            || inner.external_file_ids.contains(&revision.file_id())
        {
            return None;
        }
        let mut origins = inner
            .file_symbols
            .get(&revision.file_id())?
            .iter()
            .filter(|symbol| symbol.range == range)
            .filter_map(|symbol| {
                symbol
                    .kotlin
                    .as_ref()?
                    .jvm_origin
                    .as_ref()
                    .map(ToString::to_string)
            })
            .collect::<Vec<_>>();
        origins.sort_unstable();
        origins.dedup();
        let [origin] = origins.as_slice() else {
            return None;
        };
        let origin_symbol = inner
            .file_symbols
            .get(&revision.file_id())?
            .iter()
            .find(|symbol| symbol.fqn.as_ref() == origin.as_str() && symbol.range == range)?
            .clone();
        let target = crate::SymbolTarget::for_symbol(&origin_symbol);
        let definition = find_symbol_definition_coherent_in(&inner, &target)?;
        (definition.location.file_id == revision.file_id()
            && Arc::ptr_eq(&definition.symbol, &origin_symbol))
        .then_some(definition)
    }

    /// Resolve every currently elected declaration whose name token is wholly
    /// contained by one exact workspace source range.
    ///
    /// This is primarily useful for destructive refactorings over a containing
    /// declaration. Deleting a type also deletes its constructors, members,
    /// nested types, and compiler-synthesized declarations; scanning only the
    /// type target can therefore produce a false "unused" result. Any contained
    /// declaration shadowed by another source authority makes the result
    /// unavailable rather than mixing two owner shapes.
    pub fn find_symbol_definitions_at_revision_within_range(
        &self,
        revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
        range: crate::TextRange,
    ) -> std::result::Result<Vec<ResolvedDefinition>, CurrentUsageQueryError> {
        let inner = self.inner.read();
        if inner.workspace_revision != workspace_revision
            || inner.revision_token(revision.file_id()) != Some(revision)
            || inner.external_file_ids.contains(&revision.file_id())
        {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }

        let symbols = inner.file_symbols.get(&revision.file_id()).ok_or_else(|| {
            CurrentUsageQueryError::Unavailable(format!(
                "file {:?} has no indexed declarations",
                revision.file_id()
            ))
        })?;
        let mut targets = HashSet::new();
        let mut definitions = Vec::new();
        for symbol in symbols
            .iter()
            .filter(|symbol| range.start <= symbol.range.start && symbol.range.end <= range.end)
        {
            let target = crate::SymbolTarget::for_symbol(symbol);
            if !targets.insert(target.clone()) {
                continue;
            }
            let definition =
                find_symbol_definition_coherent_in(&inner, &target).ok_or_else(|| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "contained declaration '{}' has no elected definition",
                        target.fqn
                    ))
                })?;
            if definition.location.file_id != revision.file_id()
                || definition.revision() != Some(revision)
                || !Arc::ptr_eq(&definition.symbol, symbol)
            {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "contained declaration '{}' is shadowed by another source",
                    target.fqn
                )));
            }
            definitions.push(definition);
        }
        definitions.sort_unstable_by(|left, right| {
            left.location
                .range
                .start
                .cmp(&right.location.range.start)
                .then(left.target.fqn.cmp(&right.target.fqn))
                .then((left.target.namespace as u8).cmp(&(right.target.namespace as u8)))
        });
        Ok(definitions)
    }

    /// Count import declarations that would become invalid if `targets` were
    /// removed from the current workspace graph.
    ///
    /// Ordinary wildcard imports remain valid when one member disappears. An
    /// explicit type import, an explicit static-member import, or a static
    /// wildcard whose owner type is itself being deleted is a structural
    /// dependency even when no expression-level usage exists.
    pub fn count_current_import_dependencies_for_symbol_targets(
        &self,
        targets: &[crate::SymbolTarget],
        expected_workspace_revision: WorkspaceRevision,
    ) -> std::result::Result<usize, CurrentUsageQueryError> {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }

        let current_overlay =
            crate::import_resolver::RevisionSymbolOverlay::masking_publications([]);
        let mut deleted_publications = Vec::with_capacity(targets.len());
        for target in targets {
            let definition =
                find_symbol_definition_coherent_in(&inner, target).ok_or_else(|| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "deletion target '{}' has no unique elected publication",
                        target.fqn
                    ))
                })?;
            deleted_publications.push((target.clone(), definition.location.file_id));
        }
        let deleted_overlay = crate::import_resolver::RevisionSymbolOverlay::masking_publications(
            deleted_publications,
        );

        let mut dependencies = HashSet::new();
        for (&file_id, imports) in &inner.file_imports {
            if inner.external_file_ids.contains(&file_id) {
                continue;
            }
            let language = inner
                .file_path(file_id)
                .and_then(Language::from_path)
                .unwrap_or(Language::Java);
            for (imported_name, imported) in &imports.explicit {
                let target =
                    crate::SymbolTarget::new(imported.as_str(), crate::DeclarationNamespace::Type);
                let resolves_before = crate::import_resolver::ImportResolver::for_coherent_index(
                    imports,
                    &inner,
                    &current_overlay,
                    language,
                )
                .get_symbol_target(&target)
                .is_some();
                let resolves_after = crate::import_resolver::ImportResolver::for_coherent_index(
                    imports,
                    &inner,
                    &deleted_overlay,
                    language,
                )
                .get_symbol_target(&target)
                .is_some();
                if resolves_before && !resolves_after {
                    dependencies.insert((file_id, 0_u8, format!("{imported_name}\0{imported}")));
                }
            }
            for imported in &imports.static_imports {
                let mut isolated = ImportMap::new();
                isolated.current_package = imports.current_package.clone();
                let simple_name = imported.imported_name();
                isolated.add_static_import(simple_name.to_owned(), imported.fqn.to_string());
                let resolves = |overlay| {
                    let resolver = crate::import_resolver::ImportResolver::for_coherent_index(
                        &isolated, &inner, overlay, language,
                    );
                    !resolver.resolve_static_values(simple_name).is_empty()
                        || !resolver.resolve_static_callables(simple_name).is_empty()
                        || !resolver.resolve_static_types(simple_name).is_empty()
                };
                if resolves(&current_overlay) && !resolves(&deleted_overlay) {
                    dependencies.insert((
                        file_id,
                        1_u8,
                        format!("{}\0{}", imported.imported_name(), imported.fqn),
                    ));
                }
            }
            for owner in &imports.static_wildcards {
                let target =
                    crate::SymbolTarget::new(owner.as_str(), crate::DeclarationNamespace::Type);
                let before = {
                    let resolver = crate::import_resolver::ImportResolver::for_coherent_index(
                        imports,
                        &inner,
                        &current_overlay,
                        language,
                    );
                    resolver.get_symbol_target(&target).is_some()
                        && resolver.static_import_owner_is_accessible(owner)
                };
                let after = {
                    let resolver = crate::import_resolver::ImportResolver::for_coherent_index(
                        imports,
                        &inner,
                        &deleted_overlay,
                        language,
                    );
                    resolver.get_symbol_target(&target).is_some()
                        && resolver.static_import_owner_is_accessible(owner)
                };
                if before && !after {
                    dependencies.insert((file_id, 2_u8, owner.clone()));
                }
            }
        }
        Ok(dependencies.len())
    }

    /// Count known subtype/implementation declarations that live outside a
    /// set of type targets being deleted.
    ///
    /// Potential contextual edges must all resolve exactly; otherwise a zero
    /// would mean "not known" rather than "no dependent declarations."
    pub fn count_current_structural_type_dependents(
        &self,
        targets: &[crate::SymbolTarget],
        expected_workspace_revision: WorkspaceRevision,
    ) -> std::result::Result<usize, CurrentUsageQueryError> {
        let inner = self.inner.read();
        if inner.workspace_revision != expected_workspace_revision {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let deleted_types: HashSet<&str> = targets
            .iter()
            .filter(|target| target.namespace == crate::DeclarationNamespace::Type)
            .map(|target| target.fqn.as_ref())
            .collect();
        let mut dependents = HashSet::new();
        for type_fqn in &deleted_types {
            if !inner.direct_subtype_resolution_is_complete(type_fqn) {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "type '{type_fqn}' has a missing or ambiguous potential subtype edge"
                )));
            }
            for subtype in inner.direct_subtypes(type_fqn) {
                if !deleted_types.contains(subtype.as_ref()) {
                    dependents.insert(subtype);
                }
            }
        }
        Ok(dependents.len())
    }
}
