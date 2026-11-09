// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hyperion Refactor - Advanced refactoring engine
//!
//! This crate provides safe, type-aware refactorings for JVM languages.
//!
//! # Core Refactorings
//!
//! - **Extract Method**: Extract code into a new method (planned)
//! - **Extract Variable**: Extract expression into a variable (planned)
//! - **Extract Constant**: Extract value into a constant (planned)
//! - **Change Signature**: Modify method signatures ✓
//!
//! # Safety Guarantees
//!
//! - Preview mode (show changes before applying)
//! - Rollback support
//! - Conflict detection
//! - Type-safe transformations
//!
//! # Example Usage
//!
//! ```ignore
//! use hyperion_refactor::text_edit::TextEdit;
//!
//! let edits = vec![TextEdit::new(0..5, "hello".to_string())];
//! let new_source = TextEdit::apply_edits("world", &edits);
//! ```

/// Text edit utilities - apply changes to source code
pub mod text_edit;

/// Extract-refactoring API; operations currently return `NotImplemented`
pub mod extract;

/// Change Signature refactoring - modify method signatures and update call sites
pub mod signature;

// Re-export commonly used types
pub use extract::{ExtractConstant, ExtractMethod, ExtractVariable};
pub use signature::{
    CallSite, ChangeSignatureRefactoring, MethodSignature, Parameter, SignatureChange,
    SignatureEdit, SignatureError,
};
pub use text_edit::TextEdit;
