// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lambda parameter type inference
//!
//! This module provides type inference for lambda parameters based on the functional
//! interface type expected by the enclosing method call.
//!
//! Supports:
//! - Stream API methods (map, filter, forEach, flatMap)
//! - Optional methods (ifPresent, orElse, orElseGet)
//! - Variable declarations with functional interface types
//! - **Custom methods** - looks up method signatures from WorkspaceIndex

use crate::completion::functional_interface::{
    detect_functional_interface_from_index_with_type_arguments, get_functional_interface_info,
};
use crate::navigation::argument_inference::infer_argument_types;
use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::call_resolution::resolve_call;
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::type_parameter_mapping::apply_type_parameter_substitutions;
use hyperion_semantics::symbol::{ClassType, PrimitiveType, Type};
use hyperion_semantics::type_parser::parse_type_string;
use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::{
    java_parameter_type_from_declaration, DeclarationNamespace, SymbolTarget,
};
use rowan::{NodeOrToken, TextSize};
use std::{collections::HashMap, sync::Arc};
use tracing::debug;

/// Context in which a lambda expression appears
#[derive(Debug)]
enum LambdaContext {
    /// Lambda in a method call: list.forEach(x -> ...)
    MethodCall(SyntaxNode),
    /// Lambda in a variable declaration: Function<String, Integer> fn = s -> ...
    VariableDeclaration(SyntaxNode),
}

enum IndexedTypeResolution {
    Missing,
    Found(String),
}

/// Try to infer the type of a lambda parameter by finding the enclosing lambda
/// and the method call it's passed to.
///
/// This is a convenience wrapper that doesn't use workspace lookup.
/// For custom method support, use `infer_lambda_parameter_type_with_workspace`.
pub fn infer_lambda_parameter_type(
    root: &SyntaxNode,
    param_name: &str,
    offset: u32,
) -> Option<Type> {
    infer_lambda_parameter_type_with_workspace(root, param_name, offset, None)
}

/// Try to infer the type of a lambda parameter with optional workspace lookup.
///
/// When `workspace_index` is provided, this function can resolve lambda parameter
/// types whose declarations are already written canonically. Simple names need
/// a file import/package context; production callers should use
/// [`infer_lambda_parameter_type_with_workspace_and_file`].
///
/// # Example
/// ```java
/// // With workspace lookup, we can infer that `user` is of type User:
/// processUser(user -> user.getName())
/// // Because processUser(Consumer<User>) is found in the workspace
/// ```
pub fn infer_lambda_parameter_type_with_workspace(
    root: &SyntaxNode,
    param_name: &str,
    offset: u32,
    workspace_index: Option<Arc<WorkspaceIndex>>,
) -> Option<Type> {
    infer_lambda_parameter_type_impl(root, param_name, offset, workspace_index, None)
}

/// Infer a lambda parameter type with the file context needed for import-aware,
/// segment-by-segment receiver inference.
pub fn infer_lambda_parameter_type_with_workspace_and_file(
    root: &SyntaxNode,
    param_name: &str,
    offset: u32,
    workspace_index: Option<Arc<WorkspaceIndex>>,
    file_id: FileId,
) -> Option<Type> {
    infer_lambda_parameter_type_impl(root, param_name, offset, workspace_index, Some(file_id))
}

/// Resolve the type of a receiver expression that is rooted at an implicitly-typed
/// lambda parameter, e.g. `entry.getValue()` where `entry` comes from
/// `.map(entry -> ...)`.
///
/// # Why this exists
///
/// Lambda parameters without an explicit type annotation (`entry ->` rather than
/// `(Map.Entry<K, V> entry) ->`) are never given a type by `ResolverBuilder`: there
/// is no type node to extract, and nothing else fills in the Single Abstract Method
/// (SAM) parameter type during scope construction. Every consumer of the resulting
/// `SymbolTable` (in particular `TypeInferenceEngine::infer_expression`, which the
/// completion generator uses for chained/method-call receivers) therefore sees such
/// parameters as `Type::Unknown`, and member-access completion on any expression
/// rooted at one of them returns zero candidates -- regardless of how well the rest
/// of the chain would otherwise resolve.
///
/// This function fills that gap by reusing the same SAM-based inference this module
/// already performs for hover (see [`infer_lambda_parameter_type_with_workspace_and_file`]),
/// then threads the resulting binding through [`infer_chained_receiver_type_with_bindings`]
/// to resolve the *rest* of the receiver chain (further method calls/field access
/// after the parameter) using the same import- and generic-substitution-aware logic
/// used elsewhere in this module.
///
/// Returns `None` when `receiver`'s base identifier does not name a lambda parameter
/// enclosing it (callers should fall back to their normal inference in that case), or
/// when the parameter's type or the rest of the chain can't be determined.
/// Cheaply check whether a receiver expression's base (leftmost) identifier is
/// bound by an enclosing lambda expression's parameter list.
///
/// This is a pure syntax-tree walk -- no symbol table, workspace index, or
/// Salsa query involved -- so it's safe to call speculatively as a fast-path
/// gate before attempting any real type inference. Callers that see `true`
/// here know that `ResolverBuilder`/`TypeInferenceEngine`-based inference on
/// this receiver is guaranteed to see `Type::Unknown` for the base variable
/// (see [`infer_receiver_type_via_lambda_binding`]'s doc comment for why),
/// and can skip straight to that function instead of paying for those
/// whole-file/semantic attempts first.
pub fn receiver_is_lambda_param_rooted(receiver: &SyntaxNode) -> bool {
    let Some(base_var) = find_base_variable(receiver) else {
        return false;
    };
    let mut current = receiver.parent();
    while let Some(node) = current {
        if node.kind() == SyntaxKind::LAMBDA_EXPR
            && extract_lambda_parameters(&node)
                .iter()
                .any(|p| p == &base_var)
        {
            return true;
        }
        current = node.parent();
    }
    false
}

/// Cheaply prove that an identifier token is bound by an enclosing lambda's
/// explicit parameter list.
///
/// Hover uses this before workspace navigation so an
/// unrelated indexed value with the same spelling cannot preempt SAM inference.
pub fn identifier_is_lambda_parameter_reference(
    token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
) -> bool {
    if !token.kind().is_identifier() {
        return false;
    }
    if !token
        .parent()
        .is_some_and(|parent| matches!(parent.kind(), SyntaxKind::PARAM | SyntaxKind::NAME_EXPR))
    {
        return false;
    }
    let name = token.text();
    token.parent_ancestors().any(|node| {
        node.kind() == SyntaxKind::LAMBDA_EXPR
            && extract_lambda_parameters(&node)
                .iter()
                .any(|parameter| parameter == name)
    })
}

pub fn infer_receiver_type_via_lambda_binding(
    receiver: &SyntaxNode,
    root: &SyntaxNode,
    workspace_index: &Arc<WorkspaceIndex>,
    file_id: FileId,
) -> Option<Type> {
    // Only take over when the receiver's base identifier genuinely names an
    // enclosing lambda parameter -- otherwise this isn't our problem to solve
    // and the caller's existing inference (which handles ordinary
    // locals/fields/parameters correctly) should run instead.
    if !receiver_is_lambda_param_rooted(receiver) {
        return None;
    }
    let base_var = find_base_variable(receiver)?;
    let base_offset = u32::from(receiver.text_range().start());

    let param_type = infer_lambda_parameter_type_with_workspace_and_file(
        root,
        &base_var,
        base_offset,
        Some(Arc::clone(workspace_index)),
        file_id,
    )?;
    debug!(
        "infer_receiver_type_via_lambda_binding: bound lambda parameter '{}' to {:?}",
        base_var, param_type
    );

    let mut bindings = HashMap::new();
    bindings.insert(base_var, param_type);
    infer_chained_receiver_type_with_bindings(receiver, root, workspace_index, file_id, &bindings)
}

fn infer_lambda_parameter_type_impl(
    root: &SyntaxNode,
    param_name: &str,
    offset: u32,
    workspace_index: Option<Arc<WorkspaceIndex>>,
    file_id: Option<FileId>,
) -> Option<Type> {
    // Find the token at the offset
    let token = root
        .token_at_offset(TextSize::from(offset))
        .right_biased()?;

    // Walk up the tree to find all enclosing LAMBDA_EXPR nodes
    // For nested lambdas like: prefix -> msg -> prefix.toUpperCase()
    // We need to find the lambda that actually contains the parameter
    let mut current = token.parent();
    let mut lambda_stack: Vec<SyntaxNode> = Vec::new();

    while let Some(node) = current {
        if node.kind() == SyntaxKind::LAMBDA_EXPR {
            lambda_stack.push(node.clone());
        }
        current = node.parent();
    }

    if lambda_stack.is_empty() {
        return None;
    }

    debug!(
        "lambda_inference: Found {} enclosing lambda(s)",
        lambda_stack.len()
    );

    // Find which lambda contains our target parameter
    // lambda_stack[0] is innermost, lambda_stack[n-1] is outermost
    let mut target_lambda = None;
    let mut param_index = 0;

    for lambda in &lambda_stack {
        let lambda_params = extract_lambda_parameters(lambda);
        debug!(
            "lambda_inference: Checking lambda with params: {:?}",
            lambda_params
        );

        if let Some(idx) = lambda_params.iter().position(|p| p == param_name) {
            target_lambda = Some(lambda.clone());
            param_index = idx;
            debug!(
                "lambda_inference: Found parameter '{}' at index {} in lambda",
                param_name, param_index
            );
            break;
        }
    }

    let lambda = target_lambda?;
    debug!(
        "lambda_inference: Target parameter '{}' is at index {}",
        param_name, param_index
    );

    // Now try two approaches:
    // 1. Lambda in method call: METHOD_CALL_EXPR -> ARG_LIST -> LAMBDA_EXPR
    // 2. Lambda in variable declaration: LOCAL_VAR_DECL -> LAMBDA_EXPR
    let mut current = lambda.parent();
    let mut found_context = None;

    while let Some(node) = current {
        match node.kind() {
            SyntaxKind::ARG_LIST => {
                // Check if the lambda is a direct child of this ARG_LIST
                // (not nested deeper inside)
                let is_direct_child = node
                    .children()
                    .any(|child| child.text_range() == lambda.text_range());

                if is_direct_child {
                    // Found an ARG_LIST containing the lambda
                    if let Some(call) = node.parent() {
                        // Skip if METHOD_CALL_EXPR has same text as ARG_LIST (it's the lambda body)
                        if call.kind() == SyntaxKind::METHOD_CALL_EXPR && call.text() == node.text()
                        {
                            current = call.parent();
                            continue;
                        }
                        found_context = Some(LambdaContext::MethodCall(call));
                    }
                    break;
                }
            }
            SyntaxKind::LOCAL_VAR_DECL => {
                // An argument lambda encounters its ARG_LIST before this
                // declaration. Reaching the declaration directly means the
                // lambda is the initializer; canonical SAM detection below
                // decides whether the declared target is functional.
                found_context = Some(LambdaContext::VariableDeclaration(node));
                break;
            }
            SyntaxKind::EXPR_STMT | SyntaxKind::RETURN_STMT => {
                // Expression/return statement containing lambda - treat as method call
                found_context = Some(LambdaContext::MethodCall(node));
                break;
            }
            _ => {}
        }

        current = node.parent();
    }

    let context = found_context?;
    debug!("lambda_inference: Lambda context: {:?}", context);

    // Infer parameter type based on context
    let param_type = match context {
        LambdaContext::MethodCall(call) => {
            debug!("lambda_inference: Method call node: {:?}", call.kind());
            debug!("lambda_inference: Method call text: {}", call.text());

            // The METHOD_CALL_EXPR structure depends on the syntax:
            // For method chains like names.stream().map(...), the structure is complex
            // Let's examine the statement/declaration to understand the chain

            // Check if we already have a statement/decl node, otherwise search for one
            let decl_or_stmt = if matches!(
                call.kind(),
                SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::EXPR_STMT | SyntaxKind::RETURN_STMT
            ) {
                Some(call.clone())
            } else {
                // Find statement/decl ancestor
                let mut current_ancestor = call.parent();
                let mut found = None;
                while let Some(node) = current_ancestor {
                    if matches!(
                        node.kind(),
                        SyntaxKind::LOCAL_VAR_DECL
                            | SyntaxKind::EXPR_STMT
                            | SyntaxKind::RETURN_STMT
                    ) {
                        found = Some(node);
                        break;
                    }
                    current_ancestor = node.parent();
                }
                found
            };

            if let Some(decl) = decl_or_stmt {
                debug!("lambda_inference: Found statement/decl, analyzing structure");

                // Find which method call contains our lambda.
                // Use the tightest (innermost) METHOD_CALL_EXPR whose ARG_LIST
                // directly contains the lambda — this is the actual call site.
                let lambda_range = lambda.text_range();
                let mut method_call_node = None;

                for node in decl.descendants() {
                    if node.kind() == SyntaxKind::METHOD_CALL_EXPR
                        && node.text_range().contains_range(lambda_range)
                    {
                        // Prefer the tightest (smallest) containing METHOD_CALL_EXPR
                        if method_call_node.as_ref().is_none_or(|prev: &SyntaxNode| {
                            node.text_range().len() < prev.text_range().len()
                        }) {
                            method_call_node = Some(node);
                        }
                    }
                }

                // Extract method name from the METHOD_CALL_EXPR's direct children.
                // For chained calls like `getData().ifPresent(lambda)`, the IDENT
                // `ifPresent` is a direct child token of the outer METHOD_CALL_EXPR.
                let mut method_name = None;
                if let Some(ref mc_node) = method_call_node {
                    // The method name is the last IDENT token that appears before the
                    // ARG_LIST in the METHOD_CALL_EXPR's direct children.
                    for child in mc_node.children_with_tokens() {
                        match child {
                            NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                                method_name = Some(t.text().to_string());
                            }
                            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                                break;
                            }
                            _ => {}
                        }
                    }
                    // Static/qualified calls like Collectors.toMap(...) can store the
                    // method name inside a FIELD_ACCESS_EXPR child instead of a direct token.
                    if method_name.is_none() {
                        if let Some((fallback_name, _)) = extract_method_call_parts(mc_node) {
                            method_name = Some(fallback_name);
                        }
                    }
                    debug!(
                        "lambda_inference: Found method containing lambda: {:?}",
                        method_name
                    );
                }

                if let Some(mname) = method_name {
                    // Find the receiver for the method containing our lambda.
                    // For chained calls like `getData().ifPresent(lambda)`, the receiver
                    // is a direct child node of the METHOD_CALL_EXPR (e.g., an inner
                    // METHOD_CALL_EXPR, FIELD_ACCESS_EXPR, or NAME_EXPR).
                    let receiver_expr = if let Some(ref mc_node) = method_call_node {
                        // First try: look at direct children of the METHOD_CALL_EXPR
                        // The receiver is the first expression child node
                        mc_node
                            .children()
                            .find(|child| {
                                matches!(
                                    child.kind(),
                                    SyntaxKind::METHOD_CALL_EXPR
                                        | SyntaxKind::FIELD_ACCESS_EXPR
                                        | SyntaxKind::NAME_EXPR
                                )
                            })
                            .or_else(|| {
                                // Fallback: search via the declaration
                                find_receiver_before_method_call(&decl, mc_node)
                            })
                    } else {
                        None
                    };
                    let receiver_expr = receiver_expr?;

                    let class_type = infer_from_method_context(
                        &mname,
                        Some(receiver_expr),
                        param_index,
                        root,
                        workspace_index.as_ref(),
                        file_id,
                        method_call_node.as_ref(),
                    )?;
                    debug!(
                        "lambda_inference: Inferred parameter type: {} (package: {:?})",
                        class_type.name, class_type.package
                    );
                    return Some(Type::Class(class_type));
                }
            }

            // Fallback: try original extraction
            let (method_name, receiver_expr) = extract_method_call_parts(&call)?;
            debug!("lambda_inference: Method name: {}", method_name);
            if let Some(ref recv) = receiver_expr {
                debug!("lambda_inference: Receiver expr: {}", recv.text());
            } else {
                debug!("lambda_inference: No receiver expr");
            }

            infer_from_method_context(
                &method_name,
                receiver_expr,
                param_index,
                root,
                workspace_index.as_ref(),
                file_id,
                Some(&call),
            )?
        }
        LambdaContext::VariableDeclaration(decl) => {
            infer_from_variable_declaration(&decl, param_index, workspace_index.as_deref())?
        }
    };

    debug!(
        "lambda_inference: Inferred parameter type: {} (package: {:?})",
        param_type.name, param_type.package
    );

    Some(Type::Class(param_type))
}

/// Extract lambda parameters, handling both styles:
/// - Single param: s -> ...
/// - Multiple params: (x, y) -> ...
/// - Typed params: (String s) -> ...
fn extract_lambda_parameters(lambda: &SyntaxNode) -> Vec<String> {
    let mut params = Vec::new();
    let mut after_arrow = false;

    for child in lambda.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::ARROW => {
                after_arrow = true;
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT && !after_arrow => {
                params.push(t.text().to_string());
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::PARAM && !after_arrow => {
                // Handle typed parameters: (String s) -> ...
                for token in n.children_with_tokens() {
                    if let NodeOrToken::Token(t) = token {
                        if t.kind() == SyntaxKind::IDENT && !is_type_name(t.text()) {
                            params.push(t.text().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    params
}

/// Check if an identifier is likely a type name (starts with uppercase)
fn is_type_name(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_uppercase)
}

/// Extract method name and receiver expression from a method call
fn extract_method_call_parts(call: &SyntaxNode) -> Option<(String, Option<SyntaxNode>)> {
    let mut method_name = None;
    let mut receiver_expr = None;

    // Find receiver expression (field access or name)
    for child in call.children() {
        match child.kind() {
            SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::METHOD_CALL_EXPR => {
                receiver_expr = Some(child);
            }
            _ => {}
        }
    }

    // Find method name identifier
    for child in call.children_with_tokens() {
        if let NodeOrToken::Token(t) = child {
            if t.kind() == SyntaxKind::IDENT {
                method_name = Some(t.text().to_string());
            }
        }
    }

    // Qualified/static calls can encode method name in FIELD_ACCESS_EXPR:
    // e.g. Collectors.toMap(...)
    if method_name.is_none() {
        if let Some(field_access) = receiver_expr
            .as_ref()
            .filter(|n| n.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
        {
            if let Some((name, nested_receiver)) =
                extract_method_call_parts_from_field_access(field_access)
            {
                method_name = Some(name);
                if nested_receiver.is_some() {
                    receiver_expr = nested_receiver;
                }
            }
        }
    }

    method_name.map(|name| (name, receiver_expr))
}

/// Extract method name and receiver from a FIELD_ACCESS_EXPR that contains a METHOD_CALL_EXPR
/// Example: names.stream().map(s -> ...)
///          FIELD_ACCESS_EXPR contains: receiver (names.stream()) + method name (map)
fn extract_method_call_parts_from_field_access(
    field_access: &SyntaxNode,
) -> Option<(String, Option<SyntaxNode>)> {
    let mut method_name = None;
    let mut receiver_expr = None;

    // FIELD_ACCESS_EXPR structure:
    // - Receiver expression (FIELD_ACCESS_EXPR or NAME_EXPR)
    // - DOT token
    // - Method name identifier
    // - METHOD_CALL_EXPR (contains ARG_LIST)

    for child in field_access.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                // This is the method name (rightmost identifier)
                method_name = Some(t.text().to_string());
            }
            NodeOrToken::Node(n)
                if matches!(
                    n.kind(),
                    SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::NAME_EXPR
                ) =>
            {
                // This is the receiver expression
                receiver_expr = Some(n);
            }
            _ => {}
        }
    }

    method_name.map(|name| (name, receiver_expr))
}

/// Infer a receiver expression one call segment at a time. This is deliberately
/// based on the canonical call resolver so lambda hover/navigation sees the same
/// overload and generic substitutions as ordinary expression inference.
fn infer_chained_receiver_type(
    receiver: &SyntaxNode,
    root: &SyntaxNode,
    workspace: &Arc<WorkspaceIndex>,
    file_id: FileId,
) -> Option<Type> {
    infer_chained_receiver_type_with_bindings(receiver, root, workspace, file_id, &HashMap::new())
}

fn infer_chained_receiver_type_with_bindings(
    receiver: &SyntaxNode,
    root: &SyntaxNode,
    workspace: &Arc<WorkspaceIndex>,
    file_id: FileId,
    bindings: &HashMap<String, Type>,
) -> Option<Type> {
    match receiver.kind() {
        SyntaxKind::NAME_EXPR => {
            let name = receiver.text().to_string();
            let name = name.trim();
            if let Some(bound) = bindings.get(name) {
                return Some(bound.clone());
            }
            let declared = find_variable_type(root, receiver, name);
            let parsed = declared
                .as_deref()
                .and_then(parse_type_string)
                .or_else(|| {
                    resolve_class_fqn_via_imports(workspace, file_id, name)
                        .ok()
                        .and_then(|fqn| parse_type_string(&fqn))
                })?;
            Some(qualify_type_for_file(parsed, workspace, file_id))
        }
        SyntaxKind::METHOD_CALL_EXPR => {
            let (method_name, receiver_expr) = extract_method_call_parts(receiver)?;
            let receiver_expr = receiver_expr?;
            let receiver_type = infer_chained_receiver_type_with_bindings(
                &receiver_expr,
                root,
                workspace,
                file_id,
                bindings,
            )?;
            let owner_fqn = resolve_call_owner(workspace, &receiver_type, &method_name)?;
            let argument_types = receiver
                .children()
                .find(|child| child.kind() == SyntaxKind::ARG_LIST)
                .map(|args| {
                    infer_argument_types(
                        &args,
                        file_id,
                        workspace,
                        root,
                        u32::from(receiver.text_range().start()),
                    )
                    .into_iter()
                    .map(|inferred| inferred.ty)
                    .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let resolved = resolve_call(
                workspace,
                &owner_fqn,
                Some(&receiver_type),
                &method_name,
                &argument_types,
            )?;
            debug!(
                "lambda_inference: Chain segment {}.{} resolved to {}",
                owner_fqn, method_name, resolved.return_type
            );
            let return_type = if method_name == "map" {
                let mapped = infer_map_result_type(
                    receiver,
                    &receiver_type,
                    root,
                    workspace,
                    file_id,
                    bindings,
                );
                mapped
                    .and_then(|mapped| {
                        let declared_return = resolved.declaration.return_type.as_deref()?;
                        let declared_return = parse_type_string(declared_return)?;
                        let lambda_substituted = substitute_unresolved_method_type(
                            declared_return,
                            &mapped,
                            &resolved.declaration.type_parameters,
                        );
                        Some(apply_type_parameter_substitutions(
                            lambda_substituted,
                            &resolved.substitutions,
                        ))
                    })
                    .unwrap_or_else(|| resolved.return_type.clone())
            } else {
                resolved.return_type
            };
            Some(qualify_type_for_file(return_type, workspace, file_id))
        }
        SyntaxKind::PAREN_EXPR => receiver
            .children()
            .find(|child| is_expression_node(child.kind()))
            .and_then(|child| {
                infer_chained_receiver_type_with_bindings(
                    &child, root, workspace, file_id, bindings,
                )
            }),
        SyntaxKind::LITERAL_EXPR => infer_literal_node_type(receiver),
        SyntaxKind::NEW_EXPR => receiver
            .children()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            .and_then(|class_type| parse_type_string(class_type.text().to_string().trim()))
            .map(|ty| qualify_type_for_file(ty, workspace, file_id)),
        _ => None,
    }
}

fn infer_map_result_type(
    call: &SyntaxNode,
    receiver_type: &Type,
    root: &SyntaxNode,
    workspace: &Arc<WorkspaceIndex>,
    file_id: FileId,
    outer_bindings: &HashMap<String, Type>,
) -> Option<Type> {
    let Type::Class(receiver_class) = receiver_type else {
        return None;
    };
    let element_type = receiver_class.type_arguments.first()?.clone();
    let lambda = call
        .children()
        .find(|child| child.kind() == SyntaxKind::ARG_LIST)?
        .children()
        .find(|child| child.kind() == SyntaxKind::LAMBDA_EXPR)?;
    let parameter = extract_lambda_parameters(&lambda).into_iter().next()?;
    let body = lambda
        .children()
        .filter(|child| is_expression_node(child.kind()))
        .last()?;
    let mut bindings = outer_bindings.clone();
    bindings.insert(parameter, element_type);
    infer_chained_receiver_type_with_bindings(&body, root, workspace, file_id, &bindings)
        .map(box_type_for_generic_argument)
}

fn substitute_unresolved_method_type(
    ty: Type,
    replacement: &Type,
    declared_type_parameters: &[hyperion_workspace::TypeParameterInfo],
) -> Type {
    match ty {
        Type::Class(mut class_type) => {
            if class_type.package.is_none()
                && class_type.type_arguments.is_empty()
                && declared_type_parameters
                    .iter()
                    .any(|parameter| parameter.name.as_ref() == class_type.name)
            {
                return replacement.clone();
            }
            class_type.type_arguments = class_type
                .type_arguments
                .into_iter()
                .map(|argument| {
                    substitute_unresolved_method_type(
                        argument,
                        replacement,
                        declared_type_parameters,
                    )
                })
                .collect();
            Type::Class(class_type)
        }
        Type::Array {
            element_type,
            dimensions,
        } => Type::Array {
            element_type: Box::new(substitute_unresolved_method_type(
                *element_type,
                replacement,
                declared_type_parameters,
            )),
            dimensions,
        },
        other => other,
    }
}

fn infer_literal_node_type(node: &SyntaxNode) -> Option<Type> {
    node.children_with_tokens().find_map(|child| {
        let token = child.as_token()?;
        Some(match token.kind() {
            SyntaxKind::INT_LITERAL => Type::Primitive(PrimitiveType::Int),
            SyntaxKind::LONG_LITERAL => Type::Primitive(PrimitiveType::Long),
            SyntaxKind::FLOAT_LITERAL => Type::Primitive(PrimitiveType::Float),
            SyntaxKind::DOUBLE_LITERAL => Type::Primitive(PrimitiveType::Double),
            SyntaxKind::CHAR_LITERAL => Type::Primitive(PrimitiveType::Char),
            SyntaxKind::BOOLEAN_LITERAL => Type::Primitive(PrimitiveType::Boolean),
            SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => {
                Type::Class(ClassType::string())
            }
            SyntaxKind::NULL_LITERAL => Type::Null,
            _ => return None,
        })
    })
}

fn box_type_for_generic_argument(ty: Type) -> Type {
    let wrapper = match ty {
        Type::Primitive(PrimitiveType::Int) => "Integer",
        Type::Primitive(PrimitiveType::Long) => "Long",
        Type::Primitive(PrimitiveType::Short) => "Short",
        Type::Primitive(PrimitiveType::Byte) => "Byte",
        Type::Primitive(PrimitiveType::Float) => "Float",
        Type::Primitive(PrimitiveType::Double) => "Double",
        Type::Primitive(PrimitiveType::Char) => "Character",
        Type::Primitive(PrimitiveType::Boolean) => "Boolean",
        other => return other,
    };
    Type::Class(ClassType::new(
        wrapper.to_string(),
        Some("java.lang".to_string()),
    ))
}

fn resolve_call_owner(
    workspace: &WorkspaceIndex,
    receiver_type: &Type,
    method_name: &str,
) -> Option<String> {
    let Type::Class(class_type) = receiver_type else {
        return None;
    };
    let direct = class_type.qualified_name();
    if !workspace
        .find_effective_methods_by_name(&direct, method_name)
        .is_empty()
    {
        return Some(direct.into_owned());
    }
    None
}

fn qualify_type_for_file(ty: Type, workspace: &WorkspaceIndex, file_id: FileId) -> Type {
    match ty {
        Type::Class(mut class_type) => {
            class_type.type_arguments = class_type
                .type_arguments
                .into_iter()
                .map(|argument| qualify_type_for_file(argument, workspace, file_id))
                .collect();
            let written = class_type.qualified_name();
            let direct_is_canonical = workspace
                .find_symbol_definition(&SymbolTarget::new(
                    written.as_ref(),
                    DeclarationNamespace::Type,
                ))
                .is_some();
            if !direct_is_canonical {
                if let hyperion_workspace::TypeNameResolution::Exact(fqn) =
                    workspace.resolve_type_name(file_id, &[], written.as_ref())
                {
                    if let Some(Type::Class(mut qualified)) = parse_type_string(&fqn) {
                        qualified.type_arguments = class_type.type_arguments;
                        qualified.is_diamond = class_type.is_diamond;
                        return Type::Class(qualified);
                    }
                }
            }
            Type::Class(class_type)
        }
        Type::Array {
            element_type,
            dimensions,
        } => Type::Array {
            element_type: Box::new(qualify_type_for_file(*element_type, workspace, file_id)),
            dimensions,
        },
        other => other,
    }
}

const fn is_expression_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
            | SyntaxKind::METHOD_REF_EXPR
    )
}

fn receiver_element_type(
    method_name: &str,
    receiver_expr: Option<&SyntaxNode>,
    root: &SyntaxNode,
    workspace: &Arc<WorkspaceIndex>,
    file_id: FileId,
) -> Option<ClassType> {
    let receiver_type = infer_chained_receiver_type(receiver_expr?, root, workspace, file_id)?;
    let Type::Class(receiver_class) = receiver_type else {
        return None;
    };

    let supports_element_lambda = match receiver_class.name.as_str() {
        "Stream" => matches!(
            method_name,
            "map"
                | "filter"
                | "forEach"
                | "flatMap"
                | "peek"
                | "anyMatch"
                | "allMatch"
                | "noneMatch"
                | "takeWhile"
                | "dropWhile"
                | "sorted"
                | "reduce"
        ),
        "Optional" => matches!(method_name, "map" | "ifPresent"),
        "List" | "Set" | "Collection" | "Iterable" => method_name == "forEach",
        _ => false,
    };
    if !supports_element_lambda {
        return None;
    }

    match receiver_class.type_arguments.first()? {
        Type::Class(element) => Some(element.clone()),
        Type::Wildcard {
            upper_bound: Some(bound),
            ..
        } => match bound.as_ref() {
            Type::Class(element) => Some(element.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Infer parameter type based on method name and context
///
/// For known Stream/Optional methods, uses hardcoded inference.
/// For unknown methods, attempts workspace lookup if available.
fn infer_from_method_context(
    method_name: &str,
    receiver_expr: Option<SyntaxNode>,
    param_index: usize,
    root: &SyntaxNode,
    workspace_index: Option<&Arc<WorkspaceIndex>>,
    file_id: Option<FileId>,
    method_call_node: Option<&SyntaxNode>,
) -> Option<ClassType> {
    if let (Some(workspace), Some(file_id)) = (workspace_index, file_id) {
        if let Some(element_type) = receiver_element_type(
            method_name,
            receiver_expr.as_ref(),
            root,
            workspace,
            file_id,
        ) {
            return Some(element_type);
        }
    }

    // For methods that exist on both Stream and Optional (like map),
    // we need to check the receiver type
    if method_name == "map" {
        // Check if the receiver is Optional or Stream
        if let Some(ref recv) = receiver_expr {
            let base_var = find_base_variable(recv)?;
            if let Some(var_type) = find_variable_type(root, recv, &base_var) {
                if var_type == "Optional" {
                    debug!("lambda_inference: Receiver is Optional, using optional_element_type inference");
                    let type_name = infer_optional_element_type(receiver_expr.as_ref(), root)?;
                    return Some(create_class_type(&type_name));
                }
            }
        }
        // Default to stream inference for map
        let type_name = infer_stream_element_type(receiver_expr.as_ref(), root)?;
        return Some(create_class_type(&type_name));
    }

    match method_name {
        // Stream API methods where lambda parameter type is the stream element type.
        // This includes predicate/consumer-style terminals/intermediates and reductions
        // where tests currently assert element-typed parameters.
        "filter" | "forEach" | "flatMap" | "peek" | "anyMatch" | "allMatch" | "noneMatch"
        | "takeWhile" | "dropWhile" | "sorted" | "reduce" => {
            let type_name = infer_stream_element_type(receiver_expr.as_ref(), root)?;
            Some(create_class_type(&type_name))
        }

        // Optional methods
        "ifPresent" | "orElse" | "orElseGet" => {
            let type_name = infer_optional_element_type(receiver_expr.as_ref(), root)?;
            Some(create_class_type(&type_name))
        }

        // Collector factory methods used inside stream.collect(...)
        // e.g. names.stream().collect(Collectors.toMap(s -> ..., s -> ...))
        // The lambda parameter type should match the stream element type.
        "toMap" | "groupingBy" | "partitioningBy" => {
            if let Some(mc_node) = method_call_node {
                if let Some(ty) = infer_from_enclosing_collect_call(mc_node, root) {
                    return Some(ty);
                }
            }
            None
        }

        _ => {
            // Try workspace lookup for custom methods
            if let Some(workspace) = workspace_index {
                debug!(
                    "lambda_inference: Unknown method '{}', trying workspace lookup",
                    method_name
                );
                return infer_from_workspace_method(
                    method_name,
                    receiver_expr.as_ref(),
                    param_index,
                    root,
                    workspace,
                    file_id,
                );
            }

            debug!(
                "lambda_inference: Unknown method '{}', no workspace available for lookup",
                method_name
            );
            None
        }
    }
}

fn infer_from_enclosing_collect_call(
    method_call_node: &SyntaxNode,
    root: &SyntaxNode,
) -> Option<ClassType> {
    // Walk ancestors and find collect(...) call that encloses this collector factory call.
    for ancestor in method_call_node.ancestors() {
        if ancestor.kind() != SyntaxKind::METHOD_CALL_EXPR {
            continue;
        }
        if ancestor.text_range() == method_call_node.text_range() {
            continue;
        }

        if let Some((name, receiver_expr)) = extract_method_call_parts(&ancestor) {
            if name == "collect" {
                let type_name = infer_stream_element_type(receiver_expr.as_ref(), root)?;
                return Some(create_class_type(&type_name));
            }
        }
    }

    None
}

/// Try to infer lambda parameter type by looking up method signature in workspace
fn infer_from_workspace_method(
    method_name: &str,
    receiver_expr: Option<&SyntaxNode>,
    param_index: usize,
    root: &SyntaxNode,
    workspace: &WorkspaceIndex,
    file_id: Option<FileId>,
) -> Option<ClassType> {
    // Step 1: Determine the receiver type (class containing the method)
    let receiver_type = if let Some(recv) = receiver_expr {
        // Try to find the base variable's type
        let base_var = find_base_variable(recv)?;
        find_variable_type(root, recv, &base_var)?
    } else {
        // No receiver - might be a local method in current class
        // Try to find enclosing class name
        crate::navigation_util::find_first_class_name(root)?
    };

    debug!(
        "lambda_inference: Looking up method '{}' on receiver type '{}'",
        method_name, receiver_type
    );

    let receiver_base = receiver_type
        .split_once('<')
        .map_or(receiver_type.as_str(), |(base, _)| base)
        .trim();
    let receiver_owner = if workspace.get_symbol(receiver_base).is_some() {
        Some(receiver_base.to_string())
    } else if let Some(file_id) = file_id {
        match workspace.resolve_type_name(file_id, &[], receiver_base) {
            hyperion_workspace::TypeNameResolution::Exact(fqn) => Some(fqn),
            hyperion_workspace::TypeNameResolution::Missing
            | hyperion_workspace::TypeNameResolution::Ambiguous(_) => None,
        }
    } else {
        None
    }?;
    let receiver_arguments = parse_type_string(&receiver_type)
        .and_then(|ty| match ty {
            Type::Class(class_type) => Some(
                class_type
                    .type_arguments
                    .into_iter()
                    .map(|argument| argument.to_string())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();

    let methods = workspace.find_projected_effective_methods_by_name(
        &receiver_owner,
        &receiver_arguments,
        method_name,
    );
    if methods.iter().any(|method| method.projection.is_none()) {
        return None;
    }
    let mut inferred = methods
        .into_iter()
        .filter_map(|method| method.projection)
        .filter_map(|signature| signature.parameters.get(param_index).cloned())
        .filter_map(|parameter| {
            extract_lambda_type_from_param(Some(workspace), &parameter.type_name, param_index)
        })
        .collect::<Vec<_>>();
    inferred.sort_by_key(ToString::to_string);
    inferred.dedup();
    let [inferred] = inferred.as_slice() else {
        return None;
    };
    Some(inferred.clone())
}

/// Extract lambda parameter type from a method parameter type string
///
/// If the parameter is a functional interface like `Consumer<User>` or `Function<String, Integer>`,
/// extracts the lambda parameter type (e.g., `User` or `String`).
pub(super) fn extract_lambda_type_from_param(
    workspace: Option<&WorkspaceIndex>,
    param_type_str: &str,
    lambda_param_index: usize,
) -> Option<ClassType> {
    let param_type_str = java_parameter_type_from_declaration(param_type_str);
    // Parse the parameter type - check for generics
    let (base_type, type_args) = if let Some(generic_start) = param_type_str.find('<') {
        let base = &param_type_str[..generic_start];
        let generic_end = param_type_str.rfind('>')?;
        if generic_end <= generic_start {
            return None;
        }
        let args_str = &param_type_str[generic_start + 1..generic_end];

        // Parse comma-separated type arguments, respecting nested generics
        let mut args = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for ch in args_str.chars() {
            match ch {
                '<' => {
                    depth += 1;
                    current.push(ch);
                }
                '>' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        args.push(trimmed);
                    }
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            args.push(trimmed);
        }

        (base.to_string(), args)
    } else {
        (param_type_str.to_string(), vec![])
    };

    debug!(
        "lambda_inference: Parsed param type - base: {}, args: {:?}",
        base_type, type_args
    );

    // Build FQN for known functional interfaces
    let fqn = if base_type.contains('.') {
        base_type
    } else {
        // Try common packages
        match base_type.as_str() {
            "Consumer" | "BiConsumer" | "Function" | "BiFunction" | "Predicate" | "Supplier"
            | "UnaryOperator" | "BinaryOperator" => format!("java.util.function.{base_type}"),
            "Runnable" => "java.lang.Runnable".to_string(),
            "Callable" => "java.util.concurrent.Callable".to_string(),
            "Comparator" => "java.util.Comparator".to_string(),
            _ => base_type.clone(),
        }
    };

    let info = match workspace {
        Some(workspace) => match resolve_indexed_type(workspace, &fqn) {
            IndexedTypeResolution::Found(indexed_fqn) => {
                detect_functional_interface_from_index_with_type_arguments(
                    workspace,
                    &indexed_fqn,
                    &type_args,
                )
            }
            // The compact platform table is only for a canonical type absent
            // from the index. Context-free simple names never reach indexed
            // declaration inference.
            IndexedTypeResolution::Missing => get_functional_interface_info(&fqn, &type_args),
        },
        None => get_functional_interface_info(&fqn, &type_args),
    };
    if let Some(info) = info {
        // Validate that parameter_types is not empty
        if info.parameter_types.is_empty() {
            debug!(
                "lambda_inference: functional interface {} has empty parameter_types, \
                     type args: {:?}",
                fqn, type_args
            );
            return None;
        }

        // Get the lambda parameter type at the specified index
        if let Some(param_type) = info.parameter_types.get(lambda_param_index) {
            debug!(
                "lambda_inference: Functional interface {} -> param type: {}",
                fqn, param_type
            );
            return Some(create_class_type(param_type));
        }
        debug!(
            "lambda_inference: parameter index {} out of bounds for {} \
                 (has {} parameters), type args: {:?}",
            lambda_param_index,
            fqn,
            info.parameter_types.len(),
            type_args
        );
    }

    debug!(
        "lambda_inference:'{}' is not a recognized functional interface",
        fqn
    );
    None
}

fn resolve_indexed_type(workspace: &WorkspaceIndex, written: &str) -> IndexedTypeResolution {
    if workspace
        .find_symbol_definition(&SymbolTarget::new(written, DeclarationNamespace::Type))
        .is_some()
    {
        return IndexedTypeResolution::Found(written.to_string());
    }
    IndexedTypeResolution::Missing
}

/// Infer lambda parameter type from a variable declaration
/// Example: Function<String, Integer> fn = s -> s.length();
///          The first type parameter (String) is the lambda parameter type
fn infer_from_variable_declaration(
    decl: &SyntaxNode,
    param_index: usize,
    workspace: Option<&WorkspaceIndex>,
) -> Option<ClassType> {
    debug!("lambda_inference: Analyzing variable declaration");

    // Extract the type from the declaration
    // Look for CLASS_TYPE node which contains the functional interface type
    for child in decl.children() {
        if child.kind() == SyntaxKind::CLASS_TYPE {
            let type_text = child.text().to_string();
            debug!("lambda_inference: Found type declaration: {}", type_text);

            if let Some(parameter_type) =
                extract_lambda_type_from_param(workspace, &type_text, param_index)
            {
                return Some(parameter_type);
            }
        }
    }

    debug!("lambda_inference: Could not infer type from variable declaration");
    None
}

/// Create a ClassType with appropriate package, parsing generics from the type string
fn create_class_type(type_name: &str) -> ClassType {
    // Strip generic type arguments from the name
    let (base_name, type_args) = if let Some(angle_pos) = type_name.find('<') {
        let base = &type_name[..angle_pos];
        // Extract type arguments: "Map<String, Integer>" → ["String", "Integer"]
        let args_str = &type_name[angle_pos + 1..type_name.len() - 1]; // Remove < and >
        let args: Vec<Type> = parse_type_arguments(args_str);
        (base, args)
    } else {
        (type_name, vec![])
    };

    let (base_name, explicit_package) = base_name
        .rsplit_once('.')
        .map_or((base_name, None), |(package, name)| {
            (name, Some(package.to_string()))
        });
    let package = explicit_package.or_else(|| match base_name {
        "String" | "Integer" | "Long" | "Double" | "Float" | "Boolean" | "Character" | "Byte"
        | "Short" | "Object" => Some("java.lang".to_string()),
        // java.util types
        "Map" | "HashMap" | "TreeMap" | "LinkedHashMap" | "List" | "ArrayList" | "LinkedList"
        | "Set" | "HashSet" | "TreeSet" | "Collection" | "Iterator" | "Optional" | "Queue"
        | "Deque" | "ArrayDeque" => Some("java.util".to_string()),
        _ => None,
    });

    ClassType {
        name: base_name.to_string(),
        package,
        type_arguments: type_args,
        is_diamond: false,
    }
}

/// Parse comma-separated type arguments, handling nested generics
fn parse_type_arguments(args_str: &str) -> Vec<Type> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in args_str.chars() {
        match ch {
            '<' => {
                depth += 1;
                current.push(ch);
            }
            '>' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    args.push(Type::Class(create_class_type(trimmed)));
                }
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // Don't forget the last argument
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        args.push(Type::Class(create_class_type(trimmed)));
    }

    args
}

/// Infer the element type of a stream from the receiver expression
fn infer_stream_element_type(
    receiver_expr: Option<&SyntaxNode>,
    root: &SyntaxNode,
) -> Option<String> {
    let receiver = receiver_expr?;

    // Try to find the base variable name
    // For chains like: list.stream().map(...), we want "list"
    // For direct: names.stream().map(...), we want "names"
    let base_var = find_base_variable(receiver)?;
    debug!("lambda_inference: Base variable: {}", base_var);

    // Try to find the variable declaration in the tree to get its type
    if let Some(var_type) = find_variable_type(root, receiver, &base_var) {
        debug!(
            "lambda_inference: Found variable type from declaration: {}",
            var_type
        );

        // Extract generic type from List<User>, Optional<String>, etc.
        if let Some(generic_type) = extract_generic_type(&var_type) {
            debug!("lambda_inference: Extracted generic type: {}", generic_type);
            return Some(generic_type);
        }

        // For array types: User[] -> User
        if let Some(element_type) = var_type.strip_suffix("[]") {
            debug!("lambda_inference: Array type, element is: {}", element_type);
            return Some(element_type.to_string());
        }

        // If we have a generic collection type (List, Set, etc.) but no generic parameter
        // (due to parser limitation), try to infer from variable name
        if matches!(
            var_type.as_str(),
            "List" | "Set" | "Collection" | "ArrayList" | "LinkedList" | "HashSet"
        ) {
            debug!("lambda_inference: Collection type without generics, inferring from name");
            if let Some(guessed) = guess_type_from_name(&base_var) {
                return Some(guessed);
            }
        }

        return Some(var_type);
    }

    // Fallback: try to guess from variable name
    guess_type_from_name(&base_var)
}

/// Infer element type from Optional receiver
fn infer_optional_element_type(
    receiver_expr: Option<&SyntaxNode>,
    root: &SyntaxNode,
) -> Option<String> {
    let receiver = receiver_expr?;

    let base_var = find_base_variable(receiver)?;
    debug!("lambda_inference: Optional base variable: {}", base_var);

    if let Some(var_type) = find_variable_type(root, receiver, &base_var) {
        // Extract type from Optional<Type>
        if let Some(generic_type) = extract_generic_type(&var_type) {
            return Some(generic_type);
        }

        // If we have Optional without generic parameter (due to parser limitation),
        // try to infer from variable name
        if var_type == "Optional" {
            debug!("lambda_inference: Optional type without generics, inferring from name");
            if let Some(guessed) = guess_type_from_name(&base_var) {
                return Some(guessed);
            }
        }
    }

    // Fallback: guess from variable name
    guess_type_from_name(&base_var)
}

/// Find the base variable name from a receiver expression
/// Examples: "names" from names.stream(), "user" from user.getName()
fn find_base_variable(receiver: &SyntaxNode) -> Option<String> {
    // For FIELD_ACCESS_EXPR, get the leftmost identifier
    // For NAME_EXPR, get the identifier directly

    let mut identifiers = Vec::new();
    for child in receiver.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = child {
            if t.kind() == SyntaxKind::IDENT {
                identifiers.push(t.text().to_string());
            }
        }
    }

    // Return the first (leftmost) identifier
    identifiers.into_iter().next()
}

/// Find the declared type of a variable or method return type at a concrete use site.
///
/// The semantic resolver supplies the lexical scope and point-of-declaration rules,
/// preventing locals from other methods, exited blocks, or later declarations from
/// leaking into lambda inference.
fn find_variable_type(root: &SyntaxNode, context: &SyntaxNode, var_name: &str) -> Option<String> {
    use crate::navigation::ast_queries;
    use hyperion_semantics::resolver::ResolverBuilder;

    let offset = u32::from(context.text_range().start());
    let resolver = ResolverBuilder::new(FileId::new(0)).build_from_syntax(root);
    if let Some(scope_id) = resolver.find_scope_at_offset(offset) {
        if let Some(resolved) = resolver.resolve_in_scope_at_offset(var_name, scope_id, offset) {
            if !matches!(resolved.ty, Type::Unknown) {
                // Preserve the syntax helper's longstanding source-like output.
                // Import-aware callers qualify the result against the real file.
                let var_type = without_type_packages(&resolved.ty).to_string();
                debug!(
                    "lambda_inference: Resolved scoped type '{}' for variable '{}'",
                    var_type, var_name
                );
                return Some(var_type);
            }
        }
    }

    // A leftmost identifier may name an unqualified method call such as
    // `getData().stream()`. Resolve that declaration only when the use site
    // actually contains such a call, and only after variable lookup.
    let names_method_call = context
        .descendants()
        .any(|node| called_method_name(&node).as_deref() == Some(var_name));
    if names_method_call {
        if let Some(return_type) = ast_queries::extract_method_return_type(root, var_name) {
            if !return_type.is_empty() {
                debug!(
                    "lambda_inference: Found method return type for '{}': {}",
                    var_name, return_type
                );
                return Some(return_type);
            }
        }
    }

    None
}

/// Name of the method a `METHOD_CALL_EXPR` invokes, for both qualified and
/// unqualified call syntax.
///
/// `extract_method_call_parts` only sees the callee name when the parser emits
/// it as a direct `IDENT` token child, which is what happens for qualified calls
/// (`x.foo()`, `Collectors.toMap(...)`). An *unqualified* call such as
/// `getData()` is instead parsed as
/// `METHOD_CALL_EXPR[NAME_EXPR[IDENT "getData"], ARG_LIST]` — no direct `IDENT`
/// token and no `FIELD_ACCESS_EXPR` — so that helper returns `None` and callers
/// wrongly conclude the node is not a call at all.
fn called_method_name(node: &SyntaxNode) -> Option<String> {
    if node.kind() != SyntaxKind::METHOD_CALL_EXPR {
        return None;
    }
    if let Some((method_name, _)) = extract_method_call_parts(node) {
        return Some(method_name);
    }
    // Unqualified call: the callee identifier is wrapped in a NAME_EXPR child.
    let name_expr = node
        .children()
        .find(|child| child.kind() == SyntaxKind::NAME_EXPR)?;
    let name = name_expr.text().to_string().trim().to_string();
    (!name.is_empty()).then_some(name)
}

fn without_type_packages(ty: &Type) -> Type {
    match ty {
        Type::Class(class_type) => {
            let mut unqualified = class_type.clone();
            unqualified.package = None;
            unqualified.type_arguments = class_type
                .type_arguments
                .iter()
                .map(without_type_packages)
                .collect();
            Type::Class(unqualified)
        }
        Type::Array {
            element_type,
            dimensions,
        } => Type::Array {
            element_type: Box::new(without_type_packages(element_type)),
            dimensions: *dimensions,
        },
        Type::TypeVariable { name, bounds } => Type::TypeVariable {
            name: name.clone(),
            bounds: bounds.iter().map(without_type_packages).collect(),
        },
        Type::Wildcard {
            upper_bound,
            lower_bound,
        } => Type::Wildcard {
            upper_bound: upper_bound
                .as_deref()
                .map(without_type_packages)
                .map(Box::new),
            lower_bound: lower_bound
                .as_deref()
                .map(without_type_packages)
                .map(Box::new),
        },
        Type::Intersection { types } => Type::Intersection {
            types: types.iter().map(without_type_packages).collect(),
        },
        Type::Union { types } => Type::Union {
            types: types.iter().map(without_type_packages).collect(),
        },
        other => other.clone(),
    }
}

/// Extract the generic type parameter from a type string
/// Examples: List<User> -> User, Optional<String> -> String
fn extract_generic_type(type_str: &str) -> Option<String> {
    let start = type_str.find('<')?;
    let end = type_str.rfind('>')?;

    if end > start {
        let inner = &type_str[start + 1..end];
        // Handle nested generics by taking the outermost type
        // List<List<String>> -> List<String>
        let type_argument = inner.trim();
        if let Some(upper_bound) = type_argument.strip_prefix("? extends ") {
            // Reading/streaming from Collection<? extends T> produces a
            // captured value whose usable upper bound is T.
            Some(
                upper_bound
                    .split('&')
                    .next()
                    .unwrap_or(upper_bound)
                    .trim()
                    .to_string(),
            )
        } else if type_argument == "?" || type_argument.starts_with("? super ") {
            // An unbounded or lower-bounded wildcard has Object as its usable
            // read type. Returning the lower bound here would be unsound.
            Some("Object".to_string())
        } else {
            Some(type_argument.to_string())
        }
    } else {
        None
    }
}

/// Guess type from variable name as a fallback
fn guess_type_from_name(name: &str) -> Option<String> {
    // String collection naming conventions
    if name == "names" || name.contains("String") {
        return Some("String".to_string());
    }

    // Handle "Opt" suffix (e.g., userOpt -> User)
    if let Some(base) = name.strip_suffix("Opt") {
        let mut chars = base.chars();
        let capitalized = match chars.next() {
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            None => base.to_string(),
        };
        debug!(
            "lambda_inference: Guessed type from 'Opt' suffix: {} -> {}",
            name, capitalized
        );
        return Some(capitalized);
    }

    // Try plural to singular conversion
    let singular = if let Some(stripped) = name.strip_suffix("ies") {
        stripped.to_string() + "y"
    } else if let Some(stripped) = name.strip_suffix("es") {
        stripped.to_string()
    } else if let Some(stripped) = name.strip_suffix("s") {
        stripped.to_string()
    } else {
        name.to_string()
    };

    // Capitalize first letter
    let mut chars = singular.chars();
    let capitalized = match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => singular,
    };

    debug!(
        "lambda_inference: Guessed type from variable name '{}': {}",
        name, capitalized
    );
    Some(capitalized)
}

/// Find the receiver expression before a method call in a chain
/// For: names.stream().map(...), when at map(), find names.stream()
/// This actually needs to trace back to the base variable name
fn find_receiver_before_method_call(
    local_var_decl: &SyntaxNode,
    target_call: &SyntaxNode,
) -> Option<SyntaxNode> {
    // Collect all significant nodes (NAME_EXPR and METHOD_CALL_EXPR) in order
    let mut nodes = Vec::new();
    for desc in local_var_decl.descendants() {
        match desc.kind() {
            SyntaxKind::NAME_EXPR | SyntaxKind::METHOD_CALL_EXPR => {
                nodes.push(desc);
            }
            _ => {}
        }
    }

    debug!("lambda_inference: Nodes in chain:");
    for (i, node) in nodes.iter().enumerate() {
        debug!(
            "lambda_inference: {}: {:?} = {}",
            i,
            node.kind(),
            node.text()
        );
    }

    // Find our target METHOD_CALL_EXPR in the list
    let target_pos = nodes
        .iter()
        .position(|n| n.text_range() == target_call.text_range())?;

    debug!(
        "lambda_inference: Target call is at position {}",
        target_pos
    );

    // The receiver is the node immediately before our target
    // But if it's a METHOD_CALL_EXPR, we need to go back further to find the base variable
    if target_pos > 0 {
        let prev_node = &nodes[target_pos - 1];
        debug!(
            "lambda_inference: Previous node: {:?} = {}",
            prev_node.kind(),
            prev_node.text()
        );

        // If previous is a METHOD_CALL_EXPR, find the first NAME_EXPR
        if prev_node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            // Find the first NAME_EXPR in the chain
            for (i, node) in nodes.iter().enumerate().take(target_pos) {
                if node.kind() == SyntaxKind::NAME_EXPR {
                    debug!("lambda_inference: Using base NAME_EXPR at position {}", i);
                    return Some(node.clone());
                }
            }
        }

        Some(prev_node.clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn parse_java(src: &str) -> SyntaxNode {
        let tokens = Lexer::new(src).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "Parse errors: {errors:?}");
        SyntaxNode::new_root(green)
    }

    /// Find the byte offset of a parameter name within the source.
    fn find_param_offset(src: &str, param_name: &str) -> u32 {
        // Find the parameter in a lambda context (before ->)
        // Search for the pattern "param_name ->" or "param_name," to find it in lambda position
        src.find(&format!("{param_name} ->"))
            .or_else(|| {
                src.find(&format!("{param_name},")).or_else(|| {
                    src.find(&format!("{param_name})"))
                        .or_else(|| src.find(param_name))
                })
            })
            .unwrap_or_else(|| panic!("parameter '{param_name}' not found in source"))
            as u32
    }

    /// Regression: an *unqualified* call receiver (`getData().ifPresent(...)`)
    /// must resolve through the enclosing method's declared return type.
    /// `getData()` parses as METHOD_CALL_EXPR[NAME_EXPR[IDENT], ARG_LIST] with no
    /// direct IDENT token, which previously made the method-return-type lookup in
    /// `find_variable_type` conclude the use site named no call at all; the lambda
    /// parameter then fell back to the name-guessing heuristic and became `GetData`.
    #[test]
    fn unqualified_call_receiver_resolves_optional_element_type() {
        let src = r#"
            package com.example;
            import java.util.Optional;

            class Test {
                Optional<MyMap<String, Integer>> getData() {
                    return Optional.empty();
                }

                void test() {
                    getData().ifPresent(map -> {
                        Integer value = map.get("key");
                    });
                }
            }
        "#;
        let root = parse_java(src);
        let off = src.find("map ->").expect("lambda param") as u32;
        let result = infer_lambda_parameter_type(&root, "map", off);
        let Some(Type::Class(class_type)) = result else {
            panic!("expected Type::Class(MyMap<String, Integer>), got {result:?}");
        };
        assert_eq!(class_type.name, "MyMap", "got {class_type:?}");
        assert_eq!(
            class_type
                .type_arguments
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["java.lang.String", "java.lang.Integer"],
            "generic arguments must survive the Optional unwrap"
        );
    }

    #[test]
    fn called_method_name_handles_qualified_and_unqualified_calls() {
        let src = "class C { void t(java.util.List<String> xs) { xs.stream(); f(); } }";
        let root = parse_java(src);
        let names: Vec<_> = root
            .descendants()
            .filter_map(|node| called_method_name(&node))
            .collect();
        assert!(names.contains(&"stream".to_string()), "got {names:?}");
        assert!(names.contains(&"f".to_string()), "got {names:?}");
    }

    #[test]
    fn test_lambda_param_scoped_to_enclosing_method() {
        // Two methods each declare a parameter named `data` of different Optional element
        // types. The lambda parameter in `second` must resolve using `second`'s `data`
        // (Optional<Coord>), not the first `data` declaration found in the file.
        let src = "class C { \
            void first(java.util.Optional<String> data) { data.map(v -> v.length()); } \
            void second(java.util.Optional<Coord> data) { data.map(w -> w.x()); } \
        }";
        let root = parse_java(src);
        let off = src.find("w ->").expect("lambda param not found") as u32;
        let result = infer_lambda_parameter_type(&root, "w", off);
        if let Some(Type::Class(ct)) = result {
            assert_eq!(
                ct.name, "Coord",
                "lambda param in `second` must resolve to Coord, got {ct:?}"
            );
        } else {
            panic!("Expected Type::Class(Coord), got {result:?}");
        }
    }

    #[test]
    fn test_multi_call_receiver_type_threads_generics() {
        let src = r"
            package test;
            class Coord {}
            interface Bucket<T> {}
            interface Items<T> { Flow<T> stream(); }
            interface Flow<T> { Maybe<T> max(Object comparator); }
            interface Maybe<T> {}
            class Board { Items<Bucket<Coord>> getChunks() { return null; } }
            class Main {
                void test(Board board, Object comparator) {
                    board.getChunks().stream().max(comparator);
                }
            }
        ";
        let root = parse_java(src);
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let path = temp.path().join("test/Main.java");
        workspace
            .index_file_with_source(&path, src)
            .expect("source should index");
        let file_id = workspace.get_file_id(&path).expect("indexed file id");

        let calls: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .filter_map(|node| extract_method_call_parts(&node).map(|(name, _)| (name, node)))
            .collect();
        for (method_name, expected_type) in
            [("getChunks", "Items"), ("stream", "Flow"), ("max", "Maybe")]
        {
            let (_, call) = calls
                .iter()
                .find(|(name, _)| name == method_name)
                .unwrap_or_else(|| panic!("missing {method_name} call"));
            let inferred = infer_chained_receiver_type(call, &root, &workspace, file_id);
            assert!(
                matches!(inferred, Some(Type::Class(ref class)) if class.name == expected_type),
                "expected {method_name} to return {expected_type}, got {inferred:?}"
            );
        }
    }

    /// Regression: `Map<K,V>.entrySet().stream().map(entry -> ...)` -- the
    /// implicit lambda parameter `entry` must resolve to `Entry<Integer, Address>`
    /// via SAM inference, and further member access on it (`entry.getValue()`) must
    /// resolve through that binding rather than coming back `Unknown`.
    ///
    /// `Map`/`Set`/`Stream`/`Entry` are modeled locally (not `java.util.*`) so the
    /// test stays fast and self-contained, but they share simple names with the
    /// JDK types so they exercise the same `"Stream" => matches!(method_name, "map" | ...)`
    /// hardcoded dispatch in `receiver_element_type` that the real bug hit.
    #[test]
    fn receiver_type_via_lambda_binding_resolves_map_entry_get_value() {
        let src = r"
            package test;

            interface Map<K, V> {
                Set<Entry<K, V>> entrySet();
            }
            interface Set<T> {
                Stream<T> stream();
            }
            interface Stream<T> {
            }
            interface Entry<K, V> {
                K getKey();
                V getValue();
            }
            class Address {
                String getHostString() { return null; }
            }
            class Main {
                void test(Map<Integer, Address> voterAddresses) {
                    voterAddresses
                        .entrySet()
                        .stream()
                        .map(entry -> entry.getValue().getHostString());
                }
            }
        ";
        let root = parse_java(src);
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let path = temp.path().join("test/Main.java");
        workspace
            .index_file_with_source(&path, src)
            .expect("source should index");
        let file_id = workspace.get_file_id(&path).expect("indexed file id");

        let receiver = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::METHOD_CALL_EXPR && n.text() == "entry.getValue()")
            .expect("entry.getValue() node");

        let inferred =
            infer_receiver_type_via_lambda_binding(&receiver, &root, &workspace, file_id);

        assert!(
            matches!(inferred, Some(Type::Class(ref class)) if class.name == "Address"),
            "expected entry.getValue() to resolve to Address, got {inferred:?}"
        );
    }

    #[test]
    fn file_qualification_canonicalizes_dotted_nested_type_before_owner_lookup() {
        let src = r"
            package java.util;
            interface Map<K, V> {
                interface Entry<K, V> {
                    K getKey();
                }
            }
            class IdentityHashMap {
                interface Entry<K, V> {
                    K getKey();
                }
            }
        ";
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("java/util/Map.java");
        workspace
            .index_file_with_source(&path, src)
            .expect("nested source type should index");

        let receiver_type =
            parse_type_string("Map.Entry<String,Integer>").expect("parse dotted nested type");
        let file_id = workspace.get_file_id(&path).expect("indexed file id");
        let receiver_type = qualify_type_for_file(receiver_type, &workspace, file_id);
        assert_eq!(
            resolve_call_owner(&workspace, &receiver_type, "getKey").as_deref(),
            Some("java.util.Map.Entry")
        );
        assert!(
            workspace.find_by_simple_name("Map$Entry").is_empty(),
            "navigation must not depend on the removed binary-name workaround"
        );
    }

    #[test]
    fn call_owner_rejects_an_ambiguous_simple_receiver() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace
            .index_file_with_source(
                &temp.path().join("first/Entry.java"),
                "package first; class Entry { String getKey() { return null; } }",
            )
            .expect("first entry should index");
        workspace
            .index_file_with_source(
                &temp.path().join("second/Entry.java"),
                "package second; class Entry { String getKey() { return null; } }",
            )
            .expect("second entry should index");

        let receiver_type = parse_type_string("Entry<String>").expect("simple receiver type");
        assert_eq!(
            resolve_call_owner(&workspace, &receiver_type, "getKey"),
            None
        );
    }

    #[test]
    fn file_qualification_rejects_unimported_global_collisions() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        for package in ["first", "second"] {
            workspace
                .index_file_with_source(
                    &temp.path().join(format!("{package}/Item.java")),
                    &format!("package {package}; class Item {{}}"),
                )
                .expect("colliding item should index");
        }
        let caller_path = temp.path().join("caller/Main.java");
        workspace
            .index_file_with_source(
                &caller_path,
                "package caller; class Main { void use(Item value) {} }",
            )
            .expect("caller should index");
        let caller_id = workspace.get_file_id(&caller_path).expect("caller file id");
        let unqualified = parse_type_string("Item").expect("unqualified item");

        assert_eq!(
            qualify_type_for_file(unqualified.clone(), &workspace, caller_id),
            unqualified
        );
    }

    #[test]
    fn workspace_lambda_inference_requires_overloads_to_agree() {
        fn infer(parameter_overloads: &str) -> Option<ClassType> {
            let temp = tempfile::tempdir().expect("temp workspace");
            let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
            workspace
                .index_file_with_source(
                    &temp.path().join("p/Consumer.java"),
                    "package p; interface Consumer<T> { void accept(T value); }",
                )
                .expect("consumer should index");
            workspace
                .index_file_with_source(
                    &temp.path().join("p/Function.java"),
                    "package p; interface Function<T, R> { R apply(T value); }",
                )
                .expect("function should index");
            let source = format!(
                "package p; class Service {{ {parameter_overloads} \
                 void use() {{ process(value -> value.toString()); }} }}"
            );
            let path = temp.path().join("p/Service.java");
            workspace
                .index_file_with_source(&path, &source)
                .expect("service should index");
            let file_id = workspace.get_file_id(&path).expect("service file id");
            let root = parse_java(&source);
            infer_from_workspace_method("process", None, 0, &root, &workspace, Some(file_id))
        }

        let agreed = infer("void process(Consumer<String> action) {}")
            .expect("one exact overload should infer its SAM parameter");
        assert_eq!(agreed.to_string(), "java.lang.String");

        assert!(
            infer(concat!(
                "void process(Consumer<String> action) {} ",
                "void process(Function<Integer, String> action) {}"
            ))
            .is_none(),
            "incomparable overload SAM parameter types must remain ambiguous"
        );
    }

    /// A receiver whose base identifier is an ordinary parameter (not a lambda
    /// parameter) must be left alone -- callers rely on `None` here to keep using
    /// their existing (correct) inference for the common case.
    #[test]
    fn receiver_type_via_lambda_binding_ignores_non_lambda_receivers() {
        let src = r"
            package test;
            class Address {
                String getHostString() { return null; }
            }
            class Main {
                void test(Address address) {
                    address.getHostString();
                }
            }
        ";
        let root = parse_java(src);
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let path = temp.path().join("test/Main.java");
        workspace
            .index_file_with_source(&path, src)
            .expect("source should index");
        let file_id = workspace.get_file_id(&path).expect("indexed file id");

        let receiver = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::NAME_EXPR && n.text() == "address")
            .expect("address node");

        let inferred =
            infer_receiver_type_via_lambda_binding(&receiver, &root, &workspace, file_id);
        assert!(
            inferred.is_none(),
            "expected None for a non-lambda-parameter receiver, got {inferred:?}"
        );
    }

    #[test]
    fn variable_type_lookup_respects_lexical_scope_and_declaration_order() {
        let cases = [
            (
                r"
                    class Test {
                        void first() { java.util.List<String> leaked = null; }
                        void second() { leaked.stream(); }
                    }
                ",
                "leaked",
                None,
            ),
            (
                r"
                    class Test {
                        void test() {
                            { java.util.List<String> blockOnly = null; }
                            blockOnly.stream();
                        }
                    }
                ",
                "blockOnly",
                None,
            ),
            (
                r"
                    class Test {
                        void test() {
                            later.stream();
                            java.util.List<String> later = null;
                        }
                    }
                ",
                "later",
                None,
            ),
            (
                r"
                    class Test {
                        java.util.List<String> field;
                        void test() { field.stream(); }
                    }
                ",
                "field",
                Some("List<String>"),
            ),
        ];

        for (source, name, expected) in cases {
            let root = parse_java(source);
            let usage = root
                .descendants()
                .find(|node| {
                    node.kind() == SyntaxKind::NAME_EXPR && node.text().to_string().trim() == name
                })
                .unwrap_or_else(|| panic!("missing name expression for {name}"));
            assert_eq!(
                find_variable_type(&root, &usage, name).as_deref(),
                expected,
                "unexpected lookup result for {name}"
            );
        }
    }

    #[test]
    fn test_extract_lambda_parameters_single() {
        let root = parse_java("class C { void test() { names.forEach(s -> s.length()); } }");
        let lambdas: Vec<_> = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
            .collect();
        assert_eq!(lambdas.len(), 1);
        let params = extract_lambda_parameters(&lambdas[0]);
        assert_eq!(params, vec!["s"]);
    }

    #[test]
    fn test_extract_lambda_parameters_multiple() {
        let root =
            parse_java("class C { void test() { map.forEach((k, v) -> System.out.println(k)); } }");
        let lambdas: Vec<_> = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
            .collect();
        assert_eq!(lambdas.len(), 1);
        let params = extract_lambda_parameters(&lambdas[0]);
        assert_eq!(params, vec!["k", "v"]);
    }

    #[test]
    fn identifier_binding_gate_rejects_same_named_non_lambda_values() {
        let root = parse_java(
            "class C { Object chunk; void test() { values.map(chunk -> { chunk.size(); obj.chunk(); }); } }",
        );
        let chunk_tokens = root
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier() && token.text() == "chunk")
            .collect::<Vec<_>>();

        assert_eq!(chunk_tokens.len(), 4);
        assert!(!identifier_is_lambda_parameter_reference(&chunk_tokens[0]));
        assert!(identifier_is_lambda_parameter_reference(&chunk_tokens[1]));
        assert!(identifier_is_lambda_parameter_reference(&chunk_tokens[2]));
        assert!(!identifier_is_lambda_parameter_reference(&chunk_tokens[3]));
    }

    #[test]
    fn test_infer_stream_foreach_param() {
        let src = "class C { void test() { java.util.List<String> names = null; names.forEach(s -> s.length()); } }";
        let root = parse_java(src);
        let offset = find_param_offset(src, "s");
        let result = infer_lambda_parameter_type(&root, "s", offset);
        assert!(
            result.is_some(),
            "Expected lambda param type to be inferred"
        );
        if let Some(Type::Class(ct)) = result {
            assert_eq!(ct.name, "String");
        } else {
            panic!("Expected Type::Class, got {:?}", result);
        }
    }

    #[test]
    fn test_infer_stream_filter_param() {
        let src =
            "class C { void test() { java.util.List<User> users = null; users.stream().filter(u -> u.isActive()); } }";
        let root = parse_java(src);
        let offset = find_param_offset(src, "u");
        let result = infer_lambda_parameter_type(&root, "u", offset);
        assert!(
            result.is_some(),
            "Expected lambda param type to be inferred"
        );
        if let Some(Type::Class(ct)) = result {
            assert_eq!(ct.name, "User");
        } else {
            panic!("Expected Type::Class, got {:?}", result);
        }
    }

    #[test]
    fn test_infer_stream_map_param() {
        let src = "class C { void test() { java.util.List<String> names = null; names.stream().map(s -> s.toUpperCase()); } }";
        let root = parse_java(src);
        let offset = find_param_offset(src, "s");
        let result = infer_lambda_parameter_type(&root, "s", offset);
        assert!(
            result.is_some(),
            "Expected lambda param type to be inferred"
        );
        if let Some(Type::Class(ct)) = result {
            assert_eq!(ct.name, "String");
        } else {
            panic!("Expected Type::Class, got {:?}", result);
        }
    }

    #[test]
    fn test_infer_stream_param_from_extends_wildcard() {
        let src = "class C { void test() { java.util.Collection<? extends User> users = null; users.stream().map(u -> u.getName()); } }";
        let root = parse_java(src);
        let offset = find_param_offset(src, "u");
        let result = infer_lambda_parameter_type(&root, "u", offset);

        assert!(
            matches!(result, Some(Type::Class(ref ct)) if ct.name == "User"),
            "expected wildcard capture upper bound User, got {result:?}"
        );
    }

    #[test]
    fn test_stream_read_type_from_super_wildcard_is_object() {
        assert_eq!(
            extract_generic_type("Collection<? super User>").as_deref(),
            Some("Object")
        );
    }

    #[test]
    fn test_infer_optional_ifpresent_param() {
        let src = "class C { void test() { java.util.Optional<String> opt = null; opt.ifPresent(s -> System.out.println(s)); } }";
        let root = parse_java(src);
        let offset = find_param_offset(src, "s");
        let result = infer_lambda_parameter_type(&root, "s", offset);
        assert!(
            result.is_some(),
            "Expected lambda param type to be inferred"
        );
        if let Some(Type::Class(ct)) = result {
            assert_eq!(ct.name, "String");
        } else {
            panic!("Expected Type::Class, got {:?}", result);
        }
    }

    #[test]
    fn test_create_class_type_simple() {
        let ct = create_class_type("String");
        assert_eq!(ct.name, "String");
        assert_eq!(ct.package, Some("java.lang".to_string()));
        assert!(ct.type_arguments.is_empty());
    }

    #[test]
    fn test_create_class_type_generic() {
        let ct = create_class_type("Map<String, Integer>");
        assert_eq!(ct.name, "Map");
        assert_eq!(ct.package, Some("java.util".to_string()));
        assert_eq!(ct.type_arguments.len(), 2);
    }

    #[test]
    fn test_create_class_type_unknown() {
        let ct = create_class_type("MyCustomClass");
        assert_eq!(ct.name, "MyCustomClass");
        assert_eq!(ct.package, None);
    }

    #[test]
    fn test_guess_type_from_name_plural() {
        assert_eq!(guess_type_from_name("users"), Some("User".to_string()));
        assert_eq!(guess_type_from_name("records"), Some("Record".to_string()));
        assert_eq!(
            guess_type_from_name("activeUsers"),
            Some("ActiveUser".to_string())
        );
        // "names" is hardcoded to return "String" (common convention)
        assert_eq!(guess_type_from_name("names"), Some("String".to_string()));
        // Generic plural → singular capitalized
        assert_eq!(guess_type_from_name("items"), Some("Item".to_string()));
    }

    #[test]
    fn test_guess_type_from_name_opt_suffix() {
        assert_eq!(guess_type_from_name("userOpt"), Some("User".to_string()));
    }

    #[test]
    fn test_extract_lambda_type_from_param_consumer() {
        let result = extract_lambda_type_from_param(None, "Consumer<User>", 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "User");
    }

    #[test]
    fn test_extract_lambda_type_from_param_function() {
        let result = extract_lambda_type_from_param(None, "Function<String, Integer>", 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "String");
    }

    #[test]
    fn test_extract_lambda_type_from_param_predicate() {
        let result = extract_lambda_type_from_param(None, "Predicate<User>", 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "User");
    }

    #[test]
    fn test_variable_declaration_functional_interface() {
        let src = "class C { void test() { java.util.function.Consumer<String> action = s -> System.out.println(s); } }";
        let root = parse_java(src);
        let offset = find_param_offset(src, "s");
        let result = infer_lambda_parameter_type(&root, "s", offset);
        // This tests the variable declaration path
        assert!(
            result.is_some(),
            "Expected lambda param type from variable declaration"
        );
        if let Some(Type::Class(ct)) = result {
            assert_eq!(ct.name, "String");
        }
    }

    #[test]
    fn map_substitution_uses_declared_method_parameters_only() {
        let parameters = vec![hyperion_workspace::TypeParameterInfo::new("Result", 0)];
        let replacement = Type::Class(ClassType::string());
        let unresolved = Type::Class(ClassType::with_type_args(
            "Stream".to_string(),
            Some("java.util.stream".to_string()),
            vec![Type::Class(ClassType {
                name: "Result".to_string(),
                package: None,
                type_arguments: vec![],
                is_diamond: false,
            })],
        ));

        let substituted = substitute_unresolved_method_type(unresolved, &replacement, &parameters);
        let concrete_s = Type::Class(ClassType {
            name: "S".to_string(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        });
        let preserved =
            substitute_unresolved_method_type(concrete_s.clone(), &replacement, &parameters);

        let Type::Class(stream) = substituted else {
            panic!("expected parameterized Stream return type");
        };
        assert_eq!(stream.type_arguments, [replacement]);
        assert_eq!(preserved, concrete_s);
    }
}
