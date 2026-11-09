// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Modifier and annotation lookup for declarations.
//!
//! In this CST the `MODIFIER_LIST` of a member or top-level type is the
//! *preceding sibling node* of the declaration (for parameters and local
//! variables it is a child). Every syntax check that needs `private`, `final`,
//! `static` or an annotation must find it the same way, so the lookup lives
//! here.

use crate::syntax::{SyntaxKind, SyntaxNode};

/// Modifiers and annotations written on one declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationModifiers {
    keywords: Vec<SyntaxKind>,
    annotations: Vec<String>,
}

impl DeclarationModifiers {
    /// True when the modifier keyword (`SyntaxKind::PRIVATE_KW`, ...) is written.
    #[must_use]
    pub fn has(&self, keyword: SyntaxKind) -> bool {
        self.keywords.contains(&keyword)
    }

    /// Modifier keywords in source order.
    #[must_use]
    pub fn keywords(&self) -> &[SyntaxKind] {
        &self.keywords
    }

    /// Annotation names as written (`Override`, `lombok.Getter`), without `@`.
    #[must_use]
    pub fn annotations(&self) -> &[String] {
        &self.annotations
    }

    /// True when an annotation with this simple name is written.
    #[must_use]
    pub fn has_annotation(&self, simple_name: &str) -> bool {
        self.annotations
            .iter()
            .any(|name| name.rsplit('.').next() == Some(simple_name))
    }

    #[must_use]
    pub const fn has_any_annotation(&self) -> bool {
        !self.annotations.is_empty()
    }

    #[must_use]
    pub fn is_private(&self) -> bool {
        self.has(SyntaxKind::PRIVATE_KW)
    }

    #[must_use]
    pub fn is_static(&self) -> bool {
        self.has(SyntaxKind::STATIC_KW)
    }

    #[must_use]
    pub fn is_final(&self) -> bool {
        self.has(SyntaxKind::FINAL_KW)
    }

    #[must_use]
    pub fn is_abstract(&self) -> bool {
        self.has(SyntaxKind::ABSTRACT_KW)
    }
}

/// Collect the modifiers and annotations written on `declaration`.
///
/// Works for `METHOD_DECL`, `CONSTRUCTOR_DECL`, `FIELD_DECL`, type
/// declarations, `PARAM` and `LOCAL_VAR_DECL`.
#[must_use]
pub fn declaration_modifiers(declaration: &SyntaxNode) -> DeclarationModifiers {
    // A type body is inline in its declaration node, so the first member's
    // modifier list is a *child* of the type: only parameters and locals own
    // theirs as a child.
    let owns_child_list = matches!(
        declaration.kind(),
        SyntaxKind::PARAM | SyntaxKind::LOCAL_VAR_DECL
    );
    let list = owns_child_list
        .then(|| {
            declaration
                .children()
                .find(|child| child.kind() == SyntaxKind::MODIFIER_LIST)
        })
        .flatten()
        .or_else(|| {
            declaration
                .prev_sibling()
                .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
        });
    let Some(list) = list else {
        return DeclarationModifiers::default();
    };

    let mut modifiers = DeclarationModifiers::default();
    for child in list.children() {
        match child.kind() {
            SyntaxKind::MODIFIER => {
                if let Some(token) = child.children_with_tokens().find_map(|element| {
                    element
                        .into_token()
                        .filter(|token| !token.kind().is_trivia())
                }) {
                    modifiers.keywords.push(token.kind());
                }
            }
            SyntaxKind::ANNOTATION => {
                let name = child
                    .children()
                    .find(|node| node.kind() == SyntaxKind::QUALIFIED_NAME)
                    .map(|node| node.text().to_string())
                    .unwrap_or_default();
                modifiers.annotations.push(name.trim().to_string());
            }
            _ => {}
        }
    }
    modifiers
}
