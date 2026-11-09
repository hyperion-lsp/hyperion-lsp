// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scope analysis for Kotlin files
//!
//! Tracks local variables, parameters, and other lexically-scoped symbols
//! to enable navigation within a single file.
//!
//! # Module Structure
//!
//! - `types`: Core types (ScopeSymbol, MethodCallInfo, ImportInfo, SmartCast)
//! - `graph`: Scope and ScopeGraph implementations
//! - `builder`: ScopeGraphBuilder for constructing scope graphs
//! - `visitor`: AST visitor and extraction logic (build_scope_graph)

mod builder;
mod graph;
mod types;
mod visitor;

// Re-export public types
pub use builder::ScopeGraphBuilder;
pub use graph::{Scope, ScopeGraph};
pub use types::{
    ImportInfo, ImportKind, MethodCallInfo, Offset, ScopeSymbol, ScopeSymbolKind, SmartCast,
};

// Re-export the main entry point
pub use visitor::build_scope_graph;
