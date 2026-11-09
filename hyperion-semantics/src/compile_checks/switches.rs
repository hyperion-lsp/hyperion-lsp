// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `the switch expression does not cover all possible input values`
//! (JLS 14.11.1.1, 15.28.1).
//!
//! A switch expression -- and a switch statement that uses patterns or
//! `case null` -- must be exhaustive. Exhaustiveness is decided for enums and
//! sealed hierarchies from the index, and for open types (`String`, `int`,
//! ...) by the absence of a `default`. Anything not provable (an unknown
//! selector type, record patterns with real sub-patterns, an incomplete
//! hierarchy) is left alone.

use std::collections::HashSet;

use hyperion_parser::{SyntaxKind, SyntaxNode};
use lsp_types::SymbolKind;

use super::facts::TypeFacts;
use crate::file_semantics::FileSemantics;
use crate::resolved_name::ResolvedName;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::Type;

/// Check every switch in the file.
pub fn check_switch_exhaustiveness(
    root: &SyntaxNode,
    file_semantics: &FileSemantics,
    facts: &TypeFacts<'_>,
) -> Vec<SemanticError> {
    let mut errors = Vec::new();
    for node in root.descendants() {
        if matches!(
            node.kind(),
            SyntaxKind::SWITCH_EXPR | SyntaxKind::SWITCH_STMT
        ) {
            if let Some(error) = check_switch(&node, file_semantics, facts) {
                errors.push(error);
            }
        }
    }
    errors
}

/// What the case labels of one switch contribute.
#[derive(Default)]
struct Labels {
    has_default: bool,
    has_patterns: bool,
    has_null: bool,
    /// Unqualified/qualified enum constant names.
    constants: HashSet<String>,
    /// Types covered by unguarded type patterns (FQNs).
    covered_types: HashSet<String>,
    /// A pattern this analysis cannot interpret was seen.
    opaque: bool,
    /// A constant label of a non-enum kind (`case 1`, `case "a"`).
    has_literals: bool,
}

fn check_switch(
    node: &SyntaxNode,
    file_semantics: &FileSemantics,
    facts: &TypeFacts<'_>,
) -> Option<SemanticError> {
    let is_expression = node.kind() == SyntaxKind::SWITCH_EXPR;
    let selector = node.children().find(|child| {
        !matches!(
            child.kind(),
            SyntaxKind::CASE_CLAUSE | SyntaxKind::BLOCK_STMT
        )
    })?;
    let labels = collect_labels(node, file_semantics)?;
    if labels.has_default || labels.opaque {
        return None;
    }
    // A statement switch over legacy constants need not be exhaustive.
    if !is_expression && !labels.has_patterns && !labels.has_null {
        return None;
    }

    let selector_type = file_semantics.expression_type_at(selector.text_range())?;
    let exhaustive = match selector_type {
        Type::Class(class) => {
            let fqn = class.raw_qualified_name();
            // `Object` is what inference falls back to; a pattern switch over a
            // sealed type mis-typed as `Object` must not become an error.
            if fqn == "java.lang.Object" {
                return None;
            }
            let symbol = facts.type_symbol(&fqn)?;
            if facts.is_foreign_language_type(&fqn) {
                return None;
            }
            if labels.has_patterns {
                covers_type(facts, &fqn, &labels.covered_types, 0)?
            } else {
                // Constant labels only: exhaustiveness is knowable for source
                // enums (constant list) and for the open constant types.
                if symbol.kind == SymbolKind::ENUM {
                    covers_enum(facts, &fqn, &labels)?
                } else if matches!(
                    fqn.as_str(),
                    "java.lang.String"
                        | "java.lang.Integer"
                        | "java.lang.Long"
                        | "java.lang.Short"
                        | "java.lang.Byte"
                        | "java.lang.Character"
                ) {
                    false
                } else {
                    return None;
                }
            }
        }
        // `switch (int)` over constants can never be exhaustive without `default`.
        Type::Primitive(primitive)
            if *primitive != crate::symbol::PrimitiveType::Boolean && !labels.has_patterns =>
        {
            false
        }
        _ => return None,
    };
    if exhaustive {
        return None;
    }
    let keyword = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind() == SyntaxKind::SWITCH_KW)?;
    Some(SemanticError::new(
        keyword.text_range(),
        SemanticErrorKind::NonExhaustiveSwitch { is_expression },
    ))
}

fn collect_labels(node: &SyntaxNode, file_semantics: &FileSemantics) -> Option<Labels> {
    let mut labels = Labels::default();
    for clause in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::CASE_CLAUSE)
    {
        let guarded = clause
            .children()
            .any(|child| child.kind() == SyntaxKind::WHEN_CLAUSE);
        let mut children = clause.children().peekable();
        while let Some(child) = children.next() {
            match child.kind() {
                SyntaxKind::TYPE_PATTERN => {
                    labels.has_patterns = true;
                    // `case Add(Expr l, var r)` is a TYPE_PATTERN for `Add`
                    // followed by a RECORD_PATTERN of components.
                    let record_components =
                        children.next_if(|next| next.kind() == SyntaxKind::RECORD_PATTERN);
                    let fqn = pattern_type_fqn(&child, file_semantics);
                    if guarded {
                        continue;
                    }
                    match (fqn, record_components) {
                        (Some(fqn), None) => {
                            labels.covered_types.insert(fqn);
                        }
                        (Some(fqn), Some(components)) => {
                            if components.descendants().all(|descendant| {
                                descendant.kind() != SyntaxKind::TYPE_PATTERN
                                    || descendant
                                        .children()
                                        .any(|c| c.kind() == SyntaxKind::VAR_TYPE)
                            }) && components
                                .descendants()
                                .all(|d| d.kind() != SyntaxKind::RECORD_PATTERN)
                            {
                                labels.covered_types.insert(fqn);
                            } else {
                                labels.opaque = true;
                            }
                        }
                        (None, _) => labels.opaque = true,
                    }
                }
                SyntaxKind::RECORD_PATTERN => {
                    labels.has_patterns = true;
                    labels.opaque = true;
                }
                SyntaxKind::NAME_EXPR => {
                    if let Some(name) = last_identifier(&child) {
                        labels.constants.insert(name);
                    }
                }
                SyntaxKind::FIELD_ACCESS_EXPR => {
                    if let Some(name) = last_identifier(&child) {
                        labels.constants.insert(name);
                    }
                }
                SyntaxKind::LITERAL_EXPR => {
                    if child.text() == "null" {
                        labels.has_null = true;
                    } else {
                        labels.has_literals = true;
                    }
                }
                _ => {}
            }
        }
        if clause
            .children_with_tokens()
            .any(|element| element.kind() == SyntaxKind::DEFAULT_KW)
        {
            labels.has_default = true;
        }
    }
    Some(labels)
}

fn last_identifier(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .last()
        .map(|token| token.text().to_owned())
}

fn pattern_type_fqn(pattern: &SyntaxNode, file_semantics: &FileSemantics) -> Option<String> {
    let class_type = pattern
        .children()
        .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)?;
    let (_, ident_range) = crate::utils::extract_class_type_simple_name(&class_type)?;
    match file_semantics.resolve_at(ident_range)? {
        ResolvedName::Type { fqn, .. } => Some(fqn.clone()),
        _ => None,
    }
}

/// `Some(true)` when every constant of the enum is named.
fn covers_enum(facts: &TypeFacts<'_>, enum_fqn: &str, labels: &Labels) -> Option<bool> {
    // Bytecode enums index their constants as plain static fields, so only
    // enums declared in source have a knowable constant list.
    let location = facts.index().find_symbol(enum_fqn)?;
    if !facts.index().is_workspace_file(location.file_id) {
        return None;
    }
    let constants: Vec<String> = facts
        .index()
        .get_class_members(enum_fqn)
        .iter()
        .filter(|member| member.kind == SymbolKind::ENUM_MEMBER)
        .map(|member| member.simple_name.to_string())
        .collect();
    if constants.is_empty() {
        return None;
    }
    Some(
        constants
            .iter()
            .all(|constant| labels.constants.contains(constant)),
    )
}

/// Is every value of `fqn` matched by one of the covered types?
///
/// `None` when the hierarchy cannot be judged.
fn covers_type(
    facts: &TypeFacts<'_>,
    fqn: &str,
    covered: &HashSet<String>,
    depth: usize,
) -> Option<bool> {
    if depth > 16 {
        return None;
    }
    for candidate in covered {
        if facts.is_subtype(fqn, candidate)? {
            return Some(true);
        }
    }
    let symbol = facts.type_symbol(fqn)?;
    if !symbol.modifiers.is_sealed {
        // Not sealed and not matched by a supertype pattern: values of this
        // type escape the switch. (Enums handled by the caller.)
        return Some(false);
    }
    let permitted = facts.index().get_sealed_subclasses(fqn);
    if permitted.is_empty() {
        return None;
    }
    // A sealed *class* that is not abstract can itself be instantiated.
    if symbol.kind == SymbolKind::CLASS && !symbol.modifiers.is_abstract {
        return Some(false);
    }
    for subtype in permitted {
        if !covers_type(facts, &subtype, covered, depth + 1)? {
            return Some(false);
        }
    }
    Some(true)
}
