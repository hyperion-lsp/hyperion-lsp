// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Conservative cross-file usage extraction for Scala source.
//!
//! Every emitted reference is resolved to an existing workspace symbol. When
//! overloads or receiver types are ambiguous, the extractor deliberately emits
//! nothing rather than polluting find-references and rename with guesses.

use crate::usage_resolution::{
    emit_symbol, is_callable_declaration, resolve_field, resolve_method, resolve_type_name,
};
use crate::{
    import_resolver::{ImportResolver, RevisionSymbolOverlay},
    usage_extractor::Usage,
    FileId, ImportMap, WorkspaceIndex,
};
use anyhow::Result;
use tree_sitter::{Node, Tree};

#[derive(Clone, Default)]
struct ScalaContext {
    container_fqn: Option<String>,
}

#[derive(Debug)]
struct Binding {
    name: String,
    type_fqn: Option<String>,
    scope_start: usize,
    scope_end: usize,
    visible_from: usize,
}

pub struct ScalaUsageExtractor;

impl ScalaUsageExtractor {
    pub fn extract(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Result<Vec<Usage>> {
        let parsed = hyperion_scala::parser::parse(source)?;
        Self::extract_from_tree(source, file_id, imports, workspace_index, &parsed.tree)
    }

    /// Extract usages from a caller-owned parse tree so request providers and
    /// indexing can share one immutable Scala CST when they already have it.
    pub fn extract_from_tree(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
        tree: &Tree,
    ) -> Result<Vec<Usage>> {
        let resolver =
            ImportResolver::for_language(imports, workspace_index, crate::Language::Scala);
        Ok(Self::extract_from_tree_with_resolver(
            source, file_id, imports, tree, &resolver,
        ))
    }

    pub(crate) fn extract_for_revision(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
        revision: &RevisionSymbolOverlay,
    ) -> Result<Vec<Usage>> {
        let parsed = hyperion_scala::parser::parse(source)?;
        let resolver = ImportResolver::for_revision(
            imports,
            workspace_index,
            revision,
            crate::Language::Scala,
        );
        Ok(Self::extract_from_tree_with_resolver(
            source,
            file_id,
            imports,
            &parsed.tree,
            &resolver,
        ))
    }

    fn extract_from_tree_with_resolver(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        tree: &Tree,
        resolver: &ImportResolver<'_>,
    ) -> Vec<Usage> {
        let context = ScalaContext::default();
        let mut bindings = Vec::new();
        collect_bindings(&tree.root_node(), source, resolver, &mut bindings);

        let mut usages = Vec::new();
        walk(
            &tree.root_node(),
            source,
            file_id,
            imports,
            resolver,
            &context,
            &bindings,
            &mut usages,
        );
        usages.sort_unstable_by(|left, right| {
            left.location
                .range
                .start
                .cmp(&right.location.range.start)
                .then_with(|| left.location.range.end.cmp(&right.location.range.end))
                .then_with(|| left.fqn.cmp(&right.fqn))
        });
        usages.dedup_by(|left, right| {
            left.fqn == right.fqn && left.location.range == right.location.range
        });
        usages
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: &Node<'_>,
    source: &str,
    file_id: FileId,
    imports: &ImportMap,
    resolver: &ImportResolver<'_>,
    context: &ScalaContext,
    bindings: &[Binding],
    usages: &mut Vec<Usage>,
) {
    if node.kind() == "import_declaration" {
        return;
    }

    let mut scoped_context;
    let context = if is_type_scope(node.kind()) {
        scoped_context = context.clone();
        if let Some(name) = field_text(node, "name", source) {
            scoped_context.container_fqn = Some(match context.container_fqn.as_deref() {
                Some(container) => format!("{container}.{name}"),
                None => imports
                    .current_package
                    .as_deref()
                    .map_or_else(|| name.to_string(), |package| format!("{package}.{name}")),
            });
        }
        &scoped_context
    } else {
        context
    };

    extract_declared_type_usages(node, source, file_id, resolver, usages);

    match node.kind() {
        "call_expression" => {
            extract_call_usage(node, source, file_id, resolver, context, bindings, usages);
        }
        "field_expression" if !is_call_target(node) => {
            extract_field_usage(node, source, file_id, resolver, bindings, usages);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(
            &child, source, file_id, imports, resolver, context, bindings, usages,
        );
    }
}

fn is_type_scope(kind: &str) -> bool {
    matches!(
        kind,
        "class_definition" | "object_definition" | "trait_definition" | "enum_definition"
    )
}

fn collect_bindings(
    node: &Node<'_>,
    source: &str,
    resolver: &ImportResolver<'_>,
    bindings: &mut Vec<Binding>,
) {
    if matches!(
        node.kind(),
        "parameter" | "class_parameter" | "val_definition" | "var_definition"
    ) {
        if let (Some(name), Some(scope)) = (binding_name(node, source), binding_scope(node)) {
            let explicit_type = node
                .child_by_field_name("type")
                .and_then(|type_node| type_node.utf8_text(source.as_bytes()).ok())
                .and_then(|raw_type| resolve_type_name(raw_type, resolver));
            let inferred_type = node
                .child_by_field_name("value")
                .and_then(|value| initializer_type(&value, source, resolver));
            let declaration_wide = matches!(node.kind(), "parameter" | "class_parameter")
                || scope.kind() == "template_body";
            bindings.push(Binding {
                name: name.to_string(),
                type_fqn: explicit_type.or(inferred_type),
                scope_start: scope.start_byte(),
                scope_end: scope.end_byte(),
                visible_from: if declaration_wide {
                    scope.start_byte()
                } else {
                    node.end_byte()
                },
            });
        }
    }

    // A nested local function shadows an equally named type/member after its
    // declaration, even though it has no receiver type of its own.
    if matches!(node.kind(), "function_definition" | "function_declaration") {
        if let (Some(name), Some(scope)) = (field_text(node, "name", source), binding_scope(node)) {
            if scope.kind() == "block" {
                bindings.push(Binding {
                    name: name.to_string(),
                    type_fqn: None,
                    scope_start: scope.start_byte(),
                    scope_end: scope.end_byte(),
                    // Local defs are visible recursively throughout their
                    // enclosing block, unlike vals/vars which become visible
                    // only after their declaration.
                    visible_from: scope.start_byte(),
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_bindings(&child, source, resolver, bindings);
    }
}

fn binding_scope<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if matches!(
            ancestor.kind(),
            "block"
                | "template_body"
                | "case_clause"
                | "for_expression"
                | "function_definition"
                | "function_declaration"
                | "class_definition"
                | "object_definition"
                | "trait_definition"
                | "enum_definition"
                | "compilation_unit"
        ) {
            return Some(ancestor);
        }
        current = ancestor.parent();
    }
    None
}

fn initializer_type(
    node: &Node<'_>,
    source: &str,
    resolver: &ImportResolver<'_>,
) -> Option<String> {
    if matches!(node.kind(), "new_expression" | "instance_expression") {
        let type_node = node.child_by_field_name("type").or_else(|| {
            node.named_children(&mut node.walk())
                .find(|child| child.kind().contains("type"))
        })?;
        let raw_type = type_node.utf8_text(source.as_bytes()).ok()?;
        return resolve_type_name(raw_type, resolver);
    }

    // Parentheses and a small number of wrapper nodes may surround `new`.
    let mut cursor = node.walk();
    let mut children = node.named_children(&mut cursor);
    let child = children.next()?;
    if children.next().is_none() {
        initializer_type(&child, source, resolver)
    } else {
        None
    }
}

fn binding_at<'a>(bindings: &'a [Binding], name: &str, offset: usize) -> Option<&'a Binding> {
    bindings
        .iter()
        .filter(|binding| {
            binding.name == name
                && binding.scope_start <= offset
                && offset < binding.scope_end
                && binding.visible_from <= offset
        })
        .max_by_key(|binding| (binding.scope_start, binding.visible_from))
}

fn extract_declared_type_usages(
    node: &Node<'_>,
    source: &str,
    file_id: FileId,
    resolver: &ImportResolver<'_>,
    usages: &mut Vec<Usage>,
) {
    match node.kind() {
        "parameter" | "class_parameter" | "val_definition" | "var_definition" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                emit_type_tree(&type_node, source, file_id, resolver, usages);
            }
        }
        "function_definition" | "function_declaration" => {
            if let Some(type_node) = node.child_by_field_name("return_type") {
                emit_type_tree(&type_node, source, file_id, resolver, usages);
            }
        }
        "extends_clause" => {
            let mut cursor = node.walk();
            for type_node in node
                .children_by_field_name("type", &mut cursor)
                .filter(Node::is_named)
            {
                emit_type_tree(&type_node, source, file_id, resolver, usages);
            }
        }
        "new_expression" | "instance_expression" => {
            if let Some(type_node) = node.child_by_field_name("type").or_else(|| {
                node.named_children(&mut node.walk())
                    .find(|child| child.kind().contains("type"))
            }) {
                emit_type_tree(&type_node, source, file_id, resolver, usages);
            }
        }
        _ => {}
    }
}

fn emit_type_tree(
    node: &Node<'_>,
    source: &str,
    file_id: FileId,
    resolver: &ImportResolver<'_>,
    usages: &mut Vec<Usage>,
) {
    if matches!(node.kind(), "type_identifier" | "stable_type_identifier") {
        let Ok(name) = node.utf8_text(source.as_bytes()) else {
            return;
        };
        let Some(fqn) = resolve_type_name(name, resolver) else {
            return;
        };
        let Some(symbol) = resolver.get_symbol(&fqn) else {
            return;
        };
        emit_symbol(usages, file_id, node, symbol);
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        emit_type_tree(&child, source, file_id, resolver, usages);
    }
}

fn extract_call_usage(
    node: &Node<'_>,
    source: &str,
    file_id: FileId,
    resolver: &ImportResolver<'_>,
    context: &ScalaContext,
    bindings: &[Binding],
    usages: &mut Vec<Usage>,
) {
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    match function.kind() {
        "field_expression" => {
            let Some(receiver) = function.child_by_field_name("value") else {
                return;
            };
            let Some(member) = function.child_by_field_name("field") else {
                return;
            };
            let Some(receiver_fqn) = resolve_receiver(&receiver, source, resolver, bindings) else {
                return;
            };
            let Ok(member_name) = member.utf8_text(source.as_bytes()) else {
                return;
            };
            if let Some(symbol) = resolve_method(&receiver_fqn, member_name, resolver) {
                emit_symbol(usages, file_id, &member, symbol);
            }
        }
        "identifier" => {
            let Ok(name) = function.utf8_text(source.as_bytes()) else {
                return;
            };
            if binding_at(bindings, name, function.start_byte()).is_some() {
                return;
            }
            let symbol = context
                .container_fqn
                .as_deref()
                .and_then(|container| resolve_method(container, name, resolver))
                .or_else(|| {
                    resolver
                        .imports()
                        .current_package
                        .as_deref()
                        .and_then(|package| {
                            resolver
                                .get_symbol(&format!("{package}.{name}"))
                                .filter(|symbol| is_callable_declaration(symbol.kind))
                        })
                })
                .or_else(|| {
                    resolver
                        .imports()
                        .explicit
                        .get(name)
                        .and_then(|fqn| resolver.get_symbol(fqn))
                        .filter(|symbol| is_callable_declaration(symbol.kind))
                })
                .or_else(|| {
                    resolver
                        .imports()
                        .static_wildcards
                        .iter()
                        .find_map(|owner| resolve_method(owner, name, resolver))
                });
            if let Some(symbol) = symbol {
                emit_symbol(usages, file_id, &function, symbol);
            }
        }
        _ => {}
    }
}

fn extract_field_usage(
    node: &Node<'_>,
    source: &str,
    file_id: FileId,
    resolver: &ImportResolver<'_>,
    bindings: &[Binding],
    usages: &mut Vec<Usage>,
) {
    let (Some(receiver), Some(member)) = (
        node.child_by_field_name("value"),
        node.child_by_field_name("field"),
    ) else {
        return;
    };
    let Some(receiver_fqn) = resolve_receiver(&receiver, source, resolver, bindings) else {
        return;
    };
    let Ok(member_name) = member.utf8_text(source.as_bytes()) else {
        return;
    };
    if let Some(symbol) = resolve_field(&receiver_fqn, member_name, resolver) {
        emit_symbol(usages, file_id, &member, symbol);
    }
}

fn is_call_target(node: &Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "call_expression"
            && parent
                .child_by_field_name("function")
                .is_some_and(|function| function.id() == node.id())
    })
}

fn resolve_receiver(
    node: &Node<'_>,
    source: &str,
    resolver: &ImportResolver<'_>,
    bindings: &[Binding],
) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    if node.kind() == "identifier" {
        if let Some(binding) = binding_at(bindings, text, node.start_byte()) {
            return binding.type_fqn.clone();
        }
        resolve_type_name(text, resolver)
    } else {
        None
    }
}

fn field_text<'a>(node: &Node<'_>, field: &str, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name(field)?
        .utf8_text(source.as_bytes())
        .ok()
}

fn binding_name<'a>(node: &Node<'_>, source: &'a str) -> Option<&'a str> {
    field_text(node, "name", source).or_else(|| {
        let pattern = node.child_by_field_name("pattern")?;
        (pattern.kind() == "identifier")
            .then(|| pattern.utf8_text(source.as_bytes()).ok())
            .flatten()
    })
}
