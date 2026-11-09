// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hover, signature help and inlay hints integration tests.
//!
//! Aggregator binary. Every module below used to be its own
//! `tests/*.rs` file, and therefore its own binary statically linking the
//! whole dependency graph. They are now modules of a single test binary, so
//! the shared harness in `tests/support/` is compiled and linked once for the
//! whole group instead of once per file.
//!
//! Tests in one binary share a process and run on parallel threads. The
//! harness already guards the only process-global state (the JDK symbol cache
//! in `support/lsp_test_framework.rs`, behind a single-flight mutex and an
//! `RwLock`ed memory cache), which this grouping now amortises across the
//! whole group rather than paying it per binary.

#[macro_use]
#[path = "../support/test_macros.rs"]
mod test_macros;

#[macro_use]
#[path = "../support/lsp_test_framework.rs"]
mod lsp_test_framework;

#[path = "../support/common_workspace/mod.rs"]
mod common_workspace;

mod arg_count_test;
mod complex_arg_inference_test;
mod hover_macro_tests;
mod import_hover_tests;
mod inlay_hints_tests;
mod lambda_hover_tests;
mod method_chain_hover_tests;
mod method_documentation_tests;
mod method_overload_tests;
mod overload_sorting_tests;
mod overload_type_resolution_tests;
mod override_relationship_tests;
mod pattern_variable_hover_tests;
mod signature_help_edge_cases;
mod signature_help_tests;
