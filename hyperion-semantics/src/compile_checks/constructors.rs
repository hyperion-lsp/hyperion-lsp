// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Provable `new T(args)` errors: no constructor accepts the arguments, or the
//! type cannot be instantiated at all.

use lsp_types::SymbolKind;

use super::calls::{params_reject_arguments, visible_type_variables, Inapplicable, ParamSpec};
use super::facts::TypeFacts;
use crate::call_resolution::parse_parameter_type;
use crate::symbol::Type;

/// What is wrong with an instantiation, if anything provable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstructorOutcome {
    Fine,
    /// The type is an interface or abstract class and no class body follows.
    Abstract,
    /// Every constructor provably rejects the arguments. Each entry is the
    /// candidate's parameter spellings with the reason it was rejected.
    Inapplicable(Vec<(Vec<String>, Inapplicable)>),
}

/// Decide `new type_fqn(arguments)`; `has_body` is true for an anonymous
/// class (`new T(..) { .. }`).
pub fn check_constructor(
    facts: &TypeFacts<'_>,
    type_fqn: &str,
    arguments: &[Option<Type>],
    strict_arguments: &[bool],
    has_body: bool,
) -> ConstructorOutcome {
    let Some(symbol) = facts.type_symbol(type_fqn) else {
        return ConstructorOutcome::Fine;
    };
    if facts.is_foreign_language_type(type_fqn) {
        return ConstructorOutcome::Fine;
    }

    match symbol.kind {
        SymbolKind::INTERFACE => {
            return if has_body {
                ConstructorOutcome::Fine
            } else {
                ConstructorOutcome::Abstract
            };
        }
        // Enums cannot be instantiated and records/annotations have implicit
        // or special constructors: those errors are not this check's business.
        SymbolKind::ENUM | SymbolKind::STRUCT | SymbolKind::OBJECT => {
            return ConstructorOutcome::Fine;
        }
        SymbolKind::CLASS => {}
        _ => return ConstructorOutcome::Fine,
    }
    if symbol.modifiers.is_abstract && !has_body {
        return ConstructorOutcome::Abstract;
    }
    // An anonymous subclass passes its arguments to the superclass
    // constructor, which is exactly what is checked below.

    let constructors = facts.index().get_constructors_for_class(type_fqn);
    // A class without declared constructors has the implicit default one --
    // unless Lombok may have generated constructors for it.
    let implicit_default = constructors.is_empty();
    if constructors_may_be_generated(facts, type_fqn) {
        return ConstructorOutcome::Fine;
    }

    let mut rejected = Vec::new();
    if implicit_default {
        if let Some(reason) = params_reject_arguments(
            facts,
            &[],
            &std::collections::HashSet::default(),
            arguments,
            strict_arguments,
        ) {
            rejected.push((Vec::new(), reason));
        } else {
            return ConstructorOutcome::Fine;
        }
    }
    for constructor in &constructors {
        let params = constructor_param_specs(constructor);
        let type_variables = visible_type_variables(facts, constructor);
        match params_reject_arguments(facts, &params, &type_variables, arguments, strict_arguments)
        {
            Some(reason) => rejected.push((
                constructor
                    .parameters
                    .iter()
                    .map(|parameter| display_parameter_type(parameter))
                    .collect(),
                reason,
            )),
            None => return ConstructorOutcome::Fine,
        }
    }
    ConstructorOutcome::Inapplicable(rejected)
}

/// Lombok can add constructors next to hand-written ones
/// (`@AllArgsConstructor`) or replace the implicit default one
/// (`@Data` with final fields); a class declared in a Lombok file is never
/// judged.
fn constructors_may_be_generated(facts: &TypeFacts<'_>, type_fqn: &str) -> bool {
    let index = facts.index();
    let Some(location) = index.find_symbol(type_fqn) else {
        return false;
    };
    if !index.is_workspace_file(location.file_id) {
        return false;
    }
    index
        .get_file_contents(location.file_id)
        .is_none_or(|source| source.contains("lombok"))
}

/// `java.util.List<T> items` -> `List<T>` (simple names, no parameter name).
fn display_parameter_type(parameter: &str) -> String {
    let trimmed = crate::type_parser::strip_parameter_name(parameter);
    simplify_qualified_names(trimmed)
}

/// Drop package prefixes from every dotted name in a type spelling.
pub fn simplify_qualified_names(spelling: &str) -> String {
    let mut out = String::with_capacity(spelling.len());
    let mut segment = String::new();
    for ch in spelling.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '.' {
            segment.push(ch);
        } else {
            out.push_str(segment.rsplit('.').next().unwrap_or_default());
            segment.clear();
            out.push(ch);
        }
    }
    out.push_str(segment.rsplit('.').next().unwrap_or_default());
    out
}

/// Every constructor of `type_fqn` that is not provably inapplicable, or `None`
/// when the set cannot be trusted (unknown type, Lombok, foreign language).
pub fn possibly_applicable_constructors(
    facts: &TypeFacts<'_>,
    type_fqn: &str,
    arguments: &[Option<Type>],
    strict_arguments: &[bool],
) -> Option<Vec<std::sync::Arc<hyperion_workspace::Symbol>>> {
    let symbol = facts.type_symbol(type_fqn)?;
    if facts.is_foreign_language_type(type_fqn)
        || symbol.kind != SymbolKind::CLASS
        || constructors_may_be_generated(facts, type_fqn)
    {
        return None;
    }
    Some(
        facts
            .index()
            .get_constructors_for_class(type_fqn)
            .into_iter()
            .filter(|constructor| {
                let params = constructor_param_specs(constructor);
                let type_variables = visible_type_variables(facts, constructor);
                params_reject_arguments(
                    facts,
                    &params,
                    &type_variables,
                    arguments,
                    strict_arguments,
                )
                .is_none()
            })
            .collect(),
    )
}

/// The parameter list of a constructor with the best type spellings available.
///
/// Bytecode constructors keep simple names in `parameters` (`File`) but carry
/// exact qualified types in the descriptor embedded in their FQN
/// (`java.io.FileReader.FileReader(java.io.File)`); source constructors only
/// have the written spellings. A qualified descriptor entry wins.
fn constructor_param_specs(constructor: &hyperion_workspace::Symbol) -> Vec<ParamSpec> {
    let descriptor: Vec<&str> = constructor
        .fqn
        .split_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .map(|list| {
            if list.is_empty() {
                Vec::new()
            } else {
                list.split(',').collect()
            }
        })
        .unwrap_or_default();
    let aligned = descriptor.len() == constructor.parameters.len();
    constructor
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let variadic = parameter.contains("...");
            let qualified = aligned
                .then(|| descriptor[index])
                .filter(|written| written.contains('.') || is_primitive_name(written));
            let ty = match qualified {
                Some(written) => crate::type_parser::parse_type_string(written).map(|ty| {
                    if variadic && !matches!(ty, Type::Array { .. }) {
                        Type::Array {
                            element_type: Box::new(ty),
                            dimensions: 1,
                        }
                    } else {
                        ty
                    }
                }),
                None => parse_parameter_type(parameter),
            };
            ParamSpec { ty, variadic }
        })
        .collect()
}

fn is_primitive_name(name: &str) -> bool {
    matches!(
        name.trim_end_matches("[]"),
        "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double"
    )
}
