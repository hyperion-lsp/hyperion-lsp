// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Field access extraction - handles qualified field access like ResourceType.UNKNOWN

use crate::{
    import_resolver::ImportResolver, usage_resolution::resolve_field, FileId, ImportMap,
    SymbolLocation, TextRange,
};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::sync::Arc;
use tracing::debug;

use super::type_spelling::parse_parameterized_type;
use super::{context, ExtractionContext, Usage};

/// Resolve every hop of a value chain and return the exact final declaration.
/// A textual `Owner.a.b` concatenation is not a symbol identity: `a` may be
/// inherited and its projected type owns `b`.
pub(super) fn resolve_field_chain(
    receiver: &str,
    fields: &[String],
    resolver: &ImportResolver<'_>,
) -> Option<Arc<crate::Symbol>> {
    let receiver = parse_parameterized_type(receiver)?;
    let mut owner = receiver.base;
    let mut arguments = receiver.arguments;

    for (index, field_name) in fields.iter().enumerate() {
        let Some(field) = resolve_field(&owner, field_name, resolver) else {
            // A source-qualified static value may cross a nested nominal type:
            // `Order.OrderStatus.PENDING`. Advance only through an exact type
            // declaration; never treat the segment as a guessed field.
            if index + 1 < fields.len() {
                let nested = format!("{owner}.{field_name}");
                if resolver
                    .get_symbol_target(&crate::SymbolTarget::new(
                        nested.as_str(),
                        crate::DeclarationNamespace::Type,
                    ))
                    .is_some()
                {
                    owner = nested;
                    arguments.clear();
                    continue;
                }
            }
            return None;
        };
        if index + 1 == fields.len() {
            return Some(field);
        }

        let projected = resolver
            .project_member_type_with_arguments(&owner, &arguments, &field)
            .or_else(|| field.return_type.as_ref().map(ToString::to_string))?;
        let projected = parse_parameterized_type(&projected)?;
        let crate::TypeNameResolution::Exact(next_owner) =
            resolver.resolve_declaration_type_name(&field, &projected.base)
        else {
            return None;
        };
        owner = next_owner;
        arguments = projected.arguments;
    }
    None
}

/// Recursively collect parts from a FIELD_ACCESS_EXPR chain.
///
/// With nested trees: `A.B.C` is:
/// ```text
/// FIELD_ACCESS_EXPR
///   FIELD_ACCESS_EXPR
///     NAME_EXPR("A")
///     DOT
///     IDENT("B")
///   DOT
///   IDENT("C")
/// ```
/// This produces parts: ["A", "B", "C"]
fn collect_field_chain_parts(
    node: &SyntaxNode,
    parts: &mut Vec<String>,
    last_ident_range: &mut Option<rowan::TextRange>,
) {
    for elem in node.children_with_tokens() {
        match elem {
            rowan::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::FIELD_ACCESS_EXPR
                    || child.kind() == SyntaxKind::METHOD_CALL_EXPR
                {
                    collect_field_chain_parts(&child, parts, last_ident_range);
                } else if child.kind() == SyntaxKind::THIS_EXPR {
                    parts.push("this".to_string());
                } else if child.kind() == SyntaxKind::SUPER_EXPR {
                    parts.push("super".to_string());
                } else if child.kind() == SyntaxKind::NAME_EXPR {
                    for token in child.descendants_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = token {
                            if t.kind().is_identifier() {
                                parts.push(t.text().to_string());
                                *last_ident_range = Some(t.text_range());
                            }
                        }
                    }
                }
            }
            rowan::NodeOrToken::Token(token) => {
                if token.kind().is_identifier() {
                    parts.push(token.text().to_string());
                    *last_ident_range = Some(token.text_range());
                }
            }
        }
    }
}

/// Extract qualified field access like ResourceType.UNKNOWN
///
/// AST structure (nested expression tree):
/// ```text
/// FIELD_ACCESS_EXPR
///   NAME_EXPR (ResourceType)
///   DOT (.)
///   IDENT (UNKNOWN)
/// ```
pub fn extract_field_access(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    _imports: &ImportMap,
    usages: &mut Vec<Usage>,
    ctx: &ExtractionContext,
) {
    // With nested expression trees, FIELD_ACCESS_EXPR contains:
    //   receiver_expr DOT IDENT
    // e.g., FIELD_ACCESS_EXPR(NAME_EXPR("ResourceType"), DOT, IDENT("UNKNOWN"))
    //
    // Skip if this is a receiver of a METHOD_CALL_EXPR (the method handler will deal with it)
    if let Some(parent) = node.parent() {
        if parent.kind() == SyntaxKind::METHOD_CALL_EXPR
            || parent.kind() == SyntaxKind::FIELD_ACCESS_EXPR
        {
            debug!(
                "Skipping field access extraction - child of method call or deeper field access"
            );
            return;
        }
    }

    // Collect all parts from the nested field access chain
    let mut parts = Vec::new();
    let mut last_ident_range: Option<rowan::TextRange> = None;
    collect_field_chain_parts(node, &mut parts, &mut last_ident_range);

    let found_dot = parts.len() >= 2;

    debug!(
        "Extracted field access parts: {:?}, found_dot: {}",
        parts, found_dot
    );

    if parts.len() < 2 || !found_dot {
        debug!("Not enough parts for qualified access, skipping");
        return; // Not a qualified access
    }

    // Build the qualified name: ResourceType.UNKNOWN
    let qualified_name = parts.join(".");

    // Try to resolve the qualified name
    // First part might be a class name that needs resolution
    let first_part = &parts[0];
    let remaining_parts = &parts[1..];

    // Resolve the first part (the class/enum name). Prefer candidates the
    // index knows: plain `resolve` ranks the current package above wildcard
    // imports, which records usages under FQNs that do not exist.
    debug!(
        "Resolving field receiver '{}' from its contextual type binding",
        first_part
    );

    // Use first candidate (most likely match based on import resolution order)
    let base_fqn =
        match first_part.as_str() {
            "this" => ctx.enclosing_class_fqn.clone(),
            "super" => ctx
                .enclosing_class_fqn
                .as_deref()
                .and_then(|owner| resolver.get_direct_superclass(owner)),
            _ => context::resolve_visible_variable_type(node, first_part, ctx, resolver).or_else(
                || match resolver.resolve_type_identity(first_part) {
                    crate::TypeNameResolution::Exact(fqn) => Some(fqn),
                    crate::TypeNameResolution::Missing
                    | crate::TypeNameResolution::Ambiguous(_) => None,
                },
            ),
        };
    if let Some(base_fqn) = base_fqn {
        let Some(resolved_symbol) = resolve_field_chain(&base_fqn, remaining_parts, resolver)
        else {
            debug!("Skipping unresolved field chain: {qualified_name}");
            return;
        };
        let fqn = resolved_symbol.fqn.to_string();

        debug!("USAGE: Extracted field access: {}", fqn);

        // Create usage record - use the last identifier's range, not the entire node
        // This ensures document highlighting only highlights the identifier, not the whole expression
        let ident_range = last_ident_range.unwrap_or(node.text_range());
        let range = TextRange::new(ident_range.start().into(), ident_range.end().into());

        let location = SymbolLocation {
            file_id,
            source_revision_id: None,
            range,
            kind: resolved_symbol.kind,
            name: Arc::clone(&resolved_symbol.simple_name),
            fqn: fqn.clone().into(),
            container_name: resolved_symbol.container.clone(),
            documentation: None,
        };

        usages.push(Usage { fqn, location });
    }
}

/// Extract qualified name from NAME_EXPR followed by DOT+IDENT
///
/// AST structure:
/// ```text
/// ARG_LIST (or other parent)
///   NAME_EXPR (ResourceType)
///   DOT (.)
///   IDENT (UNKNOWN)
/// ```
///
/// This pattern appears in method arguments and other contexts where
/// FIELD_ACCESS_EXPR marker is not generated.
pub fn extract_qualified_name_expr(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
    ctx: &ExtractionContext,
) -> bool {
    let parent = match node.parent() {
        Some(p) => p,
        None => return false,
    };

    // Check if this NAME_EXPR is inside a FIELD_ACCESS_EXPR or METHOD_CALL_EXPR
    // (either as a child in nested trees, or as a sibling in flat structures)
    // If so, let those handlers deal with the extraction to avoid double-counting
    let has_field_access_sibling = parent.kind() == SyntaxKind::FIELD_ACCESS_EXPR
        || parent
            .children()
            .any(|c| c.kind() == SyntaxKind::FIELD_ACCESS_EXPR);
    let has_method_call_sibling = parent.kind() == SyntaxKind::METHOD_CALL_EXPR
        || parent
            .children()
            .any(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR);

    // Special case: unqualified static import used as a method call receiver, e.g. OK.name()
    // Capture the receiver usage before exiting.
    if has_method_call_sibling && !has_field_access_sibling {
        let mut simple_name = None;
        let mut ident_range: Option<rowan::TextRange> = None;

        for token in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(t) = token {
                if t.kind().is_identifier() {
                    simple_name = Some(t.text().to_string());
                    ident_range = Some(t.text_range());
                    break;
                }
            }
        }

        if let Some(name) = simple_name {
            if context::is_local_binding_visible_with_context(node, &name, ctx)
                || ctx
                    .enclosing_class_fqn
                    .as_deref()
                    .is_some_and(|owner| resolver.has_value_member_named(owner, &name))
            {
                return false;
            }
            let candidates = resolver.resolve_static_values(&name);
            if let [symbol] = candidates.as_slice() {
                let range = ident_range.unwrap_or(node.text_range());
                let location = SymbolLocation {
                    file_id,
                    source_revision_id: None,
                    range: TextRange::new(range.start().into(), range.end().into()),
                    kind: symbol.kind,
                    name: name.clone().into(),
                    fqn: Arc::clone(&symbol.fqn),
                    container_name: symbol.container.clone(),
                    documentation: None,
                };
                usages.push(Usage {
                    fqn: symbol.fqn.to_string(),
                    location,
                });
                return true;
            }
        }
    }

    let has_expr_sibling = has_field_access_sibling || has_method_call_sibling;
    if has_expr_sibling {
        return false;
    }

    // Check if this NAME_EXPR is followed by DOT+IDENT in the parent
    let mut found_name_expr = false;
    let mut found_dot = false;
    let mut parts = Vec::new();
    let mut last_ident_range: Option<rowan::TextRange> = None;

    for sibling in parent.children_with_tokens() {
        if let rowan::NodeOrToken::Node(n) = &sibling {
            if n.text_range() == node.text_range() {
                // This is our NAME_EXPR
                found_name_expr = true;
                // Extract identifiers from this NAME_EXPR
                for token in node.descendants_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = token {
                        if t.kind().is_identifier() {
                            parts.push(t.text().to_string());
                            last_ident_range = Some(t.text_range());
                        }
                    }
                }
                continue;
            }
        }

        // Only look at siblings after the NAME_EXPR
        if !found_name_expr {
            continue;
        }

        match sibling {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DOT {
                    found_dot = true;
                } else if found_dot && token.kind().is_identifier() {
                    parts.push(token.text().to_string());
                    last_ident_range = Some(token.text_range());
                } else if found_dot && token.kind() != SyntaxKind::WHITESPACE {
                    // Hit a non-identifier, non-whitespace token after DOT - stop
                    break;
                }
            }
            rowan::NodeOrToken::Node(_) => {
                // Hit another node - stop
                if found_dot {
                    break;
                }
            }
        }
    }

    // If we found a qualified access (at least 2 parts and a DOT), record it
    if parts.len() >= 2 && found_dot {
        let first_part = &parts[0];
        let remaining_parts = &parts[1..];

        // Resolve the first part - use the first index-validated candidate
        let base_fqn = context::resolve_visible_variable_type(node, first_part, ctx, resolver)
            .or_else(|| match resolver.resolve_type_identity(first_part) {
                crate::TypeNameResolution::Exact(fqn) => Some(fqn),
                crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => {
                    None
                }
            });
        if let Some(base_fqn) = base_fqn {
            let Some(resolved_symbol) = resolve_field_chain(&base_fqn, remaining_parts, resolver)
            else {
                return false;
            };
            let fqn = resolved_symbol.fqn.to_string();

            // Use the last identifier's range for proper document highlighting
            let ident_range = last_ident_range.unwrap_or(node.text_range());
            let range = TextRange::new(ident_range.start().into(), ident_range.end().into());

            let location = SymbolLocation {
                file_id,
                source_revision_id: None,
                range,
                kind: resolved_symbol.kind,
                name: Arc::clone(&resolved_symbol.simple_name),
                fqn: fqn.clone().into(),
                container_name: resolved_symbol.container.clone(),
                documentation: None,
            };

            usages.push(Usage { fqn, location });
            return true;
        }
    } else if parts.len() == 1 && !found_dot {
        // Handle simple identifiers (static imports, enum constants, same-package references)
        // Example: `NO_LEADER` after `import static ...LeaderConstants.NO_LEADER;`
        // Example: `NOT_RUNNING` inside enum State { ... equals(NOT_RUNNING) ... }
        let simple_name = &parts[0];

        if context::is_local_binding_visible_with_context(node, simple_name, ctx)
            || ctx
                .enclosing_class_fqn
                .as_deref()
                .is_some_and(|owner| resolver.has_value_member_named(owner, simple_name))
        {
            return false;
        }

        // Static imports inhabit the value namespace. A same-named type or
        // callable must not be recorded as a field usage, and two wildcard
        // owners remain ambiguous.
        let candidates = resolver.resolve_static_values(simple_name);
        if let [symbol] = candidates.as_slice() {
            let ident_range = last_ident_range.unwrap_or(node.text_range());
            let range = TextRange::new(ident_range.start().into(), ident_range.end().into());

            let location = SymbolLocation {
                file_id,
                source_revision_id: None,
                range,
                kind: symbol.kind,
                name: simple_name.clone().into(),
                fqn: Arc::clone(&symbol.fqn),
                container_name: symbol.container.clone(),
                documentation: None,
            };

            debug!(
                "Extracted static import/simple reference: {} -> {}",
                simple_name, symbol.fqn
            );
            usages.push(Usage {
                fqn: symbol.fqn.to_string(),
                location,
            });
            return true;
        }
    }
    false
}
