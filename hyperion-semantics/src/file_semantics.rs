// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Pre-computed name resolution results for a file.
//!
//! `FileSemantics` holds the resolution of every name node in a file,
//! cached via a Salsa query. Diagnostics read unresolved-symbol errors
//! from here instead of running the type checker's AST walk.

use crate::resolved_name::ResolvedName;
use crate::semantic_error::SemanticError;
use crate::symbol::Type;
use rowan::TextRange;
use std::collections::HashMap;

/// Pre-computed semantic analysis for a single file.
///
/// Contains name resolution results for every name node and any
/// diagnostics (unresolved symbols, invalid imports) found during
/// the resolution pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSemantics {
    /// Map from AST node text range → what it resolved to
    resolved_names: HashMap<TextRange, ResolvedName>,
    /// Map from expression node text range → inferred type
    expression_types: HashMap<TextRange, Type>,
    /// Diagnostics found during semantic analysis (unresolved symbols, bad imports)
    diagnostics: Vec<SemanticError>,
}

impl FileSemantics {
    /// Create a new FileSemantics with the given resolution results and diagnostics.
    pub const fn new(
        resolved_names: HashMap<TextRange, ResolvedName>,
        expression_types: HashMap<TextRange, Type>,
        diagnostics: Vec<SemanticError>,
    ) -> Self {
        Self {
            resolved_names,
            expression_types,
            diagnostics,
        }
    }

    /// Create an empty FileSemantics (for non-Java files or files with no names).
    pub fn empty() -> Self {
        Self {
            resolved_names: HashMap::new(),
            expression_types: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Look up the resolution for a name at the given text range.
    pub fn resolve_at(&self, range: TextRange) -> Option<&ResolvedName> {
        self.resolved_names.get(&range)
    }

    /// All resolved names in this file.
    pub const fn resolved_names(&self) -> &HashMap<TextRange, ResolvedName> {
        &self.resolved_names
    }

    /// Look up the inferred type for an expression at the given text range.
    pub fn expression_type_at(&self, range: TextRange) -> Option<&Type> {
        self.expression_types.get(&range)
    }

    /// All inferred expression types in this file.
    pub const fn expression_types(&self) -> &HashMap<TextRange, Type> {
        &self.expression_types
    }

    /// Diagnostics found during semantic analysis.
    pub fn diagnostics(&self) -> &[SemanticError] {
        &self.diagnostics
    }
}
