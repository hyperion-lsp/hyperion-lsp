// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hyperion Actions - Code actions and quick fixes
//!
//! This crate provides LSP code actions including quick fixes, intentions, and code generation.
//!
//! # Features
//!
//! - **Quick Fixes**: Automatic fixes for diagnostics
//! - **Code Intentions**: Context-aware code improvements
//! - **Code Generation**: Generate boilerplate code
//! - **Import Actions**: Add, remove, organize imports
//! - **Formatting**: Code formatting and style fixes
//!
//! # Code Actions
//!
//! - Add missing imports
//! - Implement interface methods
//! - Override methods
//! - Generate getters/setters
//! - Generate constructors
//! - Generate equals/hashCode/toString
//! - Surround with (try/catch, if, etc.)
//! - Convert to lambda/method reference
//! - Organize imports
//!
//! # Example Usage
//!
//! ```rust
//! use hyperion_actions::{QuickFixProvider, CodeGenerator};
//!
//! // Generate a quick fix for a diagnostic
//! // let fixes = QuickFixProvider::get_quick_fixes(&node, "cannot find symbol: List");
//!
//! // Generate getter/setter methods
//! let getter = CodeGenerator::generate_getter("name", "String");
//! let setter = CodeGenerator::generate_setter("name", "String");
//! ```

/// Quick fix actions for diagnostic issues
pub mod quick_fix;

/// Import organization utilities
pub mod organize_imports;

/// Code generation utilities
pub mod generate;

// Re-export main types
pub use generate::CodeGenerator;
pub use organize_imports::{ImportOrganizer, TextEdit as ImportTextEdit};
pub use quick_fix::{QuickFix, QuickFixProvider, TextEdit as QuickFixTextEdit};
