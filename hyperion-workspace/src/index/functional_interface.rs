// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java functional-interface election and function-signature projection.

use super::effective_methods::{
    effective_receiver_methods_in, erased_callable_signature_in,
    receiver_erased_callable_signature_in, ErasedCallableSignature, ReceiverProjection,
};
use super::generic_supertypes::{qualify_type_with_bindings_in, TypeBindings};
use super::*;

/// The elected declaration and receiver-substituted Java function signature.
#[derive(Clone, Debug)]
pub struct FunctionalInterfaceSignature {
    pub method: Arc<crate::Symbol>,
    pub parameter_types: Vec<String>,
    pub return_type: String,
}

/// Result of querying a Java function type without conflating an absent class
/// path entry with an indexed declaration that is invalid or unprojectable.
#[derive(Clone, Debug)]
pub enum FunctionalInterfaceQuery {
    Missing,
    NotFunctional,
    Unavailable,
    Resolved(FunctionalInterfaceSignature),
}

fn is_public_object_signature(signature: &ErasedCallableSignature) -> bool {
    match (signature.name.as_ref(), signature.parameters.as_slice()) {
        ("equals", [parameter]) => matches!(parameter.as_str(), "Object" | "java.lang.Object"),
        ("getClass" | "hashCode" | "notify" | "notifyAll" | "toString", []) => true,
        ("wait", []) => true,
        ("wait", [parameter]) => parameter == "long",
        ("wait", [first, second]) => first == "long" && second == "int",
        _ => false,
    }
}

fn functional_interface_method_in(
    inner: &IndexInner,
    interface_fqn: &str,
) -> Option<ResolvedDefinition> {
    let interface = inner.symbols.get(interface_fqn)?;
    if interface.kind != SymbolKind::INTERFACE || interface.modifiers.is_sealed {
        return None;
    }

    let effective_methods = effective_receiver_methods_in(inner, interface_fqn, None);
    let mut abstract_methods = effective_methods.iter().filter(|definition| {
        let method = &definition.symbol;
        method.modifiers.is_abstract
            && !method.modifiers.is_private
            && !method.modifiers.is_static
            && erased_callable_signature_in(inner, method)
                .is_some_and(|signature| !is_public_object_signature(&signature))
    });
    let method = abstract_methods.next()?.clone();
    if abstract_methods.next().is_some() {
        return None;
    }

    // An unrelated default and abstract declaration with the same erased
    // signature is an inherited conflict, not a one-method function type. A
    // maximally-specific legal redeclaration has already collapsed the slot in
    // effective method election and therefore does not reach this check.
    let signature = receiver_erased_callable_signature_in(inner, interface_fqn, &method.symbol)?;
    if effective_methods.iter().any(|candidate| {
        candidate.target != method.target
            && receiver_erased_callable_signature_in(inner, interface_fqn, &candidate.symbol)
                .as_ref()
                == Some(&signature)
    }) {
        return None;
    }
    Some(method)
}

/// Remove a Java source parameter name while preserving whitespace nested in
/// its generic type. Bytecode parameters contain only the type and pass
/// through unchanged.
pub fn java_parameter_parts_from_declaration(parameter: &str) -> (&str, Option<&str>) {
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
    last_top_level_whitespace.map_or((parameter, None), |offset| {
        let parameter_type = parameter[..offset].trim_end();
        let name = parameter[offset..].trim();
        (parameter_type, (!name.is_empty()).then_some(name))
    })
}

pub fn java_parameter_type_from_declaration(parameter: &str) -> &str {
    java_parameter_parts_from_declaration(parameter).0
}

fn mentions_type_parameter(written: &str, parameters: &[crate::TypeParameterInfo]) -> bool {
    written
        .split(|character: char| {
            !(character.is_alphanumeric() || matches!(character, '_' | '$' | '.'))
        })
        .any(|component| {
            !component.is_empty()
                && parameters
                    .iter()
                    .any(|parameter| parameter.name.as_ref() == component)
        })
}

fn project_functional_interface_signature_in(
    inner: &IndexInner,
    interface_fqn: &str,
    method: Arc<crate::Symbol>,
    type_arguments: &[String],
) -> Option<FunctionalInterfaceSignature> {
    let interface = inner.symbols.get(interface_fqn)?;
    let raw = type_arguments.is_empty() && !interface.type_parameters.is_empty();
    let arguments = ground_function_type_arguments(inner, interface, type_arguments)?;
    let mut projection = ReceiverProjection::with_arguments(inner, interface_fqn, &arguments);
    let mut project = |written: &str| {
        if mentions_type_parameter(written, &method.type_parameters) {
            None
        } else if raw {
            // JLS 9.9 erases the entire function signature of a raw target,
            // including parameterized types that do not mention its variables.
            projection.erased_member_type(&method, written)
        } else {
            projection.member_type(&method, written)
        }
    };

    let parameter_types = method
        .parameters
        .iter()
        .map(|parameter| project(java_parameter_type_from_declaration(parameter)))
        .collect::<Option<Vec<_>>>()?;
    let return_type = method
        .return_type
        .as_deref()
        .map_or_else(|| Some("void".to_string()), project)?;
    Some(FunctionalInterfaceSignature {
        method,
        parameter_types,
        return_type,
    })
}

/// JLS 9.9: an implicitly typed lambda uses the non-wildcard
/// parameterization of its functional target, not wildcard parameter types.
fn ground_function_type_arguments(
    inner: &IndexInner,
    interface: &crate::Symbol,
    arguments: &[String],
) -> Option<Vec<String>> {
    if arguments.is_empty() {
        return Some(Vec::new());
    }
    if arguments.len() != interface.type_parameters.len() {
        return None;
    }
    arguments
        .iter()
        .zip(&interface.type_parameters)
        .map(|(argument, parameter)| {
            let argument = argument.trim();
            let Some(wildcard) = argument.strip_prefix('?') else {
                return Some(argument.to_owned());
            };
            if parameter
                .bounds
                .iter()
                .any(|bound| mentions_type_parameter(bound, &interface.type_parameters))
            {
                return None;
            }
            // An intersection bound needs a structured greatest-lower-bound
            // representation. Keep that contract unavailable until supported.
            let bound = match parameter.bounds.as_slice() {
                [] => "java.lang.Object",
                [bound] => bound.as_ref(),
                _ => return None,
            };
            let resolved_bound = qualify_type_with_bindings_in(
                &OwnerElection::new(inner, None),
                &interface.fqn,
                bound,
                &TypeBindings::new(),
            )?;
            let bound = resolved_bound.as_str();
            let wildcard = wildcard.trim();
            if wildcard.is_empty() {
                return Some(bound.to_owned());
            }
            if let Some(lower) = wildcard
                .strip_prefix("super")
                .filter(|rest| rest.starts_with(char::is_whitespace))
            {
                return (!lower.trim().is_empty()).then(|| lower.trim().to_owned());
            }
            if let Some(upper) = wildcard
                .strip_prefix("extends")
                .filter(|rest| rest.starts_with(char::is_whitespace))
            {
                let upper = upper.trim();
                if !upper.is_empty()
                    && (matches!(bound, "Object" | "java.lang.Object") || bound == upper)
                {
                    return Some(upper.to_owned());
                }
                if matches!(upper, "Object" | "java.lang.Object") {
                    return Some(bound.to_owned());
                }
            }
            None
        })
        .collect()
}

impl WorkspaceIndex {
    /// Return the single abstract method defining `interface_fqn`'s Java
    /// function type. Election includes inherited contracts and excludes
    /// Object-equivalent, default, static/private, and sealed declarations.
    pub fn find_functional_interface_method(
        &self,
        interface_fqn: &str,
    ) -> Option<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        functional_interface_method_in(&inner, interface_fqn).map(|definition| definition.symbol)
    }

    /// Project a Java function signature through the receiver's complete
    /// parameterized interface hierarchy.
    pub fn find_functional_interface_signature(
        &self,
        interface_fqn: &str,
        type_arguments: &[String],
    ) -> Option<FunctionalInterfaceSignature> {
        match self.query_functional_interface(interface_fqn, type_arguments) {
            FunctionalInterfaceQuery::Resolved(signature) => Some(signature),
            FunctionalInterfaceQuery::Missing
            | FunctionalInterfaceQuery::NotFunctional
            | FunctionalInterfaceQuery::Unavailable => None,
        }
    }

    /// Query and project one Java function type under a single coherent index
    /// read. Callers may safely apply hard-coded platform fallbacks only to
    /// [`FunctionalInterfaceQuery::Missing`].
    pub fn query_functional_interface(
        &self,
        interface_fqn: &str,
        type_arguments: &[String],
    ) -> FunctionalInterfaceQuery {
        let inner = self.inner.read();
        let Some(interface) = inner.symbols.get(interface_fqn) else {
            return FunctionalInterfaceQuery::Missing;
        };
        if interface.kind != SymbolKind::INTERFACE || interface.modifiers.is_sealed {
            return FunctionalInterfaceQuery::NotFunctional;
        }
        let Some(method) = functional_interface_method_in(&inner, interface_fqn) else {
            return FunctionalInterfaceQuery::NotFunctional;
        };
        project_functional_interface_signature_in(
            &inner,
            interface_fqn,
            method.symbol,
            type_arguments,
        )
        .map_or(
            FunctionalInterfaceQuery::Unavailable,
            FunctionalInterfaceQuery::Resolved,
        )
    }
}
