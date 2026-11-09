// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Production resolution paths must handle failure explicitly. Tests intentionally
// use unwrap when reaching the asserted value is itself the test contract.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![allow(
    clippy::redundant_closure,
    clippy::unnested_or_patterns,
    clippy::use_self
)]

//! Shared type resolution utilities for Hyperion LSP
//!
//! This crate provides type structure extraction and name resolution
//! that can be shared between workspace indexing and semantic analysis.
//!
//! # Overview
//!
//! The crate provides:
//! - [`TypeStructure`] - AST-agnostic representation of Java types
//! - [`extract_type_structure`] - Extract type from parser AST nodes
//! - [`ResolutionContext`] - Context for resolving simple names to FQNs
//! - [`resolve_type_structure_without_lookup`] - Resolve all identifiers in a type structure
//!
//! # Example
//!
//! ```ignore
//! use hyperion_types::{extract_and_resolve_type_without_lookup, ResolutionContext};
//!
//! let context = ResolutionContext::new(imports, package, parent_class);
//! let type_structure = extract_and_resolve_type_without_lookup(node, &context)?;
//! let fqn = type_structure.to_fqn_string(); // "java.util.List<com.example.User>[]"
//! ```

mod context;
mod declaration;
mod extraction;
mod field_declarators;
mod java_lang;
pub mod jvm_names;
mod location;
mod resolution;
mod semantic_highlight;
mod structure;

pub use context::{ResolutionContext, SymbolLookup};
pub use declaration::{DeclarationImports, DeclarationModifiers, StaticImport, TypeParameterInfo};
pub use extraction::{
    count_array_dimensions, enclosing_type_parameter_declarations, enclosing_type_parameter_names,
    extract_type_structure, TypeExtractionError,
};
pub use field_declarators::{field_declarators, FieldDeclarator};
pub use java_lang::{is_core_java_lang_type, CORE_JAVA_LANG_TYPES};
pub use location::TextRange;
pub use resolution::{
    is_conventional_type_parameter_name, resolve_type_structure_with_lookup,
    resolve_type_structure_without_lookup,
};
pub use semantic_highlight::{
    SemanticHighlight, SemanticHighlightKind, SemanticHighlightModifiers,
};
pub use structure::TypeStructure;

/// Extract and resolve type structure WITH workspace lookup (preferred)
///
/// Combines extraction and workspace-aware resolution.
pub fn extract_and_resolve_type_with_lookup(
    node: &hyperion_parser::SyntaxNode,
    context: &ResolutionContext,
    lookup: &dyn SymbolLookup,
) -> Result<TypeStructure, TypeExtractionError> {
    let unresolved = extract_type_structure(node)?;
    Ok(resolve_type_structure_with_lookup(
        &unresolved,
        context,
        lookup,
    ))
}

/// Extract and resolve a type structure with no workspace evidence.
///
/// The degraded mode: see
/// [`ResolutionContext::resolve_identifier_without_lookup`]. Reserved for the
/// indexer, which runs before there is an index to consult.
pub fn extract_and_resolve_type_without_lookup(
    node: &hyperion_parser::SyntaxNode,
    context: &ResolutionContext,
) -> Result<TypeStructure, TypeExtractionError> {
    let unresolved = extract_type_structure(node)?;
    Ok(resolve_type_structure_without_lookup(&unresolved, context))
}
