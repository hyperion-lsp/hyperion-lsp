// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Visitor trait for type inference
//!
//! This module defines the core `InferType` trait that enables a visitor pattern
//! for type inference. Each expression type implements this trait, encapsulating
//! its inference logic and making it independently testable.
//!
//! # Architecture
//!
//! Instead of a monolithic match statement that handles all expression types,
//! we use trait-based dispatch:
//!
//! ```text
//! Old: match node.kind() { ... 35+ arms ... }
//! New: MethodCallExpr::cast(node)?.infer_type(engine, resolver, expected)
//! ```
//!
//! # Benefits
//!
//! 1. **Type Safety**: Can't pass wrong SyntaxNode type to wrong handler
//! 2. **Testability**: Each expression type independently testable
//! 3. **Encapsulation**: Each type owns its extraction and inference logic
//! 4. **Extensibility**: Adding new Java features = add wrapper + impl InferType
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::inference::visitor::InferType;
//!
//! pub struct MethodCallExpr<'a>(&'a SyntaxNode);
//!
//! impl<'a> InferType for MethodCallExpr<'a> {
//!     fn infer_type(
//!         &self,
//!         engine: &TypeInferenceEngine,
//!         resolver: &SymbolResolver,
//!         _expected_type: Option<&Type>,
//!     ) -> Type {
//!         // Method call inference logic
//!         // Fully unit testable
//!     }
//! }
//! ```

use super::TypeInferenceEngine;
use crate::resolver::SymbolResolver;
use crate::symbol::Type;

/// Core trait for type inference on expressions
///
/// Each expression type implements this trait to provide its inference logic.
/// This enables:
/// - Independent testing of each expression type
/// - Type-safe dispatch (wrappers validate node kind)
/// - Clean separation of concerns
///
/// # Implementer's Guide
///
/// When implementing this trait:
///
/// 1. Create a type-safe wrapper struct (e.g., `MethodCallExpr<'a>(&'a SyntaxNode)`)
/// 2. Implement a `cast()` method that validates node kind
/// 3. Add helper methods to extract expression components
/// 4. Implement `InferType::infer_type()` with the inference logic
///
/// # Example
///
/// ```rust,ignore
/// pub struct LiteralExpr<'a> {
///     node: &'a SyntaxNode,
///     prim_type: PrimitiveType,
/// }
///
/// impl<'a> LiteralExpr<'a> {
///     pub fn cast_int(node: &'a SyntaxNode) -> Option<Self> {
///         if node.kind() == SyntaxKind::INT_LITERAL {
///             Some(Self { node, prim_type: PrimitiveType::Int })
///         } else {
///             None
///         }
///     }
/// }
///
/// impl<'a> InferType for LiteralExpr<'a> {
///     fn infer_type(
///         &self,
///         _engine: &TypeInferenceEngine,
///         _resolver: &SymbolResolver,
///         _expected_type: Option<&Type>,
///     ) -> Type {
///         Type::Primitive(self.prim_type)
///     }
/// }
/// ```
pub trait InferType {
    /// Infer the type of this expression
    ///
    /// # Arguments
    ///
    /// * `engine` - Type inference engine with workspace access
    /// * `resolver` - Symbol resolver for looking up identifiers
    /// * `expected_type` - Optional expected type from context (for lambdas, etc.)
    ///
    /// # Returns
    ///
    /// The inferred type, or `Type::Unknown` if inference fails
    fn infer_type(
        &self,
        engine: &TypeInferenceEngine,
        resolver: &SymbolResolver,
        expected_type: Option<&Type>,
    ) -> Type;
}
