// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared Java field declarator extraction.

use hyperion_parser::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// One declarator in a Java field declaration. A declaration can introduce
/// several fields and attach additional array dimensions per declarator.
///
/// Indexing and semantic analysis share this parser so every consumer sees
/// the same names, source ranges, initializers, and per-variable dimensions.
pub struct FieldDeclarator {
    pub name: String,
    pub name_range: rowan::TextRange,
    pub initializer: Option<SyntaxNode>,
    pub extra_array_dimensions: usize,
}

pub fn field_declarators(field: &SyntaxNode) -> Vec<FieldDeclarator> {
    if field.kind() != SyntaxKind::FIELD_DECL {
        return Vec::new();
    }
    let mut declarators = Vec::new();
    let mut current = None;
    let mut after_type = false;
    let mut in_initializer = false;
    for element in field.children_with_tokens() {
        match element {
            NodeOrToken::Node(child)
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) && !after_type =>
            {
                after_type = true;
            }
            NodeOrToken::Token(token)
                if after_type
                    && current.is_none()
                    && !in_initializer
                    && token.kind().is_identifier() =>
            {
                current = Some(FieldDeclarator {
                    name: token.text().to_string(),
                    name_range: token.text_range(),
                    initializer: None,
                    extra_array_dimensions: 0,
                });
            }
            NodeOrToken::Token(token)
                if current.is_some()
                    && !in_initializer
                    && token.kind() == SyntaxKind::L_BRACKET =>
            {
                current
                    .as_mut()
                    .expect("checked declarator")
                    .extra_array_dimensions += 1;
            }
            NodeOrToken::Token(token) if current.is_some() && token.kind() == SyntaxKind::EQ => {
                in_initializer = true;
            }
            NodeOrToken::Node(child)
                if current.is_some()
                    && in_initializer
                    && field_initializer_expression(child.kind()) =>
            {
                current.as_mut().expect("checked declarator").initializer = Some(child);
            }
            NodeOrToken::Token(token)
                if matches!(token.kind(), SyntaxKind::COMMA | SyntaxKind::SEMICOLON) =>
            {
                if let Some(declarator) = current.take() {
                    declarators.push(declarator);
                }
                in_initializer = false;
            }
            _ => {}
        }
    }
    if let Some(declarator) = current {
        declarators.push(declarator);
    }
    declarators
}

const fn field_initializer_expression(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::IDENT
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::PATTERN_INSTANCEOF_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::SWITCH_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
    )
}
