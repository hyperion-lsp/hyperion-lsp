// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type resolution module
//!
//! This module provides shared type resolution logic for both navigation (goto-definition)
//! and hover providers. It resolves types in method chains, qualified accesses, and
//! handles inheritance, imports, and semantic analysis.

pub mod chain_resolver;
pub mod types;
pub mod utils;

// Re-export main types for convenience
pub use chain_resolver::ChainResolver;
pub use types::{ResolveError, ResolveResult, TypeInfo};
pub use utils::{extract_package, extract_simple_name, is_fqn};
