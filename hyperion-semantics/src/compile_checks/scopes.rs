// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scope rules javac enforces on locals and on static contexts.
//!
//! `variable x is already defined in method m()` (JLS 6.4) and
//! `non-static variable x cannot be referenced from a static context`
//! (JLS 8.1.3, 15.8.3).

use std::collections::HashSet;

use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::{SyntaxKind, SyntaxNode};

use crate::file_semantics::FileSemantics;
use crate::resolved_name::ResolvedName;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::utils::declarator_identifiers;

type Token = rowan::SyntaxToken<hyperion_parser::JavaLanguage>;

/// Run both scope checks over the file.
pub fn check_scopes(root: &SyntaxNode, file_semantics: &FileSemantics) -> Vec<SemanticError> {
    let mut errors = Vec::new();
    for node in root.descendants() {
        if matches!(
            node.kind(),
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
        ) {
            check_duplicate_locals(&node, &mut errors);
        }
    }
    check_static_context(root, file_semantics, &mut errors);
    errors
}

// ---------------------------------------------------------------------------
// Duplicate local variables
// ---------------------------------------------------------------------------

struct Scopes {
    stack: Vec<HashSet<String>>,
}

impl Scopes {
    fn push(&mut self) {
        self.stack.push(HashSet::new());
    }

    fn pop(&mut self) {
        self.stack.pop();
    }

    fn contains(&self, name: &str) -> bool {
        self.stack.iter().any(|scope| scope.contains(name))
    }

    fn insert(&mut self, name: &str) {
        if let Some(top) = self.stack.last_mut() {
            top.insert(name.to_owned());
        }
    }
}

fn check_duplicate_locals(method: &SyntaxNode, errors: &mut Vec<SemanticError>) {
    let Some(body) = method
        .children()
        .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
    else {
        return;
    };
    let mut scopes = Scopes { stack: Vec::new() };
    scopes.push();
    let signature = describe_member(method);
    if let Some(params) = method
        .children()
        .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
    {
        for param in params.children().filter(|c| c.kind() == SyntaxKind::PARAM) {
            if let Some(name) = param_name(&param) {
                scopes.insert(&name);
            }
        }
    }
    walk(&body, &mut scopes, &signature, errors);
}

fn param_name(param: &SyntaxNode) -> Option<String> {
    param
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .last()
        .map(|token| token.text().to_owned())
}

/// `method m(int,String)` / `constructor Main(int)` as javac words it.
fn describe_member(member: &SyntaxNode) -> String {
    let name = member
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind().is_identifier())
        .map(|token| token.text().to_owned())
        .unwrap_or_default();
    let params = member
        .children()
        .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
        .map(|list| {
            list.children()
                .filter(|child| child.kind() == SyntaxKind::PARAM)
                .map(|param| {
                    let ty = param
                        .children()
                        .find(|child| {
                            matches!(
                                child.kind(),
                                SyntaxKind::CLASS_TYPE
                                    | SyntaxKind::PRIMITIVE_TYPE
                                    | SyntaxKind::ARRAY_TYPE
                            )
                        })
                        .map(|ty| ty.text().to_string().split_whitespace().collect::<String>())
                        .unwrap_or_default();
                    let arrays = param
                        .children()
                        .filter(|child| child.kind() == SyntaxKind::ARRAY_TYPE)
                        .count();
                    let varargs = param
                        .children_with_tokens()
                        .any(|element| element.kind() == SyntaxKind::ELLIPSIS);
                    format!(
                        "{ty}{}{}",
                        if arrays > 0 && !ty.ends_with("[]") {
                            "[]".repeat(arrays)
                        } else {
                            String::new()
                        },
                        if varargs { "..." } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let kind = if member.kind() == SyntaxKind::CONSTRUCTOR_DECL {
        "constructor"
    } else {
        "method"
    };
    format!("{kind} {name}({params})")
}

fn report(token: &Token, signature: &str, errors: &mut Vec<SemanticError>) {
    errors.push(SemanticError::new(
        token.text_range(),
        SemanticErrorKind::DuplicateVariable {
            name: token.text().to_owned(),
            location: signature.to_owned(),
        },
    ));
}

fn declare(token: &Token, scopes: &mut Scopes, signature: &str, errors: &mut Vec<SemanticError>) {
    if scopes.contains(token.text()) {
        report(token, signature, errors);
    } else {
        scopes.insert(token.text());
    }
}

fn walk(node: &SyntaxNode, scopes: &mut Scopes, signature: &str, errors: &mut Vec<SemanticError>) {
    match node.kind() {
        SyntaxKind::BLOCK_STMT => {
            scopes.push();
            walk_children(node, scopes, signature, errors);
            scopes.pop();
        }
        SyntaxKind::LOCAL_VAR_DECL => {
            for name in declarator_identifiers(node) {
                declare(&name, scopes, signature, errors);
            }
            walk_children(node, scopes, signature, errors);
        }
        SyntaxKind::FOR_STMT | SyntaxKind::FOREACH_STMT => {
            scopes.push();
            if node.kind() == SyntaxKind::FOREACH_STMT {
                // `for (T x : xs)`: the variable is a direct IDENT after the type.
                let mut seen_type = false;
                for element in node.children_with_tokens() {
                    match element {
                        rowan::NodeOrToken::Node(child)
                            if matches!(
                                child.kind(),
                                SyntaxKind::CLASS_TYPE
                                    | SyntaxKind::PRIMITIVE_TYPE
                                    | SyntaxKind::VAR_TYPE
                            ) =>
                        {
                            seen_type = true;
                        }
                        rowan::NodeOrToken::Token(token)
                            if seen_type && token.kind().is_identifier() =>
                        {
                            declare(&token, scopes, signature, errors);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            walk_children(node, scopes, signature, errors);
            scopes.pop();
        }
        SyntaxKind::TRY_STMT | SyntaxKind::TRY_WITH_RESOURCES_STMT => {
            // Resources scope over the try block only; catch parameters get
            // their own scope each.
            scopes.push();
            for child in node.children() {
                match child.kind() {
                    SyntaxKind::CATCH_CLAUSE => {
                        scopes.push();
                        if let Some(parameter) = child
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .filter(|token| token.kind().is_identifier())
                            .last()
                        {
                            declare(&parameter, scopes, signature, errors);
                        }
                        walk_children(&child, scopes, signature, errors);
                        scopes.pop();
                    }
                    SyntaxKind::FINALLY_CLAUSE => {
                        // Resources are out of scope in `catch`/`finally`.
                        walk_children(&child, scopes, signature, errors);
                    }
                    _ => walk(&child, scopes, signature, errors),
                }
            }
            scopes.pop();
        }
        SyntaxKind::SWITCH_STMT | SyntaxKind::SWITCH_EXPR => {
            // An old-style switch block is one scope shared by every group.
            scopes.push();
            walk_children(node, scopes, signature, errors);
            scopes.pop();
        }
        SyntaxKind::LAMBDA_EXPR => {
            scopes.push();
            for param in node.children().filter(|c| c.kind() == SyntaxKind::PARAM) {
                if let Some(token) = param
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind().is_identifier())
                    .last()
                {
                    declare(&token, scopes, signature, errors);
                }
            }
            walk_children(node, scopes, signature, errors);
            scopes.pop();
        }
        // Types declared in a body get their own scopes and are analysed as
        // members in their own right; a class body may shadow enclosing locals.
        SyntaxKind::CLASS_DECL
        | SyntaxKind::INTERFACE_DECL
        | SyntaxKind::ENUM_DECL
        | SyntaxKind::RECORD_DECL
        | SyntaxKind::METHOD_DECL
        | SyntaxKind::FIELD_DECL
        | SyntaxKind::CONSTRUCTOR_DECL
        | SyntaxKind::STATIC_INIT
        | SyntaxKind::INSTANCE_INIT => {}
        _ => walk_children(node, scopes, signature, errors),
    }
}

fn walk_children(
    node: &SyntaxNode,
    scopes: &mut Scopes,
    signature: &str,
    errors: &mut Vec<SemanticError>,
) {
    for child in node.children() {
        // The lambda parameter list and pattern bindings are not declarations
        // this pass models; PARAM nodes under a lambda were handled above.
        if child.kind() == SyntaxKind::PARAM {
            continue;
        }
        walk(&child, scopes, signature, errors);
    }
}

// ---------------------------------------------------------------------------
// Static context
// ---------------------------------------------------------------------------

fn check_static_context(
    root: &SyntaxNode,
    file_semantics: &FileSemantics,
    errors: &mut Vec<SemanticError>,
) {
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::THIS_EXPR => {
                if node.children().next().is_some() {
                    continue; // `Outer.this`
                }
                if static_owner(&node).is_some() {
                    errors.push(SemanticError::new(
                        node.text_range(),
                        SemanticErrorKind::StaticContextReference {
                            what: "variable this".to_owned(),
                        },
                    ));
                }
            }
            SyntaxKind::NAME_EXPR => {
                if let Some(error) = static_name_error(root, file_semantics, &node) {
                    errors.push(error);
                }
            }
            _ => {}
        }
    }
}

/// The type declaration whose static context `node` is in, if any: the node's
/// nearest enclosing member is a `static` method, initializer or field.
fn static_owner(node: &SyntaxNode) -> Option<SyntaxNode> {
    let member = node.ancestors().find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::METHOD_DECL
                | SyntaxKind::CONSTRUCTOR_DECL
                | SyntaxKind::STATIC_INIT
                | SyntaxKind::INSTANCE_INIT
                | SyntaxKind::FIELD_DECL
                | SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL
        )
    })?;
    let is_static = match member.kind() {
        SyntaxKind::METHOD_DECL | SyntaxKind::FIELD_DECL => {
            declaration_modifiers(&member).is_static()
        }
        SyntaxKind::STATIC_INIT => true,
        _ => false,
    };
    if !is_static {
        return None;
    }
    let owner = member.ancestors().skip(1).find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL
                | SyntaxKind::NEW_EXPR
        )
    })?;
    // Members of anonymous classes are not judged.
    (owner.kind() != SyntaxKind::NEW_EXPR).then_some(owner)
}

fn static_name_error(
    root: &SyntaxNode,
    file_semantics: &FileSemantics,
    name_expr: &SyntaxNode,
) -> Option<SemanticError> {
    let ident = name_expr
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind().is_identifier())?;
    // `a.b`: only the leftmost simple name is an unqualified reference.
    if name_expr.parent().is_some_and(|parent| {
        matches!(parent.kind(), SyntaxKind::FIELD_ACCESS_EXPR)
            && parent
                .children()
                .next()
                .is_some_and(|first| first != *name_expr)
    }) {
        return None;
    }
    let owner = static_owner(name_expr)?;
    let (name, location, is_method) = match file_semantics.resolve_at(ident.text_range())? {
        ResolvedName::Field {
            name,
            location: Some(location),
            ..
        } => (name, location, false),
        ResolvedName::Method {
            name,
            location: Some(location),
            ..
        } => (name, location, true),
        _ => return None,
    };
    let token = root
        .token_at_offset(rowan::TextSize::from(location.range.start))
        .right_biased()?;
    if token.text() != name {
        return None;
    }
    let declaration = token.parent_ancestors().find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::FIELD_DECL | SyntaxKind::METHOD_DECL
        )
    })?;
    if (is_method && declaration.kind() != SyntaxKind::METHOD_DECL)
        || (!is_method && declaration.kind() != SyntaxKind::FIELD_DECL)
    {
        return None;
    }
    // Only members of the very class whose static context this is.
    let declaring_type = declaration.ancestors().skip(1).find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL
                | SyntaxKind::NEW_EXPR
        )
    })?;
    if declaring_type != owner
        || matches!(
            owner.kind(),
            SyntaxKind::INTERFACE_DECL | SyntaxKind::RECORD_DECL | SyntaxKind::ANNOTATION_DECL
        )
        || declaration_modifiers(&declaration).is_static()
    {
        return None;
    }
    // `f(int)` static next to `f()` instance: the resolver may have picked
    // either; only a name whose every declaration is an instance method is
    // certainly the one referenced.
    if is_method
        && declaring_type.children().any(|member| {
            member.kind() == SyntaxKind::METHOD_DECL
                && describe_member(&member) != describe_member(&declaration)
                && member
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind().is_identifier())
                    .is_some_and(|ident| ident.text() == name)
                && declaration_modifiers(&member).is_static()
        })
    {
        return None;
    }
    let what = if is_method {
        format!(
            "method {}",
            describe_member(&declaration).trim_start_matches("method ")
        )
    } else {
        format!("variable {name}")
    };
    Some(SemanticError::new(
        name_expr.text_range(),
        SemanticErrorKind::StaticContextReference { what },
    ))
}
