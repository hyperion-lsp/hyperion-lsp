// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type-safe expression wrappers for the visitor pattern
//!
//! This module contains type-safe wrappers for all Java expression types.
//! Each wrapper:
//! - Validates that a SyntaxNode is the correct kind
//! - Provides typed access to expression components
//! - Implements `InferType` trait for type inference
//!
//! # Architecture
//!
//! Instead of passing raw `SyntaxNode` to functions and pattern matching on kind,
//! we use the newtype pattern to create zero-cost abstractions:
//!
//! ```rust,ignore
//! // Old: Error-prone, no type safety
//! fn infer_method_call(node: &SyntaxNode) -> Type {
//!     // Assumes node is METHOD_CALL_EXPR, but no compile-time check
//! }
//!
//! // New: Type-safe, self-documenting
//! impl InferType for MethodCallExpr<'_> {
//!     fn infer_type(&self, ...) -> Type {
//!         // self.0 is guaranteed to be METHOD_CALL_EXPR
//!     }
//! }
//! ```
//!
//! # Modules
//!
//! - `literals` - Literal expressions (int, string, boolean, null)
//! - `identifiers` - Identifier expressions (variables, fields, parameters)
//! - `simple` - Simple expressions (this, super, parenthesized, cast, new)
//! - More to come as migration progresses

pub mod identifiers;
pub mod literals;
pub mod simple;
