// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Simple expression type inference
//!
//! Handles type inference for simple, self-contained expressions:
//! - `this` keyword
//! - `super` keyword
//! - Parenthesized expressions: `(expr)`
//! - Cast expressions: `(Type) expr`
//! - New expressions: `new Type(...)`

use crate::inference::visitor::InferType;
use crate::inference::TypeInferenceEngine;
use crate::resolver::SymbolResolver;
use crate::symbol::Type;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Type-safe wrapper for `this` keyword
///
/// # Example
///
/// ```java
/// class User {
///     void method() {
///         this.name = "John";  // `this` has type User
///     }
/// }
/// ```
pub struct ThisExpr<'a> {
    node: &'a SyntaxNode,
}

impl<'a> ThisExpr<'a> {
    pub fn cast(node: &'a SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::THIS_EXPR {
            Some(Self { node })
        } else {
            None
        }
    }
}

impl InferType for ThisExpr<'_> {
    fn infer_type(
        &self,
        _engine: &TypeInferenceEngine,
        resolver: &SymbolResolver,
        _expected_type: Option<&Type>,
    ) -> Type {
        resolver
            .get_enclosing_class_at_offset(u32::from(self.node.text_range().start()))
            .map_or(Type::Unknown, Type::Class)
    }
}

/// Type-safe wrapper for `super` keyword
///
/// # Example
///
/// ```java
/// class Child extends Parent {
///     void method() {
///         super.parentMethod();  // `super` has type Parent
///     }
/// }
/// ```
pub struct SuperExpr<'a> {
    node: &'a SyntaxNode,
}

impl<'a> SuperExpr<'a> {
    pub fn cast(node: &'a SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::SUPER_EXPR {
            Some(Self { node })
        } else {
            None
        }
    }
}

impl InferType for SuperExpr<'_> {
    fn infer_type(
        &self,
        _engine: &TypeInferenceEngine,
        resolver: &SymbolResolver,
        _expected_type: Option<&Type>,
    ) -> Type {
        resolver
            .get_super_class_at_offset(u32::from(self.node.text_range().start()))
            .map_or(Type::Unknown, Type::Class)
    }
}

/// Type-safe wrapper for parenthesized expressions
///
/// # Example
///
/// ```java
/// int x = (1 + 2);  // Parenthesized expression, type is int
/// ```
pub struct ParenExpr<'a> {
    node: &'a SyntaxNode,
}

impl<'a> ParenExpr<'a> {
    pub fn cast(node: &'a SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::PAREN_EXPR {
            Some(Self { node })
        } else {
            None
        }
    }

    /// Get the inner expression (unwrap parentheses)
    pub fn inner_expr(&self) -> Option<SyntaxNode> {
        self.node.children().find(|child| {
            !matches!(
                child.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::L_PAREN | SyntaxKind::R_PAREN
            )
        })
    }
}

impl InferType for ParenExpr<'_> {
    fn infer_type(
        &self,
        engine: &TypeInferenceEngine,
        resolver: &SymbolResolver,
        expected_type: Option<&Type>,
    ) -> Type {
        // Unwrap parentheses and infer the inner expression
        self.inner_expr().map_or(Type::Unknown, |inner| {
            super::super::expressions::infer_expression(engine, &inner, resolver, expected_type)
        })
    }
}

/// Type-safe wrapper for cast expressions
///
/// # Example
///
/// ```java
/// String s = (String) obj;  // Cast to String
/// ```
pub struct CastExpr<'a> {
    node: &'a SyntaxNode,
}

impl<'a> CastExpr<'a> {
    pub fn cast(node: &'a SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::CAST_EXPR {
            Some(Self { node })
        } else {
            None
        }
    }
}

impl InferType for CastExpr<'_> {
    fn infer_type(
        &self,
        engine: &TypeInferenceEngine,
        _resolver: &SymbolResolver,
        _expected_type: Option<&Type>,
    ) -> Type {
        engine.syntax_type_resolver.infer_expression_type(self.node)
    }
}

/// Type-safe wrapper for new expressions (object instantiation)
///
/// # Example
///
/// ```java
/// User user = new User("John");  // New expression, type is User
/// ```
pub struct NewExpr<'a> {
    node: &'a SyntaxNode,
}

impl<'a> NewExpr<'a> {
    pub fn cast(node: &'a SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::NEW_EXPR {
            Some(Self { node })
        } else {
            None
        }
    }
}

impl InferType for NewExpr<'_> {
    fn infer_type(
        &self,
        engine: &TypeInferenceEngine,
        _resolver: &SymbolResolver,
        _expected_type: Option<&Type>,
    ) -> Type {
        engine.syntax_type_resolver.infer_expression_type(self.node)
    }
}

#[cfg(test)]
mod tests {
    // Integration tests with real syntax trees are in the LSP test suite
}
