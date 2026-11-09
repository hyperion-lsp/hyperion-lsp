// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Identifier utilities - helpers for distinguishing declarations from usages

use crate::{import_resolver::ImportResolver, FileId, SymbolLocation, TextRange};
use hyperion_parser::syntax::SyntaxToken;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use lsp_types::SymbolKind;
use rowan::NodeOrToken;
use std::sync::Arc;

use super::{context, context::ExtractionContext, Usage};

/// Extract simple identifier usage (variables, parameters, fields)
pub fn extract_identifier_usage(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    ctx: &ExtractionContext,
    usages: &mut Vec<Usage>,
) {
    let mut identifier_tokens = Vec::new();
    let mut has_dot = false;

    for token in node.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = token {
            if t.kind() == SyntaxKind::DOT {
                has_dot = true;
            } else if t.kind().is_identifier() {
                identifier_tokens.push(t);
            }
        }
    }

    if has_dot || identifier_tokens.len() != 1 {
        return;
    }

    let ident = &identifier_tokens[0];
    if is_declaration_context(ident) {
        return;
    }

    // In the Java CST an unqualified call is `METHOD_CALL_EXPR(NAME_EXPR)`;
    // the NAME_EXPR is the callable name, not an implicit-this field read.
    // Qualified calls keep only their receiver in a NAME_EXPR and contain a
    // direct DOT, so that receiver remains eligible for value extraction.
    if node.parent().is_some_and(|parent| {
        parent.kind() == SyntaxKind::METHOD_CALL_EXPR
            && !parent
                .children_with_tokens()
                .any(|element| element.kind() == SyntaxKind::DOT)
    }) {
        return;
    }

    let name = ident.text().to_string();
    if let Some(symbol) = resolve_switch_enum_label(node, &name, resolver, ctx) {
        let range = TextRange::new(
            ident.text_range().start().into(),
            ident.text_range().end().into(),
        );
        let location = SymbolLocation {
            file_id,
            source_revision_id: None,
            range,
            kind: symbol.kind,
            name: Arc::clone(&symbol.simple_name),
            fqn: Arc::clone(&symbol.fqn),
            container_name: symbol.container.clone(),
            documentation: None,
        };
        usages.push(Usage {
            fqn: symbol.fqn.to_string(),
            location,
        });
        return;
    }

    if context::is_local_binding_visible_with_context(node, &name, ctx) {
        return;
    }

    if let Some(symbol) = ctx
        .enclosing_class_fqn
        .as_deref()
        .and_then(|owner| crate::usage_resolution::resolve_field(owner, &name, resolver))
    {
        let range = TextRange::new(
            ident.text_range().start().into(),
            ident.text_range().end().into(),
        );
        let location = SymbolLocation {
            file_id,
            source_revision_id: None,
            range,
            kind: symbol.kind,
            name: symbol.simple_name.clone(),
            fqn: symbol.fqn.clone(),
            container_name: symbol.container.clone(),
            documentation: None,
        };
        usages.push(Usage {
            fqn: symbol.fqn.to_string(),
            location,
        });
    }
}

/// Resolve a simple enum label through its switch selector's declared type.
///
/// This records the same receiver-dependent identity used by interactive
/// semantics. It deliberately fails closed for compound selector expressions;
/// a bare-name search here would merge equal constants from unrelated enums.
fn resolve_switch_enum_label(
    label: &SyntaxNode,
    name: &str,
    resolver: &ImportResolver<'_>,
    ctx: &ExtractionContext,
) -> Option<Arc<crate::Symbol>> {
    let case_clause = label
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::CASE_CLAUSE)?;
    let delimiter = case_clause
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| matches!(token.kind(), SyntaxKind::COLON | SyntaxKind::ARROW))?;
    if label.text_range().end() > delimiter.text_range().start() {
        return None;
    }

    let switch_node = case_clause.ancestors().find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::SWITCH_STMT | SyntaxKind::SWITCH_EXPR
        )
    })?;
    let first_case_start = switch_node
        .children()
        .find(|child| child.kind() == SyntaxKind::CASE_CLAUSE)
        .map_or(case_clause.text_range().start(), |child| {
            child.text_range().start()
        });
    let selector = switch_node.children().find(|child| {
        child.text_range().end() <= first_case_start
            && matches!(
                child.kind(),
                SyntaxKind::NAME_EXPR
                    | SyntaxKind::FIELD_ACCESS_EXPR
                    | SyntaxKind::METHOD_CALL_EXPR
                    | SyntaxKind::PAREN_EXPR
                    | SyntaxKind::CAST_EXPR
                    | SyntaxKind::ARRAY_ACCESS_EXPR
                    | SyntaxKind::TERNARY_EXPR
            )
    })?;
    let selector_names = selector
        .descendants()
        .filter(|candidate| candidate.kind() == SyntaxKind::NAME_EXPR)
        .collect::<Vec<_>>();
    let [selector_name] = selector_names.as_slice() else {
        return None;
    };
    let mut identifiers = selector_name
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier());
    let variable_name = identifiers.next()?.text().to_owned();
    if identifiers.next().is_some() {
        return None;
    }

    let selector_type =
        context::resolve_visible_variable_type(selector_name, &variable_name, ctx, resolver)?;
    let selector_type = super::type_spelling::parse_parameterized_type(&selector_type)?.base;
    crate::usage_resolution::resolve_field(&selector_type, name, resolver)
        .filter(|symbol| symbol.kind == SymbolKind::ENUM_MEMBER)
}

/// Check if an identifier is in a declaration context
///
/// Returns true if the identifier is:
/// - A class/interface/enum name in a type declaration
/// - A method name in a method declaration
/// - A field name in a field declaration
/// - A variable name in a local variable declaration
///
pub fn is_declaration_context(ident_token: &SyntaxToken) -> bool {
    let mut current = ident_token.parent();

    while let Some(parent) = current {
        match parent.kind() {
            // Type declarations
            SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL => {
                // Check if this IDENT is the name of the type (not a usage within the body)
                // The type name is the first IDENT child
                if let Some(NodeOrToken::Token(first_token)) =
                    parent.children_with_tokens().find(|child| {
                        if let NodeOrToken::Token(token) = child {
                            token.kind().is_type_identifier()
                        } else {
                            false
                        }
                    })
                {
                    // Check if our node is at the same position as the type name
                    if ident_token.text_range() == first_token.text_range() {
                        return true;
                    }
                }
            }

            // Method declarations
            SyntaxKind::METHOD_DECL => {
                // Check if this is the method name (comes before L_PAREN)
                for child in parent.children_with_tokens() {
                    if let NodeOrToken::Token(token) = child {
                        if token.kind() == SyntaxKind::L_PAREN {
                            // Found lparen - any IDENT before this point was the method name
                            break;
                        }
                        if token.kind().is_identifier()
                            && token.text_range() == ident_token.text_range()
                        {
                            return true;
                        }
                    }
                }
            }

            // Field declarations
            SyntaxKind::FIELD_DECL => {
                // Field name comes after the type
                // We consider it a declaration if it's a direct child IDENT
                for child in parent.children_with_tokens() {
                    if let NodeOrToken::Token(token) = child {
                        if token.kind().is_identifier()
                            && token.text_range() == ident_token.text_range()
                        {
                            // This is likely the field name
                            return true;
                        }
                    }
                }
            }

            // Variable declarations
            SyntaxKind::LOCAL_VAR_DECL => {
                // Declarator names are direct identifier tokens. Identifiers
                // nested in the initializer are ordinary reads (`int y = x`)
                // and must continue through field/local resolution.
                return parent.children_with_tokens().any(|element| {
                    element.into_token().is_some_and(|token| {
                        token.kind().is_identifier()
                            && token.text_range() == ident_token.text_range()
                    })
                });
            }

            // Enum constants
            SyntaxKind::ENUM_CONSTANT => {
                return true;
            }

            _ => {}
        }

        current = parent.parent();
    }

    false
}
