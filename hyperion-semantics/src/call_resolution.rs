// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Canonical, syntax-independent Java method overload resolution.
//!
//! Feature layers infer receiver and argument types from their own context, then
//! call [`resolve_call`]. Keeping candidate selection here ensures navigation,
//! hover, inlay hints, and semantic inference use the same conversion rules.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use hyperion_workspace::{
    ProjectedCallableParameter, ProjectedEffectiveMethod, ProjectedOwnerBinding,
    ResolvedDefinition, Symbol, WorkspaceIndex,
};

use crate::resolver::type_parameter_mapping::{
    apply_type_parameter_substitutions, parse_type_in_declaration, type_variable_bindings,
};
use crate::symbol::{ClassType, PrimitiveType, Type};
use crate::type_parser::{parse_type_string, strip_parameter_name};

/// Scoring for overload matching. Lower scores are preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchScore {
    ExactMatch = 0,
    WideningMatch = 1,
    SubtypeMatch = 2,
    BoxingMatch = 3,
    VarargsMatch = 4,
}

/// Conversion required for one argument at the selected call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    Identity,
    PrimitiveWidening,
    Boxing,
    Unboxing,
    ReferenceWidening,
    Varargs,
}

/// How strongly the available types determine the selected declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionConfidence {
    /// Every argument type was known and the signature matched exactly.
    Exact,
    /// Every argument type was known and Java conversions produced a match.
    Compatible,
    /// At least one argument type was unknown, so multiple overloads may fit.
    Incomplete,
}

/// Why a resolved call contains incomplete information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallFallbackReason {
    UnknownArgumentTypes,
    MissingReturnType,
    InvalidReturnType,
}

/// Return information supplied by a syntax-aware lambda analyzer for one
/// candidate's functional parameter.
pub enum LambdaReturnEvidence {
    /// The body has no usable result type yet (for example, it only returns null).
    Unknown,
    /// A concrete expression result constrains the SAM result.
    Value(Type),
    /// A statement expression may implement either a value or void SAM.
    StatementExpression(Type),
    /// A block completes without a value result and can implement a void SAM.
    VoidOnly,
    /// The lambda cannot implement this candidate's functional parameter.
    Incompatible,
}

/// Syntax-derived facts needed to choose a lambda's ground target type.
#[derive(Debug, Clone)]
pub struct LambdaArgument {
    pub argument_index: usize,
    pub explicit_parameter_types: Option<Vec<Type>>,
}

/// The complete output of method candidate selection.
#[derive(Debug, Clone)]
pub struct ResolvedCall {
    /// Declaration, location, and publication selected by the same projected
    /// workspace query. Location consumers must use this instead of looking
    /// the declaration up again by FQN.
    pub definition: ResolvedDefinition,
    /// Compatibility alias for callers that only consume declaration metadata.
    /// New location-sensitive code should use `definition`; both Arcs point to
    /// the same immutable symbol publication.
    pub declaration: Arc<Symbol>,
    /// Receiver- and method-relative parameter types used for applicability.
    pub parameter_types: Vec<Type>,
    /// Receiver- and inference-relative parameters rendered with their source
    /// names and varargs spelling for hover/signature UI.
    pub parameter_declarations: Vec<String>,
    pub substitutions: HashMap<String, Type>,
    pub conversions: Vec<Conversion>,
    pub return_type: Type,
    pub score: MatchScore,
    pub confidence: ResolutionConfidence,
    pub fallback_reason: Option<CallFallbackReason>,
}

#[derive(Debug)]
struct CandidateMatch {
    definition: ResolvedDefinition,
    parameter_types: Vec<Type>,
    projected_parameters: Vec<ProjectedCallableParameter>,
    projected_return_type: Option<String>,
    conversions: Vec<Conversion>,
    score: MatchScore,
    owner_substitutions: HashMap<String, Type>,
    method_substitutions: HashMap<String, Type>,
}

/// Select the best overload for an owner, method name, and inferred arguments.
///
/// `receiver_type` is optional because static and navigation-only callers may
/// know only the owner FQN. When present, its class type arguments are used to
/// substitute generic receiver variables in the return type.
#[must_use]
pub fn resolve_call(
    workspace_index: &WorkspaceIndex,
    owner_fqn: &str,
    receiver_type: Option<&Type>,
    method_name: &str,
    argument_types: &[Type],
) -> Option<ResolvedCall> {
    resolve_call_with_lambda_inference(
        workspace_index,
        owner_fqn,
        receiver_type,
        method_name,
        argument_types,
        &[],
        |_, _| LambdaReturnEvidence::Unknown,
    )
}

/// Resolve a call using return types inferred from lambda arguments.
///
/// The callback receives each candidate's projected SAM input types, so inference
/// never borrows parameter context from a different overload.
#[must_use]
pub fn resolve_call_with_lambda_inference(
    workspace_index: &WorkspaceIndex,
    owner_fqn: &str,
    receiver_type: Option<&Type>,
    method_name: &str,
    argument_types: &[Type],
    lambda_arguments: &[LambdaArgument],
    infer_lambda_return: impl Fn(usize, &[Type]) -> LambdaReturnEvidence,
) -> Option<ResolvedCall> {
    let receiver_arguments = receiver_type
        .and_then(|receiver| match receiver {
            Type::Class(class) => Some(
                class
                    .type_arguments
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let candidates = workspace_index.find_projected_effective_methods_by_name(
        owner_fqn,
        &receiver_arguments,
        method_name,
    );
    resolve_projected_call_with_lambda_inference(
        workspace_index,
        candidates,
        receiver_type,
        argument_types,
        lambda_arguments,
        infer_lambda_return,
    )
}

/// Select one declaration from a visibility-filtered set of projected methods.
///
/// This lets static imports apply their binding rules before all overloads are
/// compared together under the same Java applicability and specificity rules.
#[must_use]
pub fn resolve_projected_call_with_lambda_inference(
    workspace_index: &WorkspaceIndex,
    candidates: Vec<ProjectedEffectiveMethod>,
    receiver_type: Option<&Type>,
    argument_types: &[Type],
    lambda_arguments: &[LambdaArgument],
    infer_lambda_return: impl Fn(usize, &[Type]) -> LambdaReturnEvidence,
) -> Option<ResolvedCall> {
    // An unavailable candidate is not permission to elect one of its siblings:
    // malformed/conflicting generic metadata can change applicability and
    // specificity. Preserve the workspace query's conservative result.
    if candidates
        .iter()
        .any(|candidate| candidate.projection.is_none())
    {
        return None;
    }
    if candidates.iter().any(|candidate| {
        let projection = candidate.projection.as_ref().expect("checked above");
        let is_varargs = projection
            .parameters
            .last()
            .is_some_and(|parameter| parameter.is_variadic());
        let arity_matches = if is_varargs {
            argument_types.len() >= projection.parameters.len().saturating_sub(1)
        } else {
            argument_types.len() == projection.parameters.len()
        };
        if !arity_matches {
            return false;
        }
        let Some(owner) = candidate.definition.symbol.container.as_deref() else {
            return true;
        };
        let declared = projection
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.as_ref())
            .collect::<HashSet<_>>();
        lambda_arguments.iter().any(|lambda| {
            matches!(
                lambda_function_signature(
                    workspace_index,
                    owner,
                    &projection.parameters,
                    lambda,
                    &declared,
                ),
                LambdaSignature::Unavailable
            )
        })
    }) {
        return None;
    }
    let receiver_variables = receiver_type.map_or_else(HashMap::new, |receiver| {
        type_variable_bindings(std::slice::from_ref(receiver))
    });
    let matches: Vec<_> = candidates
        .into_iter()
        .filter_map(|candidate| {
            score_candidate(
                candidate,
                argument_types,
                lambda_arguments,
                &infer_lambda_return,
                workspace_index,
                &receiver_variables,
            )
        })
        .collect();

    let selected = select_best_match(matches, workspace_index)?;

    let mut return_substitutions = receiver_variables;
    return_substitutions.extend(selected.method_substitutions.clone());
    let mut substitutions = selected.owner_substitutions;
    let (return_type, return_fallback) = parse_projected_return_type(
        workspace_index,
        &selected.definition,
        selected.projected_return_type.as_deref(),
        &return_substitutions,
    );
    substitutions.extend(selected.method_substitutions);
    let has_unknown_arguments = argument_types.iter().any(|ty| matches!(ty, Type::Unknown));
    let confidence = if has_unknown_arguments {
        ResolutionConfidence::Incomplete
    } else if selected.score == MatchScore::ExactMatch {
        ResolutionConfidence::Exact
    } else {
        ResolutionConfidence::Compatible
    };
    let fallback_reason = return_fallback
        .or_else(|| has_unknown_arguments.then_some(CallFallbackReason::UnknownArgumentTypes));

    let parameter_declarations = selected
        .projected_parameters
        .iter()
        .zip(&selected.parameter_types)
        .map(|(parameter, ty)| parameter.declaration_with_type(&ty.to_string()))
        .collect();
    let declaration = Arc::clone(&selected.definition.symbol);
    Some(ResolvedCall {
        definition: selected.definition,
        declaration,
        parameter_types: selected.parameter_types,
        parameter_declarations,
        substitutions,
        conversions: selected.conversions,
        return_type,
        score: selected.score,
        confidence,
        fallback_reason,
    })
}

/// Whether an enclosing method tier could own a lambda invocation. An
/// indeterminate signature retains that tier; only proven incompatibility
/// permits later static-import lookup.
#[must_use]
pub fn lambda_call_may_apply(
    workspace_index: &WorkspaceIndex,
    owner_fqn: &str,
    method_name: &str,
    argument_types: &[Type],
    lambda_arguments: &[LambdaArgument],
    infer_lambda_return: impl Fn(usize, &[Type]) -> LambdaReturnEvidence,
) -> bool {
    let candidates =
        workspace_index.find_projected_effective_methods_by_name(owner_fqn, &[], method_name);
    candidates.into_iter().any(|candidate| {
        let Some(projection) = candidate.projection.as_ref() else {
            return true;
        };
        let is_varargs = projection
            .parameters
            .last()
            .is_some_and(|parameter| parameter.is_variadic());
        let arity_mismatch = if is_varargs {
            argument_types.len() < projection.parameters.len().saturating_sub(1)
        } else {
            argument_types.len() != projection.parameters.len()
        };
        if arity_mismatch {
            return false;
        }
        let Some(owner) = candidate.definition.symbol.container.as_deref() else {
            return true;
        };
        let declared = projection
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.as_ref())
            .collect::<HashSet<_>>();
        for lambda in lambda_arguments {
            let index = lambda.argument_index;
            match lambda_function_signature(
                workspace_index,
                owner,
                &projection.parameters,
                lambda,
                &declared,
            ) {
                LambdaSignature::Incompatible => return false,
                LambdaSignature::Unavailable => return true,
                LambdaSignature::Resolved(signature) => {
                    let Some(inputs) = signature
                        .parameter_types
                        .iter()
                        .map(|written| parse_type_string(written))
                        .collect::<Option<Vec<_>>>()
                    else {
                        return true;
                    };
                    let Some(sam_return) = parse_type_string(&signature.return_type) else {
                        return true;
                    };
                    let evidence = infer_lambda_return(index, &inputs);
                    let incompatible = matches!(&evidence, LambdaReturnEvidence::Incompatible)
                        || (sam_return == Type::Void
                            && matches!(&evidence, LambdaReturnEvidence::Value(_)))
                        || (sam_return != Type::Void
                            && matches!(&evidence, LambdaReturnEvidence::VoidOnly));
                    if incompatible {
                        return false;
                    }
                }
            }
        }
        true
    })
}

/// Select from Java's partial "more specific than" relation without presenting
/// it to `slice::sort_by` as a total order.
///
/// Two overloads can be incomparable (for example `append(int)` and
/// `append(String)`). Mixing that partial relation with discovery-order
/// tie-breaking inside a sort comparator is non-transitive and makes Rust's
/// stable sort panic on sufficiently large overload sets such as
/// `StringBuilder.append`.
fn select_best_match(
    mut matches: Vec<CandidateMatch>,
    workspace_index: &WorkspaceIndex,
) -> Option<CandidateMatch> {
    let best_score = matches.iter().map(|candidate| candidate.score).min()?;
    matches.retain(|candidate| candidate.score == best_score);

    let undominated: Vec<usize> = matches
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            let is_dominated = matches.iter().enumerate().any(|(other_index, other)| {
                other_index != candidate_index
                    && compare_specificity(other, candidate, workspace_index)
                        == std::cmp::Ordering::Less
            });
            (!is_dominated).then_some(candidate_index)
        })
        .collect();

    // Java requires one most-specific declaration. Multiple undominated
    // candidates are a genuine ambiguity (`pick(String)` vs `pick(Integer)`
    // for `pick(null)`), not an ordering problem. Likewise, an empty maximal
    // set means the indexed hierarchy is cyclic or incomplete; electing an
    // index-order winner would make semantic answers publication-order
    // dependent.
    let [selected_index] = undominated.as_slice() else {
        return None;
    };

    Some(matches.swap_remove(*selected_index))
}

fn score_candidate(
    candidate: ProjectedEffectiveMethod,
    argument_types: &[Type],
    lambda_arguments: &[LambdaArgument],
    infer_lambda_return: &impl Fn(usize, &[Type]) -> LambdaReturnEvidence,
    workspace_index: &WorkspaceIndex,
    receiver_variables: &HashMap<String, Type>,
) -> Option<CandidateMatch> {
    let projection = candidate.projection?;
    let mut receiver_variables = receiver_variables.clone();
    for parameter in &projection.type_parameters {
        receiver_variables.remove(parameter.name.as_ref());
    }
    let owner_substitutions = parse_projected_owner_bindings(&projection.owner_bindings)?;
    let mut visible_owner_substitutions = owner_substitutions.clone();
    for parameter in &projection.type_parameters {
        visible_owner_substitutions.remove(parameter.name.as_ref());
    }
    let parameter_types: Vec<Type> = projection
        .parameters
        .iter()
        .map(|parameter| {
            // Projection already separates the type from the parameter name.
            // A captured parameter such as `? super T` must retain its bound.
            parse_type_string(&parameter.type_name)
                .map(|ty| apply_type_parameter_substitutions(ty, &receiver_variables))
        })
        .collect::<Option<Vec<_>>>()?;
    let is_varargs = projection
        .parameters
        .last()
        .is_some_and(hyperion_workspace::ProjectedCallableParameter::is_variadic);
    let mut method_substitutions = infer_method_substitutions_from_parameters(
        &projection.type_parameters,
        is_varargs,
        &parameter_types,
        argument_types,
    );
    let declared = projection
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.as_ref())
        .collect::<HashSet<_>>();
    let declaring_owner = candidate.definition.symbol.container.as_deref()?;
    let mut lambda_contracts = Vec::new();
    for lambda in lambda_arguments {
        let index = lambda.argument_index;
        let LambdaSignature::Resolved(signature) = lambda_function_signature(
            workspace_index,
            declaring_owner,
            &projection.parameters,
            lambda,
            &declared,
        ) else {
            return None;
        };
        let Some(input_types) = signature
            .parameter_types
            .iter()
            .map(|written| parse_type_string(written))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let input_types = input_types
            .into_iter()
            .map(|input| {
                let input = apply_type_parameter_substitutions(input, &receiver_variables);
                apply_type_parameter_substitutions(input, &method_substitutions)
            })
            .collect::<Vec<_>>();
        let Some(sam_return) = parse_type_string(&signature.return_type) else {
            continue;
        };
        let lambda_return = match infer_lambda_return(index, &input_types) {
            LambdaReturnEvidence::Unknown => continue,
            LambdaReturnEvidence::Value(value) if sam_return != Type::Void => value,
            LambdaReturnEvidence::StatementExpression(value) if sam_return != Type::Void => value,
            LambdaReturnEvidence::StatementExpression(_) | LambdaReturnEvidence::VoidOnly
                if sam_return == Type::Void =>
            {
                continue
            }
            LambdaReturnEvidence::Incompatible
            | LambdaReturnEvidence::Value(_)
            | LambdaReturnEvidence::StatementExpression(_)
            | LambdaReturnEvidence::VoidOnly => return None,
        };
        let mut inferred_from_lambda = HashMap::new();
        let mut conflicts = HashSet::new();
        collect_method_substitutions(
            &sam_return,
            &lambda_return,
            &declared,
            &mut inferred_from_lambda,
            &mut conflicts,
        );
        if !conflicts.is_empty() {
            return None;
        }
        for (name, inferred) in inferred_from_lambda {
            if let Some(existing) = method_substitutions.get(&name) {
                if match_argument_to_parameter(&inferred, existing, workspace_index).is_some() {
                    continue;
                }
                if match_argument_to_parameter(existing, &inferred, workspace_index).is_some() {
                    method_substitutions.insert(name, inferred);
                } else {
                    return None;
                }
            } else {
                method_substitutions.insert(name, inferred);
            }
        }
        lambda_contracts.push((sam_return, lambda_return));
    }
    let method_substitutions = complete_uninferred_method_substitutions(
        &projection.type_parameters,
        declaring_owner,
        &visible_owner_substitutions,
        method_substitutions,
        workspace_index,
    )?;
    for (sam_return, lambda_return) in lambda_contracts {
        let expected = apply_type_parameter_substitutions(sam_return, &method_substitutions);
        match_argument_to_parameter(&lambda_return, &expected, workspace_index)?;
    }
    if !method_substitutions_satisfy_bounds(
        &projection.type_parameters,
        declaring_owner,
        &visible_owner_substitutions,
        &method_substitutions,
        workspace_index,
    ) {
        return None;
    }
    let parameter_types: Vec<_> = parameter_types
        .into_iter()
        .map(|parameter| apply_type_parameter_substitutions(parameter, &method_substitutions))
        .collect();
    let parameter_count = parameter_types.len();
    let argument_count = argument_types.len();

    if is_varargs {
        if argument_count < parameter_count.saturating_sub(1) {
            return None;
        }
    } else if parameter_count != argument_count {
        return None;
    }

    let mut conversions = Vec::with_capacity(argument_count);
    let mut worst_score = if is_varargs && argument_count == parameter_count.saturating_sub(1) {
        MatchScore::VarargsMatch
    } else {
        MatchScore::ExactMatch
    };

    for (index, argument) in argument_types.iter().enumerate() {
        if matches!(argument, Type::Unknown) {
            conversions.push(Conversion::Identity);
            continue;
        }

        let in_varargs_position = is_varargs && index >= parameter_count.saturating_sub(1);
        if in_varargs_position {
            let array_type = parameter_types.last()?;

            // A single array argument invokes a varargs declaration in fixed-arity
            // mode and must not receive the varargs penalty.
            if index == parameter_count - 1 && argument_count == parameter_count {
                if let Some((score, conversion)) =
                    match_argument_to_parameter(argument, array_type, workspace_index)
                {
                    conversions.push(conversion);
                    worst_score = worst_score.max(score);
                    continue;
                }
            }

            let Type::Array { element_type, .. } = array_type else {
                return None;
            };
            match_argument_to_parameter(argument, element_type, workspace_index)?;
            conversions.push(Conversion::Varargs);
            worst_score = MatchScore::VarargsMatch;
            continue;
        }

        let parameter = parameter_types.get(index)?;
        let (score, conversion) =
            match_argument_to_parameter(argument, parameter, workspace_index)?;
        conversions.push(conversion);
        worst_score = worst_score.max(score);
    }

    Some(CandidateMatch {
        definition: candidate.definition,
        parameter_types,
        projected_parameters: projection.parameters,
        projected_return_type: projection.return_type,
        conversions,
        score: worst_score,
        owner_substitutions,
        method_substitutions,
    })
}

fn parse_projected_owner_bindings(
    bindings: &[ProjectedOwnerBinding],
) -> Option<HashMap<String, Type>> {
    bindings
        .iter()
        .map(|binding| {
            Some((
                binding.name.to_string(),
                parse_type_string(&binding.type_name)?,
            ))
        })
        .collect()
}

/// Infer declared method type parameters from actual argument types.
///
/// This is shared with the legacy semantic resolver until all call paths route
/// through [`resolve_call`]. Unknown names are never treated as type variables.
pub fn infer_method_type_substitutions(
    declaration: &Symbol,
    argument_types: &[Type],
) -> HashMap<String, Type> {
    let Some(parameter_types) = declaration
        .parameters
        .iter()
        .map(|parameter| parse_parameter_type(parameter))
        .collect::<Option<Vec<_>>>()
    else {
        return HashMap::new();
    };
    let is_varargs = declaration
        .parameters
        .last()
        .is_some_and(|parameter| parameter.contains("..."));
    infer_method_substitutions_from_parameters(
        &declaration.type_parameters,
        is_varargs,
        &parameter_types,
        argument_types,
    )
}

fn infer_method_substitutions_from_parameters(
    type_parameters: &[hyperion_workspace::TypeParameterInfo],
    is_varargs: bool,
    parameter_types: &[Type],
    argument_types: &[Type],
) -> HashMap<String, Type> {
    let declared: HashSet<&str> = type_parameters
        .iter()
        .map(|parameter| parameter.name.as_ref())
        .collect();
    if declared.is_empty() {
        return HashMap::new();
    }

    let mut substitutions = HashMap::new();
    let mut conflicts = HashSet::new();
    let varargs_index = is_varargs.then(|| parameter_types.len().saturating_sub(1));

    for (index, argument) in argument_types.iter().enumerate() {
        let parameter_index = index.min(parameter_types.len().saturating_sub(1));
        let Some(parameter) = parameter_types.get(parameter_index) else {
            break;
        };
        let parameter = if varargs_index == Some(parameter_index)
            && !(index == parameter_index
                && argument_types.len() == parameter_types.len()
                && matches!(argument, Type::Array { .. }))
        {
            let Type::Array { element_type, .. } = parameter else {
                continue;
            };
            element_type.as_ref()
        } else {
            parameter
        };
        collect_method_substitutions(
            parameter,
            argument,
            &declared,
            &mut substitutions,
            &mut conflicts,
        );
    }
    substitutions
}

/// Find the functional contract for a lambda argument in this candidate's
/// projected parameter list. Each expanded vararg uses the array element type.
enum LambdaSignature {
    Resolved(hyperion_workspace::FunctionalInterfaceSignature),
    Incompatible,
    Unavailable,
}

fn lambda_function_signature(
    workspace_index: &WorkspaceIndex,
    declaring_owner: &str,
    parameters: &[ProjectedCallableParameter],
    lambda: &LambdaArgument,
    declared: &HashSet<&str>,
) -> LambdaSignature {
    let argument_index = lambda.argument_index;
    let variadic = parameters
        .last()
        .filter(|parameter| parameter.is_variadic());
    let Some(parameter) = parameters.get(argument_index).or(variadic) else {
        return LambdaSignature::Incompatible;
    };
    let Some(parameter_type) = parse_type_in_declaration(
        workspace_index,
        declaring_owner,
        &parameter.type_name,
        declared,
    ) else {
        return LambdaSignature::Unavailable;
    };
    let functional_type = if parameter.is_variadic() {
        match parameter_type {
            Type::Array {
                element_type,
                dimensions: 1,
            } => *element_type,
            other => other,
        }
    } else {
        parameter_type
    };
    let interface = match functional_type {
        Type::Class(interface) => interface,
        Type::Primitive(_) | Type::Array { .. } | Type::Null | Type::Void => {
            return LambdaSignature::Incompatible;
        }
        Type::TypeVariable { .. }
        | Type::Wildcard { .. }
        | Type::Intersection { .. }
        | Type::Union { .. }
        | Type::Unknown => return LambdaSignature::Unavailable,
    };
    let interface = match specialize_explicit_lambda_target(workspace_index, interface, lambda) {
        Ok(interface) => interface,
        Err(failure) => return failure,
    };
    let type_arguments = interface
        .type_arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    match workspace_index.query_functional_interface(&interface.qualified_name(), &type_arguments) {
        hyperion_workspace::FunctionalInterfaceQuery::Resolved(signature)
            if parse_type_string(&signature.return_type).is_some()
                && signature
                    .parameter_types
                    .iter()
                    .all(|written| parse_type_string(written).is_some()) =>
        {
            LambdaSignature::Resolved(signature)
        }
        hyperion_workspace::FunctionalInterfaceQuery::Resolved(_) => LambdaSignature::Unavailable,
        hyperion_workspace::FunctionalInterfaceQuery::NotFunctional => {
            LambdaSignature::Incompatible
        }
        hyperion_workspace::FunctionalInterfaceQuery::Missing
        | hyperion_workspace::FunctionalInterfaceQuery::Unavailable => LambdaSignature::Unavailable,
    }
}

/// JLS 18.5.3: explicit lambda inputs constrain wildcard target arguments.
/// Re-querying the specialized target also projects its output consistently;
/// accepting a wider input while keeping an implicitly grounded output would
/// incorrectly reject e.g. UnaryOperator<? super String> with Object -> Object.
fn specialize_explicit_lambda_target(
    workspace: &WorkspaceIndex,
    mut interface: ClassType,
    lambda: &LambdaArgument,
) -> Result<ClassType, LambdaSignature> {
    let Some(explicit) = lambda.explicit_parameter_types.as_ref() else {
        return Ok(interface);
    };
    if !interface
        .type_arguments
        .iter()
        .any(|argument| matches!(argument, Type::Wildcard { .. }))
    {
        return Ok(interface);
    }
    let owner = interface.qualified_name().into_owned();
    let declaration = workspace
        .get_symbol(&owner)
        .ok_or(LambdaSignature::Unavailable)?;
    if declaration.type_parameters.len() != interface.type_arguments.len() {
        return Err(LambdaSignature::Unavailable);
    }
    let variables = declaration
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.to_string())
        .collect::<Vec<_>>();
    let hyperion_workspace::FunctionalInterfaceQuery::Resolved(generic) =
        workspace.query_functional_interface(&owner, &variables)
    else {
        return Err(LambdaSignature::Unavailable);
    };
    if generic.parameter_types.len() != explicit.len() {
        return Err(LambdaSignature::Incompatible);
    }
    let declared = variables.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut inferred = HashMap::new();
    let mut conflicts = HashSet::new();
    for (parameter, actual) in generic.parameter_types.iter().zip(explicit) {
        let parameter = parse_type_string(parameter).ok_or(LambdaSignature::Unavailable)?;
        collect_method_substitutions(&parameter, actual, &declared, &mut inferred, &mut conflicts);
    }
    if !conflicts.is_empty() {
        return Err(LambdaSignature::Incompatible);
    }
    for (parameter, target) in declaration
        .type_parameters
        .iter()
        .zip(&mut interface.type_arguments)
    {
        let Some(actual) = inferred.get(parameter.name.as_ref()) else {
            continue;
        };
        let compatible = match &*target {
            Type::Wildcard {
                lower_bound,
                upper_bound,
            } => {
                lower_bound
                    .as_ref()
                    .is_none_or(|bound| actual.is_assignable_from_with_workspace(bound, workspace))
                    && upper_bound.as_ref().is_none_or(|bound| {
                        bound.is_assignable_from_with_workspace(actual, workspace)
                    })
            }
            target => target == actual,
        };
        if !compatible {
            return Err(LambdaSignature::Incompatible);
        }
        *target = actual.clone();
    }
    let bindings = declaration
        .type_parameters
        .iter()
        .zip(&interface.type_arguments)
        .map(|(parameter, argument)| (parameter.name.to_string(), argument.clone()))
        .collect();
    if !method_substitutions_satisfy_bounds(
        &declaration.type_parameters,
        &owner,
        &HashMap::new(),
        &bindings,
        workspace,
    ) {
        return Err(LambdaSignature::Incompatible);
    }
    Ok(interface)
}

fn collect_method_substitutions(
    parameter: &Type,
    argument: &Type,
    declared: &HashSet<&str>,
    substitutions: &mut HashMap<String, Type>,
    conflicts: &mut HashSet<String>,
) {
    if matches!(argument, Type::Unknown) {
        return;
    }
    match (parameter, argument) {
        (Type::Class(parameter), _)
            if parameter.package.is_none()
                && parameter.type_arguments.is_empty()
                && declared.contains(parameter.name.as_str()) =>
        {
            bind_inferred_method_type(&parameter.name, argument, substitutions, conflicts);
        }
        (Type::TypeVariable { name, .. }, _) if declared.contains(name.as_str()) => {
            bind_inferred_method_type(name, argument, substitutions, conflicts);
        }
        (Type::Class(parameter), Type::Class(argument)) => {
            for (parameter_arg, argument_arg) in parameter
                .type_arguments
                .iter()
                .zip(&argument.type_arguments)
            {
                collect_method_substitutions(
                    parameter_arg,
                    argument_arg,
                    declared,
                    substitutions,
                    conflicts,
                );
            }
        }
        (
            Type::Array {
                element_type: parameter,
                dimensions: parameter_dimensions,
            },
            Type::Array {
                element_type: argument,
                dimensions: argument_dimensions,
            },
        ) if parameter_dimensions == argument_dimensions => {
            collect_method_substitutions(parameter, argument, declared, substitutions, conflicts);
        }
        _ => {}
    }
}

fn bind_inferred_method_type(
    name: &str,
    argument: &Type,
    substitutions: &mut HashMap<String, Type>,
    conflicts: &mut HashSet<String>,
) {
    if conflicts.contains(name) {
        return;
    }
    // Java type inference never instantiates a type variable with a primitive;
    // invocation conversion boxes it before the constraint is incorporated.
    let inferred = match argument {
        Type::Primitive(primitive) => Type::Class(java_lang_class(primitive_wrapper(*primitive))),
        argument => argument.clone(),
    };
    match substitutions.get(name) {
        None => {
            substitutions.insert(name.to_string(), inferred);
        }
        Some(existing) if existing == &inferred => {}
        Some(_) => {
            substitutions.remove(name);
            conflicts.insert(name.to_string());
        }
    }
}

/// Validate inferred method arguments against their declared bounds in
/// declaration order. The current variable is visible in its own recursive
/// bounds (`T extends Comparable<T>`), while substitutions validated for
/// earlier variables are visible to dependent later bounds.
fn method_substitutions_satisfy_bounds(
    type_parameters: &[hyperion_workspace::TypeParameterInfo],
    declaring_owner: &str,
    owner_substitutions: &HashMap<String, Type>,
    method_substitutions: &HashMap<String, Type>,
    workspace_index: &WorkspaceIndex,
) -> bool {
    let mut visible_substitutions = owner_substitutions.clone();
    let type_parameter_names = owner_substitutions
        .keys()
        .map(String::as_str)
        .chain(
            type_parameters
                .iter()
                .map(|parameter| parameter.name.as_ref()),
        )
        .collect::<HashSet<_>>();
    for parameter in type_parameters {
        let Some(inferred) = method_substitutions.get(parameter.name.as_ref()) else {
            continue;
        };
        visible_substitutions.insert(parameter.name.to_string(), inferred.clone());
        for bound in &parameter.bounds {
            let Some(bound) = parse_type_in_declaration(
                workspace_index,
                declaring_owner,
                bound,
                &type_parameter_names,
            ) else {
                return false;
            };
            let bound = apply_type_parameter_substitutions(bound, &visible_substitutions);
            if !inferred_type_satisfies_bound(inferred, &bound, workspace_index) {
                return false;
            }
        }
    }
    true
}

/// With no target type, javac instantiates an otherwise unconstrained method
/// variable from its upper bound: Object when unbounded, or the erasure of the
/// substituted leftmost bound. Do this in declaration order so a later bound
/// may depend on an earlier variable. Recursive/forward dependencies remain
/// unavailable instead of being guessed.
fn complete_uninferred_method_substitutions(
    type_parameters: &[hyperion_workspace::TypeParameterInfo],
    declaring_owner: &str,
    owner_substitutions: &HashMap<String, Type>,
    mut method_substitutions: HashMap<String, Type>,
    workspace_index: &WorkspaceIndex,
) -> Option<HashMap<String, Type>> {
    if type_parameters.is_empty() {
        return Some(method_substitutions);
    }
    let method_parameter_names = type_parameters
        .iter()
        .map(|parameter| parameter.name.as_ref())
        .collect::<HashSet<_>>();
    let protected_names = owner_substitutions
        .keys()
        .map(String::as_str)
        .chain(method_parameter_names.iter().copied())
        .collect::<HashSet<_>>();
    let mut visible_substitutions = owner_substitutions.clone();
    visible_substitutions.extend(method_substitutions.clone());

    for parameter in type_parameters {
        if let Some(inferred) = method_substitutions.get(parameter.name.as_ref()) {
            visible_substitutions.insert(parameter.name.to_string(), inferred.clone());
            continue;
        }
        let inferred = match parameter.bounds.first() {
            None => Type::Class(ClassType::object()),
            Some(bound) => {
                let bound = parse_type_in_declaration(
                    workspace_index,
                    declaring_owner,
                    bound,
                    &protected_names,
                )?;
                let bound = apply_type_parameter_substitutions(bound, &visible_substitutions);
                if contains_unresolved_method_parameter(
                    &bound,
                    &method_parameter_names,
                    &visible_substitutions,
                ) {
                    return None;
                }
                bound.erasure()
            }
        };
        visible_substitutions.insert(parameter.name.to_string(), inferred.clone());
        method_substitutions.insert(parameter.name.to_string(), inferred);
    }
    Some(method_substitutions)
}

fn contains_unresolved_method_parameter(
    ty: &Type,
    method_parameter_names: &HashSet<&str>,
    substitutions: &HashMap<String, Type>,
) -> bool {
    match ty {
        Type::Class(class_type) => {
            (class_type.package.is_none()
                && method_parameter_names.contains(class_type.name.as_str())
                && !substitutions.contains_key(&class_type.name))
                || class_type.type_arguments.iter().any(|argument| {
                    contains_unresolved_method_parameter(
                        argument,
                        method_parameter_names,
                        substitutions,
                    )
                })
        }
        Type::TypeVariable { name, bounds } => {
            (method_parameter_names.contains(name.as_str()) && !substitutions.contains_key(name))
                || bounds.iter().any(|bound| {
                    contains_unresolved_method_parameter(
                        bound,
                        method_parameter_names,
                        substitutions,
                    )
                })
        }
        Type::Array { element_type, .. } => contains_unresolved_method_parameter(
            element_type,
            method_parameter_names,
            substitutions,
        ),
        Type::Wildcard {
            upper_bound,
            lower_bound,
        } => upper_bound.iter().chain(lower_bound).any(|bound| {
            contains_unresolved_method_parameter(bound, method_parameter_names, substitutions)
        }),
        Type::Intersection { types } | Type::Union { types } => types.iter().any(|bound| {
            contains_unresolved_method_parameter(bound, method_parameter_names, substitutions)
        }),
        Type::Primitive(_) | Type::Null | Type::Void | Type::Unknown => false,
    }
}

fn inferred_type_satisfies_bound(
    inferred: &Type,
    bound: &Type,
    workspace_index: &WorkspaceIndex,
) -> bool {
    if let Type::Intersection { types } = bound {
        return types
            .iter()
            .all(|bound| inferred_type_satisfies_bound(inferred, bound, workspace_index));
    }
    if !bound.is_assignable_from_with_workspace(inferred, workspace_index) {
        return false;
    }

    let (Type::Class(inferred_class), Type::Class(bound_class)) = (inferred, bound) else {
        return true;
    };
    if bound_class.type_arguments.is_empty()
        || inferred_class.raw_qualified_name() == bound_class.raw_qualified_name()
    {
        return true;
    }

    // Nominal assignability alone loses generic arguments when the inferred
    // type reaches the bound through a supertype. Re-project that supertype
    // and compare the actual parameterization; an absent/raw/conflicting
    // projection is not proof that the bound holds.
    let inferred_arguments = inferred_class
        .type_arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let projected = workspace_index.project_owner_type_arguments(
        &inferred_class.raw_qualified_name(),
        &inferred_arguments,
        &bound_class.raw_qualified_name(),
    );
    let Some(projected) = projected else {
        return false;
    };
    let Some(offset) = projected
        .len()
        .checked_sub(bound_class.type_arguments.len())
    else {
        return false;
    };
    let projected_arguments = projected[offset..]
        .iter()
        .map(|(_, argument)| argument.as_deref().and_then(parse_type_string))
        .collect::<Option<Vec<_>>>();
    let Some(projected_arguments) = projected_arguments else {
        return false;
    };
    let projected_bound = Type::Class(ClassType {
        name: bound_class.name.clone(),
        package: bound_class.package.clone(),
        type_arguments: projected_arguments,
        is_diamond: false,
    });
    bound.is_assignable_from_with_workspace(&projected_bound, workspace_index)
}

fn compare_specificity(
    left: &CandidateMatch,
    right: &CandidateMatch,
    workspace_index: &WorkspaceIndex,
) -> std::cmp::Ordering {
    let left_is_more_specific = parameters_are_more_specific(
        &left.parameter_types,
        &right.parameter_types,
        workspace_index,
    );
    let right_is_more_specific = parameters_are_more_specific(
        &right.parameter_types,
        &left.parameter_types,
        workspace_index,
    );
    match (left_is_more_specific, right_is_more_specific) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

fn parameters_are_more_specific(
    candidate: &[Type],
    other: &[Type],
    workspace_index: &WorkspaceIndex,
) -> bool {
    if candidate.len() != other.len() {
        return false;
    }

    let mut found_strictly_more_specific = false;
    for (candidate_type, other_type) in candidate.iter().zip(other) {
        if types_equal(candidate_type, other_type) {
            continue;
        }
        if !is_reference_assignable(candidate_type, other_type, workspace_index) {
            return false;
        }
        found_strictly_more_specific = true;
    }
    found_strictly_more_specific
}

fn parse_projected_return_type(
    workspace_index: &WorkspaceIndex,
    definition: &ResolvedDefinition,
    projected_return_type: Option<&str>,
    substitutions: &HashMap<String, Type>,
) -> (Type, Option<CallFallbackReason>) {
    let Some(raw_return_type) = projected_return_type else {
        return (Type::Unknown, Some(CallFallbackReason::MissingReturnType));
    };
    let Some(owner_fqn) = definition.symbol.container.as_deref() else {
        return (Type::Unknown, Some(CallFallbackReason::InvalidReturnType));
    };
    let method_type_parameters = definition
        .symbol
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.as_ref())
        .collect::<HashSet<_>>();
    let Some(parsed) = parse_type_in_declaration(
        workspace_index,
        owner_fqn,
        raw_return_type,
        &method_type_parameters,
    ) else {
        return (Type::Unknown, Some(CallFallbackReason::InvalidReturnType));
    };
    let resolved = apply_type_parameter_substitutions(parsed, substitutions);
    (resolved, None)
}

fn match_argument_to_parameter(
    argument: &Type,
    parameter: &Type,
    workspace_index: &WorkspaceIndex,
) -> Option<(MatchScore, Conversion)> {
    if matches!(argument, Type::Null) && !matches!(parameter, Type::Primitive(_)) {
        return Some((MatchScore::ExactMatch, Conversion::Identity));
    }
    if let Type::Wildcard { lower_bound, .. } = parameter {
        // A wildcard projected into a method input denotes a capture, not
        // type-argument containment (JLS 5.1.10). Only its lower bound proves
        // that a non-null value may be written. An upper/unbounded capture
        // cannot accept an arbitrary value, even one with the same spelling.
        return lower_bound.as_ref().and_then(|lower| {
            let (score, conversion) =
                match_argument_to_parameter(argument, lower, workspace_index)?;
            Some((
                score.max(MatchScore::SubtypeMatch),
                if conversion == Conversion::Identity {
                    Conversion::ReferenceWidening
                } else {
                    conversion
                },
            ))
        });
    }
    if types_equal(argument, parameter) {
        return Some((MatchScore::ExactMatch, Conversion::Identity));
    }
    if let (Type::Primitive(argument), Type::Primitive(parameter)) = (argument, parameter) {
        if is_widening_primitive(*argument, *parameter) {
            return Some((MatchScore::WideningMatch, Conversion::PrimitiveWidening));
        }
    }
    if let Some(conversion) = check_boxing_conversion(argument, parameter, workspace_index) {
        return Some(conversion);
    }
    if let Some(conversion) = check_unboxing_conversion(argument, parameter) {
        return Some(conversion);
    }
    if is_reference_assignable(argument, parameter, workspace_index) {
        return Some((MatchScore::SubtypeMatch, Conversion::ReferenceWidening));
    }
    None
}

fn types_equal(left: &Type, right: &Type) -> bool {
    left == right
}

const fn is_widening_primitive(from: PrimitiveType, to: PrimitiveType) -> bool {
    use PrimitiveType::{Byte, Char, Double, Float, Int, Long, Short};
    matches!(
        (from, to),
        (Byte, Short | Int | Long | Float | Double)
            | (Short | Char, Int | Long | Float | Double)
            | (Int, Long | Float | Double)
            | (Long, Float | Double)
            | (Float, Double)
    )
}

fn check_boxing_conversion(
    argument: &Type,
    parameter: &Type,
    workspace_index: &WorkspaceIndex,
) -> Option<(MatchScore, Conversion)> {
    let (Type::Primitive(primitive), Type::Class(class)) = (argument, parameter) else {
        return None;
    };
    let boxed = Type::Class(java_lang_class(primitive_wrapper(*primitive)));
    // Source declarations are normally canonicalized before reaching this
    // layer. Preserve the parser-only shorthand (`Integer`, `Object`, ...)
    // without allowing an explicitly different package to impersonate a
    // java.lang declaration.
    let canonical_parameter = canonical_java_lang_spelling(class).map(Type::Class);
    let parameter = canonical_parameter.as_ref().unwrap_or(parameter);

    (is_reference_assignable(&boxed, parameter, workspace_index)
        || is_known_jdk_reference_widening(&boxed, parameter))
    .then_some((MatchScore::BoxingMatch, Conversion::Boxing))
}

fn check_unboxing_conversion(
    argument: &Type,
    parameter: &Type,
) -> Option<(MatchScore, Conversion)> {
    let (Type::Class(class), Type::Primitive(primitive)) = (argument, parameter) else {
        return None;
    };
    wrapper_primitive(class)
        .is_some_and(|unboxed| unboxed == *primitive || is_widening_primitive(unboxed, *primitive))
        .then_some((MatchScore::BoxingMatch, Conversion::Unboxing))
}

fn wrapper_primitive(class: &ClassType) -> Option<PrimitiveType> {
    if class
        .package
        .as_deref()
        .is_some_and(|package| package != "java.lang")
    {
        return None;
    }
    match class.name.as_str() {
        "Boolean" => Some(PrimitiveType::Boolean),
        "Byte" => Some(PrimitiveType::Byte),
        "Short" => Some(PrimitiveType::Short),
        "Integer" => Some(PrimitiveType::Int),
        "Long" => Some(PrimitiveType::Long),
        "Character" => Some(PrimitiveType::Char),
        "Float" => Some(PrimitiveType::Float),
        "Double" => Some(PrimitiveType::Double),
        _ => None,
    }
}

const fn primitive_wrapper(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Int => "Integer",
        PrimitiveType::Long => "Long",
        PrimitiveType::Short => "Short",
        PrimitiveType::Byte => "Byte",
        PrimitiveType::Float => "Float",
        PrimitiveType::Double => "Double",
        PrimitiveType::Char => "Character",
        PrimitiveType::Boolean => "Boolean",
    }
}

fn is_reference_assignable(
    argument: &Type,
    parameter: &Type,
    workspace_index: &WorkspaceIndex,
) -> bool {
    if !argument.is_reference() || !parameter.is_reference() {
        return false;
    }

    if let (Type::Array { .. }, Type::Class(class)) = (argument, parameter) {
        return is_array_supertype(class);
    }

    parameter.is_assignable_from_with_workspace(argument, workspace_index)
}

fn java_lang_class(name: &str) -> ClassType {
    ClassType::new(name.to_string(), Some("java.lang".to_string()))
}

fn canonical_java_lang_spelling(class: &ClassType) -> Option<ClassType> {
    class.package.is_none().then(|| ClassType {
        package: Some("java.lang".to_string()),
        ..class.clone()
    })
}

fn is_array_supertype(class: &ClassType) -> bool {
    matches!(
        (class.package.as_deref(), class.name.as_str()),
        (Some("java.lang") | None, "Object" | "Cloneable")
            | (Some("java.io") | None, "Serializable")
    )
}

fn is_known_jdk_reference_widening(argument: &Type, parameter: &Type) -> bool {
    let (Type::Class(argument), Type::Class(parameter)) = (argument, parameter) else {
        return false;
    };
    crate::types::is_known_jdk_subtype(
        &argument.raw_qualified_name(),
        &parameter.raw_qualified_name(),
    )
}

/// Parse a workspace-index parameter declaration into its semantic type.
#[must_use]
pub fn parse_parameter_type(parameter: &str) -> Option<Type> {
    let parameter = strip_leading_parameter_modifiers(parameter);
    let is_varargs = parameter.contains("...");
    let type_name = strip_parameter_name(parameter);
    let parsed = parse_type_string(type_name)?;
    if is_varargs {
        Some(Type::Array {
            element_type: Box::new(parsed),
            dimensions: 1,
        })
    } else {
        Some(parsed)
    }
}

fn strip_leading_parameter_modifiers(mut parameter: &str) -> &str {
    loop {
        let trimmed = parameter.trim_start();
        if let Some(rest) = trimmed.strip_prefix("final ") {
            parameter = rest;
            continue;
        }
        if trimmed.starts_with('@') {
            if let Some(space) = trimmed.find(char::is_whitespace) {
                parameter = &trimmed[space..];
                continue;
            }
        }
        return trimmed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str) -> Type {
        Type::Class(ClassType {
            name: name.to_string(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        })
    }

    fn qualified_class(package: &str, name: &str) -> Type {
        Type::Class(ClassType::new(name.to_string(), Some(package.to_string())))
    }

    fn array(element_type: Type, dimensions: u8) -> Type {
        Type::Array {
            element_type: Box::new(element_type),
            dimensions,
        }
    }

    #[test]
    fn parses_named_unnamed_array_and_varargs_parameters() {
        assert_eq!(
            parse_parameter_type("int count"),
            Some(Type::Primitive(PrimitiveType::Int))
        );
        assert_eq!(
            parse_parameter_type("java.lang.String"),
            parse_type_string("java.lang.String")
        );
        assert!(matches!(
            parse_parameter_type("String[][] names"),
            Some(Type::Array { dimensions: 2, .. })
        ));
        assert!(matches!(
            parse_parameter_type("final @Nullable String... names"),
            Some(Type::Array { dimensions: 1, .. })
        ));
    }

    #[test]
    fn projected_wildcard_inputs_follow_capture_write_bounds() {
        let temp = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file_with_source(&temp.path().join("p/Types.java"),
            "package p; class Base {} class Child extends Base {} interface Sink<E> { void put(E value); }").unwrap();
        let base = qualified_class("p", "Base");
        let child = qualified_class("p", "Child");
        let object = Type::Class(ClassType::object());
        let receiver = |argument| {
            Type::Class(ClassType {
                name: "Sink".into(),
                package: Some("p".into()),
                type_arguments: vec![argument],
                is_diamond: false,
            })
        };
        let lower = receiver(Type::Wildcard {
            upper_bound: None,
            lower_bound: Some(Box::new(base.clone())),
        });
        for argument in [&base, &child, &Type::Null] {
            assert!(
                resolve_call(
                    &index,
                    "p.Sink",
                    Some(&lower),
                    "put",
                    std::slice::from_ref(argument)
                )
                .is_some(),
                "{argument}"
            );
        }
        assert!(resolve_call(
            &index,
            "p.Sink",
            Some(&lower),
            "put",
            std::slice::from_ref(&object)
        )
        .is_none());
        for wildcard in [
            Type::Wildcard {
                upper_bound: Some(Box::new(base.clone())),
                lower_bound: None,
            },
            Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            },
        ] {
            let captured = receiver(wildcard.clone());
            for argument in [&base, &child, &object, &wildcard] {
                assert!(
                    resolve_call(
                        &index,
                        "p.Sink",
                        Some(&captured),
                        "put",
                        std::slice::from_ref(argument)
                    )
                    .is_none(),
                    "{captured}: {argument}"
                );
            }
            assert!(
                resolve_call(&index, "p.Sink", Some(&captured), "put", &[Type::Null]).is_some()
            );
        }
        let variable = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![base],
        };
        let generic = receiver(Type::Wildcard {
            upper_bound: None,
            lower_bound: Some(Box::new(variable.clone())),
        });
        assert!(resolve_call(&index, "p.Sink", Some(&generic), "put", &[variable]).is_some());
    }

    #[test]
    fn concrete_overload_precedes_widening_into_a_wildcard_capture() {
        let temp = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Sink.java"),
                "package p; interface Sink<E> { void put(E value); void put(String value); }",
            )
            .unwrap();
        let string = Type::Class(ClassType::string());
        let receiver = Type::Class(ClassType {
            name: "Sink".into(),
            package: Some("p".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(string.clone())),
            }],
            is_diamond: false,
        });
        let resolved = resolve_call(
            &index,
            "p.Sink",
            Some(&receiver),
            "put",
            std::slice::from_ref(&string),
        )
        .unwrap();
        assert_eq!(resolved.parameter_types, [string]);
        assert_eq!(resolved.score, MatchScore::ExactMatch);
    }

    #[test]
    fn primitive_conversions_are_classified() {
        let index = WorkspaceIndex::new(std::path::PathBuf::from("/tmp/call-resolution"));
        assert_eq!(
            match_argument_to_parameter(
                &Type::Primitive(PrimitiveType::Int),
                &Type::Primitive(PrimitiveType::Long),
                &index,
            ),
            Some((MatchScore::WideningMatch, Conversion::PrimitiveWidening))
        );
        assert_eq!(
            match_argument_to_parameter(
                &Type::Primitive(PrimitiveType::Int),
                &class("Integer"),
                &index,
            ),
            Some((MatchScore::BoxingMatch, Conversion::Boxing))
        );
    }

    #[test]
    fn reference_widening_requires_exact_package_identity() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                "package p; class Base {} class Child extends Base {}",
            )
            .expect("p hierarchy should index");
        index
            .index_file_with_source(&temp.path().join("q/Base.java"), "package q; class Base {}")
            .expect("unrelated q.Base should index");

        let child = qualified_class("p", "Child");
        assert_eq!(
            match_argument_to_parameter(&child, &qualified_class("p", "Base"), &index,),
            Some((MatchScore::SubtypeMatch, Conversion::ReferenceWidening))
        );
        assert_eq!(
            match_argument_to_parameter(&child, &qualified_class("q", "Base"), &index,),
            None,
            "an ancestor's simple name must not satisfy an unrelated package identity"
        );
    }

    #[test]
    fn boxing_and_unboxing_require_java_lang_wrapper_identity() {
        let index = WorkspaceIndex::new(std::path::PathBuf::from("/tmp/call-resolution-boxing"));
        let int = Type::Primitive(PrimitiveType::Int);

        assert_eq!(
            match_argument_to_parameter(&int, &qualified_class("java.lang", "Integer"), &index,),
            Some((MatchScore::BoxingMatch, Conversion::Boxing))
        );
        assert_eq!(
            match_argument_to_parameter(&int, &qualified_class("java.lang", "Number"), &index),
            Some((MatchScore::BoxingMatch, Conversion::Boxing)),
            "boxing followed by reference widening remains a loose invocation conversion"
        );
        assert_eq!(
            match_argument_to_parameter(
                &qualified_class("java.lang", "Integer"),
                &Type::Primitive(PrimitiveType::Long),
                &index,
            ),
            Some((MatchScore::BoxingMatch, Conversion::Unboxing)),
            "unboxing followed by primitive widening remains legal"
        );
        assert_eq!(
            match_argument_to_parameter(&int, &qualified_class("evil", "Integer"), &index),
            None,
            "a same-simple-name user class is not a primitive wrapper"
        );
        assert_eq!(
            match_argument_to_parameter(
                &qualified_class("evil", "Integer"),
                &Type::Primitive(PrimitiveType::Long),
                &index,
            ),
            None,
            "unboxing must reject a wrapper impostor too"
        );
    }

    #[test]
    fn only_java_lang_object_is_the_universal_reference_target() {
        let index = WorkspaceIndex::new(std::path::PathBuf::from("/tmp/call-resolution-object"));
        let evil_object = qualified_class("evil", "Object");

        assert_eq!(
            match_argument_to_parameter(&Type::Class(ClassType::string()), &evil_object, &index,),
            None
        );
        assert_eq!(
            match_argument_to_parameter(&Type::Primitive(PrimitiveType::Int), &evil_object, &index,),
            None
        );
        assert_eq!(
            match_argument_to_parameter(
                &array(Type::Class(ClassType::string()), 1),
                &evil_object,
                &index,
            ),
            None,
            "arrays widen only to the actual java.lang.Object"
        );
    }

    #[test]
    fn array_reference_conversions_use_exact_jls_supertypes() {
        let index = WorkspaceIndex::new(std::path::PathBuf::from("/tmp/call-resolution-arrays"));
        let string = Type::Class(ClassType::string());
        let string_array = array(string.clone(), 1);

        for target in [
            qualified_class("java.lang", "Object"),
            qualified_class("java.lang", "Cloneable"),
            qualified_class("java.io", "Serializable"),
        ] {
            assert_eq!(
                match_argument_to_parameter(&string_array, &target, &index),
                Some((MatchScore::SubtypeMatch, Conversion::ReferenceWidening))
            );
        }
        for impostor in [
            qualified_class("evil", "Object"),
            qualified_class("evil", "Cloneable"),
            qualified_class("evil", "Serializable"),
        ] {
            assert_eq!(
                match_argument_to_parameter(&string_array, &impostor, &index),
                None
            );
        }

        assert_eq!(
            match_argument_to_parameter(
                &array(string, 2),
                &array(qualified_class("java.lang", "Object"), 1),
                &index,
            ),
            Some((MatchScore::SubtypeMatch, Conversion::ReferenceWidening)),
            "a nested reference array is itself an Object-compatible component"
        );
        assert_eq!(
            match_argument_to_parameter(
                &array(Type::Primitive(PrimitiveType::Int), 1),
                &array(Type::Primitive(PrimitiveType::Long), 1),
                &index,
            ),
            None,
            "primitive arrays are invariant even though their elements widen"
        );
    }

    #[test]
    fn source_overload_resolution_selects_by_argument_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class TypeOverloadDemo {
                public void process(Integer value) {}
                public String process(String value) { return value; }
            }
        "#;
        let path = temp.path().join("com/example/TypeOverloadDemo.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let resolved = resolve_call(
            &index,
            "com.example.TypeOverloadDemo",
            None,
            "process",
            &[Type::Class(ClassType::string())],
        )
        .expect("String overload should resolve");

        assert_eq!(
            resolved.declaration.parameters.first().map(AsRef::as_ref),
            Some("java.lang.String value")
        );
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
        assert_eq!(resolved.confidence, ResolutionConfidence::Exact);
        assert_eq!(resolved.fallback_reason, None);
    }

    #[test]
    fn class_override_is_one_effective_candidate_with_covariant_return() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Base { ",
                    "public Object convert(String value) { return value; } } ",
                    "class Child extends Base { ",
                    "public String convert(String value) { return value; } }"
                ),
            )
            .expect("hierarchy should index");

        let resolved = resolve_call(
            &index,
            "p.Child",
            None,
            "convert",
            &[Type::Class(ClassType::string())],
        )
        .expect("the override must not be ambiguous with its parent declaration");

        assert_eq!(resolved.declaration.container.as_deref(), Some("p.Child"));
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
    }

    #[test]
    fn inherited_generic_override_is_elected_before_overload_scoring() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("java/lang/String.java"),
                "package java.lang; public final class String {}",
            )
            .expect("String fixture should index");
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Base<T> { ",
                    "public T convert(T value) { return value; } } ",
                    "class Child extends Base<String> { ",
                    "@Override public String convert(String value) { return value; } }"
                ),
            )
            .expect("generic hierarchy should index");
        let receiver = Type::Class(ClassType::new("Child".to_owned(), Some("p".to_owned())));

        let resolved = resolve_call(
            &index,
            "p.Child",
            Some(&receiver),
            "convert",
            &[Type::Class(ClassType::string())],
        )
        .expect("the projected override must not become an overload ambiguity");

        assert_eq!(resolved.declaration.container.as_deref(), Some("p.Child"));
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
    }

    #[test]
    fn owner_only_call_projects_a_multihop_generic_declaration() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("java/lang/String.java"),
                "package java.lang; public final class String {}",
            )
            .expect("String fixture should index");
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Base<T> { public T echo(T value) { return value; } } ",
                    "class Middle<U> extends Base<U> {} ",
                    "class Child extends Middle<String> {}"
                ),
            )
            .expect("generic hierarchy should index");

        let resolved = resolve_call(
            &index,
            "p.Child",
            None,
            "echo",
            &[Type::Class(ClassType::string())],
        )
        .expect("the owner FQN is sufficient to project an inherited declaration");

        assert_eq!(resolved.declaration.container.as_deref(), Some("p.Base"));
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
        assert_eq!(
            resolved.substitutions.get("T"),
            Some(&Type::Class(ClassType::string()))
        );
    }

    #[test]
    fn owner_only_call_erases_a_raw_inherited_parameter_to_its_bound() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Number {} class Other {} ",
                    "class Base<T extends Number> { public T echo(T value) { return value; } } ",
                    "class RawChild extends Base {}"
                ),
            )
            .expect("raw hierarchy should index");
        let number = qualified_class("p", "Number");

        let resolved = resolve_call(
            &index,
            "p.RawChild",
            None,
            "echo",
            std::slice::from_ref(&number),
        )
        .expect("a raw owner parameter erases to its leftmost bound");

        assert_eq!(resolved.return_type, number);
        assert_eq!(resolved.substitutions.get("T"), Some(&number));
        assert!(resolve_call(
            &index,
            "p.RawChild",
            None,
            "echo",
            &[qualified_class("p", "Other")],
        )
        .is_none());
    }

    #[test]
    fn owner_only_call_fails_closed_for_conflicting_generic_diamond() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class LeftValue {} class RightValue {} ",
                    "interface Root<T> { T echo(T value); } ",
                    "interface Left extends Root<LeftValue> {} ",
                    "interface Right extends Root<RightValue> {} ",
                    "interface Conflict extends Left, Right {}"
                ),
            )
            .expect("conflicting hierarchy should remain indexable");

        assert!(resolve_call(
            &index,
            "p.Conflict",
            None,
            "echo",
            &[qualified_class("p", "LeftValue")],
        )
        .is_none());
        assert!(resolve_call(
            &index,
            "p.Conflict",
            None,
            "echo",
            &[qualified_class("p", "RightValue")],
        )
        .is_none());
    }

    #[test]
    fn inferred_method_type_must_satisfy_its_direct_bound() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("java/lang/Types.java"),
                concat!(
                    "package java.lang; public class Number {} ",
                    "class Integer extends Number {}"
                ),
            )
            .expect("numeric fixtures should index");
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Good extends java.lang.Number {} class Bad {} ",
                    "class Factory { public static <T extends java.lang.Number> ",
                    "T identity(T value) { return value; } }"
                ),
            )
            .expect("bounded method should index");
        let good = qualified_class("p", "Good");

        let resolved = resolve_call(
            &index,
            "p.Factory",
            None,
            "identity",
            std::slice::from_ref(&good),
        )
        .expect("a subtype of the direct bound should resolve");
        assert_eq!(resolved.return_type, good);
        assert!(resolve_call(
            &index,
            "p.Factory",
            None,
            "identity",
            &[qualified_class("p", "Bad")],
        )
        .is_none());

        let boxed = resolve_call(
            &index,
            "p.Factory",
            None,
            "identity",
            &[Type::Primitive(PrimitiveType::Int)],
        )
        .expect("primitive constraints infer their boxed reference type");
        assert_eq!(boxed.return_type, qualified_class("java.lang", "Integer"));
        assert_eq!(boxed.conversions, [Conversion::Boxing]);
    }

    #[test]
    fn inferred_method_type_must_satisfy_intersection_and_dependent_bounds() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; interface Comparable<T> {} class Number {} ",
                    "class Good extends Number implements Comparable<Good> {} ",
                    "class Wrong extends Number implements Comparable<Number> {} ",
                    "class Factory { ",
                    "public static <T extends Number & Comparable<T>> T bounded(T value) { return value; } ",
                    "public static <Expected, Actual extends Comparable<Expected>> ",
                    "Actual dependent(Expected expected, Actual actual) { return actual; } }"
                ),
            )
            .expect("dependent bound fixtures should index");
        let good = qualified_class("p", "Good");
        let number = qualified_class("p", "Number");
        let wrong = qualified_class("p", "Wrong");

        assert!(resolve_call(
            &index,
            "p.Factory",
            None,
            "bounded",
            std::slice::from_ref(&good),
        )
        .is_some());
        assert!(resolve_call(
            &index,
            "p.Factory",
            None,
            "bounded",
            std::slice::from_ref(&wrong),
        )
        .is_none());
        assert!(resolve_call(
            &index,
            "p.Factory",
            None,
            "dependent",
            &[good.clone(), good.clone()],
        )
        .is_some());
        assert!(resolve_call(&index, "p.Factory", None, "dependent", &[number, good],).is_none());
    }

    #[test]
    fn inherited_method_bound_shadows_the_owner_parameter() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; interface Comparable<T> {} class Number {} ",
                    "class Good implements Comparable<Good> {} ",
                    "class Bad implements Comparable<Number> {} ",
                    "class Owner<T> { public <T extends Comparable<T>> ",
                    "T identity(T value) { return value; } } ",
                    "class Child extends Owner<Number> {}"
                ),
            )
            .expect("shadowed generic hierarchy should index");
        let good = qualified_class("p", "Good");

        let resolved = resolve_call(
            &index,
            "p.Child",
            None,
            "identity",
            std::slice::from_ref(&good),
        )
        .expect("method T, not owner T, owns the recursive bound");
        assert_eq!(resolved.return_type, good);
        assert!(resolve_call(
            &index,
            "p.Child",
            None,
            "identity",
            &[qualified_class("p", "Bad")],
        )
        .is_none());
    }

    #[test]
    fn class_implementation_replaces_the_inherited_interface_contract() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; interface Contract { Object convert(String value); } ",
                    "class Implementation implements Contract { ",
                    "public String convert(String value) { return value; } }"
                ),
            )
            .expect("implementation should index");

        let resolved = resolve_call(
            &index,
            "p.Implementation",
            None,
            "convert",
            &[Type::Class(ClassType::string())],
        )
        .expect("the implementation must satisfy one effective dispatch slot");

        assert_eq!(
            resolved.declaration.container.as_deref(),
            Some("p.Implementation")
        );
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
    }

    #[test]
    fn inherited_private_and_interface_static_methods_are_not_call_candidates() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Base { private String hidden() { return null; } } ",
                    "interface Contract { private String helper() { return null; } ",
                    "static String factory() { return null; } } ",
                    "class Child extends Base implements Contract {}"
                ),
            )
            .expect("hierarchy should index");

        for method in ["hidden", "helper", "factory"] {
            assert!(
                resolve_call(&index, "p.Child", None, method, &[]).is_none(),
                "{method} is not inherited by Child"
            );
        }
    }

    #[test]
    fn ordinary_varargs_arguments_use_element_type_and_report_conversion() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class VarargsDemo {
                public void join(String separator, String... values) {}
            }
        "#;
        let path = temp.path().join("com/example/VarargsDemo.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let string = Type::Class(ClassType::string());
        let resolved = resolve_call(
            &index,
            "com.example.VarargsDemo",
            None,
            "join",
            &[string.clone(), string.clone(), string],
        )
        .expect("expanded varargs invocation should resolve");

        assert_eq!(resolved.score, MatchScore::VarargsMatch);
        assert_eq!(
            resolved.conversions,
            vec![
                Conversion::Identity,
                Conversion::Varargs,
                Conversion::Varargs
            ]
        );
    }

    #[test]
    fn strict_invocation_widening_is_preferred_over_boxing() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class InvocationPhases {
                public String choose(long number, Object value) { return "strict"; }
                public Object choose(Integer number, String value) { return value; }
            }
        "#;
        let path = temp.path().join("com/example/InvocationPhases.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let resolved = resolve_call(
            &index,
            "com.example.InvocationPhases",
            None,
            "choose",
            &[
                Type::Primitive(PrimitiveType::Int),
                Type::Class(ClassType::string()),
            ],
        )
        .expect("strict-invocation overload should resolve");

        assert_eq!(
            resolved.declaration.parameters.first().map(AsRef::as_ref),
            Some("long number")
        );
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
    }

    #[test]
    fn expanded_varargs_selects_the_most_specific_element_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class VarargsOverload {
                public Object choose(Object... values) { return values; }
                public String choose(String... values) { return values[0]; }
            }
        "#;
        let path = temp.path().join("com/example/VarargsOverload.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let string = Type::Class(ClassType::string());
        let resolved = resolve_call(
            &index,
            "com.example.VarargsOverload",
            None,
            "choose",
            &[string.clone(), string],
        )
        .expect("expanded varargs overload should resolve");

        assert_eq!(
            resolved.declaration.parameters.first().map(AsRef::as_ref),
            Some("java.lang.String... values")
        );
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
    }

    #[test]
    fn null_selects_the_most_specific_reference_overload() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class NullOverload {
                public Object choose(Object value) { return value; }
                public String choose(String value) { return value; }
            }
        "#;
        let path = temp.path().join("com/example/NullOverload.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let resolved = resolve_call(
            &index,
            "com.example.NullOverload",
            None,
            "choose",
            &[Type::Null],
        )
        .expect("null argument should resolve to a reference overload");

        assert_eq!(
            resolved.declaration.parameters.first().map(AsRef::as_ref),
            Some("java.lang.String value")
        );
        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
    }

    #[test]
    fn null_does_not_elect_between_incomparable_reference_overloads() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class AmbiguousNullOverload {
                public String choose(String value) { return value; }
                public Integer choose(Integer value) { return value; }
            }
        "#;
        let path = temp.path().join("com/example/AmbiguousNullOverload.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        assert!(
            resolve_call(
                &index,
                "com.example.AmbiguousNullOverload",
                None,
                "choose",
                &[Type::Null],
            )
            .is_none(),
            "incomparable maximal overloads must remain ambiguous"
        );
    }

    #[test]
    fn generic_receiver_substitutions_flow_to_return_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class Box<T> {
                public T get() { return null; }
            }
        "#;
        let path = temp.path().join("com/example/Box.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");
        let receiver = Type::Class(ClassType {
            name: "Box".to_string(),
            package: Some("com.example".to_string()),
            type_arguments: vec![Type::Class(ClassType::string())],
            is_diamond: false,
        });

        let resolved = resolve_call(&index, "com.example.Box", Some(&receiver), "get", &[])
            .expect("generic receiver method should resolve");

        assert_eq!(resolved.return_type, Type::Class(ClassType::string()));
        assert_eq!(
            resolved.substitutions.get("T"),
            Some(&Type::Class(ClassType::string()))
        );
    }

    #[test]
    fn declared_method_type_parameter_is_inferred_from_arguments() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class Factory {
                public static <Result> Result identity(Result value) { return value; }
            }
            class User {}
        "#;
        let path = temp.path().join("com/example/Factory.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");
        let user = Type::Class(ClassType {
            name: "User".to_string(),
            package: Some("com.example".to_string()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let resolved = resolve_call(
            &index,
            "com.example.Factory",
            None,
            "identity",
            std::slice::from_ref(&user),
        )
        .expect("declared generic method should resolve");

        assert_eq!(resolved.return_type, user);
        assert_eq!(resolved.substitutions.get("Result"), Some(&user));
        assert_eq!(resolved.confidence, ResolutionConfidence::Exact);
    }

    #[test]
    fn uninferred_method_type_uses_object_or_leftmost_bound_erasure() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Number {} interface Holder<T> {} ",
                    "class Factory { ",
                    "public static <T> T create() { return null; } ",
                    "public static <T extends Number> T numeric() { return null; } ",
                    "public static <Key, Value extends Holder<Key>> Value dependent() { return null; } }"
                ),
            )
            .expect("uninferred method fixtures should index");

        let create = resolve_call(&index, "p.Factory", None, "create", &[])
            .expect("an unbounded variable defaults to Object without a target type");
        assert_eq!(create.return_type, Type::Class(ClassType::object()));

        let numeric = resolve_call(&index, "p.Factory", None, "numeric", &[])
            .expect("a bounded variable defaults to its leftmost bound erasure");
        assert_eq!(numeric.return_type, qualified_class("p", "Number"));

        let dependent = resolve_call(&index, "p.Factory", None, "dependent", &[])
            .expect("a later variable may use an earlier defaulted substitution");
        assert_eq!(dependent.return_type, qualified_class("p", "Holder"));
    }

    #[test]
    fn source_return_type_uses_the_declaration_context_before_java_lang() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("test/Types.java"),
                concat!(
                    "package test; class StringBuilder {} ",
                    "class Factory { StringBuilder create() { return null; } }"
                ),
            )
            .expect("same-package return fixture should index");

        let resolved = resolve_call(&index, "test.Factory", None, "create", &[])
            .expect("source call should resolve");

        assert_eq!(
            resolved.return_type,
            qualified_class("test", "StringBuilder"),
            "a same-package declaration must shadow java.lang.StringBuilder"
        );
    }

    #[test]
    fn uninferred_recursive_method_bound_fails_closed() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; interface Comparable<T> {} class Factory { ",
                    "public static <T extends Comparable<T>> T recursive() { return null; } }"
                ),
            )
            .expect("recursive method fixture should index");

        assert!(resolve_call(&index, "p.Factory", None, "recursive", &[]).is_none());
    }

    #[test]
    fn generic_varargs_inference_uses_expanded_element_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class Factory {
                public static <Element> Element first(Element... values) { return values[0]; }
            }
        "#;
        let path = temp.path().join("com/example/Factory.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");
        let string = Type::Class(ClassType::string());

        let resolved = resolve_call(
            &index,
            "com.example.Factory",
            None,
            "first",
            &[string.clone(), string.clone()],
        )
        .expect("expanded generic varargs method should resolve");

        assert_eq!(resolved.return_type, string);
        assert_eq!(resolved.score, MatchScore::VarargsMatch);
    }

    #[test]
    fn kotlin_vararg_supports_empty_expanded_and_fixed_array_invocation() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Collector.kt"),
                "package p\nclass Collector { fun collect(vararg values: String): String = values[0] }",
            )
            .expect("Kotlin vararg fixture should index");
        let string = crate::type_parser::parse_type_string("String").unwrap();
        let string_array = Type::Array {
            element_type: Box::new(string.clone()),
            dimensions: 1,
        };

        let empty = resolve_call(&index, "p.Collector", None, "collect", &[])
            .expect("an empty Kotlin vararg call should resolve");
        assert_eq!(empty.score, MatchScore::VarargsMatch);

        let expanded = resolve_call(
            &index,
            "p.Collector",
            None,
            "collect",
            &[string.clone(), string],
        )
        .expect("an expanded Kotlin vararg call should resolve");
        assert_eq!(expanded.score, MatchScore::VarargsMatch);

        let fixed = resolve_call(&index, "p.Collector", None, "collect", &[string_array])
            .expect("an explicit array Kotlin vararg call should resolve in fixed-arity mode");
        assert_ne!(fixed.score, MatchScore::VarargsMatch);
        assert_eq!(fixed.parameter_declarations, ["vararg String values"]);
    }

    #[test]
    fn inherited_method_generic_varargs_project_owner_bound_before_inference() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Bound {} class Owner<O> { ",
                    "<Element extends O> Element first(Element... values) { return values[0]; } } ",
                    "class Child extends Owner<Bound> {}"
                ),
            )
            .expect("inherited generic varargs fixture should index");
        let bound = crate::type_parser::parse_type_string("p.Bound").unwrap();

        let resolved = resolve_call(
            &index,
            "p.Child",
            None,
            "first",
            &[bound.clone(), bound.clone()],
        )
        .expect("owner-projected method bound should accept expanded arguments");
        assert_eq!(resolved.return_type, bound);
        assert_eq!(resolved.score, MatchScore::VarargsMatch);
        assert_eq!(resolved.parameter_declarations, ["p.Bound... values"]);
    }

    #[test]
    fn inherited_declaration_substitutions_flow_through_nested_return_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            class Value {}
            interface Wrapper<T> {}
            interface Parent<A, B> {
                Wrapper<A> wrap(B value);
            }
            interface Middle<X> extends Parent<java.util.List<X>, X> {}
            interface Child extends Middle<Value> {}
        "#;
        let path = temp.path().join("com/example/Types.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");
        let receiver = Type::Class(ClassType::new(
            "Child".to_string(),
            Some("com.example".to_string()),
        ));

        let resolved = resolve_call(
            &index,
            "com.example.Child",
            Some(&receiver),
            "wrap",
            &[crate::type_parser::parse_type_string("com.example.Value").unwrap()],
        )
        .expect("inherited generic method should resolve");
        let Type::Class(wrapper) = resolved.return_type else {
            panic!("expected nested generic wrapper return type");
        };
        assert_eq!(wrapper.name, "Wrapper");
        let [Type::Class(list)] = wrapper.type_arguments.as_slice() else {
            panic!("expected Wrapper<List<String>>")
        };
        assert_eq!(list.name, "List");
        let value = crate::type_parser::parse_type_string("com.example.Value").unwrap();
        assert_eq!(list.type_arguments.as_slice(), std::slice::from_ref(&value));
        assert_eq!(resolved.substitutions.get("B"), Some(&value));
    }

    #[test]
    fn inherited_method_parameter_shadows_owner_parameter() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Owner<T> { ",
                    "public <T> T identity(T value) { return value; } } ",
                    "class Child extends Owner<String> {}"
                ),
            )
            .expect("source should index");
        let receiver = Type::Class(ClassType::new("Child".to_string(), Some("p".to_string())));
        let integer = crate::type_parser::parse_type_string("java.lang.Integer").unwrap();

        let resolved = resolve_call(
            &index,
            "p.Child",
            Some(&receiver),
            "identity",
            std::slice::from_ref(&integer),
        )
        .expect("method-level T must remain inferable after owner projection");

        assert_eq!(resolved.return_type, integer);
        assert_eq!(resolved.substitutions.get("T"), Some(&integer));
    }

    #[test]
    fn unknown_arguments_are_reported_as_incomplete_resolution() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
            package com.example;
            public class Demo {
                public String process(String value) { return value; }
            }
        "#;
        let path = temp.path().join("com/example/Demo.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let resolved = resolve_call(
            &index,
            "com.example.Demo",
            None,
            "process",
            &[Type::Unknown],
        )
        .expect("unknown argument should allow a conservative match");

        assert_eq!(resolved.confidence, ResolutionConfidence::Incomplete);
        assert_eq!(
            resolved.fallback_reason,
            Some(CallFallbackReason::UnknownArgumentTypes)
        );
    }
}
