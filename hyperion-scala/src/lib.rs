// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Narrow Scala analysis support for mixed-language workspaces.
//!
//! The crate parses Scala 2 and Scala 3 syntax with tree-sitter, extracts
//! declarations and type text from the CST, and supplies local inference and
//! pattern-exhaustiveness helpers. Hyperion uses that information for basic
//! navigation, hover and semantic highlighting in mixed Java/Scala projects.
//!
//! This is not a Scala compiler front end. Implicit resolution, path-dependent
//! types, higher-kinded inference and full cross-file semantics are outside the
//! current contract; use Metals for first-class Scala development.

/// Parser module (tree-sitter-based)
pub mod parser;

/// CST-based symbol extractor
pub mod cst_symbol_extractor;

/// Type extraction module
pub mod type_extractor;

/// Symbol extraction module
pub mod symbol_extractor;

/// Local type inference
pub mod inference;

/// Pattern match exhaustiveness checking
pub mod pattern_exhaustiveness;

/// Parser-neutral semantic highlighting facts for LSP adapters.
pub mod semantic_highlight;

// Re-export commonly used types
pub use symbol_extractor::{ExtractionResult, Symbol, SymbolExtractor, TextRange};
