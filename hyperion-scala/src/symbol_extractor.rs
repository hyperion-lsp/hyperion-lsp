// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Public Scala symbol-extraction facade.
//!
//! The public entry point delegates to the tree-sitter CST extractor.

use lsp_types::SymbolKind;
use serde::{Deserialize, Serialize};

pub use hyperion_types::{
    DeclarationImports as ImportMap, DeclarationModifiers as Modifiers, TextRange,
};

/// A symbol definition extracted from Scala source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Fully qualified name (e.g., "com.example.server.ServerConfig")
    pub fqn: String,
    /// Simple name (e.g., "ServerConfig")
    pub simple_name: String,
    /// Location in source
    pub range: TextRange,
    /// Kind of symbol (class, method, field, etc.)
    pub kind: SymbolKind,
    /// Container FQN (e.g., class for a method, package for top-level)
    pub container: Option<String>,
    /// Return type for methods (simple name)
    pub return_type: Option<String>,
    /// Parameter types for methods
    pub parameters: Vec<String>,
    /// Source-level declaration modifiers.
    #[serde(default)]
    pub modifiers: Modifiers,
    /// Direct superclass, if the declaration has one.
    #[serde(default)]
    pub parent_class: Option<String>,
    /// Directly implemented or extended traits/interfaces.
    #[serde(default)]
    pub interfaces: Vec<String>,
}

/// Result of extracting symbols from a Scala file
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub package: Option<String>,
    pub imports: ImportMap,
    pub symbols: Vec<Symbol>,
}

/// Scala symbol extractor
pub struct SymbolExtractor;

impl SymbolExtractor {
    /// Extract all symbols from a Scala source file
    ///
    /// Uses tree-sitter CST parsing and returns declarations with source ranges.
    pub fn extract(source: &str) -> anyhow::Result<ExtractionResult> {
        crate::cst_symbol_extractor::extract_from_source(source)
    }
}
