// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation Provider for LSP Features
//!
//! This module implements goto-definition and find-references functionality for the Hyperion LSP.
//! It uses the semantic analysis infrastructure to resolve symbols and find their definitions
//! and references throughout the codebase.
//!
//! ## Module layout
//!
//! - `argument_inference.rs` - Argument type inference for overload resolution
//! - `call_site_analysis.rs` - Call site analysis for method overload resolution
//! - `chain/` - Method chain parsing utilities (segment_parser, qualified_access, extraction)
//! - `chain_resolver.rs` - Method chain resolution (extracted from provider.rs)
//! - `context.rs` - Shared navigation request context
//! - `groovy.rs` - Groovy/Gradle navigation helpers (extracted from provider.rs)
//! - `import_resolver.rs` - Import resolution helpers
//! - `inheritance_resolver.rs` - Inheritance and @Override resolution (extracted from provider.rs)
//! - `kotlin.rs` - Kotlin-specific navigation helpers (extracted from provider.rs)
//! - `lambda_inference.rs` - Lambda expression type inference utilities
//! - `method_reference_resolver.rs` - Method reference syntax resolution (extracted from provider.rs)
//! - `overload_resolver.rs` - Type-based method overload resolution
//! - `provider.rs` - Primary NavigationProvider implementation
//! - `qualified_access_resolver.rs` - Qualified member access resolution (extracted from provider.rs)
//! - `scala.rs` - Scala-specific navigation helpers
//! - `type_finder.rs` - Type/variable lookup helpers
//! - `utils.rs` - Position and scope helpers reused across modules
//! - `workspace_resolver.rs` - Cross-file symbol resolution (extracted from provider.rs)

pub(crate) mod annotation_resolver;
pub mod argument_inference;
pub mod ast_queries;
pub mod call_site_analysis;
pub mod chain;
pub mod chain_resolver;
pub mod context;
pub(crate) mod groovy;
pub mod import_resolver;
pub mod inheritance_resolver;
pub(crate) mod kotlin;
pub mod lambda_inference;
#[cfg(test)]
mod lambda_inference_sam_tests;
pub mod method_reference_resolver;
pub mod overload_resolver;
pub mod provider;
pub mod qualified_access_resolver;
pub(crate) mod scala;
pub mod type_finder;
pub mod utils;
pub mod workspace_resolver;

// Re-export for convenience
pub use chain::{detect_qualified_access, extract_receiver_text, parse_method_chain, ChainSegment};
pub use utils::{find_scope_at_position, fqn_to_type, offset_to_position, position_to_offset};

pub use provider::NavigationProvider;
pub(crate) use provider::ResolvedNavigationTarget;
