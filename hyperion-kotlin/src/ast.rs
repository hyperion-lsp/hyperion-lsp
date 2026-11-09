// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// AST helper types for typed access to syntax tree nodes
///
/// These types wrap SyntaxNode and provide convenient access to specific node types.
use crate::{SyntaxKind, SyntaxNode};
use rowan::ast::AstNode;

/// Function declaration node
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunDecl(SyntaxNode);

impl AstNode for FunDecl {
    type Language = crate::syntax::KotlinLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FUNCTION_DECL
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::FUNCTION_DECL {
            Some(FunDecl(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Class declaration node
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassDecl(SyntaxNode);

impl AstNode for ClassDecl {
    type Language = crate::syntax::KotlinLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CLASS_DECL
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::CLASS_DECL {
            Some(ClassDecl(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Property declaration node (val/var)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PropertyDecl(SyntaxNode);

impl AstNode for PropertyDecl {
    type Language = crate::syntax::KotlinLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PROPERTY_DECL
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::PROPERTY_DECL {
            Some(PropertyDecl(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Object declaration node
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectDecl(SyntaxNode);

impl AstNode for ObjectDecl {
    type Language = crate::syntax::KotlinLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::OBJECT_DECL
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::OBJECT_DECL {
            Some(ObjectDecl(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}
