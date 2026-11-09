// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Parameter Mapping
//!
//! This module provides utilities for mapping type parameters to their positions
//! and performing correct type variable substitution.
//!
//! Example:
//! - For `Map<K, V>`, K is at position 0, V is at position 1
//! - When `Map.get(K key)` returns `V`, we look up V's position (1)
//!   and substitute with type_arguments[1]

use std::collections::{HashMap, HashSet};

use crate::symbol::{ClassType, Type};
use hyperion_workspace::WorkspaceIndex;
use tracing::debug;

/// Build the declared type-parameter bindings for a receiver.
///
/// Parameters are matched by their indexed position, never by spelling.
pub fn receiver_type_substitutions(
    workspace: &WorkspaceIndex,
    receiver_fqn: &str,
    owner_fqn: &str,
    receiver_type: Option<&Type>,
) -> Option<HashMap<String, Type>> {
    let receiver_arguments = match receiver_type {
        Some(Type::Class(receiver)) => receiver
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        // Arrays, primitives, unknown/error-recovery types have no declared
        // receiver parameters. Preserve the existing empty-substitution
        // behavior used by Object/array/navigation call paths.
        Some(_) => return Some(HashMap::new()),
        // Navigation and static-owner-only callers still know the concrete
        // indexed receiver declaration. Projecting from that declaration is
        // necessary for inherited edges such as Child -> Base<String>;
        // genuinely raw receiver parameters are represented by the empty
        // argument list and erased by the workspace projection.
        None => Vec::new(),
    };
    // Callers pass canonical declaration owners from indexed symbols. Let the
    // workspace projection validate that owner under its single coherent read;
    // a separate owner lookup would add a lock and could observe another
    // revision before the hierarchy projection.
    let projected =
        workspace.project_owner_type_arguments(receiver_fqn, &receiver_arguments, owner_fqn)?;
    let parameter_names = projected
        .iter()
        .map(|(parameter, _)| parameter.name.as_ref())
        .collect::<HashSet<_>>();
    let mut substitutions = HashMap::new();
    for (parameter, projected) in &projected {
        let resolved = match projected.as_deref() {
            Some(written) => crate::type_parser::parse_type_string(written)?,
            None => match parameter.bounds.first() {
                None => Type::Class(ClassType::object()),
                Some(bound) => apply_type_parameter_substitutions(
                    parse_type_in_declaration(workspace, owner_fqn, bound, &parameter_names)?,
                    &substitutions,
                )
                .erasure(),
            },
        };
        substitutions.insert(parameter.name.to_string(), resolved);
    }
    Some(substitutions)
}

/// Parse a type spelling in the declaration owner's source context. Indexed
/// symbols deliberately retain type-variable spellings, while nominal names
/// need the owner's imports/package to compare against semantic argument
/// types. Missing names remain as written for incomplete-classpath recovery;
/// ambiguous names fail closed.
pub(crate) fn parse_type_in_declaration(
    workspace: &WorkspaceIndex,
    owner_fqn: &str,
    written: &str,
    type_parameter_names: &HashSet<&str>,
) -> Option<Type> {
    parse_type_in_declaration_with_policy(
        workspace,
        owner_fqn,
        written,
        type_parameter_names,
        false,
    )
}

/// Resolve every nominal component against the declaration's own imports.
/// Callers selecting an overload cannot use an unbound placeholder as evidence.
#[must_use]
pub fn parse_type_in_declaration_exact(
    workspace: &WorkspaceIndex,
    owner_fqn: &str,
    written: &str,
    type_parameter_names: &HashSet<&str>,
) -> Option<Type> {
    parse_type_in_declaration_with_policy(workspace, owner_fqn, written, type_parameter_names, true)
}

fn parse_type_in_declaration_with_policy(
    workspace: &WorkspaceIndex,
    owner_fqn: &str,
    written: &str,
    type_parameter_names: &HashSet<&str>,
    require_exact: bool,
) -> Option<Type> {
    let file_id = workspace.find_symbol(owner_fqn)?.file_id;
    let lexical_owners = [owner_fqn.to_owned()];
    let parsed = crate::type_parser::parse_type_string(written)?;
    qualify_type_in_declaration(
        workspace,
        file_id,
        &lexical_owners,
        parsed,
        type_parameter_names,
        require_exact,
    )
}

fn qualify_type_in_declaration(
    workspace: &WorkspaceIndex,
    file_id: hyperion_workspace::FileId,
    lexical_owners: &[String],
    ty: Type,
    type_parameter_names: &HashSet<&str>,
    require_exact: bool,
) -> Option<Type> {
    match ty {
        Type::Class(mut class_type) => {
            class_type.type_arguments = class_type
                .type_arguments
                .into_iter()
                .map(|argument| {
                    qualify_type_in_declaration(
                        workspace,
                        file_id,
                        lexical_owners,
                        argument,
                        type_parameter_names,
                        require_exact,
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            if class_type.package.is_none()
                && type_parameter_names.contains(class_type.name.as_str())
            {
                return Some(Type::Class(class_type));
            }
            // The parser represents `Outer.Inner` as package=`Outer`, name=`Inner`.
            // That does not prove `Outer` is an absolute package: it may be a
            // type imported by the declaring file. Resolve the entire spelling
            // in that declaration context, including its nested components.
            let written = class_type.raw_qualified_name();
            match workspace.resolve_type_name(file_id, lexical_owners, &written) {
                hyperion_workspace::TypeNameResolution::Exact(resolved) => {
                    let Type::Class(mut resolved) =
                        crate::type_parser::parse_type_string(&resolved)?
                    else {
                        return None;
                    };
                    resolved.type_arguments = class_type.type_arguments;
                    Some(Type::Class(resolved))
                }
                hyperion_workspace::TypeNameResolution::Missing
                    if require_exact
                        && workspace.is_external_file(file_id)
                        && written.contains('.')
                        && workspace
                            .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                                written.as_str(),
                                hyperion_workspace::DeclarationNamespace::Type,
                            ))
                            .is_some() =>
                {
                    Some(Type::Class(class_type))
                }
                hyperion_workspace::TypeNameResolution::Missing if !require_exact => {
                    Some(Type::Class(class_type))
                }
                hyperion_workspace::TypeNameResolution::Missing => None,
                hyperion_workspace::TypeNameResolution::Ambiguous(_) => None,
            }
        }
        Type::Array {
            element_type,
            dimensions,
        } => Some(Type::Array {
            element_type: Box::new(qualify_type_in_declaration(
                workspace,
                file_id,
                lexical_owners,
                *element_type,
                type_parameter_names,
                require_exact,
            )?),
            dimensions,
        }),
        Type::TypeVariable { name, bounds } => Some(Type::TypeVariable {
            name,
            bounds: bounds
                .into_iter()
                .map(|bound| {
                    qualify_type_in_declaration(
                        workspace,
                        file_id,
                        lexical_owners,
                        bound,
                        type_parameter_names,
                        require_exact,
                    )
                })
                .collect::<Option<Vec<_>>>()?,
        }),
        Type::Wildcard {
            upper_bound,
            lower_bound,
        } => Some(Type::Wildcard {
            upper_bound: match upper_bound {
                Some(bound) => Some(Box::new(qualify_type_in_declaration(
                    workspace,
                    file_id,
                    lexical_owners,
                    *bound,
                    type_parameter_names,
                    require_exact,
                )?)),
                None => None,
            },
            lower_bound: match lower_bound {
                Some(bound) => Some(Box::new(qualify_type_in_declaration(
                    workspace,
                    file_id,
                    lexical_owners,
                    *bound,
                    type_parameter_names,
                    require_exact,
                )?)),
                None => None,
            },
        }),
        Type::Intersection { types } => Some(Type::Intersection {
            types: types
                .into_iter()
                .map(|bound| {
                    qualify_type_in_declaration(
                        workspace,
                        file_id,
                        lexical_owners,
                        bound,
                        type_parameter_names,
                        require_exact,
                    )
                })
                .collect::<Option<Vec<_>>>()?,
        }),
        Type::Union { types } => Some(Type::Union {
            types: types
                .into_iter()
                .map(|bound| {
                    qualify_type_in_declaration(
                        workspace,
                        file_id,
                        lexical_owners,
                        bound,
                        type_parameter_names,
                        require_exact,
                    )
                })
                .collect::<Option<Vec<_>>>()?,
        }),
        other => Some(other),
    }
}

/// Preserve lexical variable bounds when semantic types cross the workspace's
/// string-based generic projection boundary. Only actual TypeVariable nodes
/// supply bindings; a nominal class with the same spelling never does.
pub(crate) fn type_variable_bindings(types: &[Type]) -> HashMap<String, Type> {
    fn collect(ty: &Type, bindings: &mut HashMap<String, Type>) {
        match ty {
            Type::TypeVariable { name, .. } => {
                bindings.entry(name.clone()).or_insert_with(|| ty.clone());
            }
            Type::Class(class) => {
                for argument in &class.type_arguments {
                    collect(argument, bindings);
                }
            }
            Type::Array { element_type, .. } => collect(element_type, bindings),
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => {
                for bound in upper_bound.iter().chain(lower_bound) {
                    collect(bound, bindings);
                }
            }
            Type::Intersection { types } | Type::Union { types } => {
                for ty in types {
                    collect(ty, bindings);
                }
            }
            _ => {}
        }
    }
    let mut bindings = HashMap::new();
    for ty in types {
        collect(ty, &mut bindings);
    }
    bindings
}

/// Recursively apply declared type-parameter bindings to a semantic type.
pub fn apply_type_parameter_substitutions(ty: Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Class(mut class_type) => {
            if class_type.package.is_none() && class_type.type_arguments.is_empty() {
                if let Some(substituted) = substitutions.get(&class_type.name) {
                    return substituted.clone();
                }
            }
            class_type.type_arguments = class_type
                .type_arguments
                .into_iter()
                .map(|argument| apply_type_parameter_substitutions(argument, substitutions))
                .collect();
            Type::Class(class_type)
        }
        Type::TypeVariable { name, bounds } => {
            substitutions
                .get(&name)
                .cloned()
                .unwrap_or_else(|| Type::TypeVariable {
                    name,
                    bounds: bounds
                        .into_iter()
                        .map(|bound| apply_type_parameter_substitutions(bound, substitutions))
                        .collect(),
                })
        }
        Type::Array {
            element_type,
            dimensions,
        } => Type::Array {
            element_type: Box::new(apply_type_parameter_substitutions(
                *element_type,
                substitutions,
            )),
            dimensions,
        },
        Type::Wildcard {
            upper_bound,
            lower_bound,
        } => Type::Wildcard {
            upper_bound: upper_bound
                .map(|bound| Box::new(apply_type_parameter_substitutions(*bound, substitutions))),
            lower_bound: lower_bound
                .map(|bound| Box::new(apply_type_parameter_substitutions(*bound, substitutions))),
        },
        Type::Intersection { types } => Type::Intersection {
            types: types
                .into_iter()
                .map(|ty| apply_type_parameter_substitutions(ty, substitutions))
                .collect(),
        },
        Type::Union { types } => Type::Union {
            types: types
                .into_iter()
                .map(|ty| apply_type_parameter_substitutions(ty, substitutions))
                .collect(),
        },
        other => other,
    }
}

/// Substitute type variables in a method's return type using the receiver's
/// type arguments.
///
/// For `Holder<T>.fetch(): T` called on a receiver `Holder<Item>`, the parsed
/// return type is `Type::Class { name: "T" }`. This walks the type and replaces
/// type variables with the receiver's type arguments by position, recursing into
/// nested generics and arrays.
///
/// Substitution requires the indexed receiver declaration. If that declaration
/// does not identify the return name as a type parameter, the type is preserved.
pub fn substitute_receiver_type_variables(
    workspace: &WorkspaceIndex,
    return_type: Type,
    receiver_type: &Type,
) -> Type {
    let Type::Class(receiver) = receiver_type else {
        return return_type;
    };
    let receiver_fqn = receiver.package.as_ref().map_or_else(
        || receiver.name.clone(),
        |package| format!("{package}.{}", receiver.name),
    );
    let substitutions =
        receiver_type_substitutions(workspace, &receiver_fqn, &receiver_fqn, Some(receiver_type))
            .unwrap_or_default();
    let substituted = apply_type_parameter_substitutions(return_type, &substitutions);
    debug!(receiver = %receiver_fqn, ?substitutions, ?substituted, "applied receiver type substitutions");
    substituted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_generic_return_keeps_undeclared_outer_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("test/Box.java");
        workspace
            .index_file_with_source(&path, "package test; class Box<E> {}")
            .expect("source should index");
        let return_type = crate::type_parser::parse_type_string("Stream<E>")
            .expect("generic return type should parse");
        let receiver = crate::type_parser::parse_type_string("test.Box<String>")
            .expect("receiver type should parse");

        let substituted = substitute_receiver_type_variables(&workspace, return_type, &receiver);
        let Type::Class(stream) = substituted else {
            panic!("outer Stream type must be preserved");
        };
        assert_eq!(stream.name, "Stream");
        assert!(matches!(
            stream.type_arguments.as_slice(),
            [Type::Class(element)] if element.name == "String"
        ));
    }

    #[test]
    fn positional_substitution_handles_multi_letter_names_without_rewriting_classes() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("test/Pair.java");
        workspace
            .index_file_with_source(&path, "package test; class Pair<Key, Value> {}")
            .expect("source should index");
        let receiver =
            crate::type_parser::parse_type_string("test.Pair<java.lang.String, java.lang.Integer>")
                .expect("receiver type should parse");
        let unresolved = crate::type_parser::parse_type_string("Map<Value, S>")
            .expect("return type should parse");

        let substitutions =
            receiver_type_substitutions(&workspace, "test.Pair", "test.Pair", Some(&receiver))
                .unwrap();
        let resolved = apply_type_parameter_substitutions(unresolved, &substitutions);

        assert!(matches!(
            &resolved,
            Type::Class(map)
                if matches!(map.type_arguments.as_slice(),
                    [Type::Class(value), Type::Class(concrete_s)]
                    if value.name == "Integer" && concrete_s.name == "S")
        ));
        assert_eq!(
            substitutions.get("Key").map(ToString::to_string).as_deref(),
            Some("java.lang.String")
        );
        assert_eq!(
            substitutions
                .get("Value")
                .map(ToString::to_string)
                .as_deref(),
            Some("java.lang.Integer")
        );
    }

    #[test]
    fn raw_receiver_erases_unbounded_return_to_object() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("test/Box.java");
        workspace
            .index_file_with_source(
                &path,
                "package test; class Box<T> { T get() { return null; } }",
            )
            .expect("source should index");
        let return_type =
            crate::type_parser::parse_type_string("T").expect("return type should parse");
        let receiver =
            crate::type_parser::parse_type_string("test.Box").expect("receiver type should parse");

        let substituted = substitute_receiver_type_variables(&workspace, return_type, &receiver);

        assert_eq!(substituted, Type::Class(ClassType::object()));
    }

    #[test]
    fn raw_receiver_erases_return_to_leftmost_bound() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("test/Box.java");
        workspace
            .index_file_with_source(
                &path,
                "package test; class Box<T extends Number & Comparable<T>> { T get() { return null; } }",
            )
            .expect("source should index");
        let return_type =
            crate::type_parser::parse_type_string("T").expect("return type should parse");
        let receiver =
            crate::type_parser::parse_type_string("test.Box").expect("receiver type should parse");

        let substituted = substitute_receiver_type_variables(&workspace, return_type, &receiver);

        assert_eq!(substituted.to_string(), "Number");
    }

    #[test]
    fn parameterized_receiver_still_substitutes_concrete_return() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("test/Box.java");
        workspace
            .index_file_with_source(
                &path,
                "package test; class Box<T> { T get() { return null; } }",
            )
            .expect("source should index");
        let return_type =
            crate::type_parser::parse_type_string("T").expect("return type should parse");
        let receiver = crate::type_parser::parse_type_string("test.Box<java.lang.String>")
            .expect("receiver type should parse");

        let substituted = substitute_receiver_type_variables(&workspace, return_type, &receiver);

        assert_eq!(substituted.to_string(), "java.lang.String");
    }

    #[test]
    fn non_class_receiver_keeps_empty_substitutions() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());

        let substitutions = receiver_type_substitutions(
            &workspace,
            "java.lang.Object",
            "java.lang.Object",
            Some(&Type::Primitive(crate::symbol::PrimitiveType::Int)),
        )
        .expect("non-class receivers have no declaration substitutions");

        assert!(substitutions.is_empty());
    }

    #[test]
    fn exact_declaration_type_accepts_indexed_external_fqn_without_import_map() {
        fn external_type(fqn: &str) -> hyperion_workspace::Symbol {
            hyperion_workspace::Symbol {
                fqn: fqn.into(),
                simple_name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
                range: hyperion_workspace::TextRange::new(0, 0),
                kind: lsp_types::SymbolKind::CLASS,
                modifiers: hyperion_workspace::Modifiers::default(),
                container: None,
                return_type: None,
                parameters: Vec::new(),
                parent_class: None,
                interfaces: Vec::new(),
                documentation: None,
                type_parameters: Vec::new(),
                is_deprecated: false,
                receiver_type: None,
                kotlin: None,
                throws: None,
            }
        }

        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace.add_external_symbols_batch(vec![
            external_type("dep.Owner"),
            external_type("dep.Value"),
        ]);

        let resolved =
            parse_type_in_declaration_exact(&workspace, "dep.Owner", "dep.Value", &HashSet::new());
        assert_eq!(
            resolved.map(|ty| ty.to_string()),
            Some("dep.Value".to_string())
        );
        assert!(
            parse_type_in_declaration_exact(&workspace, "dep.Owner", "Value", &HashSet::new(),)
                .is_none(),
            "a bare external name still needs an exact binding"
        );
    }
}
