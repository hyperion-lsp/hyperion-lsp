// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Actions Provider for LSP
//!
//! This module implements code actions (quick fixes and refactorings) for the Hyperion LSP.
//! Code actions provide contextual suggestions for improving or fixing code.
//!
//! ## Module Structure
//!
//! - `provider`: Main CodeActionProvider implementation
//! - `extractors`: Extract variable, method, constant, field, parameter
//! - `surround`: Surround with if, try-catch, while, etc.
//! - `utils`: Shared utility functions
//!
//! ## Code Action Kind Hierarchy
//!
//! LSP built-in kinds:
//! - `quickfix` - Quick fixes for diagnostics (imports, create member)
//! - `refactor` - General refactorings
//! - `refactor.extract` - Extract variable, method, constant, field, parameter, interface
//! - `refactor.inline` - Inline variable, method
//! - `refactor.rewrite` - Transform code structure (convert to for-each)
//! - `source` - Source-level operations
//! - `source.organizeImports` - Organize imports
//!
//! Custom kinds (for editor filtering):
//! - `source.generate` - Generate constructor, getters/setters, equals, hashCode, toString
//! - `source.generate.test` - Generate test class
//! - `source.generate.implement` - Implement interface methods
//! - `source.generate.override` - Override superclass methods
//! - `refactor.move` - Move static method to utility class
//! - `refactor.surround` - Surround with if/try-catch/while

mod diagnostic_symbol;
pub mod extract_interface;
pub mod extractors;
pub(crate) mod generation;
pub mod imports;
pub mod move_refactoring;
mod provider;
pub(crate) mod refactoring;
pub mod surround;
pub mod utils;

// Re-export the main provider
pub use provider::CodeActionProvider;

/// Custom code action kinds for more specific categorization
pub mod kinds {
    use tower_lsp::lsp_types::CodeActionKind;

    /// Source-level code generation actions
    pub const fn source_generate() -> CodeActionKind {
        CodeActionKind::new("source.generate")
    }

    /// Generate test class action
    pub const fn source_generate_test() -> CodeActionKind {
        CodeActionKind::new("source.generate.test")
    }

    /// Implement interface methods action
    pub const fn source_generate_implement() -> CodeActionKind {
        CodeActionKind::new("source.generate.implement")
    }

    /// Override superclass methods action
    pub const fn source_generate_override() -> CodeActionKind {
        CodeActionKind::new("source.generate.override")
    }

    /// Extract interface (`refactor.extract.interface`)
    pub const fn refactor_extract_interface() -> CodeActionKind {
        CodeActionKind::new("refactor.extract.interface")
    }

    /// Move refactoring (e.g., move static method)
    pub const fn refactor_move() -> CodeActionKind {
        CodeActionKind::new("refactor.move")
    }

    /// Surround with refactoring
    pub const fn refactor_surround() -> CodeActionKind {
        CodeActionKind::new("refactor.surround")
    }
}
