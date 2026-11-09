// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Exact identities for source methods and conservative call-site selection.
//!
//! Workspace indexing normally provides the selected declaration for a call.
//! Direct semantic users do not necessarily have that index, so this module
//! also performs a deliberately narrow source-level fallback. It selects an
//! overload only when the available argument types identify one declaration;
//! ambiguous or incomplete calls remain on the conservative name/arity path.

use super::analysis::{method_call_name_offset, InvocationEffect, MethodCallKey};
use super::patterns::ValueKind;
use super::signature_cache::{extract_declared_type, extract_param_types};
use super::{method_call_descriptor, MethodNullabilityInfo, Nullability};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_workspace::usage_extractor::signature_inference::build_type_signature_from_arg_list_with_resolver;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) type SourceMethodId = Arc<str>;

#[derive(Debug, Clone)]
pub(crate) struct SourceMethodDeclaration {
    pub(crate) id: SourceMethodId,
    pub(crate) call_key: MethodCallKey,
    parameter_types: Arc<[String]>,
    parameter_contract: Arc<[Nullability]>,
    return_seed: Nullability,
    return_value_kind: ValueKind,
    varargs: bool,
    is_static: bool,
    invocation_effect: InvocationEffect,
}

impl SourceMethodDeclaration {
    pub(crate) fn from_method(
        method: &SyntaxNode,
        class_name: &str,
        info: &MethodNullabilityInfo,
    ) -> Self {
        let parameter_types: Arc<[String]> = extract_param_types(method).into();
        let descriptor = parameter_types.join(",");
        let parameter_contract = info
            .param_nullability
            .iter()
            .enumerate()
            .map(|(index, (name, state))| {
                if *state == Nullability::NonNull
                    && !info.explicit_nonnull_params.contains(name)
                    && !parameter_types
                        .get(index)
                        .is_some_and(|ty| is_primitive(ty))
                {
                    Nullability::Unknown
                } else {
                    *state
                }
            })
            .collect::<Vec<_>>()
            .into();
        let is_static = super::extraction::has_attached_modifier(method, SyntaxKind::STATIC_KW);
        Self {
            id: Arc::from(format!("{class_name}.{}({descriptor})", info.name)),
            call_key: MethodCallKey {
                class_name: class_name.to_string(),
                method_name: info.name.clone(),
                arity: parameter_types.len(),
            },
            varargs: parameter_types
                .last()
                .is_some_and(|parameter| parameter.ends_with("...")),
            parameter_types,
            parameter_contract,
            return_seed: if info.return_nullability == Nullability::Unknown {
                Nullability::Bottom
            } else {
                info.return_nullability
            },
            return_value_kind: extract_declared_type(method).map_or(ValueKind::Unknown, |ty| {
                super::signature_cache::declaration_value_kind(method, &ty)
            }),
            is_static,
            invocation_effect: source_method_invocation_effect(method),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SourceMethodTable {
    declarations: HashMap<MethodCallKey, Vec<SourceMethodDeclaration>>,
    source_package: Option<Arc<str>>,
}

impl SourceMethodTable {
    pub(crate) fn new(
        declarations: impl IntoIterator<Item = SourceMethodDeclaration>,
        source_package: Option<&str>,
    ) -> Self {
        let mut table = Self {
            declarations: HashMap::new(),
            source_package: source_package.map(Arc::from),
        };
        for declaration in declarations {
            table
                .declarations
                .entry(declaration.call_key.clone())
                .or_default()
                .push(declaration);
        }
        table
    }

    pub(crate) fn declarations_for(&self, key: &MethodCallKey) -> &[SourceMethodDeclaration] {
        self.declarations.get(key).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn initial_returns(&self) -> HashMap<SourceMethodId, Nullability> {
        self.declarations
            .values()
            .flatten()
            .map(|declaration| (Arc::clone(&declaration.id), declaration.return_seed))
            .collect()
    }

    pub(crate) fn exact_parameter_contracts(&self) -> HashMap<SourceMethodId, Arc<[Nullability]>> {
        self.declarations
            .values()
            .flatten()
            .map(|declaration| {
                (
                    Arc::clone(&declaration.id),
                    Arc::clone(&declaration.parameter_contract),
                )
            })
            .collect()
    }

    pub(crate) fn binding_for_declaration(
        &self,
        declaration: &str,
    ) -> Option<(&str, bool, ValueKind, InvocationEffect, &[String])> {
        self.declarations
            .values()
            .flatten()
            .find(|candidate| candidate.id.as_ref() == declaration)
            .map(|candidate| {
                (
                    candidate.call_key.class_name.as_str(),
                    candidate.is_static,
                    candidate.return_value_kind,
                    candidate.invocation_effect,
                    candidate.parameter_types.as_ref(),
                )
            })
    }

    /// Resolve dispatch only when the source fallback can select one applicable
    /// declaration. Name/arity agreement is insufficient: overload sets may
    /// legally mix static and instance methods.
    pub(crate) fn resolve_member(
        &self,
        call: &SyntaxNode,
        field_types: &HashMap<String, String>,
        owner: &str,
    ) -> Option<(bool, ValueKind, InvocationEffect, &[String])> {
        let (method_name, _, arguments) = method_call_descriptor(call);
        let candidates = self.candidates(owner, method_name.as_deref()?, arguments.len())?;
        self.select_declaration(call, field_types, &candidates)
            .map(|declaration| {
                (
                    declaration.is_static,
                    declaration.return_value_kind,
                    declaration.invocation_effect,
                    declaration.parameter_types.as_ref(),
                )
            })
    }

    pub(crate) fn conservative_parameter_contracts(
        &self,
    ) -> HashMap<MethodCallKey, Vec<Nullability>> {
        self.declarations
            .iter()
            .map(|(key, declarations)| {
                let mut summary = declarations
                    .first()
                    .map(|declaration| declaration.parameter_contract.to_vec())
                    .unwrap_or_default();
                for declaration in declarations.iter().skip(1) {
                    for (state, parameter) in summary
                        .iter_mut()
                        .zip(declaration.parameter_contract.iter())
                    {
                        *state = state.join(*parameter);
                    }
                }
                (key.clone(), summary)
            })
            .collect()
    }

    pub(crate) fn summarize_returns(
        &self,
        returns: &HashMap<SourceMethodId, Nullability>,
    ) -> HashMap<MethodCallKey, Nullability> {
        self.declarations
            .keys()
            .map(|key| (key.clone(), self.summarize_return(key, returns)))
            .collect()
    }

    pub(crate) fn summarize_return(
        &self,
        key: &MethodCallKey,
        returns: &HashMap<SourceMethodId, Nullability>,
    ) -> Nullability {
        self.declarations_for(key)
            .iter()
            .filter_map(|declaration| returns.get(&declaration.id).copied())
            .reduce(Nullability::join)
            .unwrap_or(Nullability::Unknown)
    }

    pub(crate) fn resolve_calls(
        &self,
        file_id: hyperion_workspace::FileId,
        scope: &SyntaxNode,
        field_types: &HashMap<String, String>,
        call_owners: &HashMap<u32, String>,
        signatures: &super::MethodSignatureSnapshot,
    ) -> HashMap<u32, SourceMethodId> {
        scope
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .filter(|call| {
                call.ancestors()
                    .skip(1)
                    .find(|ancestor| {
                        matches!(
                            ancestor.kind(),
                            SyntaxKind::METHOD_DECL
                                | SyntaxKind::CONSTRUCTOR_DECL
                                | SyntaxKind::FIELD_DECL
                                | SyntaxKind::INSTANCE_INIT
                                | SyntaxKind::STATIC_INIT
                        )
                    })
                    .is_some_and(|owner| owner == *scope)
            })
            .filter_map(|call| {
                let offset = method_call_name_offset(&call)?;
                let (method_name, receiver, arguments) = method_call_descriptor(&call);
                let method_name = method_name?;
                if let Some(indexed) = signatures.indexed_call_declaration(file_id, offset) {
                    return self
                        .declaration_selected_by_index(indexed, &method_name, arguments.len())
                        .map(|declaration| (offset, Arc::clone(&declaration.id)));
                }
                let owner = call_owners.get(&offset).cloned().or_else(|| {
                    let receiver_is_this = receiver
                        .as_ref()
                        .is_some_and(|receiver| receiver.kind() == SyntaxKind::THIS_EXPR);
                    (receiver.is_none() || receiver_is_this)
                        .then(|| nearest_source_owner(&call))
                        .flatten()
                });
                let owner = owner?;
                let candidates = self.candidates(&owner, &method_name, arguments.len())?;
                self.select_declaration(&call, field_types, &candidates)
                    .map(|declaration| (offset, Arc::clone(&declaration.id)))
            })
            .collect()
    }

    fn declaration_selected_by_index(
        &self,
        indexed: &str,
        method_name: &str,
        invocation_arity: usize,
    ) -> Option<&SourceMethodDeclaration> {
        let (indexed_owner, indexed_method, indexed_parameters) = declaration_parts(indexed)?;
        if indexed_method != method_name {
            return None;
        }
        let candidates: Vec<_> = self
            .declarations
            .iter()
            .filter(|(key, _)| {
                key.method_name == indexed_method
                    && self.indexed_owner_is_source_owner(indexed_owner, &key.class_name)
            })
            .flat_map(|(_, declarations)| declarations)
            .filter(|declaration| {
                declaration.call_key.arity == invocation_arity
                    || (declaration.varargs
                        && invocation_arity >= declaration.call_key.arity.saturating_sub(1))
            })
            .collect();
        unique_matching_declaration(&candidates, indexed_parameters)
    }

    fn indexed_owner_is_source_owner(&self, indexed_owner: &str, source_owner: &str) -> bool {
        self.source_package.as_ref().map_or_else(
            || indexed_owner == source_owner,
            |package| {
                indexed_owner
                    .strip_prefix(package.as_ref())
                    .and_then(|suffix| suffix.strip_prefix('.'))
                    == Some(source_owner)
            },
        )
    }

    fn candidates(
        &self,
        owner: &str,
        method_name: &str,
        arity: usize,
    ) -> Option<Vec<&SourceMethodDeclaration>> {
        let candidates: Vec<_> = self
            .declarations
            .iter()
            .filter(|(key, _)| {
                key.method_name == method_name && owner_matches(owner, &key.class_name)
            })
            .flat_map(|(_, declarations)| declarations)
            .filter(|declaration| {
                declaration.call_key.arity == arity
                    || (declaration.varargs
                        && arity >= declaration.call_key.arity.saturating_sub(1))
            })
            .collect();
        (!candidates.is_empty()).then_some(candidates)
    }

    fn select_declaration<'a>(
        &self,
        call: &SyntaxNode,
        field_types: &HashMap<String, String>,
        candidates: &[&'a SourceMethodDeclaration],
    ) -> Option<&'a SourceMethodDeclaration> {
        let arguments = call
            .children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)?;
        let descriptor =
            build_type_signature_from_arg_list_with_resolver(&arguments, |name_expression| {
                let name = name_expression.text().to_string();
                visible_variable_type(name_expression, name.trim(), field_types)
            });
        if descriptor.split(',').any(|parameter| parameter == "_") {
            return None;
        }
        unique_matching_declaration(candidates, &descriptor)
    }
}

pub(crate) fn visible_variable_type(
    name_expression: &SyntaxNode,
    name: &str,
    field_types: &HashMap<String, String>,
) -> Option<String> {
    if let Some(declaration) = visible_variable_declaration(name_expression, name) {
        return extract_declared_type(&declaration);
    }

    field_types.get(name).cloned()
}

/// Whether a name is lexically bound as a value, even when its declared type
/// could not be recovered. This is deliberately separate from type inference:
/// Java permits uppercase variable names, and a failed type extraction must
/// not turn such a value into a type-qualified static access.
pub(crate) fn visible_variable_binding(name_expression: &SyntaxNode, name: &str) -> bool {
    visible_variable_declaration(name_expression, name).is_some()
}

fn visible_variable_declaration(name_expression: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    let mut child_on_path = name_expression.clone();
    for ancestor in name_expression.ancestors().skip(1) {
        let mut preceding_declaration = None;
        for sibling in ancestor.children() {
            if sibling == child_on_path {
                break;
            }
            if sibling.kind() == SyntaxKind::LOCAL_VAR_DECL
                && declaration_declares_name(&sibling, name)
            {
                preceding_declaration = Some(sibling);
            }
        }
        if preceding_declaration.is_some() {
            return preceding_declaration;
        }

        match ancestor.kind() {
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                if let Some(parameter) = ancestor
                    .children()
                    .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
                    .into_iter()
                    .flat_map(|parameters| parameters.children())
                    .filter(|parameter| parameter.kind() == SyntaxKind::PARAM)
                    .find(|parameter| declaration_declares_name(parameter, name))
                {
                    return Some(parameter);
                }
                break;
            }
            SyntaxKind::FOREACH_STMT | SyntaxKind::CATCH_CLAUSE
                if ancestor.children().last().as_ref() == Some(&child_on_path)
                    && declaration_declares_name(&ancestor, name) =>
            {
                return Some(ancestor);
            }
            SyntaxKind::LAMBDA_EXPR
                if ancestor.children().last().as_ref() == Some(&child_on_path) =>
            {
                if let Some(parameter) = ancestor
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::PARAM)
                    .find(|parameter| declaration_declares_name(parameter, name))
                {
                    return Some(parameter);
                }
            }
            _ => {}
        }
        child_on_path = ancestor;
    }

    None
}

fn declaration_declares_name(declaration: &SyntaxNode, name: &str) -> bool {
    declaration
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .any(|token| token.kind().is_identifier() && token.text() == name)
}

fn nearest_source_owner(call: &SyntaxNode) -> Option<String> {
    super::qualified_type_name(
        &call
            .ancestors()
            .find(|ancestor| ancestor.kind().is_type_declaration())?,
    )
}

fn owner_matches(resolved_owner: &str, source_owner: &str) -> bool {
    resolved_owner == source_owner
        || resolved_owner
            .strip_suffix(source_owner)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn declaration_parts(declaration: &str) -> Option<(&str, &str, &str)> {
    let open = declaration.rfind('(')?;
    let close = declaration.rfind(')')?;
    let qualified_method = declaration.get(..open)?;
    let (owner, method) = qualified_method.rsplit_once('.')?;
    Some((owner, method, declaration.get(open + 1..close)?))
}

fn unique_matching_declaration<'a>(
    candidates: &[&'a SourceMethodDeclaration],
    argument_descriptor: &str,
) -> Option<&'a SourceMethodDeclaration> {
    let arguments: Vec<_> = if argument_descriptor.is_empty() {
        Vec::new()
    } else {
        argument_descriptor.split(',').map(str::trim).collect()
    };
    let matches = candidates.iter().copied().filter_map(|candidate| {
        parameter_match_score(candidate, &arguments).map(|score| (score, candidate))
    });
    let mut best = None;
    let mut tied = false;
    for (score, candidate) in matches {
        match best {
            None => best = Some((score, candidate)),
            Some((best_score, _)) if score > best_score => {
                best = Some((score, candidate));
                tied = false;
            }
            Some((best_score, _)) if score == best_score => tied = true,
            Some(_) => {}
        }
    }
    if tied {
        None
    } else {
        best.map(|(_, candidate)| candidate)
    }
}

fn parameter_match_score(
    declaration: &SourceMethodDeclaration,
    arguments: &[&str],
) -> Option<(u8, u8, usize)> {
    let minimum_arity = if declaration.varargs {
        declaration.parameter_types.len().saturating_sub(1)
    } else {
        declaration.parameter_types.len()
    };
    if arguments.len() < minimum_arity
        || (!declaration.varargs && arguments.len() != declaration.parameter_types.len())
    {
        return None;
    }

    arguments
        .iter()
        .copied()
        .enumerate()
        .try_fold((u8::MAX, 0), |(phase, score), (index, argument)| {
            let parameter = declaration
                .parameter_types
                .get(index)
                .or_else(|| declaration.parameter_types.last());
            type_match_score(argument, parameter?).map(|(parameter_phase, parameter_score)| {
                (phase.min(parameter_phase), score + parameter_score)
            })
        })
        .map(|(phase, type_score)| (u8::from(!declaration.varargs), phase, type_score))
}

/// Score one Java method-invocation conversion.
///
/// The first component is the applicability phase: strict conversions outrank
/// loose boxing/unboxing conversions, matching JLS overload selection. The
/// second component distinguishes conversions within a phase without allowing
/// a high loose score to beat any strict candidate.
fn type_match_score(argument: &str, parameter: &str) -> Option<(u8, usize)> {
    if argument == "null" {
        return (!is_primitive(parameter)).then_some((2, 0));
    }
    if argument == parameter {
        return Some((2, 16));
    }
    if let Some(element_type) = parameter.strip_suffix("...") {
        if argument.ends_with("[]") {
            return type_match_score(argument, &format!("{element_type}[]"));
        }
        return type_match_score(argument, element_type);
    }
    if simple_types_equal(argument, parameter) {
        return Some((2, 15));
    }
    if let Some(distance) = primitive_widening_distance(argument, parameter) {
        return Some((2, 14usize.saturating_sub(distance)));
    }

    let (argument_base, argument_suffix) = simple_type_parts(argument);
    let (parameter_base, parameter_suffix) = simple_type_parts(parameter);
    if argument_suffix.is_empty() && parameter_suffix.is_empty() {
        if let Some(unboxed) = primitive_for_wrapper(argument_base) {
            if unboxed == parameter_base {
                return Some((1, 14));
            }
            if let Some(distance) = primitive_widening_distance(unboxed, parameter_base) {
                return Some((1, 12usize.saturating_sub(distance)));
            }
        }
        if primitive_for_wrapper(parameter_base) == Some(argument_base) {
            return Some((1, 14));
        }
    }

    // Every reference type widens to Object. Primitive values can reach Object
    // only through the later loose-invocation boxing phase.
    if parameter_base == "Object" && parameter_suffix.is_empty() {
        return Some(if is_primitive(argument) {
            (1, 1)
        } else {
            (2, 1)
        });
    }
    None
}

fn primitive_for_wrapper(type_name: &str) -> Option<&'static str> {
    match type_name.rsplit('.').next().unwrap_or(type_name) {
        "Boolean" => Some("boolean"),
        "Byte" => Some("byte"),
        "Short" => Some("short"),
        "Integer" => Some("int"),
        "Long" => Some("long"),
        "Character" => Some("char"),
        "Float" => Some("float"),
        "Double" => Some("double"),
        _ => None,
    }
}

fn primitive_widening_distance(argument: &str, parameter: &str) -> Option<usize> {
    let (argument, argument_suffix) = simple_type_parts(argument);
    let (parameter, parameter_suffix) = simple_type_parts(parameter);
    if !argument_suffix.is_empty() || !parameter_suffix.is_empty() {
        return None;
    }
    let widening_chain: &[&str] = match argument {
        "byte" => &["short", "int", "long", "float", "double"],
        "short" => &["int", "long", "float", "double"],
        "char" => &["int", "long", "float", "double"],
        "int" => &["long", "float", "double"],
        "long" => &["float", "double"],
        "float" => &["double"],
        _ => return None,
    };
    widening_chain
        .iter()
        .position(|candidate| *candidate == parameter)
        .map(|index| index + 1)
}

fn simple_type_parts(ty: &str) -> (&str, &str) {
    let ty = ty.trim();
    let suffix_start = ty.find('[').or_else(|| ty.find("...")).unwrap_or(ty.len());
    let (base, suffix) = ty.split_at(suffix_start);
    (base.rsplit('.').next().unwrap_or(base), suffix)
}

fn simple_types_equal(left: &str, right: &str) -> bool {
    simple_type_parts(left) == simple_type_parts(right)
}

/// Prove the narrow subset of source methods whose invocation cannot throw.
///
/// This is shared by same-file binding and the immutable cross-file signature
/// revision. Keeping one proof prevents a dependency call from acquiring
/// different effects merely because it crossed the publication boundary.
pub(crate) fn source_method_invocation_effect(method: &SyntaxNode) -> InvocationEffect {
    if !method
        .children()
        .any(|child| child.kind() == SyntaxKind::BLOCK_STMT)
    {
        // Abstract/native declarations have no source body to prove. Native
        // code in particular may throw despite an empty-looking declaration.
        return InvocationEffect::Arbitrary;
    }
    let body_is_nonthrowing = method_body_is_trivially_nonthrowing(method);
    if super::extraction::has_attached_modifier(method, SyntaxKind::STATIC_KW) {
        return if body_is_nonthrowing {
            InvocationEffect::ClassInitialization
        } else {
            InvocationEffect::Arbitrary
        };
    }
    let is_non_overridable = super::extraction::has_attached_modifier(method, SyntaxKind::FINAL_KW)
        || super::extraction::has_attached_modifier(method, SyntaxKind::PRIVATE_KW)
        || method
            .ancestors()
            .skip(1)
            .find(|ancestor| ancestor.kind().is_type_declaration())
            .is_some_and(|owner| {
                super::extraction::has_attached_modifier(&owner, SyntaxKind::FINAL_KW)
            });
    if is_non_overridable && body_is_nonthrowing {
        InvocationEffect::NonThrowing
    } else {
        InvocationEffect::Arbitrary
    }
}

fn method_body_is_trivially_nonthrowing(method: &SyntaxNode) -> bool {
    // This is deliberately a proof, not a syntax-risk heuristic. Only empty
    // bodies and literal/`this`/class-literal returns enter the fast path. All
    // conversions, member access, calls, allocation, operators, monitors, and
    // control constructs remain conservative until invocation effects are
    // derived from accepted uncaught CFG completions.
    method.descendants().all(|node| {
        let is_deferred_body = node
            .ancestors()
            .skip(1)
            .take_while(|ancestor| ancestor != method)
            .any(|ancestor| {
                ancestor.kind().is_type_declaration() || ancestor.kind() == SyntaxKind::LAMBDA_EXPR
            });
        is_deferred_body
            || !matches!(
                node.kind(),
                SyntaxKind::ASSERT_STMT
                    | SyntaxKind::THROW_STMT
                    | SyntaxKind::METHOD_CALL_EXPR
                    | SyntaxKind::NEW_EXPR
                    | SyntaxKind::ARRAY_ACCESS_EXPR
                    | SyntaxKind::FIELD_ACCESS_EXPR
                    | SyntaxKind::CAST_EXPR
                    | SyntaxKind::BINARY_EXPR
                    | SyntaxKind::UNARY_EXPR
                    | SyntaxKind::ASSIGNMENT_EXPR
                    | SyntaxKind::LOCAL_VAR_DECL
                    | SyntaxKind::SYNCHRONIZED_STMT
                    | SyntaxKind::FOREACH_STMT
                    | SyntaxKind::SWITCH_STMT
                    | SyntaxKind::SWITCH_EXPR
                    | SyntaxKind::NAME_EXPR
            )
    })
}

fn is_primitive(ty: &str) -> bool {
    let (base, suffix) = simple_type_parts(ty);
    suffix.is_empty()
        && matches!(
            base,
            "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double"
        )
}

#[cfg(test)]
mod tests;
