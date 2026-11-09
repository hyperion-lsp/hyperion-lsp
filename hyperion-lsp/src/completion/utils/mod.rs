// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion utilities module
//!
//! Provides helper functions and utilities for code completion, including:
//! - Text manipulation and identifier extraction
//! - Import management integration
//! - Type resolution and member lookup
//! - Type inference utilities

pub mod import_manager_integration;
pub mod text_helpers;
pub mod type_inference;

// Re-export commonly used functions
pub use import_manager_integration::add_import_edits;
pub use text_helpers::{
    count_commas_before_position, extract_identifier_prefix, find_opening_paren, simplify_type_name,
};
pub use type_inference::{
    infer_expected_type, infer_type_from_method_argument, infer_type_from_return_statement,
    lookup_constructor_param_type, lookup_method_param_type, type_to_fqn,
};
