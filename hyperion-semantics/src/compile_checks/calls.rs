// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Provable call errors: a method call is reported only when *no* candidate
//! can possibly apply.
//!
//! Candidate selection here is deliberately not overload resolution. Overload
//! resolution must pick the most specific applicable method and infer generic
//! arguments; this module only asks "could any declaration accept these
//! arguments?" and answers yes whenever it cannot prove otherwise. That
//! one-sided contract is what keeps generics, boxing, varargs, lambdas and
//! partially resolved classpaths from ever producing a false error.

use std::collections::HashSet;

use hyperion_workspace::{ProjectedEffectiveMethod, Symbol};

use super::conversion::{convertible, Context, Convertibility};
use super::facts::TypeFacts;
use crate::symbol::Type;
use crate::type_parser::parse_type_string;

/// Why one candidate declaration cannot accept the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inapplicable {
    /// Wrong number of arguments.
    Arity { expected: usize, actual: usize },
    /// Argument `index` cannot be converted to the parameter type.
    Argument {
        index: usize,
        expected: Type,
        actual: Type,
    },
}

/// One declaration that was considered and rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedCandidate {
    pub owner: String,
    pub parameters: Vec<String>,
    pub reason: Inapplicable,
}

/// Result of [`check_call`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallOutcome {
    /// At least one candidate may apply, or the analysis cannot tell.
    Fine,
    /// The receiver's complete hierarchy declares no member of this name.
    NoSuchMethod,
    /// Members of that name exist, and every one of them is provably
    /// inapplicable.
    Inapplicable(Vec<RejectedCandidate>),
}

/// Decide a call on a receiver class.
///
/// `arguments` holds the static type of each argument, `None` for poly
/// expressions (lambdas, method references) and anything else whose type is not
/// known; those are compatible with everything but still count for arity.
pub fn check_call(
    facts: &TypeFacts<'_>,
    receiver_fqn: &str,
    receiver_arguments: &[String],
    method_name: &str,
    arguments: &[Option<Type>],
    strict_arguments: &[bool],
) -> CallOutcome {
    let index = facts.index();
    if !facts.is_known_type(receiver_fqn) || facts.is_foreign_language_type(receiver_fqn) {
        return CallOutcome::Fine;
    }
    // Member sets are only meaningful over a completely resolved hierarchy.
    let Some(supertypes) = facts.supertypes(receiver_fqn) else {
        return CallOutcome::Fine;
    };
    if supertypes
        .iter()
        .any(|supertype| facts.is_foreign_language_type(supertype))
    {
        return CallOutcome::Fine;
    }

    let candidates = index.find_projected_effective_methods_by_name(
        receiver_fqn,
        receiver_arguments,
        method_name,
    );
    if candidates.is_empty() {
        return CallOutcome::NoSuchMethod;
    }
    if candidates
        .iter()
        .any(|candidate| candidate.projection.is_none())
    {
        return CallOutcome::Fine;
    }

    let mut rejected = Vec::new();
    for candidate in &candidates {
        match reject_reason(facts, candidate, arguments, strict_arguments) {
            Some(reason) => rejected.push(RejectedCandidate {
                owner: candidate
                    .definition
                    .symbol
                    .container
                    .as_deref()
                    .unwrap_or_default()
                    .to_owned(),
                parameters: candidate
                    .projection
                    .as_ref()
                    .map(|projection| projection.parameter_declarations())
                    .unwrap_or_default(),
                reason,
            }),
            None => return CallOutcome::Fine,
        }
    }
    CallOutcome::Inapplicable(rejected)
}

/// One formal parameter of a candidate, with its written spelling.
pub struct ParamSpec {
    /// Parsed type; `None` when the spelling could not be parsed.
    pub ty: Option<Type>,
    pub variadic: bool,
}

/// `Some(reason)` when the candidate can be proven not to accept `arguments`.
fn reject_reason(
    facts: &TypeFacts<'_>,
    candidate: &ProjectedEffectiveMethod,
    arguments: &[Option<Type>],
    strict_arguments: &[bool],
) -> Option<Inapplicable> {
    let symbol = &candidate.definition.symbol;
    let projection = candidate.projection.as_ref()?;
    let params: Vec<ParamSpec> = projection
        .parameters
        .iter()
        .map(|parameter| ParamSpec {
            ty: parse_type_string(&parameter.type_name),
            variadic: parameter.is_variadic(),
        })
        .collect();
    let type_variables = visible_type_variables(facts, symbol);
    params_reject_arguments(facts, &params, &type_variables, arguments, strict_arguments)
}

/// Provable inapplicability of a parameter list (methods and constructors).
pub fn params_reject_arguments(
    facts: &TypeFacts<'_>,
    params: &[ParamSpec],
    type_variables: &HashSet<String>,
    arguments: &[Option<Type>],
    strict_arguments: &[bool],
) -> Option<Inapplicable> {
    let variadic = params.last().is_some_and(|parameter| parameter.variadic);

    let arity_ok = if variadic {
        arguments.len() + 1 >= params.len()
    } else {
        arguments.len() == params.len()
    };
    if !arity_ok {
        return Some(Inapplicable::Arity {
            expected: params.len(),
            actual: arguments.len(),
        });
    }

    for (index, argument) in arguments.iter().enumerate() {
        let Some(argument) = argument else { continue };
        let last = params.len().saturating_sub(1);
        let (position, in_variadic_tail) = if variadic && index >= last {
            (last, true)
        } else {
            (index, false)
        };
        let Some(Some(parameter_type)) = params.get(position).map(|spec| &spec.ty) else {
            continue;
        };
        if mentions_type_variable(parameter_type, type_variables) {
            continue;
        }
        let context = if strict_arguments.get(index).copied().unwrap_or(false) {
            Context::InvocationWithTypeArguments
        } else {
            Context::Invocation
        };
        let verdict = if in_variadic_tail {
            variadic_tail_verdict(
                facts,
                argument,
                parameter_type,
                arguments.len() == params.len(),
            )
        } else {
            convertible(facts, argument, parameter_type, context)
        };
        if verdict == Convertibility::No {
            // A rejected variable-arity argument is explained by the element
            // type the user wrote, not the array the compiler synthesizes.
            let expected = match parameter_type {
                Type::Array {
                    element_type,
                    dimensions,
                } if in_variadic_tail => {
                    if *dimensions > 1 {
                        Type::Array {
                            element_type: element_type.clone(),
                            dimensions: dimensions - 1,
                        }
                    } else {
                        (**element_type).clone()
                    }
                }
                other => other.clone(),
            };
            return Some(Inapplicable::Argument {
                index,
                expected,
                actual: argument.clone(),
            });
        }
    }
    None
}

/// A variable-arity tail accepts an element, or (when it is the only tail
/// argument) the array itself.
fn variadic_tail_verdict(
    facts: &TypeFacts<'_>,
    argument: &Type,
    array_type: &Type,
    may_be_the_array: bool,
) -> Convertibility {
    let Type::Array {
        element_type,
        dimensions,
    } = array_type
    else {
        return Convertibility::Maybe;
    };
    let element = if *dimensions > 1 {
        Type::Array {
            element_type: element_type.clone(),
            dimensions: dimensions - 1,
        }
    } else {
        (**element_type).clone()
    };
    let as_element = convertible(facts, argument, &element, Context::Invocation);
    if as_element != Convertibility::No {
        return as_element;
    }
    if may_be_the_array {
        return convertible(facts, argument, array_type, Context::Invocation);
    }
    Convertibility::No
}

/// Names of every type variable a parameter of `symbol` may mention: its own,
/// its owner's and every lexically enclosing type's.
pub fn visible_type_variables(facts: &TypeFacts<'_>, symbol: &Symbol) -> HashSet<String> {
    let mut names: HashSet<String> = symbol
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.to_string())
        .collect();
    let mut owner = symbol.container.as_deref().map(str::to_owned);
    while let Some(current) = owner {
        if let Some(type_symbol) = facts.type_symbol(&current) {
            names.extend(
                type_symbol
                    .type_parameters
                    .iter()
                    .map(|parameter| parameter.name.to_string()),
            );
        }
        owner = current.rsplit_once('.').map(|(outer, _)| outer.to_owned());
    }
    names
}

fn mentions_type_variable(ty: &Type, variables: &HashSet<String>) -> bool {
    match ty {
        Type::TypeVariable { .. } | Type::Wildcard { .. } | Type::Intersection { .. } => true,
        Type::Class(class) => {
            (class.package.is_none() && variables.contains(class.raw_name()))
                || class
                    .type_arguments
                    .iter()
                    .any(|argument| mentions_type_variable(argument, variables))
        }
        Type::Array { element_type, .. } => mentions_type_variable(element_type, variables),
        _ => false,
    }
}

/// Which access level rejected a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    Private,
    Protected,
    PackagePrivate,
}

/// A member that exists but cannot be used from where it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessViolation {
    pub owner: String,
    pub level: AccessLevel,
    /// Parameter spellings of the reported declaration, for the message.
    pub parameters: Vec<String>,
}

/// `Some` when every method named `method_name` on the receiver is
/// inaccessible from `lexical_owners` (JLS 6.6).
pub fn check_method_access(
    facts: &TypeFacts<'_>,
    receiver_fqn: &str,
    receiver_arguments: &[String],
    method_name: &str,
    lexical_owners: &[String],
    qualifier: hyperion_workspace::FieldAccessQualifier,
) -> Option<AccessViolation> {
    let index = facts.index();
    if lexical_owners.is_empty()
        || !facts.is_known_type(receiver_fqn)
        || facts.is_foreign_language_type(receiver_fqn)
        || facts.supertypes(receiver_fqn).is_none()
    {
        return None;
    }
    let candidates = index.find_projected_effective_methods_by_name(
        receiver_fqn,
        receiver_arguments,
        method_name,
    );
    if candidates.is_empty() {
        return None;
    }
    let mut first_violation = None;
    for candidate in &candidates {
        let symbol = &candidate.definition.symbol;
        if index.is_member_accessible(receiver_fqn, lexical_owners, qualifier, symbol) {
            return None;
        }
        if first_violation.is_none() {
            let level = if symbol.modifiers.is_private {
                AccessLevel::Private
            } else if symbol.modifiers.is_protected {
                AccessLevel::Protected
            } else {
                AccessLevel::PackagePrivate
            };
            first_violation = Some(AccessViolation {
                owner: symbol.container.as_deref().unwrap_or_default().to_owned(),
                level,
                parameters: candidate
                    .projection
                    .as_ref()
                    .map(|projection| {
                        projection
                            .parameters
                            .iter()
                            .map(|parameter| {
                                super::constructors::simplify_qualified_names(&parameter.type_name)
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    first_violation
}

/// Every candidate that is not provably inapplicable, or `None` when the set
/// cannot be trusted (unknown receiver, unresolved hierarchy, foreign
/// language, unprojectable signature).
pub fn possibly_applicable_methods(
    facts: &TypeFacts<'_>,
    receiver_fqn: &str,
    receiver_arguments: &[String],
    method_name: &str,
    arguments: &[Option<Type>],
    strict_arguments: &[bool],
) -> Option<Vec<ProjectedEffectiveMethod>> {
    if !facts.is_known_type(receiver_fqn) || facts.is_foreign_language_type(receiver_fqn) {
        return None;
    }
    let supertypes = facts.supertypes(receiver_fqn)?;
    if supertypes
        .iter()
        .any(|supertype| facts.is_foreign_language_type(supertype))
    {
        return None;
    }
    let candidates = facts.index().find_projected_effective_methods_by_name(
        receiver_fqn,
        receiver_arguments,
        method_name,
    );
    if candidates.is_empty() || candidates.iter().any(|c| c.projection.is_none()) {
        return None;
    }
    Some(
        candidates
            .into_iter()
            .filter(|candidate| {
                reject_reason(facts, candidate, arguments, strict_arguments).is_none()
            })
            .collect(),
    )
}
