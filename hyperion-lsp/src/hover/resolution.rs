// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol resolution for hover
//!
//! Functions that resolve identifiers to their declarations

use super::documentation::extract_documentation;
use super::extractors::{
    extract_field_info_from_node, extract_method_signature_from_node,
    extract_parameter_info_from_node, extract_pattern_variable_info_from_node,
    extract_variable_info_from_node,
};
use super::utils::{find_ancestor, find_identifier_in_node};
use crate::navigation::argument_inference::{infer_argument_types, Confidence};
use crate::navigation::call_site_analysis::{count_arguments, find_method_call_for_token};
use crate::navigation::overload_resolver::resolve_overload_with_receiver;
use crate::navigation::parse_method_chain;
use crate::position_utils::offset_to_position;
use crate::type_resolution::types::MemberResolutionStatus;
use crate::type_resolution::ChainResolver;
use hyperion_parser::{syntax::SyntaxKind, SyntaxNode};
use hyperion_workspace::{DeclarationNamespace, ResolvedDefinition, SymbolTarget, WorkspaceIndex};
use rowan::NodeOrToken;
use std::sync::Arc;
use tower_lsp::lsp_types::SymbolKind as LspSymbolKind;
use tracing::{debug, info};

/// Resolve an identifier reference to its declaration
///
/// This function looks up what an identifier refers to by searching:
/// 1. Method/constructor parameters
/// 2. Local variables in the current scope
/// 3. Fields in the enclosing class
pub fn resolve_identifier_reference(
    token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    _syntax: &SyntaxNode,
    source_text: &str,
    file_id: hyperion_semantics::db::FileId,
    offset: u32,
    workspace_index: &Arc<WorkspaceIndex>,
    canonical_callable: Option<&ResolvedDefinition>,
) -> Option<String> {
    let identifier_name = token.text();

    // First, check if we're hovering on the declaration NAME itself (not method calls in the initializer)
    // We should only return None if this is the declared variable/parameter/field name itself
    // Example: In `String name = user.getName()`, only skip if hovering on "name", not "getName"
    if let Some(parent) = token.parent() {
        match parent.kind() {
            SyntaxKind::PARAM | SyntaxKind::FIELD_DECL => {
                // For parameters and fields, skip - these are usually just the name
                return None;
            }
            SyntaxKind::LOCAL_VAR_DECL => {
                // For local variables, need to check if this is the variable name or something in the initializer
                // The variable name is typically the first IDENT in the LOCAL_VAR_DECL
                // Method calls in initializers will be deeper in the tree
                let is_var_name = parent
                    .children_with_tokens()
                    .find_map(|child| match child {
                        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t),
                        _ => None,
                    })
                    .is_some_and(|first_ident| first_ident == *token);

                if is_var_name {
                    return None;
                }
            }
            _ => {}
        }
    }

    // Check if this is a pattern variable declaration (on the definition itself)
    // The IDENT token's parent is TYPE_PATTERN, which is a child of PATTERN_INSTANCEOF_EXPR
    if let Some(parent) = token.parent() {
        debug!(
            "Hover on '{}': parent kind = {:?}",
            identifier_name,
            parent.kind()
        );
        if parent.kind() == SyntaxKind::TYPE_PATTERN {
            debug!("Parent is TYPE_PATTERN, checking grandparent");
            if let Some(grandparent) = parent.parent() {
                debug!("Grandparent kind = {:?}", grandparent.kind());
                if grandparent.kind() == SyntaxKind::PATTERN_INSTANCEOF_EXPR {
                    debug!("Found PATTERN_INSTANCEOF_EXPR, extracting pattern info");
                    if let Some(pattern_info) =
                        extract_pattern_variable_info_from_node(&grandparent, identifier_name)
                    {
                        return Some(format!(
                            "```java\n{pattern_info}\n```\n\n**Pattern variable**"
                        ));
                    }
                    debug!("extract_pattern_variable_info_from_node returned None");
                }
            }
        }
    }

    // PRIORITY 1: Check if this is a method call FIRST (before variables)
    // This prevents "getTcpOptions" from matching "tcpOptions" local variable
    if let Some(parent) = token.parent() {
        // Detect if we're hovering on a method call identifier
        // Method calls can have structure:
        // 1. EXPR_STMT -> NAME_EXPR(ident) + METHOD_CALL_EXPR(args)
        // 2. Direct METHOD_CALL_EXPR context
        // 3. FIELD_ACCESS_EXPR(receiver.method) + METHOD_CALL_EXPR(args) for qualified calls
        // 4. LOCAL_VAR_DECL/EXPR_STMT -> ... + METHOD_CALL_EXPR (method call in initializer/statement)
        let is_method_reference = parent.kind() == SyntaxKind::METHOD_REF_EXPR
            || parent
                .parent()
                .is_some_and(|gp| gp.kind() == SyntaxKind::METHOD_REF_EXPR);
        let parsed_chain = parse_method_chain(token);
        let chain_target_is_method = parsed_chain
            .as_ref()
            .and_then(|(chain, target)| chain.get(*target))
            .map(|segment| segment.is_method_call);

        let mut is_method_call = match parent.kind() {
            SyntaxKind::NAME_EXPR => {
                // Check if NAME_EXPR has METHOD_CALL_EXPR as a sibling
                // This indicates: someMethod() where someMethod is NAME_EXPR and () is METHOD_CALL_EXPR
                parent.parent().is_some_and(|gp| {
                    gp.children()
                        .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
                })
            }
            SyntaxKind::FIELD_ACCESS_EXPR => {
                // Check if FIELD_ACCESS_EXPR is followed by METHOD_CALL_EXPR
                // This indicates: receiver.method() where receiver.method is FIELD_ACCESS_EXPR
                let is_selector = parent
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|candidate| candidate.kind().is_identifier())
                    .last()
                    .is_some_and(|candidate| candidate.text_range() == token.text_range());
                is_selector
                    && parent.parent().is_some_and(|gp| {
                        gp.children()
                            .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
                    })
            }
            // A declaration or statement can contain many identifiers besides the invoked
            // method. Treating every identifier in `field = receiver.call()` as a method makes
            // field hover resolve to unrelated same-named methods elsewhere in the workspace.
            SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::EXPR_STMT => false,
            SyntaxKind::METHOD_CALL_EXPR => true,
            _ => {
                // Check if parent's parent is METHOD_CALL_EXPR
                parent
                    .parent()
                    .is_some_and(|gp| gp.kind() == SyntaxKind::METHOD_CALL_EXPR)
            }
        } || is_method_reference;

        if !is_method_call {
            // Parsing a chain does not make every segment callable: the selected
            // segment can be its receiver (`record` in `record.name()`) or a field.
            // Only the segment carrying an argument list denotes a method call.
            is_method_call = chain_target_is_method == Some(true);
        }

        // Add diagnostic logging to understand hover triggering
        info!(
            "Hover: found IDENT '{}' at {}:{}, parent kind: {:?}, is_method_call: {}",
            identifier_name,
            file_id.0,
            offset,
            parent.kind(),
            is_method_call
        );

        if is_method_call {
            // Check if this is a qualified method call (receiver.method)
            // Search the parent node's descendants for FIELD_ACCESS_EXPR
            let mut is_qualified = is_method_reference;

            // Check if the parent or any of its descendants contain FIELD_ACCESS_EXPR
            if parent
                .descendants()
                .any(|n| n.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
            {
                is_qualified = true;
                info!(
                    "Hover: found FIELD_ACCESS_EXPR in expression tree for '{}'",
                    identifier_name
                );
            }

            // If not found, try alternative: look for DOT token in the parent's descendants
            if !is_qualified
                && parent.descendants_with_tokens().any(|n| {
                    if let NodeOrToken::Token(t) = n {
                        t.kind() == SyntaxKind::DOT
                    } else {
                        false
                    }
                })
            {
                is_qualified = true;
                info!(
                    "Hover: found DOT token in expression tree for '{}'",
                    identifier_name
                );
            }

            info!(
                "Hover: is_qualified = {} for '{}'",
                is_qualified, identifier_name
            );

            // For qualified method calls (like stream.map(...)), resolve
            // the receiver type and look up the method on that specific type.
            if is_qualified {
                info!(
                    "Hover: resolving qualified method call '{}' at {}:{}",
                    identifier_name, file_id.0, offset
                );

                // Parse the method chain to get receiver and target
                if let Some((chain, target_idx)) = parse_method_chain(token) {
                    debug!(
                        "Parsed method chain: {:?}, target_idx={}",
                        chain, target_idx
                    );

                    // Create ChainResolver and resolve the type
                    let resolver = ChainResolver::new(workspace_index.clone());
                    let root_node = _syntax;

                    match resolver
                        .resolve_chain_type(file_id, &chain, target_idx, offset, root_node)
                    {
                        Ok(type_info) => {
                            if let Some(definition) = canonical_callable {
                                let resolved_target = SymbolTarget::new(
                                    type_info.fqn.as_str(),
                                    DeclarationNamespace::for_kind(type_info.kind),
                                );
                                if resolved_target != definition.target {
                                    debug!(
                                        "Hover chain identity '{}' disagrees with canonical '{}'",
                                        resolved_target.fqn, definition.target.fqn
                                    );
                                    return Some(format_workspace_definition_hover(definition));
                                }
                            }
                            info!(
                                "Hover: resolved '{}' to type {}",
                                identifier_name, type_info.fqn
                            );

                            // For method calls, refine overload selection using call-site
                            // argument count. Chain resolution may resolve by name only.
                            let mut display_return_type = type_info.return_type.clone();
                            let mut display_parameters = type_info.parameters.clone();
                            let mut display_container = type_info.container_name.clone();
                            let mut display_documentation = type_info.documentation.clone();
                            let projection_unavailable = type_info.member_resolution
                                == Some(MemberResolutionStatus::ProjectionUnavailable);
                            let mut has_applicable_signature = !projection_unavailable;

                            // Extract simple name - strip off parameter descriptor first.
                            let fqn_without_params =
                                type_info.fqn.split('(').next().unwrap_or(&type_info.fqn);
                            let simple_name = fqn_without_params
                                .rsplit('.')
                                .next()
                                .unwrap_or(fqn_without_params)
                                .to_string();

                            if type_info.kind == LspSymbolKind::METHOD
                                && !projection_unavailable
                                && type_info.member_resolution
                                    != Some(MemberResolutionStatus::Applicable)
                            {
                                if let Some(container) = display_container.as_ref() {
                                    let method_call = find_method_call_for_token(token);
                                    let has_overload_ambiguity = workspace_index
                                        .find_effective_methods_by_name(container, &simple_name)
                                        .len()
                                        > 1;
                                    let argument_types = method_call
                                        .as_ref()
                                        .and_then(|method_call| {
                                            method_call
                                                .children()
                                                .find(|n| n.kind() == SyntaxKind::ARG_LIST)
                                        })
                                        .map(|arg_list| {
                                            infer_argument_types(
                                                &arg_list,
                                                file_id,
                                                workspace_index,
                                                _syntax,
                                                offset,
                                            )
                                        });
                                    let receiver_type = method_call.as_ref().and_then(|call| {
                                        let receiver = call.children().find(|node| {
                                            matches!(
                                                node.kind(),
                                                SyntaxKind::NAME_EXPR
                                                    | SyntaxKind::QUALIFIED_NAME
                                                    | SyntaxKind::METHOD_CALL_EXPR
                                                    | SyntaxKind::FIELD_ACCESS_EXPR
                                            )
                                        })?;
                                        let builder = hyperion_semantics::resolver::ResolverBuilder::with_workspace(
                                            file_id,
                                            Arc::clone(workspace_index),
                                        );
                                        let resolver = builder.build_from_syntax(_syntax);
                                        let engine = hyperion_semantics::TypeInferenceEngine::new(
                                            Arc::clone(workspace_index),
                                            file_id,
                                        );
                                        let inferred = engine.infer_expression(
                                            &receiver,
                                            &resolver,
                                            None,
                                        );
                                        (!matches!(
                                            inferred,
                                            hyperion_semantics::symbol::Type::Unknown
                                        ))
                                        .then_some(inferred)
                                    });
                                    let has_known_argument =
                                        argument_types.as_ref().is_some_and(|types| {
                                            types.iter().any(|inferred| {
                                                inferred.confidence != Confidence::Unknown
                                            })
                                        });

                                    // Even a unique declaration can require receiver-relative
                                    // generic projection for its displayed parameter/return type.
                                    let type_matched =
                                        argument_types.as_ref().and_then(|arg_types| {
                                            resolve_overload_with_receiver(
                                                workspace_index,
                                                container,
                                                receiver_type.as_ref(),
                                                &simple_name,
                                                arg_types,
                                            )
                                        });

                                    if let Some(matched) = type_matched {
                                        display_return_type = (!matches!(
                                            &matched.return_type,
                                            hyperion_semantics::symbol::Type::Unknown
                                        ))
                                        .then(|| matched.return_type.to_string())
                                        .or(display_return_type);
                                        display_parameters
                                            .clone_from(&matched.parameter_declarations);
                                        display_container = matched
                                            .definition
                                            .symbol
                                            .container
                                            .as_ref()
                                            .map(|container| container.to_string())
                                            .or(display_container);
                                        display_documentation = matched
                                            .definition
                                            .symbol
                                            .documentation
                                            .as_ref()
                                            .map(|documentation| documentation.to_string())
                                            .or(display_documentation);
                                    } else if has_overload_ambiguity && has_known_argument {
                                        // Argument inference produced real evidence, but no
                                        // declaration is applicable. Do not overwrite that result
                                        // with an arbitrary same-arity overload from chain lookup.
                                        display_return_type = None;
                                        display_parameters.clear();
                                        has_applicable_signature = false;
                                    } else {
                                        // Fallback: arity-only selection only when type evidence is
                                        // unavailable (or when there is no overload ambiguity).
                                        let arg_count = method_call
                                            .as_ref()
                                            .and_then(|method_call| {
                                                method_call
                                                    .children()
                                                    .find(|n| n.kind() == SyntaxKind::ARG_LIST)
                                            })
                                            .map_or(0, |arg_list| count_arguments(&arg_list));

                                        if let Some(overload) = workspace_index
                                            .find_method_by_args_with_inheritance(
                                                container,
                                                &simple_name,
                                                arg_count,
                                            )
                                        {
                                            display_return_type = overload
                                                .return_type
                                                .as_ref()
                                                .map(|ty| ty.to_string())
                                                .or(display_return_type);
                                            display_parameters = overload
                                                .parameters
                                                .iter()
                                                .map(|param| param.to_string())
                                                .collect();
                                            display_container = overload
                                                .container
                                                .as_ref()
                                                .map(|container| container.to_string())
                                                .or(display_container);
                                            display_documentation = overload
                                                .documentation
                                                .as_ref()
                                                .map(|documentation| documentation.to_string())
                                                .or(display_documentation);
                                        }
                                    }
                                }
                            }

                            // Format hover information with the resolved type
                            let mut markdown = String::new();

                            // Show return type if available (only for methods)
                            if type_info.kind == LspSymbolKind::METHOD {
                                if let Some(ret) = display_return_type.as_ref() {
                                    markdown.push_str(&format!("`{ret}`\n\n"));
                                }
                            }

                            // Show signature based on symbol kind
                            if type_info.kind == LspSymbolKind::METHOD {
                                if projection_unavailable {
                                    markdown.push_str(&format!(
                                        "**{simple_name}**\n\n_Receiver-relative signature unavailable because the indexed generic hierarchy is ambiguous or incomplete._"
                                    ));
                                } else if has_applicable_signature {
                                    let param_list = if display_parameters.is_empty() {
                                        String::new()
                                    } else {
                                        display_parameters.join(", ")
                                    };
                                    markdown.push_str(&format!("**{simple_name}({param_list})**"));
                                } else {
                                    markdown.push_str(&format!(
                                        "**{simple_name}**\n\n_No applicable overload for the inferred argument types._"
                                    ));
                                }
                            } else if type_info.kind == LspSymbolKind::ENUM_MEMBER {
                                markdown.push_str(&format!("**{simple_name}**\n\n_enum constant_"));
                            } else if type_info.kind == LspSymbolKind::FIELD {
                                if let Some(ret) = type_info.return_type.as_ref() {
                                    markdown.push_str(&format!(
                                        "**{simple_name}**: `{ret}`\n\n_field_"
                                    ));
                                } else {
                                    markdown.push_str(&format!("**{simple_name}**\n\n_field_"));
                                }
                            } else {
                                markdown.push_str(&format!("**{simple_name}**"));
                            }

                            // Show container info
                            if let Some(container) = &display_container {
                                markdown.push_str(&format!("\n\n_From:_ `{container}`"));
                            }

                            if let Some(documentation) = display_documentation {
                                markdown.push_str(&format!("\n\n---\n\n{documentation}"));
                            }

                            return Some(markdown);
                        }
                        Err(e) => {
                            info!(
                                "Hover: type resolution failed for '{}': {}",
                                identifier_name, e
                            );
                            debug!("Falling back to workspace search for '{}'", identifier_name);
                            // Fall back to workspace search
                        }
                    }
                } else {
                    debug!(
                        "Failed to parse method chain for qualified call '{}'",
                        identifier_name
                    );
                }
            }

            // For unqualified method calls (like assertThatThrownBy()), try static import resolution first
            if !is_qualified {
                use crate::navigation::import_resolver::{
                    enclosing_callable_shadows_static_import, resolve_static_invocation_via_imports,
                };
                let enclosing_owner = enclosing_type_fqn(token, workspace_index, file_id);
                let argument_types = find_method_call_for_token(token)
                    .and_then(|method_call| {
                        method_call
                            .children()
                            .find(|node| node.kind() == SyntaxKind::ARG_LIST)
                    })
                    .map(|arguments| {
                        infer_argument_types(&arguments, file_id, workspace_index, _syntax, offset)
                            .into_iter()
                            .map(|inferred| inferred.ty)
                            .collect::<Vec<_>>()
                    });
                if let Some(method_location) = argument_types.as_deref().and_then(|arguments| {
                    let member_shadows_import = enclosing_owner.as_deref().is_some_and(|owner| {
                        enclosing_callable_shadows_static_import(
                            workspace_index,
                            file_id,
                            owner,
                            identifier_name,
                            arguments,
                        )
                    });
                    (!member_shadows_import).then(|| {
                        resolve_static_invocation_via_imports(
                            workspace_index.as_ref(),
                            file_id,
                            identifier_name,
                            arguments,
                        )
                    })?
                }) {
                    debug!(
                        "Hover: Resolved unqualified method '{}' via static import to '{}'",
                        identifier_name, method_location.fqn
                    );

                    // Get the method symbol to show detailed info
                    if let Some(method_symbol) =
                        workspace_index.get_symbol_target(&SymbolTarget::new(
                            method_location.fqn.clone(),
                            DeclarationNamespace::Callable,
                        ))
                    {
                        if canonical_callable.is_some_and(|definition| {
                            SymbolTarget::for_symbol(&method_symbol) != definition.target
                        }) {
                            return canonical_callable.map(format_workspace_definition_hover);
                        }
                        let mut markdown = String::new();

                        // Show return type if available
                        if let Some(ret) = &method_symbol.return_type {
                            markdown.push_str(&format!("`{ret}`\n\n"));
                        }

                        // Show method signature with modifiers
                        let simple_name = method_symbol.simple_name.as_ref();
                        let param_list = if method_symbol.parameters.is_empty() {
                            String::new()
                        } else {
                            method_symbol.parameters.join(", ")
                        };
                        let modifiers = if method_symbol.modifiers.is_static {
                            "static "
                        } else {
                            ""
                        };
                        markdown.push_str(&format!("**{modifiers}{simple_name}({param_list})**"));

                        // Show container info
                        if let Some(container) = &method_symbol.container {
                            markdown.push_str(&format!("\n\n_From:_ `{container}`"));
                        }

                        // Add documentation if available
                        if let Some(doc) = &method_symbol.documentation {
                            markdown.push_str(&format!("\n\n{doc}"));
                        }

                        return Some(markdown);
                    }
                }
            }

            // Search for method declaration in the enclosing class
            if let Some(class_node) = find_ancestor(token, SyntaxKind::CLASS_DECL) {
                if let Some(definition) = canonical_callable {
                    return Some(format_workspace_definition_hover(definition));
                }
                if let Some(method_info) =
                    find_method_declaration(&class_node, identifier_name, source_text)
                {
                    return Some(method_info);
                }
            }

            // A parsed field-chain target may use the method-chain resolver for owner/type
            // inference, but it must never fall through to a global same-named method search.
            if chain_target_is_method == Some(false) {
                return None;
            }

            // A method name is not an identity. If receiver-aware chain,
            // static-import, and enclosing-type resolution all fail, a global
            // same-name search can only guess (and historically attributed a
            // call on an unresolved receiver to an unrelated unique method).
            // Leave the call unresolved so the provider can return basic token
            // information without naming a false declaration.
            return canonical_callable.map(format_workspace_definition_hover);
        }
    }

    // Fallback: parameters, local variables, and fields for tokens FileSemantics
    // didn't resolve (e.g. contextual keywords used as identifiers: record, var, yield)
    if let Some(method_node) = find_ancestor(token, SyntaxKind::METHOD_DECL) {
        if let Some(info) = find_parameter_declaration(&method_node, identifier_name) {
            return Some(format!("```java\n{info}\n```\n\n**Parameter**"));
        }
        if let Some((info, kind)) =
            find_local_variable_declaration(&method_node, token, identifier_name)
        {
            return Some(format!("```java\n{info}\n```\n\n**{kind}**"));
        }
    } else if let Some(constructor_node) = find_ancestor(token, SyntaxKind::CONSTRUCTOR_DECL) {
        if let Some(info) = find_parameter_declaration(&constructor_node, identifier_name) {
            return Some(format!("```java\n{info}\n```\n\n**Parameter**"));
        }
        if let Some((info, kind)) =
            find_local_variable_declaration(&constructor_node, token, identifier_name)
        {
            return Some(format!("```java\n{info}\n```\n\n**{kind}**"));
        }
    }

    if let Some(class_node) = find_ancestor(token, SyntaxKind::CLASS_DECL) {
        if let Some(info) = find_field_declaration(&class_node, identifier_name) {
            return Some(format!("```java\n{info}\n```\n\n**Field**"));
        }
    }

    None
}

/// Format metadata only from an exact, revision-bound workspace declaration.
/// This is the safe fallback when Hover's presentation-oriented type resolver
/// cannot reproduce (or disagrees with) Navigation's semantic identity.
pub(super) fn format_workspace_definition_hover(definition: &ResolvedDefinition) -> String {
    let symbol = &definition.symbol;
    let mut markdown = String::new();
    match definition.target.namespace {
        DeclarationNamespace::Callable | DeclarationNamespace::Constructor => {
            if let Some(return_type) = symbol.return_type.as_ref() {
                markdown.push_str(&format!("`{return_type}`\n\n"));
            }
            let parameters = symbol
                .parameters
                .iter()
                .map(|parameter| parameter.as_ref())
                .collect::<Vec<_>>()
                .join(", ");
            let modifiers = workspace_modifier_prefix(symbol);
            markdown.push_str(&format!(
                "**{modifiers}{}({parameters})**",
                symbol.simple_name
            ));
        }
        DeclarationNamespace::Value => {
            let modifiers = if symbol.kind == tower_lsp::lsp_types::SymbolKind::ENUM_MEMBER {
                String::new()
            } else {
                workspace_modifier_prefix(symbol)
            };
            if let Some(value_type) = symbol.return_type.as_ref() {
                markdown.push_str(&format!(
                    "**{modifiers}{}**: `{value_type}`",
                    symbol.simple_name
                ));
            } else {
                markdown.push_str(&format!("**{modifiers}{}**", symbol.simple_name));
            }
            if symbol.kind == tower_lsp::lsp_types::SymbolKind::ENUM_MEMBER {
                markdown.push_str("\n\n_enum constant_");
            }
        }
        DeclarationNamespace::Type | DeclarationNamespace::Other => {
            let kind = match symbol.kind {
                tower_lsp::lsp_types::SymbolKind::CLASS => "Class",
                tower_lsp::lsp_types::SymbolKind::INTERFACE => "Interface",
                tower_lsp::lsp_types::SymbolKind::ENUM => "Enum",
                tower_lsp::lsp_types::SymbolKind::STRUCT => "Struct",
                tower_lsp::lsp_types::SymbolKind::TYPE_PARAMETER => "Type parameter",
                _ => "Type",
            };
            markdown.push_str(&format!(
                "**{kind} {}**\n\n`{}`",
                symbol.simple_name, symbol.fqn
            ));
        }
    }
    if let Some(container) = symbol.container.as_ref() {
        markdown.push_str(&format!("\n\n_From:_ `{container}`"));
    }
    if let Some(documentation) = symbol.documentation.as_ref() {
        markdown.push_str(&format!("\n\n---\n\n{documentation}"));
    }
    markdown
}

fn workspace_modifier_prefix(symbol: &hyperion_workspace::Symbol) -> String {
    let modifiers = symbol.modifiers;
    let mut rendered = Vec::new();
    if modifiers.is_public {
        rendered.push("public");
    } else if modifiers.is_protected {
        rendered.push("protected");
    } else if modifiers.is_private {
        rendered.push("private");
    } else if modifiers.is_internal {
        rendered.push("internal");
    }
    if modifiers.is_abstract {
        rendered.push("abstract");
    }
    if modifiers.is_static {
        rendered.push("static");
    }
    if modifiers.is_final {
        rendered.push("final");
    }
    if modifiers.is_sealed {
        rendered.push("sealed");
    }
    if rendered.is_empty() {
        String::new()
    } else {
        format!("{} ", rendered.join(" "))
    }
}

fn enclosing_type_fqn(
    token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    workspace_index: &WorkspaceIndex,
    file_id: hyperion_semantics::db::FileId,
) -> Option<String> {
    // One owner for "which type declaration is the cursor in".
    // Hover keeps its indexed package as a fallback for a parsed range that
    // does not reach the file's `package` declaration.
    let type_node = crate::inherited_members::type_declaration_at(token)?;
    let indexed_package = workspace_index
        .get_imports(file_id)
        .and_then(|imports| imports.current_package);
    crate::inherited_members::type_declaration_fqn_in(&type_node, indexed_package.as_deref())
}

/// Find a parameter declaration in a method/constructor
fn find_parameter_declaration(method_node: &SyntaxNode, param_name: &str) -> Option<String> {
    for child in method_node.children() {
        if child.kind() == SyntaxKind::PARAM_LIST {
            for param in child.children() {
                if param.kind() == SyntaxKind::PARAM
                    && find_identifier_in_node(&param, param_name).is_some()
                {
                    return extract_parameter_info_from_node(&param);
                }
            }
        }
    }
    None
}

/// Find a local variable declaration in a method/constructor body.
/// Only returns variables declared before the reference point.
fn find_local_variable_declaration(
    method_node: &SyntaxNode,
    reference_token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    var_name: &str,
) -> Option<(String, &'static str)> {
    let reference_offset = reference_token.text_range().start();
    for child in method_node.children() {
        if child.kind() == SyntaxKind::BLOCK_STMT {
            return find_variable_in_block(&child, var_name, reference_offset);
        }
    }
    None
}

fn find_variable_in_block(
    block: &SyntaxNode,
    var_name: &str,
    reference_offset: rowan::TextSize,
) -> Option<(String, &'static str)> {
    for child in block.descendants() {
        if child.text_range().start() >= reference_offset {
            break;
        }
        if child.kind() == SyntaxKind::PATTERN_INSTANCEOF_EXPR {
            if let Some(pattern_info) = extract_pattern_variable_info_from_node(&child, var_name) {
                return Some((pattern_info, "Pattern variable"));
            }
        } else if child.kind() == SyntaxKind::LOCAL_VAR_DECL {
            // Only match declared variable names, not identifiers used in initializers.
            let declares_var = child
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .any(|tok| tok.kind().is_identifier() && tok.text() == var_name);
            if declares_var {
                if let Some(var_info) = extract_variable_info_from_node(&child) {
                    return Some((var_info, "Local variable"));
                }
            }
        }
    }
    None
}

fn find_field_declaration(class_node: &SyntaxNode, field_name: &str) -> Option<String> {
    for child in class_node.descendants() {
        if child.kind() == SyntaxKind::FIELD_DECL
            && find_identifier_in_node(&child, field_name).is_some()
        {
            return extract_field_info_from_node(&child);
        }
    }
    None
}

/// Find a method declaration in a class
/// If there are multiple overloads, returns all of them
pub fn find_method_declaration(
    class_node: &SyntaxNode,
    method_name: &str,
    source_text: &str,
) -> Option<String> {
    let mut matching_methods = Vec::new();

    // Search through class for method declarations
    for child in class_node.descendants() {
        if child.kind() == SyntaxKind::METHOD_DECL {
            // Extract method name using same logic as extract_method_signature_from_node
            // Method name is the first IDENT after the return type
            let mut return_type_found = false;
            let mut found_method_name = None;

            for token_or_node in child.children_with_tokens() {
                match token_or_node {
                    rowan::NodeOrToken::Node(node) => {
                        if matches!(
                            node.kind(),
                            SyntaxKind::CLASS_TYPE
                                | SyntaxKind::PRIMITIVE_TYPE
                                | SyntaxKind::ARRAY_TYPE
                        ) {
                            return_type_found = true;
                        }
                    }
                    rowan::NodeOrToken::Token(token) => {
                        if token.kind() == SyntaxKind::VOID_KW {
                            return_type_found = true;
                        } else if token.kind() == SyntaxKind::IDENT
                            && return_type_found
                            && found_method_name.is_none()
                        {
                            found_method_name = Some(token.text().to_string());
                            break;
                        }
                    }
                }
            }

            // Check if this is the method we're looking for
            if let Some(name) = found_method_name {
                if name == method_name {
                    // Extract the full method signature
                    if let Some(signature) = extract_method_signature_from_node(&child) {
                        // Calculate line number from node's text range
                        let offset: usize = child.text_range().start().into();
                        let position = offset_to_position(source_text, offset);
                        let line_number = position.line + 1; // LSP is 0-indexed, display as 1-indexed

                        // Extract documentation if available
                        let documentation = extract_documentation(&child);

                        matching_methods.push((signature, line_number, documentation));
                    }
                }
            }
        }
    }

    // Format the result based on how many matches we found
    if matching_methods.is_empty() {
        None
    } else if matching_methods.len() == 1 {
        let (signature, line_number, documentation) = &matching_methods[0];
        let mut result = format!("```java\n{signature}\n```\n\n**Method** (line {line_number})");

        // Add documentation if available
        if let Some(doc) = documentation {
            result.push_str(&format!("\n\n{doc}"));
        }

        Some(result)
    } else {
        // Multiple overloads - show all of them
        let mut result = String::from("### Method Overloads\n\n");
        for (signature, line_number, documentation) in matching_methods {
            result.push_str(&format!(
                "```java\n{signature}\n```\n**line {line_number}**"
            ));

            // Add documentation snippet if available
            if let Some(doc) = documentation {
                let first_line = doc.lines().next().unwrap_or("");
                result.push_str(&format!(" - {first_line}\n\n"));
            } else {
                result.push_str("\n\n");
            }
        }
        Some(result)
    }
}

/// Check if a token is the name identifier of a method declaration
pub fn is_method_name_token(
    method_node: &SyntaxNode,
    token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
) -> bool {
    // The method name is the first IDENT after the return type
    let mut return_type_found = false;

    for child in method_node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(node) => {
                if matches!(
                    node.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) {
                    return_type_found = true;
                }
            }
            rowan::NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::VOID_KW {
                    return_type_found = true;
                } else if tok.kind() == SyntaxKind::IDENT && return_type_found {
                    // This is the method name
                    return tok.text_range() == token.text_range();
                }
            }
        }
    }

    false
}

/// Check if a token is the name identifier of a field declaration
pub fn is_field_name_token(
    field_node: &SyntaxNode,
    token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
) -> bool {
    // Field name is the IDENT after the type
    let mut type_found = false;

    for child in field_node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(node) => {
                if matches!(
                    node.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) {
                    type_found = true;
                }
            }
            rowan::NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::IDENT && type_found {
                    // This is the field name
                    return tok.text_range() == token.text_range();
                }
            }
        }
    }

    false
}

/// Check if a token is the name identifier of a class/interface declaration
pub fn is_type_name_token(
    type_node: &SyntaxNode,
    token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
) -> bool {
    // Type name is the first IDENT after class/interface keyword
    let mut keyword_found = false;

    for child in type_node.children_with_tokens() {
        if let rowan::NodeOrToken::Token(tok) = child {
            if tok.kind() == SyntaxKind::CLASS_KW || tok.kind() == SyntaxKind::INTERFACE_KW {
                keyword_found = true;
            } else if tok.kind() == SyntaxKind::IDENT && keyword_found {
                // This is the type name
                return tok.text_range() == token.text_range();
            }
        }
    }

    false
}
