// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Helpers for extracting nullability-related metadata from syntax nodes.

use super::{annotation_name, has_nonnull_annotation, has_nullable_annotation, Nullability};
use hyperion_parser::syntax::{AstNode, MethodDecl, SyntaxKind, SyntaxNode, Type};
use rowan::NodeOrToken;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct MethodNullabilityInfo {
    pub name: String,
    pub return_nullability: Nullability,
    pub return_is_primitive: bool,
    pub param_nullability: Vec<(String, Nullability)>,
    pub explicit_nonnull_params: HashSet<String>,
}

pub fn collect_class_method_returns(
    root: &SyntaxNode,
) -> HashMap<String, HashMap<String, Nullability>> {
    let mut classes: HashMap<String, HashMap<String, Nullability>> = HashMap::new();

    for class_node in root
        .descendants()
        .filter(|node| node.kind().is_type_declaration())
    {
        let class_name =
            qualified_type_name(&class_node).unwrap_or_else(|| "<anonymous>".to_string());
        let mut methods = HashMap::new();

        for method_node in class_node
            .children()
            .filter(|n| n.kind() == SyntaxKind::METHOD_DECL)
        {
            if let Some(info) = extract_method_info(&method_node) {
                methods.insert(info.name, info.return_nullability);
            }
        }

        classes.insert(class_name, methods);
    }

    classes
}

pub fn extract_method_info(method_node: &SyntaxNode) -> Option<MethodNullabilityInfo> {
    let name = extract_method_name(method_node)?;
    let (return_nullability, return_is_primitive) = extract_return_nullability(method_node);
    let param_nullability = extract_parameter_nullability(method_node);
    let explicit_nonnull_params = extract_explicit_nonnull_parameters(method_node);

    Some(MethodNullabilityInfo {
        name,
        return_nullability,
        return_is_primitive,
        param_nullability,
        explicit_nonnull_params,
    })
}

pub fn find_enclosing_class_name(method_node: &SyntaxNode) -> Option<String> {
    method_node
        .ancestors()
        .skip(1)
        .find(|node| node.kind().is_type_declaration())
        .and_then(|owner| qualified_type_name(&owner))
}

/// Return a source-qualified owner such as `Outer.Inner` for a named type.
///
/// Package qualification is intentionally omitted: intra-file summaries use
/// this key, while cross-file owner resolution adds the source package/import.
pub fn qualified_type_name(type_node: &SyntaxNode) -> Option<String> {
    if !type_node.kind().is_type_declaration() {
        return None;
    }
    let mut names = type_node
        .ancestors()
        .filter(|node| node.kind().is_type_declaration())
        .map(|node| extract_decl_name(&node))
        .collect::<Option<Vec<_>>>()?;
    names.reverse();
    Some(names.join("."))
}

pub fn collect_local_variables(method_body: &SyntaxNode) -> Vec<String> {
    let mut locals = Vec::new();
    for node in method_body.descendants() {
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            locals.extend(extract_local_var_names(&node));
        }
    }
    locals
}

fn extract_return_nullability(method_node: &SyntaxNode) -> (Nullability, bool) {
    let return_is_primitive = MethodDecl::cast(method_node.clone())
        .and_then(|decl| decl.return_type())
        .is_some_and(|ty| matches!(ty, Type::Primitive(_)))
        // Java's CST stores return-type array brackets as a direct sibling of
        // the primitive base type. `int[]` is a nullable reference, not a
        // primitive value.
        && !method_node
            .children()
            .any(|child| child.kind() == SyntaxKind::ARRAY_TYPE);
    if return_is_primitive {
        return (Nullability::NonNull, true);
    }

    let annotations = collect_method_annotations(method_node);
    if has_nonnull_annotation(&annotations) {
        (Nullability::NonNull, false)
    } else if has_nullable_annotation(&annotations) {
        (Nullability::Nullable, false)
    } else {
        (Nullability::Unknown, false)
    }
}

pub fn extract_parameter_nullability(method_node: &SyntaxNode) -> Vec<(String, Nullability)> {
    let mut params = Vec::new();
    let Some(parameter_list) = method_node
        .children()
        .find(|node| node.kind() == SyntaxKind::PARAM_LIST)
    else {
        return params;
    };

    for param_node in parameter_list
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::PARAM)
    {
        let name = match extract_param_name(&param_node) {
            Some(name) => name,
            None => continue,
        };
        let annotations = collect_param_annotations(&param_node);
        let nullability = if is_primitive_value_parameter(&param_node) {
            Nullability::NonNull
        } else if has_nonnull_annotation(&annotations) {
            Nullability::NonNull
        } else if has_nullable_annotation(&annotations) {
            Nullability::Nullable
        } else {
            // Use a quiet strict assumption inside source method bodies. This
            // is deliberately not an exported call-site contract: only an
            // explicit non-null annotation may reject a nullable argument or
            // make a defensive parameter null check redundant.
            Nullability::NonNull
        };
        params.push((name, nullability));
    }

    params
}

fn is_primitive_value_parameter(parameter: &SyntaxNode) -> bool {
    parameter
        .descendants()
        .any(|node| node.kind() == SyntaxKind::PRIMITIVE_TYPE)
        && !parameter
            .descendants()
            .any(|node| node.kind() == SyntaxKind::ARRAY_TYPE)
        && !parameter.descendants_with_tokens().any(
            |element| matches!(element, NodeOrToken::Token(token) if token.kind() == SyntaxKind::ELLIPSIS),
        )
}

pub fn extract_explicit_nonnull_parameters(method_node: &SyntaxNode) -> HashSet<String> {
    let Some(parameter_list) = method_node
        .children()
        .find(|node| node.kind() == SyntaxKind::PARAM_LIST)
    else {
        return HashSet::new();
    };

    parameter_list
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::PARAM)
        .filter_map(|parameter| {
            let name = extract_param_name(&parameter)?;
            has_nonnull_annotation(&collect_param_annotations(&parameter)).then_some(name)
        })
        .collect()
}

fn collect_method_annotations(method_node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(prev) = method_node.prev_sibling() {
        if prev.kind() == SyntaxKind::MODIFIER_LIST {
            collect_annotations(&prev, &mut names, true);
        }
    }
    collect_annotations(method_node, &mut names, true);
    names
}

pub(crate) fn attached_modifier_list(declaration: &SyntaxNode) -> Option<SyntaxNode> {
    declaration
        .prev_sibling()
        .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
}

pub(crate) fn has_attached_modifier(declaration: &SyntaxNode, modifier: SyntaxKind) -> bool {
    attached_modifier_list(declaration).is_some_and(|modifiers| {
        modifiers
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .any(|token| token.kind() == modifier)
    })
}

fn collect_param_annotations(param_node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    for node in param_node.descendants() {
        if node.kind() == SyntaxKind::ANNOTATION {
            if let Some(name) = annotation_name(&node) {
                names.push(name);
            }
        }
    }
    names
}

fn collect_annotations(node: &SyntaxNode, output: &mut Vec<String>, skip_params: bool) {
    match node.kind() {
        SyntaxKind::BLOCK_STMT => return,
        SyntaxKind::PARAM if skip_params => return,
        SyntaxKind::ANNOTATION => {
            if let Some(name) = annotation_name(node) {
                output.push(name);
            }
        }
        _ => {}
    }

    for child in node.children() {
        collect_annotations(&child, output, skip_params);
    }
}

fn extract_decl_name(node: &SyntaxNode) -> Option<String> {
    for elem in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = elem {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_method_name(node: &SyntaxNode) -> Option<String> {
    let mut saw_return_type = false;
    let mut fallback: Option<String> = None;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Node(child)
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) =>
            {
                saw_return_type = true;
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::VOID_KW => {
                saw_return_type = true;
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                fallback = Some(token.text().to_string());
                if saw_return_type {
                    return fallback;
                }
            }
            _ => {}
        }
    }

    fallback
}

fn extract_param_name(node: &SyntaxNode) -> Option<String> {
    let mut last_ident: Option<String> = None;
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                last_ident = Some(token.text().to_string());
            }
            _ => {}
        }
    }
    last_ident
}

fn extract_local_var_names(node: &SyntaxNode) -> Vec<String> {
    let mut found_type = false;
    let mut names = Vec::new();
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Node(child)
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE
                        | SyntaxKind::PRIMITIVE_TYPE
                        | SyntaxKind::VAR_TYPE
                        | SyntaxKind::ARRAY_TYPE
                ) =>
            {
                found_type = true;
            }
            NodeOrToken::Token(token) if token.kind().is_identifier() && found_type => {
                names.push(token.text().to_string());
            }
            _ => {}
        }
    }
    names
}

/// Extract field annotations from a class declaration.
///
/// Returns a map of field_name -> list of annotation simple names.
/// Used to support `@Autowired`, `@Inject`, and Lombok `@NonNull` on fields.
///
/// # Example
///
/// ```java
/// class Service {
///     @Autowired
///     private Repository repo;
///
///     @Inject
///     @Named("userRepo")
///     private Repository userRepo;
/// }
/// ```
///
/// Returns: {"repo" => ["Autowired"], "userRepo" => ["Inject", "Named"]}
pub fn collect_field_annotations(class_node: &SyntaxNode) -> HashMap<String, Vec<String>> {
    let mut field_annotations = HashMap::new();

    for field_node in class_node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FIELD_DECL)
        .filter(|field| {
            field
                .ancestors()
                .skip(1)
                .find(|ancestor| ancestor.kind().is_type_declaration())
                .is_some_and(|owner| owner == *class_node)
        })
    {
        let field_name = extract_field_name(&field_node);
        let annotations = collect_annotations_from_node(&field_node);

        if let Some(name) = field_name {
            field_annotations.insert(name, annotations);
        }
    }

    field_annotations
}

/// Extract the field name from a field declaration node.
///
/// # Example
///
/// For `private Repository repo;`, returns `Some("repo")`.
fn extract_field_name(field_node: &SyntaxNode) -> Option<String> {
    let mut found_type = false;

    for elem in field_node.children_with_tokens() {
        match elem {
            NodeOrToken::Node(child)
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) =>
            {
                found_type = true;
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT && found_type => {
                return Some(token.text().to_string());
            }
            _ => {}
        }
    }

    None
}

/// Collect annotation simple names from a node (field or method).
///
/// Extracts annotations from the MODIFIER_LIST sibling and the node itself.
///
/// # Example
///
/// For:
/// ```java
/// @Autowired
/// @Qualifier("primary")
/// private Repository repo;
/// ```
///
/// Returns: `vec!["Autowired", "Qualifier"]`
fn collect_annotations_from_node(node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();

    // Check previous sibling for MODIFIER_LIST
    if let Some(prev) = node.prev_sibling() {
        if prev.kind() == SyntaxKind::MODIFIER_LIST {
            collect_annotations(&prev, &mut names, false);
        }
    }

    // Also check the node itself
    collect_annotations(node, &mut names, false);

    names
}
