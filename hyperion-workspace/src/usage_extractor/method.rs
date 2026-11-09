// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Method and constructor call extraction

use crate::{
    import_resolver::ImportResolver,
    usage_resolution::{
        is_callable_declaration, resolve_method, resolve_method_with_descriptor,
        resolve_static_method_with_descriptor,
    },
    FileId, SymbolLocation, TextRange,
};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use lsp_types::SymbolKind;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

use super::signature_inference::{
    build_argument_descriptor_with_types, build_type_signature_from_arg_list,
    build_type_signature_from_arg_list_with_resolver,
};
use super::type_spelling::parse_parameterized_type;
use super::{context::resolve_visible_variable_type, context::ExtractionContext, Usage};

#[derive(Debug, Clone)]
struct ChainSegment {
    name: String,
    is_method_call: bool,
    is_constructed_type: bool,
    is_class_literal: bool,
    argument_descriptor: Option<String>,
    range: rowan::TextRange,
}

/// Return the written nominal type and its range from a class-instance
/// creation expression. Type arguments deliberately do not participate in the
/// nominal identity.
fn new_expression_class_type(node: &SyntaxNode) -> Option<(String, rowan::TextRange)> {
    let class_type = node
        .children()
        .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)?;
    let qualified_name = class_type
        .children()
        .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME);
    let owner = qualified_name.as_ref().unwrap_or(&class_type);
    let name = owner
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind().is_identifier())
        .map(|token| token.text().to_string())
        .collect::<Vec<_>>()
        .join(".");
    (!name.is_empty()).then_some((name, class_type.text_range()))
}

fn strip_type_name(raw: &str) -> &str {
    let base = raw.split('<').next().unwrap_or(raw);
    base.trim_end_matches("[]").trim()
}

fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "byte" | "short" | "int" | "long" | "float" | "double" | "boolean" | "char" | "void"
    )
}

/// Build chain segments from a METHOD_CALL_EXPR or FIELD_ACCESS_EXPR node.
///
/// With nested expression trees, `a.b().c()` is:
/// ```text
/// METHOD_CALL_EXPR           ← outer .c()
///   METHOD_CALL_EXPR         ← inner .b()
///     NAME_EXPR("a")
///     DOT
///     IDENT("b")
///     ARG_LIST
///   DOT
///   IDENT("c")
///   ARG_LIST
/// ```
/// We flatten this recursively into segments: [a, b, c]
fn build_chain_segments(node: &SyntaxNode) -> Vec<ChainSegment> {
    let mut segments = Vec::new();
    build_chain_segments_recursive(node, &mut segments);
    segments
}

fn build_chain_segments_recursive(node: &SyntaxNode, segments: &mut Vec<ChainSegment>) {
    let is_method_call = node.kind() == SyntaxKind::METHOD_CALL_EXPR;
    let is_field_access = node.kind() == SyntaxKind::FIELD_ACCESS_EXPR;
    let argument_descriptor = is_method_call.then(|| {
        node.children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)
            .map_or_else(String::new, |arguments| {
                build_type_signature_from_arg_list(&arguments)
            })
    });

    for elem in node.children_with_tokens() {
        match elem {
            rowan::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::ARG_LIST {
                    continue;
                }
                if matches!(
                    child.kind(),
                    SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::FIELD_ACCESS_EXPR
                ) {
                    // Recurse into nested call/access (the receiver)
                    build_chain_segments_recursive(&child, segments);
                } else if child.kind() == SyntaxKind::NAME_EXPR {
                    let name = child.text().to_string().trim().to_string();
                    segments.push(ChainSegment {
                        name,
                        // If this NAME_EXPR is the only child of a METHOD_CALL_EXPR
                        // (unqualified call), it's a method call segment
                        is_method_call: is_method_call && !node.children_with_tokens().any(|e| {
                            matches!(e, rowan::NodeOrToken::Token(ref t) if t.kind() == SyntaxKind::DOT)
                        }),
                        is_constructed_type: false,
                        is_class_literal: false,
                        argument_descriptor: argument_descriptor.clone(),
                        range: child.text_range(),
                    });
                } else if child.kind() == SyntaxKind::NEW_EXPR {
                    if let Some((name, range)) = new_expression_class_type(&child) {
                        segments.push(ChainSegment {
                            name,
                            is_method_call: false,
                            is_constructed_type: true,
                            is_class_literal: false,
                            argument_descriptor: None,
                            range,
                        });
                    }
                } else if child.kind() == SyntaxKind::CLASS_LITERAL_EXPR {
                    // Every class literal has the runtime type `java.lang.Class`,
                    // regardless of whether its operand is primitive, array, or
                    // reference-typed. Preserve that semantic fact in the chain
                    // instead of treating `Entity.class.getMethod(...)` as a call
                    // on `Entity` (or dropping the receiver altogether).
                    segments.push(ChainSegment {
                        name: child.text().to_string().trim().to_string(),
                        is_method_call: false,
                        is_constructed_type: false,
                        is_class_literal: true,
                        argument_descriptor: None,
                        range: child.text_range(),
                    });
                } else if child.kind() == SyntaxKind::THIS_EXPR {
                    segments.push(ChainSegment {
                        name: "this".to_string(),
                        is_method_call: false,
                        is_constructed_type: false,
                        is_class_literal: false,
                        argument_descriptor: None,
                        range: child.text_range(),
                    });
                } else if child.kind() == SyntaxKind::SUPER_EXPR {
                    segments.push(ChainSegment {
                        name: "super".to_string(),
                        is_method_call: false,
                        is_constructed_type: false,
                        is_class_literal: false,
                        argument_descriptor: None,
                        range: child.text_range(),
                    });
                }
            }
            rowan::NodeOrToken::Token(token) => {
                if token.kind().is_identifier() {
                    // This IDENT is the method/field name at this level
                    segments.push(ChainSegment {
                        name: token.text().to_string(),
                        is_method_call,
                        is_constructed_type: false,
                        is_class_literal: false,
                        argument_descriptor: argument_descriptor.clone(),
                        range: token.text_range(),
                    });
                }
            }
        }
    }

    // If this is a field access node and we didn't push an IDENT
    // (shouldn't happen normally), we're done
    let _ = is_field_access;
}

fn resolve_segment_type(
    segment: &ChainSegment,
    use_site: &SyntaxNode,
    resolver: &ImportResolver,
    ctx: &ExtractionContext,
) -> Option<String> {
    if segment.is_class_literal {
        return Some("java.lang.Class".to_string());
    }
    if segment.name == "this" {
        return ctx.enclosing_class_fqn.clone();
    }
    if segment.name == "super" {
        return ctx
            .enclosing_class_fqn
            .as_deref()
            .and_then(|owner| resolver.get_direct_superclass(owner));
    }
    if segment.is_constructed_type {
        return match resolver.resolve_type_identity(&segment.name) {
            crate::TypeNameResolution::Exact(fqn) => Some(fqn),
            crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
        };
    }
    if let Some(ty) = resolve_visible_variable_type(use_site, &segment.name, ctx, resolver) {
        return Some(ty);
    }
    let is_likely_type = segment
        .name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());
    if is_likely_type {
        return match resolver.resolve_type_identity(&segment.name) {
            crate::TypeNameResolution::Exact(fqn) => Some(fqn),
            crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
        };
    }
    None
}

fn resolve_method_return_type_fqn(
    resolver: &ImportResolver,
    class_fqn: &str,
    method_name: &str,
    argument_descriptor: &str,
    cache: &mut HashMap<(String, String, String), Option<String>>,
) -> Option<String> {
    let cache_key = (
        class_fqn.to_string(),
        method_name.to_string(),
        argument_descriptor.to_string(),
    );
    if let Some(cached) = cache.get(&cache_key) {
        return cached.clone();
    }

    let result =
        resolve_method_return_type_fqn_inner(resolver, class_fqn, method_name, argument_descriptor);
    cache.insert(cache_key, result.clone());
    result
}

fn resolve_method_return_type_fqn_inner(
    resolver: &ImportResolver,
    class_fqn: &str,
    method_name: &str,
    argument_descriptor: &str,
) -> Option<String> {
    let receiver = parse_parameterized_type(class_fqn)?;
    // Method symbols are stored with their parameter signature
    // (`Class.method(Type,...)`). `WorkspaceIndex::get_symbol` accepts the
    // signature-less compatibility form too, but resolving it requires a
    // linear scan over every symbol in the workspace. Usage extraction calls
    // this path for method chains throughout every source file, making cold
    // indexing quadratic in practice on large workspaces.
    //
    // Resolve through inheritance and refuse ambiguous overload sets. Picking
    // the first direct declaration here used to make chained calls depend on
    // FQN sort order and prevented inherited methods from advancing the chain.
    let symbol = resolve_method_with_descriptor(
        &receiver.base,
        method_name,
        Some(argument_descriptor),
        resolver,
    )?;
    let projected_return =
        resolver.project_member_type_with_arguments(&receiver.base, &receiver.arguments, &symbol);
    let raw_return = projected_return
        .as_deref()
        .or_else(|| symbol.return_type.as_deref())?;
    let base_type = strip_type_name(raw_return);
    if is_primitive_type(base_type) {
        return None;
    }

    // A return type is written in the declaration's lexical/import context,
    // not the caller's and not necessarily the runtime receiver's package.
    // Preserve ambiguity rather than advancing a chain through whichever
    // wildcard happens to be visited first.
    match resolver.resolve_declaration_type_name(&symbol, base_type) {
        crate::TypeNameResolution::Exact(resolved) => Some(resolved),
        crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
    }
}

fn resolve_receiver_type_from_chain(
    node: &SyntaxNode,
    method_range: rowan::TextRange,
    resolver: &ImportResolver,
    ctx: &ExtractionContext,
    return_type_cache: &mut HashMap<(String, String, String), Option<String>>,
) -> Option<String> {
    // With nested trees, build chain from the METHOD_CALL_EXPR itself
    let segments = build_chain_segments(node);
    let target_idx = segments.iter().position(|seg| seg.range == method_range)?;
    if target_idx == 0 {
        return None;
    }

    let first = &segments[0];
    let mut current_type = if first.is_method_call {
        let owner = ctx.enclosing_class_fqn.as_deref()?;
        resolve_method_return_type_fqn(
            resolver,
            owner,
            &first.name,
            first.argument_descriptor.as_deref().unwrap_or(""),
            return_type_cache,
        )?
    } else {
        resolve_segment_type(first, node, resolver, ctx)?
    };
    if target_idx > 1 {
        for segment in segments.iter().skip(1).take(target_idx - 1) {
            if segment.is_method_call {
                current_type = resolve_method_return_type_fqn(
                    resolver,
                    &current_type,
                    &segment.name,
                    segment.argument_descriptor.as_deref().unwrap_or(""),
                    return_type_cache,
                )?;
            } else {
                return None;
            }
        }
    }

    Some(current_type)
}

/// Resolve a qualified receiver to a method FQN.
///
/// Handles two cases:
/// 1. **Static call** — receiver starts with uppercase → resolve as class via imports
/// 2. **Instance call** — resolve the receiver in its exact lexical scope
///
/// Returns `None` (and logs) if the variable type cannot be found.
fn resolve_qualified_receiver_fqn(
    use_site: &SyntaxNode,
    receiver: &str,
    method: &str,
    resolver: &ImportResolver,
    ctx: &ExtractionContext,
) -> Option<String> {
    if receiver == "super" {
        let superclass = ctx
            .enclosing_class_fqn
            .as_deref()
            .and_then(|owner| resolver.get_direct_superclass(owner))?;
        return Some(format!("{superclass}.{method}"));
    }

    if let Some(type_fqn) = resolve_visible_variable_type(use_site, receiver, ctx, resolver) {
        debug!(
            "Instance method call: {}.{}() -> {}.{}",
            receiver, method, type_fqn, method
        );
        return Some(format!("{}.{}", type_fqn, method));
    }

    let is_likely_static = receiver
        .chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false);

    if is_likely_static {
        match resolver.resolve_type_identity(receiver) {
            crate::TypeNameResolution::Exact(class_fqn) => Some(format!("{class_fqn}.{method}")),
            crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
        }
    } else {
        debug!(
            "Skipping instance method call {}.{}() - variable type not found",
            receiver, method
        );
        None
    }
}

/// Extract method call references
///
/// This extracts method calls and resolves them to FQNs where possible.
///
/// Handles:
/// - Static method calls: `ClassName.methodName()` -> resolved via imports
/// - this.methodName() calls -> resolved to current class
/// - Unqualified calls (methodName()) -> resolved to current class
/// - Instance method calls: `user.getName()` -> resolved via variable type tracking
///
/// Parsed structural components of a METHOD_CALL_EXPR node.
///
/// Separates AST traversal (what children exist) from semantic resolution
/// (what FQN those children map to).
struct MethodCallParts {
    receiver_name: Option<String>,
    receiver_range: Option<rowan::TextRange>,
    method_name: Option<String>,
    method_name_range: Option<rowan::TextRange>,
    is_this_call: bool,
    found_dot: bool,
    chain_idents: Vec<(String, rowan::TextRange)>,
    arg_list_node: Option<SyntaxNode>,
}

/// Parse the direct children of a METHOD_CALL_EXPR into structural parts.
///
/// This does no semantic resolution — it only categorises the AST children
/// into receiver, method name, dots, chain identifiers, and arg list.
fn parse_method_call_children(node: &SyntaxNode) -> MethodCallParts {
    let mut parts = MethodCallParts {
        receiver_name: None,
        receiver_range: None,
        method_name: None,
        method_name_range: None,
        is_this_call: false,
        found_dot: false,
        chain_idents: Vec::new(),
        arg_list_node: None,
    };

    for elem in node.children_with_tokens() {
        match elem {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DOT {
                    parts.found_dot = true;
                } else if parts.found_dot && token.kind().is_identifier() {
                    parts
                        .chain_idents
                        .push((token.text().to_string(), token.text_range()));
                    parts.method_name = Some(token.text().to_string());
                    parts.method_name_range = Some(token.text_range());
                }
            }
            rowan::NodeOrToken::Node(child_node) => {
                if child_node.kind() == SyntaxKind::ARG_LIST {
                    parts.arg_list_node = Some(child_node);
                    break;
                }
                if child_node.kind() == SyntaxKind::THIS_EXPR {
                    parts.is_this_call = true;
                } else if child_node.kind() == SyntaxKind::SUPER_EXPR && !parts.found_dot {
                    parts.receiver_name = Some("super".to_string());
                    parts.receiver_range = Some(child_node.text_range());
                } else if child_node.kind() == SyntaxKind::NAME_EXPR && !parts.found_dot {
                    parts.receiver_name = Some(child_node.text().to_string().trim().to_string());
                    parts.receiver_range = Some(child_node.text_range());
                } else if matches!(
                    child_node.kind(),
                    SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::FIELD_ACCESS_EXPR
                ) {
                    let segments = build_chain_segments(&child_node);
                    if let Some(first) = segments.first() {
                        parts.receiver_name = Some(first.name.clone());
                        parts.receiver_range = Some(first.range);
                    }
                    for seg in segments.iter().skip(1) {
                        parts.chain_idents.push((seg.name.clone(), seg.range));
                    }
                    parts.found_dot = true;
                } else if child_node.kind() == SyntaxKind::NEW_EXPR && !parts.found_dot {
                    if let Some((name, range)) = new_expression_class_type(&child_node) {
                        parts.receiver_name = Some(name);
                        parts.receiver_range = Some(range);
                    }
                } else if child_node.kind() == SyntaxKind::CLASS_LITERAL_EXPR && !parts.found_dot {
                    parts.receiver_name = Some(child_node.text().to_string().trim().to_string());
                    parts.receiver_range = Some(child_node.text_range());
                }
            }
        }
    }

    parts
}

pub fn extract_method_call(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
    ctx: &ExtractionContext,
) {
    let mut return_type_cache: HashMap<(String, String, String), Option<String>> = HashMap::new();

    let MethodCallParts {
        receiver_name,
        receiver_range,
        mut method_name,
        mut method_name_range,
        is_this_call,
        found_dot,
        chain_idents,
        arg_list_node,
    } = parse_method_call_children(node);

    // Extract usages for intermediate field accesses in chains like KeyKind.SIG.getSpecName()
    // For this example: receiver_name = "KeyKind", chain_idents = [("SIG", ..), ("getSpecName", ..)]
    // We need to extract KeyKind.SIG as a field usage (all except the last ident which is the method)
    if let Some(ref receiver) = receiver_name {
        if chain_idents.len() > 1 {
            // There are intermediate field accesses
            // e.g., KeyKind.SIG in KeyKind.SIG.getSpecName()
            let is_likely_type = receiver
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase());

            if is_likely_type {
                // Resolve the base type (e.g., KeyKind -> com.example.crypto.KeyKind)
                let base_fqn = match resolver.resolve_type_identity(receiver) {
                    crate::TypeNameResolution::Exact(fqn) => Some(fqn),
                    crate::TypeNameResolution::Missing
                    | crate::TypeNameResolution::Ambiguous(_) => None,
                };

                if let Some(base) = base_fqn {
                    let intermediate = chain_idents
                        .iter()
                        .take(chain_idents.len() - 1)
                        .collect::<Vec<_>>();
                    for (index, (_, range)) in intermediate.iter().enumerate() {
                        let names = intermediate[..=index]
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect::<Vec<_>>();
                        let Some(field) =
                            super::field::resolve_field_chain(&base, &names, resolver)
                        else {
                            continue;
                        };
                        debug!("USAGE: Extracted exact chain field access: {}", field.fqn);

                        let location = SymbolLocation {
                            file_id,
                            source_revision_id: None,
                            range: TextRange::new(range.start().into(), range.end().into()),
                            kind: field.kind,
                            name: field.simple_name.clone(),
                            fqn: field.fqn.clone(),
                            container_name: field.container.clone(),
                            documentation: None,
                        };
                        usages.push(Usage {
                            fqn: field.fqn.to_string(),
                            location,
                        });
                    }
                }
            }
        }
    }

    let unqualified_method_name = (!is_this_call && !found_dot)
        .then(|| receiver_name.clone())
        .flatten();

    // Determine the FQN based on the call pattern
    let fqn = if is_this_call {
        // this.methodName() -> current class
        if let (Some(ref method), Some(ref class_fqn)) = (&method_name, &ctx.enclosing_class_fqn) {
            format!("{}.{}", class_fqn, method)
        } else {
            if let Some(ref method) = method_name {
                debug!(
                    "Skipping this.{}() call - no enclosing class context",
                    method
                );
            }
            return;
        }
    } else if found_dot {
        // qualified call: receiver.method()
        let Some(ref receiver) = receiver_name else {
            return;
        };
        let Some(ref method) = method_name else {
            return;
        };

        // If the receiver resolves via static imports, record it as a usage (e.g., OK.name()).
        let receiver_is_lexical =
            super::context::is_local_binding_visible_with_context(node, receiver, ctx)
                || ctx
                    .enclosing_class_fqn
                    .as_deref()
                    .is_some_and(|owner| resolver.has_value_member_named(owner, receiver));
        let static_values = if receiver_is_lexical {
            Vec::new()
        } else {
            resolver.resolve_static_values(receiver)
        };
        if let [static_value] = static_values.as_slice() {
            if let Some(range) = receiver_range {
                let location = SymbolLocation {
                    file_id,
                    source_revision_id: None,
                    range: TextRange::new(range.start().into(), range.end().into()),
                    kind: static_value.kind,
                    name: receiver.clone().into(),
                    fqn: Arc::clone(&static_value.fqn),
                    container_name: static_value.container.clone(),
                    documentation: None,
                };
                usages.push(Usage {
                    fqn: static_value.fqn.to_string(),
                    location,
                });
            }
            return;
        }

        // Try chain-based resolution first (for multi-segment calls like a.b().c())
        let chain_fqn = method_name_range.and_then(|range| {
            resolve_receiver_type_from_chain(node, range, resolver, ctx, &mut return_type_cache)
                .map(|chain_receiver| format!("{}.{}", chain_receiver, method))
        });

        if let Some(fqn) = chain_fqn {
            fqn
        } else if chain_idents.len() > 1 {
            // The immediate receiver is itself a call or field access. Falling
            // back to the root variable would turn `service.load().length()`
            // into `Service.length()`, creating a plausible but false edge.
            return;
        } else if let Some(fqn) =
            resolve_qualified_receiver_fqn(node, receiver, method, resolver, ctx)
        {
            fqn
        } else {
            return;
        }
    } else if let Some(ref method_name_from_receiver) = receiver_name {
        // Unqualified calls first consider members of the enclosing type.
        // Static-import applicability is evaluated below, after the argument
        // descriptor is available.
        if let Some(ref class_fqn) = ctx.enclosing_class_fqn {
            // For unqualified calls, the "receiver_name" is actually the method name
            method_name = Some(method_name_from_receiver.clone());
            method_name_range = receiver_range;
            format!("{}.{}", class_fqn, method_name_from_receiver)
        } else {
            method_name = Some(method_name_from_receiver.clone());
            method_name_range = receiver_range;
            method_name_from_receiver.clone()
        }
    } else {
        return;
    };

    // Append parameter descriptor to the FQN for overload disambiguation.
    // This mirrors the constructor FQN format: com.example.Foo.bar(String,int)
    let argument_descriptor = arg_list_node
        .as_ref()
        .map_or_else(String::new, |arguments| {
            build_type_signature_from_arg_list_with_resolver(arguments, |name_expression| {
                let name = name_expression.text().to_string();
                resolve_visible_variable_type(name_expression, name.trim(), ctx, resolver)
            })
        });
    let inferred_fqn = format!("{}({})", fqn, argument_descriptor);

    // Attribute inherited calls to the declaration owner rather than storing
    // a synthetic `Child.method` key that find-references can never join to
    // `Base.method`. Prefer the inferred exact overload; when that descriptor
    // is incomplete, fall back only if member resolution is unambiguous.
    let enclosing_applicability = unqualified_method_name
        .as_deref()
        .zip(ctx.enclosing_class_fqn.as_deref())
        .map_or(
            crate::usage_resolution::PotentialApplicability::No,
            |(name, owner)| {
                resolver.callable_member_applicability_with_descriptor(
                    owner,
                    name,
                    &argument_descriptor,
                )
            },
        );
    let resolved_symbol = resolver
        .get_symbol(&inferred_fqn)
        .filter(|symbol| is_callable_declaration(symbol.kind))
        .or_else(|| {
            let base = inferred_fqn.split_once('(')?.0;
            let (owner, method) = base.rsplit_once('.')?;
            resolve_method_with_descriptor(owner, method, Some(&argument_descriptor), resolver)
        })
        .or_else(|| {
            if enclosing_applicability != crate::usage_resolution::PotentialApplicability::No {
                return None;
            }
            resolve_static_method_with_descriptor(
                unqualified_method_name.as_deref()?,
                Some(&argument_descriptor),
                resolver,
            )
        });
    let (fqn, kind, container_name) = if let Some(symbol) = resolved_symbol {
        (
            symbol.fqn.to_string(),
            symbol.kind,
            symbol.container.clone(),
        )
    } else if unqualified_method_name.is_some() {
        // An unresolved enclosing call may be ill-typed, ambiguous, or depend
        // on hierarchy facts missing from the compact index. None of those is
        // a declaration edge, so do not fabricate `Owner.name(_)`.
        return;
    } else {
        (inferred_fqn, SymbolKind::METHOD, None)
    };

    debug!("Extracted method call usage: {}", fqn);

    // Create the usage record
    let range = method_name_range.unwrap_or(node.text_range());
    let location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: TextRange::new(range.start().into(), range.end().into()),
        kind,
        name: method_name.clone().unwrap_or_default().into(),
        fqn: fqn.clone().into(),
        container_name,
        documentation: None,
    };

    usages.push(Usage { fqn, location });
}

/// Extract method/constructor references (e.g., String::toUpperCase, Person::new).
pub fn extract_method_reference(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
    ctx: &ExtractionContext,
) {
    debug!("Extracting method reference: '{}'", node.text());
    // With nested trees, METHOD_REF_EXPR/CONSTRUCTOR_REF_EXPR contains:
    //   NAME_EXPR("ClassName") COLON_COLON IDENT("method") or NEW_KW
    let mut receiver_text: Option<String> = None;
    let mut found_colon_colon = false;

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::COLON_COLON {
                    found_colon_colon = true;
                    break;
                }
                if token.kind().is_identifier() {
                    receiver_text = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(child_node) => {
                if matches!(
                    child_node.kind(),
                    SyntaxKind::NAME_EXPR
                        | SyntaxKind::CLASS_TYPE
                        | SyntaxKind::QUALIFIED_NAME
                        | SyntaxKind::FIELD_ACCESS_EXPR
                        | SyntaxKind::THIS_EXPR
                        | SyntaxKind::SUPER_EXPR
                ) {
                    receiver_text = Some(child_node.text().to_string().trim().to_string());
                }
            }
        }
    }

    if !found_colon_colon {
        return;
    }

    let mut method_name: Option<String> = None;
    let mut method_range: Option<rowan::TextRange> = None;
    for child in node.children_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = child {
            if token.kind().is_identifier() {
                method_name = Some(token.text().to_string());
                method_range = Some(token.text_range());
                break;
            } else if token.kind() == SyntaxKind::NEW_KW {
                method_name = Some("new".to_string());
                method_range = Some(token.text_range());
                break;
            }
        }
    }
    if method_name.is_none() {
        for child in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child {
                if token.kind().is_identifier() {
                    method_name = Some(token.text().to_string());
                    method_range = Some(token.text_range());
                    break;
                } else if token.kind() == SyntaxKind::NEW_KW {
                    method_name = Some("new".to_string());
                    method_range = Some(token.text_range());
                    break;
                }
            }
        }
    }

    if receiver_text.is_none() {
        let search_text = node.text().to_string();
        if let Some((left, right)) = search_text.split_once("::") {
            receiver_text = Some(left.trim().to_string());
            if method_name.is_none() {
                let right_clean = right.trim();
                if right_clean == "new" {
                    method_name = Some("new".to_string());
                } else {
                    method_name = Some(right_clean.to_string());
                }
            }
        }
    }

    let Some(method_name) = method_name else {
        return;
    };
    let Some(receiver) = receiver_text else {
        return;
    };

    let receiver_clean = receiver
        .split('<')
        .next()
        .unwrap_or(&receiver)
        .trim()
        .trim_end_matches("[]");

    let receiver_fqn = if receiver_clean == "this" {
        ctx.enclosing_class_fqn.clone()
    } else if receiver_clean == "super" {
        ctx.enclosing_class_fqn
            .as_deref()
            .and_then(|owner| resolver.get_direct_superclass(owner))
    } else if let Some(variable_type) =
        resolve_visible_variable_type(node, receiver_clean, ctx, resolver)
    {
        Some(variable_type)
    } else if receiver_clean
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        match resolver.resolve_type_identity(receiver_clean) {
            crate::TypeNameResolution::Exact(fqn) => Some(fqn),
            crate::TypeNameResolution::Missing | crate::TypeNameResolution::Ambiguous(_) => None,
        }
    } else {
        None
    };

    let Some(receiver_fqn) = receiver_fqn else {
        debug!(
            "Skipping method reference {}::{} - receiver not resolved",
            receiver_clean, method_name
        );
        return;
    };

    debug!(
        "Resolved method reference {}::{} -> {}",
        receiver_clean, method_name, receiver_fqn
    );

    let (fqn, kind, name, container_name) = if method_name == "new" {
        let class_simple = receiver_fqn.rsplit('.').next().unwrap_or(receiver_clean);
        (
            format!("{}.{}()", receiver_fqn, class_simple),
            SymbolKind::CONSTRUCTOR,
            class_simple.to_string(),
            None,
        )
    } else if let Some(symbol) = resolve_method(&receiver_fqn, &method_name, resolver) {
        (
            symbol.fqn.to_string(),
            symbol.kind,
            symbol.simple_name.to_string(),
            symbol.container.clone(),
        )
    } else {
        return;
    };

    let range = method_range.unwrap_or(node.text_range());
    let location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: TextRange::new(range.start().into(), range.end().into()),
        kind,
        name: name.into(),
        fqn: fqn.clone().into(),
        container_name,
        documentation: None,
    };

    usages.push(Usage { fqn, location });
}

/// Extract constructor call: new User(), new ArrayList<String>()
///
/// Records a usage of the class being instantiated.
///
/// The constructor FQN includes argument-count placeholders, allowing overload
/// candidates to be distinguished:
/// - `new Person()` -> `com.example.Person.Person()`
/// - `new Person("Alice")` -> `com.example.Person.Person(_)`
/// - `new Person("Alice", 25)` -> `com.example.Person.Person(_,_)`
pub fn extract_constructor_call(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
) {
    // NEW_EXPR structure: NEW_KW followed by CLASS_TYPE (or qualified type)
    // We want to find the class name being constructed
    let mut class_name: Option<String> = None;
    let mut type_range: Option<rowan::TextRange> = None;

    for child in node.children() {
        if child.kind() == SyntaxKind::CLASS_TYPE {
            // Extract the simple class name from CLASS_TYPE
            // CLASS_TYPE may contain: QUALIFIED_NAME (with IDENTs inside), TYPE_ARGS
            let mut name_parts = Vec::new();

            // The class name might be in a QUALIFIED_NAME child
            for elem in child.children() {
                if elem.kind() == SyntaxKind::QUALIFIED_NAME {
                    // Extract IDENTs from QUALIFIED_NAME
                    for token in elem.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = token {
                            if t.kind().is_identifier() {
                                name_parts.push(t.text().to_string());
                            }
                        }
                    }
                }
            }

            // If no QUALIFIED_NAME, check for direct IDENT tokens
            if name_parts.is_empty() {
                for elem in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(token) = elem {
                        if token.kind().is_identifier() {
                            name_parts.push(token.text().to_string());
                        }
                    }
                }
            }

            if !name_parts.is_empty() {
                class_name = Some(name_parts.join("."));
                type_range = Some(child.text_range());
            }
            break;
        }
    }

    let Some(name) = class_name else { return };
    let range = type_range.unwrap_or(node.text_range());

    // Preserve the entire nominal spelling. Resolving only the first segment
    // attributes `new Outer.Inner()` to `Outer.Outer()` and loses the nested
    // constructor identity.
    let constructor_name = name.rsplit('.').next().unwrap_or(&name);
    let crate::TypeNameResolution::Exact(class_fqn) = resolver.resolve_type_identity(&name) else {
        return;
    };

    // Phase 2: Infer argument types from the constructor call
    // Build type signature: "String,int" for `new Person("Alice", 25)`
    let param_descriptor = build_argument_descriptor_with_types(node);

    // Build full constructor FQN including parameter descriptor
    // Format: com.example.Person.Person(String,int)
    let fqn = format!("{class_fqn}.{constructor_name}({param_descriptor})");

    debug!(
        "Extracted constructor usage: {} -> {} (signature: {})",
        name, fqn, param_descriptor
    );

    let location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: TextRange::new(range.start().into(), range.end().into()),
        kind: SymbolKind::CONSTRUCTOR,
        name: constructor_name.to_string().into(),
        fqn: fqn.clone().into(),
        container_name: None,
        documentation: None,
    };

    usages.push(Usage { fqn, location });
}
