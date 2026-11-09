// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `cannot assign a value to final variable x` (JLS 4.12.4, 16).
//!
//! Only assignments whose illegality does not depend on definite-assignment
//! analysis are reported: a `final` variable that already has an initializer,
//! a `final` parameter, and a `final` field assigned from an ordinary method.
//! A blank `final` local or a blank `final` field assigned in a constructor
//! or initializer may be legal and is never judged.

use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::{SyntaxKind, SyntaxNode};

use crate::file_semantics::FileSemantics;
use crate::resolved_name::ResolvedName;
use crate::semantic_error::{SemanticError, SemanticErrorKind};

/// Check every assignment and increment in the file.
pub fn check_final_assignments(
    root: &SyntaxNode,
    file_semantics: &FileSemantics,
) -> Vec<SemanticError> {
    let mut errors = Vec::new();
    for node in root.descendants() {
        let target = match node.kind() {
            SyntaxKind::ASSIGNMENT_EXPR => node
                .prev_sibling()
                .filter(|sibling| is_assignable_expression(sibling.kind())),
            SyntaxKind::UNARY_EXPR => {
                let increments = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .any(|token| {
                        matches!(
                            token.kind(),
                            SyntaxKind::PLUS_PLUS | SyntaxKind::MINUS_MINUS
                        )
                    });
                if increments {
                    node.children()
                        .find(|child| is_assignable_expression(child.kind()))
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(target) = target else { continue };
        if let Some(error) = check_target(root, file_semantics, &target) {
            errors.push(error);
        }
    }
    errors
}

const fn is_assignable_expression(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR)
}

fn check_target(
    root: &SyntaxNode,
    file_semantics: &FileSemantics,
    target: &SyntaxNode,
) -> Option<SemanticError> {
    // `x` or `this.x`; other receivers name a different object's field.
    let ident = match target.kind() {
        SyntaxKind::NAME_EXPR => target
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind().is_identifier())?,
        SyntaxKind::FIELD_ACCESS_EXPR => {
            let receiver = target.children().next()?;
            if receiver.kind() != SyntaxKind::THIS_EXPR || receiver.children().next().is_some() {
                return None;
            }
            target
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|token| token.kind().is_identifier())
                .last()?
        }
        _ => return None,
    };
    let (name, location, is_field) = match file_semantics.resolve_at(ident.text_range())? {
        ResolvedName::LocalVariable {
            name,
            location: Some(location),
            ..
        } => (name, location, false),
        ResolvedName::Field {
            name,
            location: Some(location),
            ..
        } => (name, location, true),
        _ => return None,
    };
    let offset = rowan::TextSize::from(location.range.start);
    let declaring_token = root.token_at_offset(offset).right_biased()?;
    // The location must be this file's own declaring identifier.
    if declaring_token.text() != name {
        return None;
    }
    let declaration = declaring_token.parent_ancestors().find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::FIELD_DECL | SyntaxKind::PARAM
        )
    })?;

    let message = |is_parameter: bool| {
        SemanticError::new(
            target.text_range(),
            SemanticErrorKind::FinalAssignment {
                name: name.clone(),
                is_parameter,
            },
        )
    };

    match declaration.kind() {
        SyntaxKind::PARAM => {
            // Lambda parameters and catch parameters share PARAM-like shapes; a
            // plain `final` on a method parameter is what is judged.
            if !is_field
                && declaration_modifiers(&declaration).is_final()
                && declaration
                    .ancestors()
                    .nth(2)
                    .is_some_and(|owner| owner.kind() == SyntaxKind::METHOD_DECL)
            {
                return Some(message(true));
            }
            None
        }
        SyntaxKind::LOCAL_VAR_DECL => {
            if !is_field
                && declares_final(&declaration)
                && has_initializer(&declaration, &declaring_token)
            {
                return Some(message(false));
            }
            None
        }
        SyntaxKind::FIELD_DECL if is_field => {
            if !declares_final(&declaration) {
                return None;
            }
            let owner = declaration.ancestors().skip(1).find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    SyntaxKind::CLASS_DECL
                        | SyntaxKind::INTERFACE_DECL
                        | SyntaxKind::ENUM_DECL
                        | SyntaxKind::RECORD_DECL
                )
            })?;
            if matches!(
                owner.kind(),
                SyntaxKind::RECORD_DECL | SyntaxKind::INTERFACE_DECL
            ) {
                return None;
            }
            if has_initializer(&declaration, &declaring_token) {
                return Some(message(false));
            }
            // A blank final field may only be assigned by its own class's
            // constructors and initializers; from an ordinary method of that
            // class it is an error.
            let member = target.ancestors().find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    SyntaxKind::METHOD_DECL
                        | SyntaxKind::CONSTRUCTOR_DECL
                        | SyntaxKind::STATIC_INIT
                        | SyntaxKind::INSTANCE_INIT
                        | SyntaxKind::LAMBDA_EXPR
                        | SyntaxKind::FIELD_DECL
                )
            })?;
            let same_class = member
                .ancestors()
                .skip(1)
                .find(|ancestor| {
                    matches!(
                        ancestor.kind(),
                        SyntaxKind::CLASS_DECL
                            | SyntaxKind::INTERFACE_DECL
                            | SyntaxKind::ENUM_DECL
                            | SyntaxKind::RECORD_DECL
                            | SyntaxKind::NEW_EXPR
                    )
                })
                .is_some_and(|enclosing| enclosing == owner);
            if member.kind() == SyntaxKind::METHOD_DECL && same_class {
                return Some(message(false));
            }
            None
        }
        _ => None,
    }
}

fn declares_final(declaration: &SyntaxNode) -> bool {
    declaration_modifiers(declaration).is_final()
        || declaration
            .children_with_tokens()
            .any(|element| element.kind() == SyntaxKind::FINAL_KW)
}

/// Does the declarator that introduces `name_token` carry an initializer?
fn has_initializer(
    declaration: &SyntaxNode,
    name_token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
) -> bool {
    let mut tokens = declaration
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| !token.kind().is_trivia())
        .skip_while(|token| token.text_range() != name_token.text_range());
    tokens.next();
    tokens
        .next()
        .is_some_and(|token| token.kind() == SyntaxKind::EQ)
}
