// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lexical-context queries over the syntax tree shared by the compile checks.

use hyperion_parser::{SyntaxKind, SyntaxNode};

/// True for the declaration kinds that introduce a named type.
pub const fn is_type_declaration(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::ANNOTATION_DECL
    )
}

/// Simple name of a type declaration node.
pub fn type_declaration_name(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind().is_type_identifier())
        .map(|token| token.text().to_owned())
}

/// FQNs of the types lexically enclosing `node`, nearest first, assuming every
/// enclosing type is a member of the next (no local or anonymous classes; see
/// [`is_in_local_or_anonymous_type`]).
pub fn lexical_type_fqns(node: &SyntaxNode, package: Option<&str>) -> Vec<String> {
    let mut names: Vec<String> = node
        .ancestors()
        .filter(|ancestor| is_type_declaration(ancestor.kind()))
        .filter_map(|ancestor| type_declaration_name(&ancestor))
        .collect();
    if names.is_empty() {
        return Vec::new();
    }
    names.reverse();
    let mut fqn = package.unwrap_or_default().to_owned();
    let mut owners = Vec::with_capacity(names.len());
    for name in names {
        if !fqn.is_empty() {
            fqn.push('.');
        }
        fqn.push_str(&name);
        owners.push(fqn.clone());
    }
    owners.reverse();
    owners
}

/// True inside an anonymous class body or a class declared in a method body.
/// Their FQNs are not derivable from names, and they inherit members the
/// enclosing declaration's view does not include.
pub fn is_in_local_or_anonymous_type(node: &SyntaxNode) -> bool {
    node.ancestors().any(|ancestor| match ancestor.kind() {
        SyntaxKind::NEW_EXPR => ancestor
            .children_with_tokens()
            .any(|element| element.kind() == SyntaxKind::L_BRACE),
        // A type declared inside a method, constructor, initializer or lambda.
        kind if is_type_declaration(kind) => ancestor.ancestors().skip(1).any(|outer| {
            matches!(
                outer.kind(),
                SyntaxKind::METHOD_DECL
                    | SyntaxKind::CONSTRUCTOR_DECL
                    | SyntaxKind::STATIC_INIT
                    | SyntaxKind::INSTANCE_INIT
                    | SyntaxKind::LAMBDA_EXPR
                    | SyntaxKind::BLOCK_STMT
            )
        }),
        _ => false,
    })
}
