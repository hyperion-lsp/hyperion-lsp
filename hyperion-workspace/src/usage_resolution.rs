// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Conservative symbol resolution shared by tree-sitter JVM-language usage walkers.

use crate::{
    import_resolver::ImportResolver, usage_extractor::Usage, FileId, Symbol, SymbolLocation,
    TextRange,
};
use lsp_types::SymbolKind;
use std::collections::HashSet;
use std::sync::Arc;
use tree_sitter::Node;

pub(crate) const fn is_callable_declaration(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::METHOD | SymbolKind::FUNCTION)
}

/// Syntax-independent argument fact used by exact static-call navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvocationArgumentType {
    Known(String),
    Null,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PotentialApplicability {
    No,
    Yes,
    Unknown,
}

pub(crate) fn resolve_type_name(raw: &str, resolver: &ImportResolver<'_>) -> Option<String> {
    let name = raw
        .split(['[', '<'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_start_matches("=>")
        .trim();
    let crate::TypeNameResolution::Exact(fqn) = resolver.resolve_type_identity(name) else {
        return None;
    };
    resolver
        .get_symbol_target(&crate::SymbolTarget::new(
            Arc::from(fqn.as_str()),
            crate::DeclarationNamespace::Type,
        ))
        .map(|_| fqn)
}

pub(crate) fn resolve_method(
    receiver_fqn: &str,
    method_name: &str,
    resolver: &ImportResolver<'_>,
) -> Option<Arc<Symbol>> {
    resolve_method_with_descriptor(receiver_fqn, method_name, None, resolver)
}

pub(crate) fn resolve_method_with_descriptor(
    receiver_fqn: &str,
    method_name: &str,
    argument_descriptor: Option<&str>,
    resolver: &ImportResolver<'_>,
) -> Option<Arc<Symbol>> {
    let inheritance = resolver.get_inheritance_chain(receiver_fqn);

    let is_interface = |owner: &&String| {
        resolver
            .get_symbol(owner)
            .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE)
    };
    let class_owners: Vec<_> = inheritance
        .iter()
        .filter(|owner| !is_interface(owner))
        .collect();
    let interface_owners: Vec<_> = inheritance.iter().filter(is_interface).collect();

    // A declaration overrides only the same signature. It does not hide other
    // overloads inherited from a superclass. Keeping the whole effective set is
    // essential for calls such as `Child.load(1)` where Child declares only
    // `load(String)` and Base declares `load(int)`.
    let mut class_signatures = HashSet::new();
    let mut candidates = Vec::new();
    for owner in class_owners {
        for symbol in declared_methods(owner, method_name, resolver) {
            if !resolver.member_is_inherited_by(receiver_fqn, &symbol) {
                continue;
            }
            let signature = declared_signature(&symbol);
            if class_signatures.insert(signature.clone()) {
                candidates.push(MethodCandidate {
                    owner,
                    signature,
                    symbol,
                });
            }
        }
    }

    // Class declarations take precedence over an override-equivalent interface
    // declaration. Among interfaces, a subinterface declaration shadows its
    // parent. Unrelated declarations remain in the set so resolution stays
    // conservative instead of depending on `implements` order.
    let mut interface_candidates = Vec::new();
    for owner in interface_owners {
        for symbol in declared_methods(owner, method_name, resolver) {
            if !resolver.member_is_inherited_by(receiver_fqn, &symbol)
                || (owner != receiver_fqn && symbol.modifiers.is_static)
            {
                continue;
            }
            let signature = declared_signature(&symbol);
            if !class_signatures.contains(&signature) {
                interface_candidates.push(MethodCandidate {
                    owner,
                    signature,
                    symbol,
                });
            }
        }
    }
    let interface_declarations: Vec<_> = interface_candidates
        .iter()
        .map(|candidate| (candidate.owner, candidate.signature.clone()))
        .collect();
    interface_candidates.retain(|candidate| {
        !interface_declarations
            .iter()
            .any(|(other_owner, other_signature)| {
                *other_owner != candidate.owner
                    && *other_signature == candidate.signature
                    && resolver
                        .get_inheritance_chain(other_owner)
                        .iter()
                        .skip(1)
                        .any(|ancestor| ancestor == candidate.owner)
            })
    });
    candidates.extend(interface_candidates);

    select_method(candidates, argument_descriptor)
}

/// Resolve an unqualified statically imported call after its arguments are
/// known. The import resolver deliberately retains overloads from every
/// contributing owner; invocation applicability, rather than import order,
/// selects the declaration.
pub(crate) fn resolve_static_method_with_descriptor(
    method_name: &str,
    argument_descriptor: Option<&str>,
    resolver: &ImportResolver<'_>,
) -> Option<Arc<Symbol>> {
    let candidates = resolver
        .resolve_static_callables(method_name)
        .into_iter()
        .map(|symbol| MethodCandidate {
            owner: "",
            signature: declared_signature(&symbol),
            symbol,
        })
        .collect();
    select_method(candidates, argument_descriptor)
}

pub(crate) fn resolve_static_method_invocation(
    method_name: &str,
    argument_types: &[InvocationArgumentType],
    resolver: &ImportResolver<'_>,
) -> Option<Arc<Symbol>> {
    let candidates = resolver
        .resolve_static_callables(method_name)
        .into_iter()
        .map(|symbol| MethodCandidate {
            owner: "",
            signature: declared_signature(&symbol),
            symbol,
        })
        .collect();
    select_method_for_arguments(candidates, argument_types)
}

pub(crate) fn method_potential_applicability_with(
    method: &Symbol,
    arguments: &[InvocationArgumentType],
    reference_assignability: impl Fn(&str, &str) -> Option<bool>,
) -> PotentialApplicability {
    let arity = arguments.len();
    let variants = invocation_parameter_variants(method, arity);
    if variants.is_empty() {
        return PotentialApplicability::No;
    }
    let mut saw_unknown = false;
    for (parameters, _) in variants {
        let mut variant_unknown = false;
        let mut impossible = false;
        for (argument, parameter) in arguments.iter().zip(&parameters) {
            match invocation_conversion_possibility(argument, parameter, &reference_assignability) {
                PotentialApplicability::No => {
                    impossible = true;
                    break;
                }
                PotentialApplicability::Yes => {}
                PotentialApplicability::Unknown => variant_unknown = true,
            }
        }
        if !impossible && !variant_unknown {
            return PotentialApplicability::Yes;
        }
        saw_unknown |= !impossible && variant_unknown;
    }
    if saw_unknown {
        PotentialApplicability::Unknown
    } else {
        PotentialApplicability::No
    }
}

struct MethodCandidate<'a> {
    owner: &'a str,
    signature: Vec<String>,
    symbol: Arc<Symbol>,
}

fn declared_methods(
    owner: &str,
    method_name: &str,
    resolver: &ImportResolver<'_>,
) -> Vec<Arc<Symbol>> {
    let methods = resolver.find_methods_by_name(owner, method_name);
    if !methods.is_empty() {
        return methods;
    }

    // Some legacy/external indexes contain descriptorless callable symbols.
    // Preserve compatibility without letting the bare lookup choose among a
    // normal overload set (the indexed-name lookup above would have found it).
    resolver
        .get_symbol(&format!("{owner}.{method_name}"))
        .filter(|symbol| is_callable_declaration(symbol.kind))
        .into_iter()
        .collect()
}

fn select_method(
    candidates: Vec<MethodCandidate<'_>>,
    argument_descriptor: Option<&str>,
) -> Option<Arc<Symbol>> {
    let Some(descriptor) = argument_descriptor else {
        return (candidates.len() == 1).then(|| Arc::clone(&candidates[0].symbol));
    };

    let arguments = descriptor_types(descriptor);
    select_method_for_arguments(candidates, &arguments)
}

fn select_method_for_arguments(
    candidates: Vec<MethodCandidate<'_>>,
    arguments: &[InvocationArgumentType],
) -> Option<Arc<Symbol>> {
    let every_argument_is_known = arguments
        .iter()
        .all(|argument| !matches!(argument, InvocationArgumentType::Unknown));
    let every_argument_is_concrete = arguments
        .iter()
        .all(|argument| matches!(argument, InvocationArgumentType::Known(_)));
    if every_argument_is_concrete {
        let concrete = arguments
            .iter()
            .filter_map(|argument| match argument {
                InvocationArgumentType::Known(ty) => Some(simple_type_name(ty)),
                InvocationArgumentType::Null | InvocationArgumentType::Unknown => None,
            })
            .collect::<Vec<_>>();
        let exact: Vec<_> = candidates
            .iter()
            .filter(|candidate| invocation_signature(&candidate.symbol) == concrete)
            .collect();
        match exact.as_slice() {
            [candidate] => return Some(Arc::clone(&candidate.symbol)),
            [] => {}
            _ => return None,
        }
    }

    let arity = arguments.len();
    if every_argument_is_known {
        let applicable = candidates
            .iter()
            .flat_map(|candidate| {
                invocation_parameter_variants(&candidate.symbol, arity)
                    .into_iter()
                    .filter_map(move |(parameters, uses_varargs)| {
                        let conversion_phase = arguments
                            .iter()
                            .zip(&parameters)
                            .map(|(argument, parameter)| {
                                invocation_conversion_phase(argument, parameter)
                            })
                            .collect::<Option<Vec<_>>>()?
                            .into_iter()
                            .max()
                            .unwrap_or(0);
                        let phase = if uses_varargs { 2 } else { conversion_phase };
                        Some((phase, parameters, candidate))
                    })
            })
            .collect::<Vec<_>>();
        let best_phase = applicable.iter().map(|(phase, _, _)| *phase).min()?;
        let phase_candidates = applicable
            .iter()
            .filter(|(phase, _, _)| *phase == best_phase)
            .collect::<Vec<_>>();
        let most_specific = phase_candidates
            .iter()
            .enumerate()
            .filter(|(candidate_index, candidate)| {
                phase_candidates
                    .iter()
                    .enumerate()
                    .all(|(other_index, other)| {
                        candidate_index == &other_index
                            || invocation_signature_is_more_specific(&candidate.1, &other.1)
                    })
            })
            .map(|(_, candidate)| candidate)
            .collect::<Vec<_>>();
        return match most_specific.as_slice() {
            [candidate] => Some(Arc::clone(&candidate.2.symbol)),
            _ => None,
        };
    }

    // A missing argument type cannot prove applicability. In particular, a
    // unique arity match may still be an ill-typed call; navigation and usage
    // extraction must not turn that syntax accident into a declaration edge.
    None
}

fn invocation_parameter_variants(method: &Symbol, arity: usize) -> Vec<(Vec<String>, bool)> {
    // Preserve canonical parameter spellings for the conservative
    // assignability callback. Conversion phases normalize to simple names
    // themselves, while discarding qualification here makes two unrelated
    // `Accepted` types indistinguishable and turns a provable rejection into
    // `Unknown`.
    let mut parameters = declared_signature(method);
    let mut variants = Vec::with_capacity(2);
    if parameters.len() == arity {
        // A variable-arity declaration participates in the fixed-arity phases
        // first, with its final parameter treated as an array. If that is not
        // applicable, the same declaration gets another chance in the later
        // variable-arity phase with the array expanded to its component type.
        variants.push((parameters.clone(), false));
    }
    let declared_vararg = method
        .parameters
        .last()
        .is_some_and(|parameter| parameter.contains("...") || parameter.starts_with("vararg "));
    if !declared_vararg || arity < parameters.len().saturating_sub(1) {
        return variants;
    }
    let Some(component) = parameters
        .pop()
        .and_then(|parameter| parameter.strip_suffix("[]").map(str::to_owned))
    else {
        return variants;
    };
    parameters.resize(arity, component);
    variants.push((parameters, true));
    variants
}

fn invocation_signature_is_more_specific(left: &[String], right: &[String]) -> bool {
    left.len() == right.len()
        && left != right
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| strict_invocation_conversion(left, right))
}

fn strict_invocation_conversion(argument: &str, parameter: &str) -> bool {
    let argument = simple_type_name(argument);
    let parameter = simple_type_name(parameter);
    argument == parameter
        || primitive_widening_distance(&argument, &parameter).is_some()
        || known_reference_widening(&argument, &parameter)
}

fn known_reference_widening(from: &str, to: &str) -> bool {
    if matches!(to, "Object") && !is_primitive(from) {
        return true;
    }
    matches!(to, "Number") && is_numeric_wrapper(from)
}

fn is_primitive(type_name: &str) -> bool {
    matches!(
        type_name,
        "byte" | "short" | "int" | "long" | "float" | "double" | "char" | "boolean"
    )
}

fn is_numeric_wrapper(type_name: &str) -> bool {
    matches!(
        type_name,
        "Byte" | "Short" | "Integer" | "Long" | "Float" | "Double"
    )
}

fn invocation_conversion_phase(argument: &InvocationArgumentType, parameter: &str) -> Option<u8> {
    let InvocationArgumentType::Known(argument) = argument else {
        return match argument {
            InvocationArgumentType::Null if !is_primitive(simple_type_name(parameter).as_str()) => {
                Some(0)
            }
            InvocationArgumentType::Null | InvocationArgumentType::Unknown => None,
            InvocationArgumentType::Known(_) => unreachable!(),
        };
    };
    let argument = simple_type_name(argument);
    let parameter = simple_type_name(parameter);
    if strict_invocation_conversion(&argument, &parameter) {
        return Some(0);
    }
    if primitive_wrapper(&argument) == Some(parameter.as_str()) {
        return Some(1);
    }
    if matches!(parameter.as_str(), "Object" | "Number")
        && primitive_wrapper(&argument).is_some()
        && !(argument == "char" && parameter == "Number")
        && !(argument == "boolean" && parameter == "Number")
    {
        return Some(1);
    }
    if let Some(unboxed) = wrapper_primitive(&argument) {
        if unboxed == parameter.as_str()
            || primitive_widening_distance(unboxed, &parameter).is_some()
        {
            return Some(1);
        }
    }
    None
}

fn invocation_conversion_possibility(
    argument: &InvocationArgumentType,
    parameter: &str,
    reference_assignability: &impl Fn(&str, &str) -> Option<bool>,
) -> PotentialApplicability {
    if invocation_conversion_phase(argument, parameter).is_some() {
        return PotentialApplicability::Yes;
    }
    let InvocationArgumentType::Known(argument) = argument else {
        return match argument {
            InvocationArgumentType::Unknown => PotentialApplicability::Unknown,
            InvocationArgumentType::Null => PotentialApplicability::No,
            InvocationArgumentType::Known(_) => unreachable!(),
        };
    };
    let simple_argument = simple_type_name(argument);
    let simple_parameter = simple_type_name(parameter);
    let argument_is_primitive = is_primitive(&simple_argument);
    let parameter_is_primitive = is_primitive(&simple_parameter);

    if argument_is_primitive || parameter_is_primitive {
        return PotentialApplicability::No;
    }
    match reference_assignability(argument, parameter) {
        Some(true) => PotentialApplicability::Yes,
        Some(false) => PotentialApplicability::No,
        None => PotentialApplicability::Unknown,
    }
}

fn declared_signature(method: &Symbol) -> Vec<String> {
    method
        .parameters
        .iter()
        .map(|parameter| {
            let parameter = parameter
                .trim()
                .strip_prefix("vararg ")
                .unwrap_or(parameter);
            let ty = parameter
                .rsplit_once(char::is_whitespace)
                .map_or(parameter, |(ty, _)| ty);
            // Varargs and arrays have the same JVM override signature.
            ty.strip_suffix("...")
                .map_or_else(|| ty.to_string(), |base| format!("{base}[]"))
        })
        .collect()
}

fn invocation_signature(method: &Symbol) -> Vec<String> {
    declared_signature(method)
        .into_iter()
        .map(|parameter| simple_type_name(&parameter))
        .collect()
}

pub(crate) fn descriptor_types(descriptor: &str) -> Vec<InvocationArgumentType> {
    if descriptor.is_empty() {
        Vec::new()
    } else {
        descriptor
            .split(',')
            .map(|argument| match argument.trim() {
                "_" => InvocationArgumentType::Unknown,
                "null" => InvocationArgumentType::Null,
                known => InvocationArgumentType::Known(known.to_owned()),
            })
            .collect()
    }
}

fn simple_type_name(raw: &str) -> String {
    let raw = raw.trim();
    let suffix_start = raw
        .find('[')
        .or_else(|| raw.find("..."))
        .unwrap_or(raw.len());
    let (base_with_package, suffix) = raw.split_at(suffix_start);
    let base = base_with_package
        .rsplit('.')
        .next()
        .unwrap_or(base_with_package);
    format!("{base}{suffix}")
}

fn primitive_widening_distance(from: &str, to: &str) -> Option<u8> {
    let widening_chain: &[&str] = match from {
        "byte" => &["short", "int", "long", "float", "double"],
        "short" | "char" => &["int", "long", "float", "double"],
        "int" => &["long", "float", "double"],
        "long" => &["float", "double"],
        "float" => &["double"],
        _ => &[],
    };
    widening_chain
        .iter()
        .position(|candidate| *candidate == to)
        .and_then(|position| u8::try_from(position + 1).ok())
}

fn primitive_wrapper(primitive: &str) -> Option<&'static str> {
    Some(match primitive {
        "byte" => "Byte",
        "short" => "Short",
        "int" => "Integer",
        "long" => "Long",
        "float" => "Float",
        "double" => "Double",
        "char" => "Character",
        "boolean" => "Boolean",
        _ => return None,
    })
}

fn wrapper_primitive(wrapper: &str) -> Option<&'static str> {
    Some(match wrapper {
        "Byte" => "byte",
        "Short" => "short",
        "Integer" => "int",
        "Long" => "long",
        "Float" => "float",
        "Double" => "double",
        "Character" => "char",
        "Boolean" => "boolean",
        _ => return None,
    })
}

pub(crate) fn resolve_field(
    receiver_fqn: &str,
    field_name: &str,
    resolver: &ImportResolver<'_>,
) -> Option<Arc<Symbol>> {
    resolver.resolve_value_member(receiver_fqn, field_name)
}

pub(crate) fn emit_symbol(
    usages: &mut Vec<Usage>,
    file_id: FileId,
    node: &Node<'_>,
    symbol: Arc<Symbol>,
) {
    emit_symbol_range(usages, file_id, node.start_byte(), node.end_byte(), symbol);
}

pub(crate) fn emit_symbol_range(
    usages: &mut Vec<Usage>,
    file_id: FileId,
    start: usize,
    end: usize,
    symbol: Arc<Symbol>,
) {
    let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end)) else {
        return;
    };
    usages.push(Usage {
        fqn: symbol.fqn.to_string(),
        location: SymbolLocation {
            file_id,
            source_revision_id: None,
            range: TextRange::new(start, end),
            kind: symbol.kind,
            name: symbol.simple_name.clone(),
            fqn: symbol.fqn.clone(),
            container_name: symbol.container.clone(),
            documentation: None,
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImportMap, WorkspaceIndex};

    fn index_source(index: &WorkspaceIndex, root: &std::path::Path, relative: &str, source: &str) {
        index
            .index_file_with_source(&root.join(relative), source)
            .unwrap_or_else(|error| panic!("failed to index {relative}: {error}"));
    }

    #[test]
    fn inherited_method_resolution_filters_non_inherited_declarations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "a/Base.java",
            r#"
                package a;
                public class Base {
                    public void visible(int value) {}
                    private void secret(int value) {}
                    void packageOnly(int value) {}
                }
            "#,
        );
        index_source(
            &index,
            temp.path(),
            "b/Child.java",
            "package b; public class Child extends a.Base {}",
        );
        let imports = ImportMap::new();
        let resolver = ImportResolver::new(&imports, &index);

        assert_eq!(
            resolver.get_inheritance_chain("b.Child"),
            ["b.Child", "a.Base"]
        );
        let base_members = index.get_class_members("a.Base");
        assert_eq!(
            resolver.find_methods_by_name("a.Base", "visible").len(),
            1,
            "base members: {:?}",
            base_members
                .iter()
                .map(|member| member.fqn.as_ref())
                .collect::<Vec<_>>()
        );
        let open = resolve_method_with_descriptor("b.Child", "visible", Some("int"), &resolver)
            .expect("public method is inherited");
        assert_eq!(open.container.as_deref(), Some("a.Base"));
        assert!(
            resolve_method_with_descriptor("b.Child", "secret", Some("int"), &resolver).is_none()
        );
        assert!(
            resolve_method_with_descriptor("b.Child", "packageOnly", Some("int"), &resolver)
                .is_none()
        );
    }

    #[test]
    fn interface_static_methods_are_not_inherited() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Factory.java",
            "package p; public interface Factory { static Factory create() { return null; } }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Impl.java",
            "package p; public class Impl implements Factory {}",
        );
        let imports = ImportMap::new();
        let resolver = ImportResolver::new(&imports, &index);

        assert!(resolve_method_with_descriptor("p.Impl", "create", Some(""), &resolver).is_none());
        let direct = resolve_method_with_descriptor("p.Factory", "create", Some(""), &resolver)
            .expect("interface owns its static method");
        assert_eq!(direct.container.as_deref(), Some("p.Factory"));
    }
}
