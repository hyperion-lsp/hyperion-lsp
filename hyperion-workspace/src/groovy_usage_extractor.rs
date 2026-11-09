// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Conservative cross-file usage extraction for Groovy source.
//!
//! `tree-sitter-groovy` does not represent safe navigation (`?.`) as a normal
//! expression node. Declarations and lexical scopes therefore come from the
//! CST, while member access comes from a small token scanner that excludes
//! imports, comments, and every string-like CST range. Resolution is shared
//! with the Scala walker and never emits an unverified symbol guess.

mod lexical;

use crate::usage_resolution::{
    emit_symbol, emit_symbol_range, resolve_field, resolve_method, resolve_type_name,
};
use crate::{
    import_resolver::{ImportResolver, RevisionSymbolOverlay},
    usage_extractor::Usage,
    FileId, ImportMap, WorkspaceIndex,
};
use anyhow::Result;
use lexical::{
    enclosing_brace_scope, matching_pairs, member_tokens, IdentifierToken, Punctuation, Token,
};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

#[derive(Debug)]
struct Binding {
    name: String,
    type_fqn: Option<String>,
    scope_start: usize,
    scope_end: usize,
    visible_from: usize,
}

#[derive(Debug)]
struct ContainerScope {
    fqn: String,
    start: usize,
    end: usize,
}

struct GroovyUsageWalker<'a> {
    source: &'a str,
    file_id: FileId,
    imports: &'a ImportMap,
    resolver: ImportResolver<'a>,
    usages: Vec<Usage>,
    bindings: Vec<Binding>,
    containers: Vec<ContainerScope>,
    excluded_ranges: Vec<(usize, usize)>,
}

pub struct GroovyUsageExtractor;

impl GroovyUsageExtractor {
    pub fn extract(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Result<Vec<Usage>> {
        let mut parser = hyperion_groovy::tree_sitter_wrapper::GroovyParser::new()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let tree = parser
            .parse_tree(source)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Self::extract_from_tree(source, file_id, imports, workspace_index, &tree)
    }

    /// Extract usages from a caller-owned parse tree. Request providers that
    /// already parsed the same immutable source revision use this entry point
    /// to keep declaration lookup and semantic resolution on one CST.
    pub fn extract_from_tree(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
        tree: &Tree,
    ) -> Result<Vec<Usage>> {
        let resolver =
            ImportResolver::for_language(imports, workspace_index, crate::Language::Groovy);
        Ok(Self::extract_from_tree_with_resolver(
            source, file_id, imports, tree, resolver,
        ))
    }

    pub(crate) fn extract_for_revision(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
        revision: &RevisionSymbolOverlay,
    ) -> Result<Vec<Usage>> {
        let mut parser = hyperion_groovy::tree_sitter_wrapper::GroovyParser::new()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let tree = parser
            .parse_tree(source)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let resolver = ImportResolver::for_revision(
            imports,
            workspace_index,
            revision,
            crate::Language::Groovy,
        );
        Ok(Self::extract_from_tree_with_resolver(
            source, file_id, imports, &tree, resolver,
        ))
    }

    fn extract_from_tree_with_resolver(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        tree: &Tree,
        resolver: ImportResolver<'_>,
    ) -> Vec<Usage> {
        let mut walker = GroovyUsageWalker {
            source,
            file_id,
            imports,
            resolver,
            usages: Vec::new(),
            bindings: Vec::new(),
            containers: Vec::new(),
            excluded_ranges: Vec::new(),
        };
        walker.walk_declarations(&tree.root_node(), None);
        walker.normalize_excluded_ranges();
        let tokens = member_tokens(source, &walker.excluded_ranges);
        walker.extract_lexical_declarations(&tokens);
        walker.extract_member_accesses(&tokens);
        walker.usages.sort_unstable_by(|left, right| {
            left.location
                .range
                .start
                .cmp(&right.location.range.start)
                .then_with(|| left.location.range.end.cmp(&right.location.range.end))
                .then_with(|| left.fqn.cmp(&right.fqn))
        });
        walker.usages.dedup_by(|left, right| {
            left.fqn == right.fqn && left.location.range == right.location.range
        });
        walker.usages
    }
}

impl GroovyUsageWalker<'_> {
    fn walk_declarations(&mut self, node: &Node<'_>, container: Option<&str>) {
        if is_excluded_node(node.kind()) {
            self.excluded_ranges
                .push((node.start_byte(), node.end_byte()));
            return;
        }

        let owned_container;
        let container = if is_type_declaration(node.kind()) {
            owned_container = field_text(node, "name", self.source).map(|name| {
                container.map_or_else(
                    || {
                        self.imports
                            .current_package
                            .as_deref()
                            .map_or_else(|| name.to_string(), |package| format!("{package}.{name}"))
                    },
                    |outer| format!("{outer}.{name}"),
                )
            });
            if let Some(fqn) = &owned_container {
                self.containers.push(ContainerScope {
                    fqn: fqn.clone(),
                    start: node.start_byte(),
                    end: node.end_byte(),
                });
            }
            owned_container.as_deref().or(container)
        } else {
            container
        };

        self.extract_declared_types(node);
        self.collect_binding(node);
        if node.kind() == "method_invocation" && node.child_by_field_name("object").is_none() {
            self.extract_bare_call(node, container);
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk_declarations(&child, container);
        }
    }

    fn extract_declared_types(&mut self, node: &Node<'_>) {
        match node.kind() {
            "formal_parameter" | "field_declaration" | "local_variable_declaration" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    self.emit_type_tree(&type_node);
                }
            }
            "method_declaration" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    self.emit_type_tree(&type_node);
                }
            }
            "superclass" | "super_interfaces" | "extends_interfaces" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.emit_type_tree(&child);
                }
            }
            "object_creation_expression" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    self.emit_type_tree(&type_node);
                }
            }
            _ => {}
        }
    }

    fn emit_type_tree(&mut self, node: &Node<'_>) {
        if matches!(node.kind(), "type_identifier" | "scoped_type_identifier") {
            let Ok(name) = node.utf8_text(self.source.as_bytes()) else {
                return;
            };
            if let Some(fqn) = resolve_type_name(name, &self.resolver) {
                if let Some(symbol) = self.resolver.get_symbol(&fqn) {
                    emit_symbol(&mut self.usages, self.file_id, node, symbol);
                }
            }
            return;
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.emit_type_tree(&child);
        }
    }

    fn collect_binding(&mut self, node: &Node<'_>) {
        match node.kind() {
            "formal_parameter" => {
                let Some(name) = field_text(node, "name", self.source) else {
                    return;
                };
                let type_fqn = node
                    .child_by_field_name("type")
                    .and_then(|type_node| type_node.utf8_text(self.source.as_bytes()).ok())
                    .and_then(|raw| resolve_type_name(raw, &self.resolver));
                let scope = nearest_ancestor(node, |kind| {
                    matches!(
                        kind,
                        "method_declaration" | "constructor_declaration" | "closure"
                    )
                })
                .or_else(|| nearest_ancestor(node, is_type_declaration));
                if let Some(scope) = scope {
                    self.bindings.push(Binding {
                        name: name.to_string(),
                        type_fqn,
                        scope_start: scope.start_byte(),
                        scope_end: scope.end_byte(),
                        visible_from: scope.start_byte(),
                    });
                }
            }
            "local_variable_declaration" => {
                let explicit_type = node
                    .child_by_field_name("type")
                    .and_then(|type_node| type_node.utf8_text(self.source.as_bytes()).ok())
                    .and_then(|raw| resolve_type_name(raw, &self.resolver));
                let Some(scope) = nearest_ancestor(node, |kind| kind == "block") else {
                    return;
                };
                for declarator in children_by_field(node, "declarator") {
                    let Some(name) = field_text(&declarator, "name", self.source) else {
                        continue;
                    };
                    let inferred_type = declarator
                        .child_by_field_name("value")
                        .filter(|value| value.kind() == "object_creation_expression")
                        .and_then(|value| value.child_by_field_name("type"))
                        .and_then(|type_node| type_node.utf8_text(self.source.as_bytes()).ok())
                        .and_then(|raw| resolve_type_name(raw, &self.resolver));
                    self.bindings.push(Binding {
                        name: name.to_string(),
                        type_fqn: explicit_type.clone().or(inferred_type),
                        scope_start: scope.start_byte(),
                        scope_end: scope.end_byte(),
                        visible_from: declarator.end_byte(),
                    });
                }
            }
            "field_declaration" => {
                let type_fqn = node
                    .child_by_field_name("type")
                    .and_then(|type_node| type_node.utf8_text(self.source.as_bytes()).ok())
                    .and_then(|raw| resolve_type_name(raw, &self.resolver));
                let Some(scope) = nearest_ancestor(node, is_type_declaration) else {
                    return;
                };
                for declarator in children_by_field(node, "declarator") {
                    if let Some(name) = field_text(&declarator, "name", self.source) {
                        self.bindings.push(Binding {
                            name: name.to_string(),
                            type_fqn: type_fqn.clone(),
                            scope_start: scope.start_byte(),
                            scope_end: scope.end_byte(),
                            visible_from: scope.start_byte(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn extract_bare_call(&mut self, node: &Node<'_>, container: Option<&str>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(self.source.as_bytes()) else {
            return;
        };
        let member_tier =
            container.filter(|owner| self.resolver.has_callable_member_named(owner, name));
        let symbol = if let Some(owner) = member_tier {
            // Groovy argument typing is intentionally not guessed here. A
            // descriptorless call is navigable only when the effective member
            // set contains one declaration; overloads remain ambiguous.
            resolve_method(owner, name, &self.resolver)
        } else {
            let package_candidates = self
                .imports
                .current_package
                .as_deref()
                .map(|package| self.resolver.find_methods_by_name(package, name))
                .unwrap_or_default();
            if !package_candidates.is_empty() {
                (package_candidates.len() == 1).then(|| package_candidates[0].clone())
            } else {
                let mut candidates = self.resolver.resolve_static_callables(name);
                if let Some(imported) = self.imports.explicit.get(name) {
                    if let Some((owner, declared_name)) = imported.rsplit_once('.') {
                        candidates.extend(self.resolver.find_methods_by_name(owner, declared_name));
                    }
                }
                candidates.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
                candidates.dedup_by(|left, right| left.fqn == right.fqn);
                (candidates.len() == 1).then(|| candidates[0].clone())
            }
        };
        if let Some(symbol) = symbol {
            emit_symbol(&mut self.usages, self.file_id, &name_node, symbol);
        }
    }

    fn normalize_excluded_ranges(&mut self) {
        self.excluded_ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(self.excluded_ranges.len());
        for (start, end) in self.excluded_ranges.drain(..) {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            merged.push((start, end));
        }
        self.excluded_ranges = merged;
    }

    fn extract_lexical_declarations(&mut self, tokens: &[Token]) {
        let parens = matching_pairs(tokens, Punctuation::LeftParen, Punctuation::RightParen);
        let braces = matching_pairs(tokens, Punctuation::LeftBrace, Punctuation::RightBrace);
        let mut parameter_type_tokens = HashSet::new();

        // A declaration-like `name(...) { ... }` gives its typed parameters
        // the following body as scope. Calls followed by trailing closures can
        // also have this shape, but their argument lists do not contain
        // declaration pairs (`IndexedType identifier`), so no binding is made.
        for (open_index, close_index) in parens.iter().enumerate().filter_map(|(open, close)| {
            close
                .map(|close| (open, close))
                .filter(|_| open > 1 && matches!(tokens[open - 1], Token::Identifier(_)))
        }) {
            let Token::Identifier(return_type) = tokens[open_index - 2] else {
                continue;
            };
            let return_type_text = &self.source[return_type.start..return_type.end];
            if !is_builtin_type_spelling(return_type_text)
                && !return_type_text
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_uppercase())
                && self.resolve_type_token(&return_type).is_none()
            {
                continue;
            }
            let Some(body_open) = tokens
                .get(close_index + 1)
                .filter(|token| token.is_punctuation(Punctuation::LeftBrace))
                .map(|_| close_index + 1)
            else {
                continue;
            };
            let Some(body_close) = braces.get(body_open).copied().flatten() else {
                continue;
            };
            let (scope_start, scope_end) = (tokens[body_open].start(), tokens[body_close].end());
            let mut index = open_index + 1;
            while index + 1 < close_index {
                let (Token::Identifier(type_token), Token::Identifier(name_token)) =
                    (tokens[index], tokens[index + 1])
                else {
                    index += 1;
                    continue;
                };
                if let Some((type_fqn, symbol)) = self.resolve_type_token(&type_token) {
                    parameter_type_tokens.insert(index);
                    emit_symbol_range(
                        &mut self.usages,
                        self.file_id,
                        type_token.start,
                        type_token.end,
                        symbol,
                    );
                    self.bindings.push(Binding {
                        name: self.source[name_token.start..name_token.end].to_string(),
                        type_fqn: Some(type_fqn),
                        scope_start,
                        scope_end,
                        visible_from: scope_start,
                    });
                    index += 2;
                } else {
                    index += 1;
                }
            }
        }

        // Supplement recovery-damaged CSTs with simple `Type name` facts.
        // This covers fields, return types, and explicitly typed locals. A
        // type followed by a method name and `(` is a return type, not a
        // variable binding.
        for index in 0..tokens.len().saturating_sub(1) {
            if parameter_type_tokens.contains(&index) {
                continue;
            }
            let (Token::Identifier(type_token), Token::Identifier(name_token)) =
                (tokens[index], tokens[index + 1])
            else {
                continue;
            };
            let Some((type_fqn, symbol)) = self.resolve_type_token(&type_token) else {
                continue;
            };
            emit_symbol_range(
                &mut self.usages,
                self.file_id,
                type_token.start,
                type_token.end,
                symbol,
            );
            if tokens
                .get(index + 2)
                .is_some_and(|token| token.is_punctuation(Punctuation::LeftParen))
            {
                continue;
            }
            let (scope_start, scope_end) =
                enclosing_brace_scope(tokens, &braces, index).unwrap_or((0, self.source.len()));
            self.bindings.push(Binding {
                name: self.source[name_token.start..name_token.end].to_string(),
                type_fqn: Some(type_fqn),
                scope_start,
                scope_end,
                visible_from: name_token.end,
            });
        }

        // Groovy's `def value = new Type()` carries its useful type only in
        // the initializer. Record both the constructor type usage and the
        // inferred binding even when error recovery hid the CST declaration.
        for index in 0..tokens.len().saturating_sub(4) {
            let (
                Token::Identifier(def_token),
                Token::Identifier(name_token),
                equals,
                Token::Identifier(new_token),
                Token::Identifier(type_token),
            ) = (
                tokens[index],
                tokens[index + 1],
                tokens[index + 2],
                tokens[index + 3],
                tokens[index + 4],
            )
            else {
                continue;
            };
            if &self.source[def_token.start..def_token.end] != "def"
                || !equals.is_punctuation(Punctuation::Equals)
                || &self.source[new_token.start..new_token.end] != "new"
            {
                continue;
            }
            let Some((type_fqn, symbol)) = self.resolve_type_token(&type_token) else {
                continue;
            };
            emit_symbol_range(
                &mut self.usages,
                self.file_id,
                type_token.start,
                type_token.end,
                symbol,
            );
            let (scope_start, scope_end) =
                enclosing_brace_scope(tokens, &braces, index).unwrap_or((0, self.source.len()));
            self.bindings.push(Binding {
                name: self.source[name_token.start..name_token.end].to_string(),
                type_fqn: Some(type_fqn),
                scope_start,
                scope_end,
                visible_from: name_token.end,
            });
        }
    }

    fn resolve_type_token(
        &self,
        token: &IdentifierToken,
    ) -> Option<(String, std::sync::Arc<crate::Symbol>)> {
        let fqn = resolve_type_name(&self.source[token.start..token.end], &self.resolver)?;
        let symbol = self.resolver.get_symbol(&fqn)?;
        Some((fqn, symbol))
    }

    fn extract_member_accesses(&mut self, tokens: &[Token]) {
        for (index, window) in tokens.windows(3).enumerate() {
            let [Token::Identifier(receiver), operator, Token::Identifier(member)] = window else {
                continue;
            };
            if !operator.is_member_access() {
                continue;
            }
            let Some(receiver_fqn) = self.resolve_receiver(receiver) else {
                continue;
            };
            let is_call = tokens
                .get(index + 3)
                .is_some_and(|token| token.is_punctuation(Punctuation::LeftParen));
            let member_name = &self.source[member.start..member.end];
            let symbol = if is_call {
                resolve_method(&receiver_fqn, member_name, &self.resolver)
            } else {
                resolve_field(&receiver_fqn, member_name, &self.resolver)
            };
            if let Some(symbol) = symbol {
                emit_symbol_range(
                    &mut self.usages,
                    self.file_id,
                    member.start,
                    member.end,
                    symbol,
                );
            }
        }
    }

    fn resolve_receiver(&self, token: &IdentifierToken) -> Option<String> {
        let name = &self.source[token.start..token.end];
        let binding = self
            .bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && binding.scope_start <= token.start
                    && token.start <= binding.scope_end
                    && binding.visible_from <= token.start
            })
            .max_by_key(|binding| (binding.scope_start, binding.visible_from));
        if let Some(binding) = binding {
            return binding.type_fqn.clone();
        }
        if name == "this" {
            return self
                .containers
                .iter()
                .filter(|scope| scope.start <= token.start && token.start <= scope.end)
                .max_by_key(|scope| scope.start)
                .map(|scope| scope.fqn.clone());
        }
        resolve_type_name(name, &self.resolver)
    }
}

fn is_type_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration" | "interface_declaration" | "enum_declaration"
    )
}

fn is_builtin_type_spelling(name: &str) -> bool {
    matches!(
        name,
        "def"
            | "void"
            | "boolean"
            | "byte"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
    )
}

fn is_excluded_node(kind: &str) -> bool {
    kind == "import_declaration"
        || kind == "package_declaration"
        || kind.contains("comment")
        || kind.contains("string")
        || matches!(kind, "gstring" | "regex" | "slashy_string")
}

fn nearest_ancestor<'tree>(
    node: &Node<'tree>,
    predicate: impl Fn(&str) -> bool,
) -> Option<Node<'tree>> {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if predicate(ancestor.kind()) {
            return Some(ancestor);
        }
        current = ancestor.parent();
    }
    None
}

fn children_by_field<'tree>(node: &Node<'tree>, field: &str) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.children_by_field_name(field, &mut cursor).collect()
}

fn field_text<'a>(node: &Node<'_>, field: &str, source: &'a str) -> Option<&'a str> {
    node.child_by_field_name(field)?
        .utf8_text(source.as_bytes())
        .ok()
}
