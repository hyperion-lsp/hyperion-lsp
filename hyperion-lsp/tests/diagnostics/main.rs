// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Diagnostics, inspections and type checking integration tests.
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

mod boolean_condition_tests;
mod compile_error_tests;
mod cross_file_nullability_tests;
mod diagnostic_navigation_alignment_tests;
mod diagnostics_tests;
mod inherited_symbol_alignment_tests;
mod inspection_tests;
mod nullability_tests;
mod override_annotation_tests;
mod redundant_modifiers_test;
mod security_diagnostic_pipeline_tests;
mod type_checking_tests;
mod unused_imports_tests;
