// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! # Hyperion Semantics
//!
//! Semantic analysis layer for the Hyperion LSP, built on Salsa's incremental
//! computation framework.
//!
//! This crate provides:
//! - Query-based semantic analysis with automatic change propagation
//! - Symbol resolution and type inference
//! - Incremental recomputation for efficient LSP operations
//! - Thread-safe database suitable for concurrent LSP requests
//!
//! ## Architecture
//!
//! The semantic analysis is organized into a layered query system:
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │         LSP Server (Queries)            │
//! └─────────────────┬───────────────────────┘
//!                   │
//! ┌─────────────────▼───────────────────────┐
//! │      SemanticDatabase (Derived)         │
//! │  - Symbol resolution                    │
//! │  - Type inference                       │
//! │  - Scope analysis                       │
//! └─────────────────┬───────────────────────┘
//!                   │
//! ┌─────────────────▼───────────────────────┐
//! │       ParseDatabase (Derived)           │
//! │  - Syntax tree construction             │
//! │  - Error recovery                       │
//! └─────────────────┬───────────────────────┘
//!                   │
//! ┌─────────────────▼───────────────────────┐
//! │       SourceDatabase (Input)            │
//! │  - File contents                        │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Example Usage
//!
//! ```rust
//! use hyperion_semantics::db::{Database, SourceDatabase, SemanticDatabase};
//! use hyperion_semantics::db::FileId;
//! use hyperion_workspace::Language;
//! use std::sync::Arc;
//!
//! let mut db = Database::new();
//!
//! // Set file contents and language (input)
//! let file_id = FileId::new(0);
//! db.set_file_text(file_id, Arc::new("public class Hello { }".to_string()));
//! db.set_file_language(file_id, Language::Java);
//!
//! // Query symbols (automatically parses and analyzes)
//! let symbols = db.file_symbols(file_id);
//! println!("Found {} symbols", symbols.symbols.len());
//!
//! // Update file (only changed queries are recomputed)
//! db.set_file_text(file_id, Arc::new("public class HelloWorld { }".to_string()));
//! let new_symbols = db.file_symbols(file_id);
//! ```

pub mod call_resolution;
pub mod compile_checks;
pub mod db;
pub mod file_semantics;
pub mod inference;
pub mod kotlin_certain_errors;
pub mod kotlin_inference;
pub mod kotlin_resolver;
pub mod kotlin_semantic_walker;
pub mod kotlin_type_checker;
pub mod nullability;
pub mod resolved_name;
pub mod resolver;
pub mod semantic_error;
pub mod semantic_walker;
pub mod symbol;
pub mod type_checker;
pub mod type_parser;
pub mod type_substitution;
pub mod types;
pub mod unified_resolver;
pub mod utils;

// Re-export commonly used types
pub use db::{
    Database, FileId, FileSymbols, ParseDatabase, ParseTree, SemanticDatabase, SourceDatabase,
    WorkspaceIndexHandle,
};

// Re-export symbol table types
pub use symbol::{
    Annotation, AnnotationValue, ClassType, Import, Modifiers, Parameter, PrimitiveType, Scope,
    ScopeId, ScopeKind, SourceLocation, Symbol, SymbolId, SymbolKind, SymbolMetadata, SymbolTable,
    SymbolTableStatistics, Type, TypeParameter, Visibility,
};

// Re-export type system extensions
pub use types::TypeExt;

// Re-export resolver types
pub use resolver::{ResolvedSymbol, ResolverBuilder, SymbolResolver};

// Re-export Kotlin resolver and inference
pub use kotlin_inference::{semantics_type_to_kotlin, KotlinTypeInferenceEngine};
pub use kotlin_resolver::KotlinResolverBuilder;

// Re-export type inference engine
pub use inference::TypeInferenceEngine;

// Re-export type substitution
pub use type_substitution::TypeSubstitution;

// Re-export semantic error types
pub use semantic_error::{SemanticError, SemanticErrorKind};

// Re-export type checker
pub use type_checker::TypeChecker;

// Re-export Kotlin type checker
pub use kotlin_type_checker::KotlinTypeChecker;

// Re-export unified resolver types
pub use unified_resolver::{NameContext, ResolutionResult, UnifiedTypeResolver};

// Re-export resolved name types
pub use resolved_name::ResolvedName;

// Re-export file semantics
pub use file_semantics::FileSemantics;

// Re-export nullability types
pub use nullability::{
    DiagnosticKind as NullabilityDiagnosticKind, FileNullabilityAnalysis, Nullability,
    NullabilityAnalysis, NullabilityDiagnostic, NullableType, PackageDefaultsRegistry,
    PackageNullabilityDefaults,
};

// Test modules (only compiled during testing)
#[cfg(test)]
mod symbol_tests;

#[cfg(test)]
mod types_tests;

#[cfg(test)]
mod db_tests;

#[cfg(test)]
mod resolver_tests;

#[cfg(test)]
mod scope_file_id_tests;

// Type inference bug tests
#[cfg(test)]
mod overload_resolution_tests;

#[cfg(test)]
mod generic_inference_tests;

#[cfg(test)]
mod bounds_tests;

#[cfg(test)]
mod raw_type_tests;

#[cfg(test)]
mod wildcard_tests;

#[cfg(test)]
mod capture_tests;

#[cfg(test)]
mod wildcard_asymmetry_tests;

#[cfg(test)]
mod type_param_position_tests;

#[cfg(test)]
mod intersection_tests;

#[cfg(test)]
mod substitution_cycle_tests;

#[cfg(test)]
#[cfg(test)]
mod var_optional_inference_tests;

// Test utilities
#[cfg(test)]
pub mod test_types;
