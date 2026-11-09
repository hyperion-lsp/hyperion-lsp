// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Effective receiver-method election under Java inheritance rules.
//!
//! This module's signatures are deliberately receiver-relative: owner type
//! variables are projected through the queried hierarchy, method variables
//! are alpha-normalized, and valid raw edges use erasure. They must not be
//! replaced with `refactoring`'s declaration-relative signatures, which reject
//! every unsubstituted generic identity because rename/reference expansion
//! requires a stronger fail-closed proof. Directional subsignature tests cover
//! both representations' most easily confused erasure cases.

use super::generic_supertypes::{
    effective_type_parameters_with_election, owner_type_bindings_with_election,
    substitute_member_type_parameters, TypeBindings,
};
use super::refactoring::{
    erase_descriptor_type_arguments, exact_erased_callable_signature_in, extend_uniform_package,
    method_is_virtual, method_visibility_rank, nominal_type_is_covariant_in,
    normalize_override_parameter, owner_is_interface, owner_prevents_overrides,
    return_type_is_covariant_in, split_callable_descriptor, type_package_in,
};
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// A receiver dispatch slot. Return types deliberately do not participate:
/// Java permits an override (and a static hide) to narrow the return type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ErasedCallableSignature {
    pub(super) name: Arc<str>,
    pub(super) parameters: Vec<String>,
}

/// A method signature after declaring-owner type variables have been projected
/// into the queried receiver. `slot` is the receiver-relative erasure used to
/// collect potentially override-equivalent methods; the richer source shape
/// then proves whether one declaration actually subsigns another.
#[derive(Clone, Debug)]
struct ReceiverCallableSignature {
    slot: ErasedCallableSignature,
    declared_parameters: Vec<String>,
    method_parameter_bounds: Vec<Vec<String>>,
    projection_available: bool,
}

#[derive(Clone, Debug)]
struct ProjectedReturnType {
    declared: String,
    erased: String,
}

#[derive(Clone, Debug)]
struct EffectiveMethodCandidate {
    definition: ResolvedDefinition,
    signature: ReceiverCallableSignature,
    /// The outer option records whether projection was trustworthy. The inner
    /// option preserves declarations without a return type.
    projected_return: Option<Option<ProjectedReturnType>>,
}

pub(super) struct ReceiverProjection<'a> {
    inner: &'a IndexInner,
    scope: Option<&'a SourceScope>,
    receiver_fqn: &'a str,
    receiver_parameters: Vec<crate::TypeParameterInfo>,
    receiver_arguments: Vec<String>,
    owner_bindings: HashMap<String, Option<TypeBindings>>,
}

impl<'a> ReceiverProjection<'a> {
    pub(super) fn new(inner: &'a IndexInner, receiver_fqn: &'a str) -> Self {
        Self::new_scoped(inner, None, receiver_fqn)
    }

    pub(super) fn new_scoped(
        inner: &'a IndexInner,
        scope: Option<&'a SourceScope>,
        receiver_fqn: &'a str,
    ) -> Self {
        let election = OwnerElection::new(inner, scope);
        let receiver_parameters =
            effective_type_parameters_with_election(&election, receiver_fqn).unwrap_or_default();
        let receiver_arguments = receiver_parameters
            .iter()
            .map(|parameter| parameter.name.to_string())
            .collect();
        Self {
            inner,
            scope,
            receiver_fqn,
            receiver_parameters,
            receiver_arguments,
            owner_bindings: HashMap::new(),
        }
    }

    pub(super) fn with_arguments(
        inner: &'a IndexInner,
        receiver_fqn: &'a str,
        receiver_arguments: &[String],
    ) -> Self {
        Self::with_arguments_scoped(inner, None, receiver_fqn, receiver_arguments)
    }

    pub(super) fn with_arguments_scoped(
        inner: &'a IndexInner,
        scope: Option<&'a SourceScope>,
        receiver_fqn: &'a str,
        receiver_arguments: &[String],
    ) -> Self {
        let election = OwnerElection::new(inner, scope);
        Self {
            inner,
            scope,
            receiver_fqn,
            receiver_parameters: effective_type_parameters_with_election(&election, receiver_fqn)
                .unwrap_or_default(),
            receiver_arguments: receiver_arguments.to_vec(),
            owner_bindings: HashMap::new(),
        }
    }

    fn owner_bindings(&mut self, owner_fqn: &str) -> Option<&TypeBindings> {
        if !self.owner_bindings.contains_key(owner_fqn) {
            let election = OwnerElection::new(self.inner, self.scope);
            let bindings = owner_type_bindings_with_election(
                &election,
                self.receiver_fqn,
                &self.receiver_arguments,
                owner_fqn,
            );
            self.owner_bindings.insert(owner_fqn.to_owned(), bindings);
        }
        self.owner_bindings.get(owner_fqn)?.as_ref()
    }

    /// Project a member spelling under one cached owner mapping. A valid raw
    /// edge has `None` bindings and therefore falls back to Java erasure;
    /// malformed/conflicting hierarchy metadata has no mapping and fails
    /// closed instead of electing an arbitrary declaration.
    pub(super) fn member_type(&mut self, symbol: &crate::Symbol, written: &str) -> Option<String> {
        let owner = symbol.container.as_deref()?;
        let inner = self.inner;
        let bindings = self.owner_bindings(owner)?;
        substitute_member_type_parameters(written, bindings, &symbol.type_parameters).or_else(
            || {
                if self.scope.is_some() {
                    return None;
                }
                erased_type_for_symbol_in(inner, symbol, written)
                    .and_then(|erased| qualify_erased_type_in(inner, owner, &erased))
            },
        )
    }

    pub(super) fn erased_member_type(
        &mut self,
        symbol: &crate::Symbol,
        written: &str,
    ) -> Option<String> {
        let projected = self.member_type(symbol, written)?;
        projected_erasure_in(self, symbol, &projected)
    }
}

fn projected_callable_signature_in(
    projection: &mut ReceiverProjection<'_>,
    definition: &ResolvedDefinition,
) -> Option<ProjectedCallableSignature> {
    let owner_bindings = projected_owner_bindings_in(projection, &definition.symbol)?;
    let parameters = definition
        .symbol
        .parameters
        .iter()
        .map(|parameter| {
            let (parameter, variadic_style) = parameter.trim().strip_prefix("vararg ").map_or(
                (parameter.as_ref(), ProjectedVariadicStyle::None),
                |parameter| (parameter, ProjectedVariadicStyle::KotlinPrefix),
            );
            let (written_type, name) = java_parameter_parts_from_declaration(parameter);
            let variadic_style = if written_type.contains("...") {
                ProjectedVariadicStyle::JavaEllipsis
            } else {
                variadic_style
            };
            let semantic_type = if variadic_style == ProjectedVariadicStyle::JavaEllipsis {
                written_type.trim_end_matches("...").trim_end()
            } else {
                written_type
            };
            let mut type_name = projection.member_type(&definition.symbol, semantic_type)?;
            if variadic_style != ProjectedVariadicStyle::None {
                type_name.push_str("[]");
            }
            Some(ProjectedCallableParameter {
                type_name,
                name: name.map(Arc::<str>::from),
                variadic_style,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let return_type = match definition.symbol.return_type.as_deref() {
        Some(return_type) => Some(projection.member_type(&definition.symbol, return_type)?),
        None => None,
    };
    let type_parameters = definition
        .symbol
        .type_parameters
        .iter()
        .map(|parameter| {
            let bounds = parameter
                .bounds
                .iter()
                .map(|bound| projection.member_type(&definition.symbol, bound))
                .map(|bound| bound.map(Arc::<str>::from))
                .collect::<Option<Vec<_>>>()?;
            Some(crate::TypeParameterInfo {
                name: Arc::clone(&parameter.name),
                position: parameter.position,
                bounds,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ProjectedCallableSignature {
        parameters,
        return_type,
        owner_bindings,
        type_parameters,
    })
}

fn projected_owner_bindings_in(
    projection: &mut ReceiverProjection<'_>,
    symbol: &crate::Symbol,
) -> Option<Vec<ProjectedOwnerBinding>> {
    let owner = symbol.container.as_deref()?;
    let bindings = projection.owner_bindings(owner)?.clone();
    let election = OwnerElection::new(projection.inner, projection.scope);
    let parameters = effective_type_parameters_with_election(&election, owner)?;
    if bindings.len() != parameters.len() {
        return None;
    }
    parameters
        .iter()
        .zip(bindings)
        .map(|(parameter, (binding_name, argument))| {
            if parameter.name != binding_name {
                return None;
            }
            let type_name = match argument {
                Some(argument) => argument,
                None => {
                    let parameter_refs = parameters.iter().collect::<Vec<_>>();
                    let erased =
                        erased_type_with_parameters(parameter.name.as_ref(), &parameter_refs)?;
                    qualify_erased_type_in(projection.inner, owner, &erased)?
                }
            };
            Some(ProjectedOwnerBinding {
                name: Arc::clone(&parameter.name),
                type_name,
            })
        })
        .collect()
}

pub(super) fn project_effective_field_type_in(
    projection: &mut ReceiverProjection<'_>,
    definition: &ResolvedDefinition,
) -> Option<String> {
    let written = definition.symbol.return_type.as_deref()?;
    projection.member_type(&definition.symbol, written)
}

fn erased_type_for_symbol_in(
    inner: &IndexInner,
    symbol: &crate::Symbol,
    written: &str,
) -> Option<String> {
    let owner_type_parameters = symbol
        .container
        .as_deref()
        .and_then(|owner| inner.symbols.get(owner))
        .map(|owner| owner.type_parameters.as_slice())
        .unwrap_or_default();
    let type_parameters = symbol
        .type_parameters
        .iter()
        .chain(owner_type_parameters)
        .collect::<Vec<_>>();
    erased_type_with_parameters(written, &type_parameters)
}

fn erased_type_with_parameters(
    written: &str,
    type_parameters: &[&crate::TypeParameterInfo],
) -> Option<String> {
    let normalized = normalize_override_parameter(written)?;
    let mut erased = erase_descriptor_type_arguments(&normalized)?;
    let mut dimensions = 0_usize;
    while let Some(component) = erased.strip_suffix("[]") {
        erased = component.to_owned();
        dimensions += 1;
    }
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(erased.clone()) {
            return None;
        }
        let replacement = type_parameters
            .iter()
            .find(|parameter| parameter.name.as_ref() == erased)
            .map(|parameter| {
                parameter.bounds.first().map_or_else(
                    || "java.lang.Object".to_owned(),
                    |bound| {
                        normalize_override_parameter(bound)
                            .and_then(|bound| erase_descriptor_type_arguments(&bound))
                            .unwrap_or_else(|| bound.to_string())
                    },
                )
            });
        let Some(replacement) = replacement else {
            break;
        };
        erased = erase_descriptor_type_arguments(&normalize_override_parameter(&replacement)?)?;
        while let Some(component) = erased.strip_suffix("[]") {
            erased = component.to_owned();
            dimensions += 1;
        }
    }
    erased.extend(std::iter::repeat_n("[]", dimensions));
    Some(erased)
}

/// Remove a Java source parameter name without treating whitespace nested in
/// generic arguments as the declaration boundary. Dependency symbols contain
/// only the type and pass through unchanged.
fn declared_parameter_type(parameter: &str) -> &str {
    let parameter = parameter.trim();
    let mut generic_depth = 0_u32;
    let mut last_top_level_whitespace = None;
    for (offset, character) in parameter.char_indices() {
        match character {
            '<' => generic_depth = generic_depth.saturating_add(1),
            '>' => generic_depth = generic_depth.saturating_sub(1),
            _ if character.is_whitespace() && generic_depth == 0 => {
                last_top_level_whitespace = Some(offset);
            }
            _ => {}
        }
    }
    last_top_level_whitespace.map_or(parameter, |offset| parameter[..offset].trim_end())
}

fn canonicalize_method_type_parameters(
    written: &str,
    parameters: &[crate::TypeParameterInfo],
) -> String {
    let mut output = String::with_capacity(written.len());
    let mut token = String::new();
    let flush = |token: &mut String, output: &mut String| {
        if token.is_empty() {
            return;
        }
        if let Some(position) = parameters
            .iter()
            .position(|parameter| parameter.name.as_ref() == token)
        {
            output.push_str("#method");
            output.push_str(&position.to_string());
        } else {
            output.push_str(token);
        }
        token.clear();
    };
    for character in written.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '$' | '.') {
            token.push(character);
        } else {
            flush(&mut token, &mut output);
            output.push(character);
        }
    }
    flush(&mut token, &mut output);
    output
}

fn qualify_method_bound_in(
    projection: &ReceiverProjection<'_>,
    symbol: &crate::Symbol,
    written: &str,
) -> Option<String> {
    let mut output = String::with_capacity(written.len());
    let mut token = String::new();
    let flush = |token: &mut String, output: &mut String| -> Option<()> {
        if token.is_empty() {
            return Some(());
        }
        let preserved = symbol
            .type_parameters
            .iter()
            .any(|parameter| parameter.name.as_ref() == token)
            || projection
                .receiver_parameters
                .iter()
                .any(|parameter| parameter.name.as_ref() == token)
            || matches!(
                token.as_str(),
                "boolean"
                    | "byte"
                    | "short"
                    | "char"
                    | "int"
                    | "long"
                    | "float"
                    | "double"
                    | "void"
                    | "extends"
                    | "super"
            );
        if preserved {
            output.push_str(token);
        } else {
            let owner = symbol.container.as_deref()?;
            match projection.inner.resolve_recorded_supertype(owner, token) {
                crate::TypeNameResolution::Exact(resolved) => output.push_str(&resolved),
                crate::TypeNameResolution::Missing => output.push_str(token),
                crate::TypeNameResolution::Ambiguous(_) => return None,
            }
        }
        token.clear();
        Some(())
    };
    for character in written.chars() {
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

fn qualify_erased_type_in(
    inner: &IndexInner,
    context_owner: &str,
    written: &str,
) -> Option<String> {
    let mut base = written.to_owned();
    let mut dimensions = 0_usize;
    while let Some(component) = base.strip_suffix("[]") {
        base = component.to_owned();
        dimensions += 1;
    }
    if !matches!(
        base.as_str(),
        "boolean" | "byte" | "short" | "char" | "int" | "long" | "float" | "double" | "void"
    ) {
        base = match inner.resolve_recorded_supertype(context_owner, &base) {
            crate::TypeNameResolution::Exact(resolved) => resolved,
            crate::TypeNameResolution::Missing => base,
            crate::TypeNameResolution::Ambiguous(_) => return None,
        };
    }
    base.extend(std::iter::repeat_n("[]", dimensions));
    Some(base)
}

fn projected_erasure_in(
    projection: &mut ReceiverProjection<'_>,
    symbol: &crate::Symbol,
    projected: &str,
) -> Option<String> {
    let normalized = normalize_override_parameter(projected)?;
    let mut erased = erase_descriptor_type_arguments(&normalized)?;
    let mut dimensions = 0_usize;
    while let Some(component) = erased.strip_suffix("[]") {
        erased = component.to_owned();
        dimensions += 1;
    }
    let mut context_owner = symbol.container.as_deref()?;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(erased.clone()) {
            return None;
        }
        let replacement = if let Some(parameter) = symbol
            .type_parameters
            .iter()
            .find(|parameter| parameter.name.as_ref() == erased)
        {
            context_owner = symbol.container.as_deref()?;
            Some(parameter.bounds.first().map_or_else(
                || "java.lang.Object".to_owned(),
                |bound| {
                    projection
                        .member_type(symbol, bound)
                        .unwrap_or_else(|| bound.to_string())
                },
            ))
        } else {
            projection
                .receiver_parameters
                .iter()
                .find(|parameter| parameter.name.as_ref() == erased)
                .map(|parameter| {
                    context_owner = projection.receiver_fqn;
                    parameter
                        .bounds
                        .first()
                        .map_or_else(|| "java.lang.Object".to_owned(), ToString::to_string)
                })
        };
        let Some(replacement) = replacement else {
            break;
        };
        erased = erase_descriptor_type_arguments(&normalize_override_parameter(&replacement)?)?;
        while let Some(component) = erased.strip_suffix("[]") {
            erased = component.to_owned();
            dimensions += 1;
        }
    }
    erased.extend(std::iter::repeat_n("[]", dimensions));
    qualify_erased_type_in(projection.inner, context_owner, &erased)
}

fn receiver_callable_signature_in(
    projection: &mut ReceiverProjection<'_>,
    symbol: &crate::Symbol,
) -> ReceiverCallableSignature {
    let projected_parameters = symbol
        .parameters
        .iter()
        .map(|parameter| projection.member_type(symbol, declared_parameter_type(parameter)))
        .collect::<Option<Vec<_>>>();
    let Some(projected_parameters) = projected_parameters else {
        let slot = erased_callable_signature_in(projection.inner, symbol).unwrap_or_else(|| {
            ErasedCallableSignature {
                name: Arc::clone(&symbol.simple_name),
                parameters: Vec::new(),
            }
        });
        return ReceiverCallableSignature {
            declared_parameters: slot.parameters.clone(),
            slot,
            method_parameter_bounds: Vec::new(),
            projection_available: false,
        };
    };

    let declared_parameters = projected_parameters
        .iter()
        .map(|parameter| {
            normalize_override_parameter(parameter)
                .map(|parameter| {
                    canonicalize_method_type_parameters(&parameter, &symbol.type_parameters)
                })
                .unwrap_or_else(|| parameter.clone())
        })
        .collect::<Vec<_>>();
    let erased_parameters = projected_parameters
        .iter()
        .map(|parameter| projected_erasure_in(projection, symbol, parameter))
        .collect::<Option<Vec<_>>>();
    let method_parameter_bounds = symbol
        .type_parameters
        .iter()
        .map(|parameter| {
            let implicit_object = [Arc::<str>::from("java.lang.Object")];
            let bounds = if parameter.bounds.is_empty() {
                implicit_object.as_slice()
            } else {
                parameter.bounds.as_slice()
            };
            bounds
                .iter()
                .map(|bound| {
                    projection.member_type(symbol, bound).and_then(|bound| {
                        let bound = qualify_method_bound_in(projection, symbol, &bound)?;
                        let bound = normalize_override_parameter(&bound)?;
                        Some(canonicalize_method_type_parameters(
                            &bound,
                            &symbol.type_parameters,
                        ))
                    })
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>();

    let (Some(parameters), Some(method_parameter_bounds)) =
        (erased_parameters, method_parameter_bounds)
    else {
        let slot = erased_callable_signature_in(projection.inner, symbol).unwrap_or_else(|| {
            ErasedCallableSignature {
                name: Arc::clone(&symbol.simple_name),
                parameters: Vec::new(),
            }
        });
        return ReceiverCallableSignature {
            declared_parameters: slot.parameters.clone(),
            slot,
            method_parameter_bounds: Vec::new(),
            projection_available: false,
        };
    };
    ReceiverCallableSignature {
        slot: ErasedCallableSignature {
            name: Arc::clone(&symbol.simple_name),
            parameters,
        },
        declared_parameters,
        method_parameter_bounds,
        projection_available: true,
    }
}

fn effective_candidate_in(
    projection: &mut ReceiverProjection<'_>,
    definition: ResolvedDefinition,
) -> EffectiveMethodCandidate {
    let signature = receiver_callable_signature_in(projection, &definition.symbol);
    let projected_return = if !signature.projection_available {
        None
    } else {
        match definition.symbol.return_type.as_deref() {
            None => Some(None),
            Some(return_type) => projection
                .member_type(&definition.symbol, return_type)
                .and_then(|return_type| {
                    let erased =
                        projected_erasure_in(projection, &definition.symbol, &return_type)?;
                    let normalized = normalize_override_parameter(&return_type)?;
                    Some(ProjectedReturnType {
                        declared: canonicalize_method_type_parameters(
                            &normalized,
                            &definition.symbol.type_parameters,
                        ),
                        erased,
                    })
                })
                .map(Some),
        }
    };
    EffectiveMethodCandidate {
        definition,
        signature,
        projected_return,
    }
}

fn standalone_candidate_in(
    inner: &IndexInner,
    definition: ResolvedDefinition,
) -> Option<EffectiveMethodCandidate> {
    let slot = erased_callable_signature_in(inner, &definition.symbol)?;
    let projected_return = match definition.symbol.return_type.as_deref() {
        None => Some(None),
        Some(return_type) => {
            let declared = normalize_override_parameter(return_type)?;
            let erased = erased_type_for_symbol_in(inner, &definition.symbol, return_type)?;
            Some(Some(ProjectedReturnType { declared, erased }))
        }
    };
    Some(EffectiveMethodCandidate {
        signature: ReceiverCallableSignature {
            declared_parameters: slot.parameters.clone(),
            slot,
            method_parameter_bounds: Vec::new(),
            projection_available: true,
        },
        definition,
        projected_return,
    })
}

pub(super) fn receiver_erased_callable_signature_in(
    inner: &IndexInner,
    receiver_fqn: &str,
    symbol: &crate::Symbol,
) -> Option<ErasedCallableSignature> {
    let mut projection = ReceiverProjection::new(inner, receiver_fqn);
    let signature = receiver_callable_signature_in(&mut projection, symbol);
    signature.projection_available.then_some(signature.slot)
}

pub(super) fn erased_callable_signature_in(
    inner: &IndexInner,
    symbol: &crate::Symbol,
) -> Option<ErasedCallableSignature> {
    if let Some((name, parameters)) = exact_erased_callable_signature_in(inner, symbol) {
        return Some(ErasedCallableSignature { name, parameters });
    }

    // Exact override-family queries reject unsubstituted generic identities,
    // because a refactoring must prove every edge. Receiver lookup has a more
    // modest requirement: keep each declaration in its own JVM erasure slot.
    // A bare type variable erases to its first bound, or Object when unbound.
    let descriptor_parameters = split_callable_descriptor(&symbol.fqn)?;
    let parameters = descriptor_parameters
        .into_iter()
        .map(|parameter| erased_type_for_symbol_in(inner, symbol, &parameter))
        .collect::<Option<Vec<_>>>()?;
    Some(ErasedCallableSignature {
        name: Arc::clone(&symbol.simple_name),
        parameters,
    })
}

/// Member inheritance used by ordinary receiver lookup. Class static methods
/// are inherited (and may be hidden), whereas interface static methods are
/// owned only by the interface that declares them.
fn receiver_method_is_inheritable_with_package_state(
    inner: &IndexInner,
    method: &crate::Symbol,
    declaring_owner: &str,
    uniform_descendant_package: Option<&str>,
) -> bool {
    if !matches!(method.kind, SymbolKind::METHOD | SymbolKind::FUNCTION)
        || method.modifiers.is_private
        || (owner_is_interface(inner, declaring_owner) && method.modifiers.is_static)
    {
        return false;
    }
    if owner_is_interface(inner, declaring_owner)
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

fn definition_order(left: &ResolvedDefinition, right: &ResolvedDefinition) -> std::cmp::Ordering {
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
}

fn signature_is_subsignature(
    child: &ReceiverCallableSignature,
    parent: &ReceiverCallableSignature,
) -> bool {
    child.projection_available
        && parent.projection_available
        && child.slot.name == parent.slot.name
        && (signatures_match_after_adapting_type_parameters(child, parent)
            || signature_matches_erasure(child, parent))
}

fn signatures_match_after_adapting_type_parameters(
    child: &ReceiverCallableSignature,
    parent: &ReceiverCallableSignature,
) -> bool {
    child.method_parameter_bounds == parent.method_parameter_bounds
        && child.declared_parameters == parent.declared_parameters
}

fn signature_matches_erasure(
    child: &ReceiverCallableSignature,
    parent: &ReceiverCallableSignature,
) -> bool {
    child.method_parameter_bounds.is_empty()
        && child.declared_parameters == parent.slot.parameters
        && !signatures_match_after_adapting_type_parameters(child, parent)
}

fn methods_have_related_signatures(
    left: &EffectiveMethodCandidate,
    right: &EffectiveMethodCandidate,
) -> bool {
    signature_is_subsignature(&left.signature, &right.signature)
        || signature_is_subsignature(&right.signature, &left.signature)
}

fn return_is_compatible(
    inner: &IndexInner,
    child: &EffectiveMethodCandidate,
    parent: &EffectiveMethodCandidate,
) -> bool {
    match (&child.projected_return, &parent.projected_return) {
        (Some(Some(child_return)), Some(Some(parent_return))) => {
            return_type_is_covariant_in(inner, &child_return.declared, &parent_return.declared)
                .unwrap_or(false)
                || (signature_matches_erasure(&child.signature, &parent.signature)
                    && return_type_is_covariant_in(
                        inner,
                        &child_return.erased,
                        &parent_return.erased,
                    )
                    .unwrap_or(false))
        }
        (Some(None), Some(None)) => true,
        _ => false,
    }
}

fn class_method_hides_or_overrides(
    inner: &IndexInner,
    child: &EffectiveMethodCandidate,
    parent: &EffectiveMethodCandidate,
) -> bool {
    if !signature_is_subsignature(&child.signature, &parent.signature) {
        return false;
    }
    if child.definition.symbol.modifiers.is_static && parent.definition.symbol.modifiers.is_static {
        return !child.definition.symbol.modifiers.is_private
            && !parent.definition.symbol.modifiers.is_private
            && !parent.definition.symbol.modifiers.is_final
            && method_visibility_rank(inner, &child.definition.symbol)
                >= method_visibility_rank(inner, &parent.definition.symbol)
            && return_is_compatible(inner, child, parent);
    }
    if child.definition.symbol.modifiers.is_static || parent.definition.symbol.modifiers.is_static {
        return false;
    }
    let Some(parent_owner) = parent.definition.symbol.container.as_deref() else {
        return false;
    };
    method_is_virtual(&child.definition.symbol)
        && method_is_virtual(&parent.definition.symbol)
        && !parent.definition.symbol.modifiers.is_final
        && !owner_prevents_overrides(inner, parent_owner)
        && method_visibility_rank(inner, &child.definition.symbol)
            >= method_visibility_rank(inner, &parent.definition.symbol)
        && return_is_compatible(inner, child, parent)
}

fn merge_class_method(
    inner: &IndexInner,
    methods: &mut BTreeMap<ErasedCallableSignature, Vec<EffectiveMethodCandidate>>,
    candidate: EffectiveMethodCandidate,
) {
    let slot = methods.entry(candidate.signature.slot.clone()).or_default();
    if slot
        .iter()
        .any(|existing| existing.definition.target == candidate.definition.target)
    {
        return;
    }
    // The class spine is visited receiver-first. A legal nearer override/hide
    // owns the dispatch slot; malformed or incompatible declarations remain
    // visible as ambiguity instead of being silently elected.
    if slot
        .iter()
        .any(|existing| class_method_hides_or_overrides(inner, existing, &candidate))
    {
        return;
    }
    slot.push(candidate);
    slot.sort_unstable_by(|left, right| definition_order(&left.definition, &right.definition));
}

fn merge_interface_method(
    inner: &IndexInner,
    methods: &mut BTreeMap<ErasedCallableSignature, Vec<EffectiveMethodCandidate>>,
    candidate: EffectiveMethodCandidate,
) {
    let slot = methods.entry(candidate.signature.slot.clone()).or_default();
    if slot
        .iter()
        .any(|existing| existing.definition.target == candidate.definition.target)
    {
        return;
    }
    let Some(candidate_owner) = candidate.definition.symbol.container.as_deref() else {
        slot.push(candidate);
        slot.sort_unstable_by(|left, right| definition_order(&left.definition, &right.definition));
        return;
    };

    let mut superseded = Vec::new();
    for (position, existing) in slot.iter().enumerate() {
        let Some(existing_owner) = existing.definition.symbol.container.as_deref() else {
            continue;
        };
        let candidate_is_subinterface =
            nominal_type_is_covariant_in(inner, candidate_owner, existing_owner);
        let existing_is_subinterface =
            nominal_type_is_covariant_in(inner, existing_owner, candidate_owner);
        if candidate_is_subinterface
            && method_is_virtual(&candidate.definition.symbol)
            && method_is_virtual(&existing.definition.symbol)
            && signature_is_subsignature(&candidate.signature, &existing.signature)
            && return_is_compatible(inner, &candidate, existing)
        {
            superseded.push(position);
            continue;
        }
        if existing_is_subinterface
            && method_is_virtual(&existing.definition.symbol)
            && method_is_virtual(&candidate.definition.symbol)
            && signature_is_subsignature(&existing.signature, &candidate.signature)
            && return_is_compatible(inner, existing, &candidate)
        {
            return;
        }

        // Unrelated abstract contracts with the same subsignature describe one
        // receiver method. Prefer the declaration with the narrower covariant
        // return, then exact FQN order. Conflicting defaults remain separate.
        if !candidate_is_subinterface
            && !existing_is_subinterface
            && existing.definition.symbol.modifiers.is_abstract
            && candidate.definition.symbol.modifiers.is_abstract
            && methods_have_related_signatures(&candidate, existing)
        {
            let candidate_narrows = return_is_compatible(inner, &candidate, existing);
            let existing_narrows = return_is_compatible(inner, existing, &candidate);
            match (candidate_narrows, existing_narrows) {
                (true, false) => superseded.push(position),
                (false, true) => return,
                (true, true) => {
                    if definition_order(&candidate.definition, &existing.definition).is_lt() {
                        superseded.push(position);
                    } else {
                        return;
                    }
                }
                (false, false) => {}
            }
        }
    }
    for position in superseded.into_iter().rev() {
        slot.remove(position);
    }
    slot.push(candidate);
    slot.sort_unstable_by(|left, right| definition_order(&left.definition, &right.definition));
}

fn receiver_method_definitions_in(
    election: &OwnerElection<'_>,
    owner: &str,
    method_name: Option<&str>,
) -> Vec<ResolvedDefinition> {
    election
        .member_definitions(owner)
        .iter()
        .flat_map(|members| members.iter())
        .filter(|definition| {
            definition.target.namespace == crate::DeclarationNamespace::Callable
                && method_name.is_none_or(|name| definition.symbol.simple_name.as_ref() == name)
        })
        .cloned()
        .collect()
}

fn effective_receiver_method_candidates_in(
    election: &OwnerElection<'_>,
    receiver_fqn: &str,
    method_name: Option<&str>,
) -> Vec<EffectiveMethodCandidate> {
    let inner = election.inner;
    let receiver_is_interface = election
        .symbol(receiver_fqn)
        .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE);
    let receiver_package = type_package_in(inner, receiver_fqn);
    let mut projection = ReceiverProjection::new_scoped(inner, election.scope, receiver_fqn);
    let mut class_methods = BTreeMap::new();
    let mut object_fallback_methods = BTreeMap::new();
    let mut interface_roots = Vec::new();

    if receiver_is_interface {
        // Interfaces do not nominally extend Object, but Java method lookup
        // admits Object's public instance methods for interface-typed
        // receivers (JLS 9.2). Keep that language rule out of the inheritance
        // graph: it is member lookup, not a subtype edge.
        for definition in receiver_method_definitions_in(election, "java.lang.Object", method_name)
        {
            if !definition.symbol.modifiers.is_public
                || definition.symbol.modifiers.is_private
                || definition.symbol.modifiers.is_static
            {
                continue;
            }
            let Some(candidate) = standalone_candidate_in(inner, definition) else {
                continue;
            };
            merge_class_method(inner, &mut object_fallback_methods, candidate);
        }
        interface_roots.push((receiver_fqn.to_owned(), true));
    } else {
        let mut owner = Some(receiver_fqn.to_owned());
        let mut uniform_package = Some(receiver_package);
        let mut is_receiver = true;
        let mut visited = HashSet::new();
        while let Some(class_owner) = owner {
            if !visited.insert(class_owner.clone()) {
                break;
            }
            for definition in receiver_method_definitions_in(election, &class_owner, method_name) {
                if !is_receiver
                    && !receiver_method_is_inheritable_with_package_state(
                        inner,
                        &definition.symbol,
                        &class_owner,
                        uniform_package.as_deref(),
                    )
                {
                    continue;
                }
                let candidate = effective_candidate_in(&mut projection, definition);
                merge_class_method(inner, &mut class_methods, candidate);
            }

            let (parent, interfaces) =
                inner.resolved_supertype_roles_with_election(election, &class_owner);
            interface_roots.extend(interfaces.into_iter().map(|interface| (interface, false)));
            uniform_package = extend_uniform_package(inner, uniform_package, &class_owner);
            owner = parent;
            is_receiver = false;
        }
    }

    interface_roots.sort_unstable();
    interface_roots.dedup();
    let mut pending = VecDeque::from(interface_roots);
    let mut visited_interfaces = HashSet::new();
    let mut interface_methods = BTreeMap::new();
    while let Some((interface, is_receiver)) = pending.pop_front() {
        if !visited_interfaces.insert(interface.clone()) {
            continue;
        }
        for definition in receiver_method_definitions_in(election, &interface, method_name) {
            if !is_receiver
                && !receiver_method_is_inheritable_with_package_state(
                    inner,
                    &definition.symbol,
                    &interface,
                    None,
                )
            {
                continue;
            }
            let candidate = effective_candidate_in(&mut projection, definition);
            merge_interface_method(inner, &mut interface_methods, candidate);
        }
        let (_, superinterfaces) =
            inner.resolved_supertype_roles_with_election(election, &interface);
        pending.extend(
            superinterfaces
                .into_iter()
                .map(|interface| (interface, false)),
        );
    }

    for (signature, candidates) in interface_methods {
        // Class-spine declarations win only when they actually implement the
        // receiver-relative interface signature. Merely sharing a JVM erasure
        // is a name clash in malformed/incomplete source, not implementation.
        let remaining = candidates
            .into_iter()
            .filter(|interface_method| {
                !class_methods
                    .get(&signature)
                    .is_some_and(|class_candidates| {
                        class_candidates.iter().any(|class_method| {
                            !class_method.definition.symbol.modifiers.is_private
                                && !class_method.definition.symbol.modifiers.is_static
                                && signature_is_subsignature(
                                    &class_method.signature,
                                    &interface_method.signature,
                                )
                                && method_visibility_rank(inner, &class_method.definition.symbol)
                                    >= method_visibility_rank(
                                        inner,
                                        &interface_method.definition.symbol,
                                    )
                                && return_is_compatible(inner, class_method, interface_method)
                        })
                    })
            })
            .collect::<Vec<_>>();
        class_methods
            .entry(signature)
            .or_default()
            .extend(remaining);
    }

    // JLS 9.2 exposes Object's public instance methods on an interface-typed
    // receiver, but an explicit or inherited interface declaration remains
    // the member for that slot. Object is therefore a fallback, not a class
    // declaration that participates in class-over-interface precedence.
    for (signature, candidates) in object_fallback_methods {
        class_methods.entry(signature).or_insert(candidates);
    }

    class_methods.into_values().flatten().collect()
}

pub(super) fn effective_receiver_methods_in(
    inner: &IndexInner,
    receiver_fqn: &str,
    method_name: Option<&str>,
) -> Vec<ResolvedDefinition> {
    let election = OwnerElection::new(inner, None);
    effective_receiver_method_candidates_in(&election, receiver_fqn, method_name)
        .into_iter()
        .map(|candidate| candidate.definition)
        .collect()
}

impl WorkspaceIndex {
    fn projected_effective_methods(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
        method_name: Option<&str>,
    ) -> Vec<ProjectedEffectiveMethod> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        let candidates =
            effective_receiver_method_candidates_in(&election, receiver_fqn, method_name);
        let mut projection = ReceiverProjection::with_arguments_scoped(
            &inner,
            self.source_scope.as_ref(),
            receiver_fqn,
            receiver_arguments,
        );
        candidates
            .into_iter()
            .map(|candidate| {
                // Declaration-relative election already detected malformed or
                // conflicting generic paths. A concrete instantiation where
                // those paths happen to become equal must not make the invalid
                // declaration appear trustworthy.
                let declaration_projection_available = candidate.signature.projection_available
                    && candidate.projected_return.is_some();
                let mut projected = declaration_projection_available
                    .then(|| {
                        projected_callable_signature_in(&mut projection, &candidate.definition)
                    })
                    .flatten();
                if projected.is_none()
                    && declaration_projection_available
                    && election
                        .symbol(receiver_fqn)
                        .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE)
                    && candidate.definition.symbol.container.as_deref() == Some("java.lang.Object")
                {
                    let mut object_projection =
                        ReceiverProjection::with_arguments(&inner, "java.lang.Object", &[]);
                    projected = projected_callable_signature_in(
                        &mut object_projection,
                        &candidate.definition,
                    );
                }
                ProjectedEffectiveMethod {
                    projection: projected,
                    definition: candidate.definition,
                }
            })
            .collect()
    }

    /// Elect and project every effective method under one coherent index read.
    pub fn find_projected_effective_methods(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
    ) -> Vec<ProjectedEffectiveMethod> {
        self.projected_effective_methods(receiver_fqn, receiver_arguments, None)
    }

    /// Elect and project every same-named effective method under one coherent
    /// index read. Raw receivers use Java erasure; malformed or conflicting
    /// generic paths retain the declaration with an unavailable projection.
    pub fn find_projected_effective_methods_by_name(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
        method_name: &str,
    ) -> Vec<ProjectedEffectiveMethod> {
        self.projected_effective_methods(receiver_fqn, receiver_arguments, Some(method_name))
    }

    /// Return every effective callable visible through `receiver_fqn` in one
    /// hierarchy walk and one coherent index read.
    ///
    /// This has the same membership, access-context, and generic-edge contract
    /// as [`Self::find_effective_methods_by_name`].
    pub fn find_effective_methods(&self, receiver_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        effective_receiver_method_candidates_in(&election, receiver_fqn, None)
            .into_iter()
            .map(|candidate| candidate.definition.symbol)
            .collect()
    }

    /// Every method `receiver_fqn` inherits and could legally redeclare.
    ///
    /// This is the single hierarchy walk behind every editor feature that
    /// offers inherited members -- "implement interface methods", "override
    /// methods", and their completion equivalents. It is the
    /// effective-method election of [`Self::find_effective_methods`] restricted
    /// to the inherited part of the receiver's slot table, so it is transitive
    /// through superclasses and superinterfaces alike (JLS 8.4.8, 9.4.1) and
    /// applies uniformly to classes, interfaces, enums and records. A slot the
    /// receiver itself won is not inherited by definition and is therefore
    /// absent, which is exactly the "already implemented" test a caller needs;
    /// callers editing an unsaved buffer should still exclude the names written
    /// in it, because the index reflects the last indexed revision.
    ///
    /// Constructors are not inherited, and `static`, `final` and `private`
    /// declarations cannot be overridden (JLS 8.4.3.3), so none of them appear.
    /// Every remaining entry carries its declaring owner, whether a concrete
    /// receiver still owes it a body, and its receiver-relative projection.
    pub fn find_inherited_methods(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
    ) -> Vec<InheritedMethod> {
        let projected = self.projected_effective_methods(receiver_fqn, receiver_arguments, None);
        let inner = self.inner.read();
        projected
            .into_iter()
            .filter_map(|method| {
                let symbol = &method.definition.symbol;
                if symbol.kind == SymbolKind::CONSTRUCTOR
                    || symbol.modifiers.is_static
                    || symbol.modifiers.is_final
                    || symbol.modifiers.is_private
                {
                    return None;
                }
                let declaring_owner = symbol.container.as_deref()?;
                if declaring_owner == receiver_fqn {
                    return None;
                }
                let declaring_owner_is_interface = owner_is_interface(&inner, declaring_owner);
                Some(InheritedMethod {
                    declaring_owner: declaring_owner.to_owned(),
                    requires_implementation: symbol.modifiers.is_abstract,
                    declaring_owner_is_interface,
                    projection: method.projection,
                    definition: method.definition,
                })
            })
            .collect()
    }

    /// Return the effective callable set visible through `receiver_fqn`.
    ///
    /// Override-equivalent declarations share one erased dispatch slot. A
    /// legal class override/hide replaces its ancestor, class implementations
    /// take precedence over interface contracts, and only maximally specific
    /// inherited interface declarations remain. The whole election runs under
    /// one index read, so workspace and dependency owners cannot be mixed
    /// across publications.
    ///
    /// This query models receiver membership, not caller access control. A
    /// private declaration is retained when `receiver_fqn` is its declaring
    /// owner so lexical calls from that owner remain representable; inherited
    /// private declarations are never returned. Callers that know a distinct
    /// lexical access owner must apply that context separately.
    ///
    /// Generic owner parameters are projected through the receiver before
    /// election. Valid raw edges use Java erasure; conflicting or malformed
    /// projection metadata remains visible as ambiguity instead of electing an
    /// arbitrary declaration.
    pub fn find_effective_methods_by_name(
        &self,
        receiver_fqn: &str,
        method_name: &str,
    ) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        effective_receiver_method_candidates_in(&election, receiver_fqn, Some(method_name))
            .into_iter()
            .map(|candidate| candidate.definition.symbol)
            .collect()
    }
}

#[cfg(test)]
mod inherited_method_tests {
    use super::*;
    use std::path::Path;

    fn index_source(index: &WorkspaceIndex, root: &Path, relative: &str, source: &str) {
        index
            .index_file_with_source(&root.join(relative), source)
            .unwrap_or_else(|error| panic!("failed to index {relative}: {error}"));
    }

    /// `name -> declaring owner`, sorted, for everything the receiver inherits.
    fn inherited(index: &WorkspaceIndex, receiver: &str) -> Vec<(String, String)> {
        let mut entries = index
            .find_inherited_methods(receiver, &[])
            .into_iter()
            .map(|method| {
                (
                    method.definition.symbol.simple_name.to_string(),
                    method.declaring_owner,
                )
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    fn requires_implementation(index: &WorkspaceIndex, receiver: &str, name: &str) -> Vec<bool> {
        index
            .find_inherited_methods(receiver, &[])
            .into_iter()
            .filter(|method| method.definition.symbol.simple_name.as_ref() == name)
            .map(|method| method.requires_implementation)
            .collect()
    }

    /// JLS 8.4.8: inheritance is transitive through the superclass chain, so a
    /// grandparent's method is a member of the grandchild.
    #[test]
    fn a_grandparent_method_is_inherited() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Grand.java",
            "package p; public class Grand { public void fromGrand() {} }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Parent.java",
            "package p; public class Parent extends Grand { public void fromParent() {} }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Child.java",
            "package p; public class Child extends Parent {}",
        );

        assert_eq!(
            inherited(&index, "p.Child"),
            [
                ("fromGrand".to_owned(), "p.Grand".to_owned()),
                ("fromParent".to_owned(), "p.Parent".to_owned()),
            ]
        );
    }

    /// JLS 9.4.1: a class implementing an interface inherits the abstract
    /// methods of that interface's superinterfaces as well, and a `default`
    /// method is inherited already implemented.
    #[test]
    fn superinterface_methods_are_inherited_transitively_with_their_obligation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Base.java",
            "package p; public interface Base { void base(); default void shared() {} }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Derived.java",
            "package p; public interface Derived extends Base { void derived(); }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Impl.java",
            "package p; public class Impl implements Derived {}",
        );

        assert_eq!(
            inherited(&index, "p.Impl"),
            [
                ("base".to_owned(), "p.Base".to_owned()),
                ("derived".to_owned(), "p.Derived".to_owned()),
                ("shared".to_owned(), "p.Base".to_owned()),
            ]
        );
        assert_eq!(requires_implementation(&index, "p.Impl", "base"), [true]);
        assert_eq!(requires_implementation(&index, "p.Impl", "shared"), [false]);
    }

    /// A slot the receiver itself won is not inherited, which is the exact
    /// "already implemented" test the editor features need.
    #[test]
    fn a_slot_the_receiver_declares_is_not_inherited() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Contract.java",
            "package p; public interface Contract { void run(); void stop(); }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Partial.java",
            "package p; public class Partial implements Contract { public void run() {} }",
        );

        assert_eq!(
            inherited(&index, "p.Partial"),
            [("stop".to_owned(), "p.Contract".to_owned())]
        );
    }

    /// JLS 8.4.3.3: a `final` method cannot be overridden, a `static` method is
    /// hidden rather than overridden, and a `private` method is not inherited
    /// at all. None of them belongs in an override candidate set.
    #[test]
    fn unoverridable_declarations_are_excluded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Locked.java",
            "package p; public class Locked { public void open() {} public final void sealed() {} public static void helper() {} private void hidden() {} }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Sub.java",
            "package p; public class Sub extends Locked {}",
        );

        assert_eq!(
            inherited(&index, "p.Sub"),
            [("open".to_owned(), "p.Locked".to_owned())]
        );
    }

    /// An enum is a class and may implement interfaces; walking only
    /// `parent_class` from a `CLASS_DECL` made every enum invisible here.
    #[test]
    fn an_enum_inherits_its_interface_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Named.java",
            "package p; public interface Named { String label(); }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Color.java",
            "package p; public enum Color implements Named { RED, GREEN; }",
        );

        assert!(inherited(&index, "p.Color").contains(&("label".to_owned(), "p.Named".to_owned())));
    }

    /// A record may implement interfaces too.
    #[test]
    fn a_record_inherits_its_interface_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Sized.java",
            "package p; public interface Sized { int size(); }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Pair.java",
            "package p; public record Pair(int a, int b) implements Sized {}",
        );

        assert!(inherited(&index, "p.Pair").contains(&("size".to_owned(), "p.Sized".to_owned())));
    }

    /// The receiver's own type arguments are projected into the inherited
    /// signature, so a generated stub carries `save(User)` rather than
    /// `save(T)`.
    #[test]
    fn inherited_signatures_are_projected_through_the_receiver() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/User.java",
            "package p; public class User {}",
        );
        index_source(
            &index,
            temp.path(),
            "p/Repo.java",
            "package p; public interface Repo<T> { void save(T item); }",
        );
        index_source(
            &index,
            temp.path(),
            "p/UserRepo.java",
            "package p; public class UserRepo implements Repo<User> {}",
        );

        let methods = index.find_inherited_methods("p.UserRepo", &[]);
        let [save] = methods.as_slice() else {
            panic!("expected exactly one inherited method, got {methods:?}");
        };
        let projection = save
            .projection
            .as_ref()
            .expect("Repo<User> is a well-formed generic edge");
        assert_eq!(
            projection
                .parameters
                .iter()
                .map(|parameter| parameter.type_name.clone())
                .collect::<Vec<_>>(),
            ["p.User"]
        );
    }
}
