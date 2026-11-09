// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Framework-annotation and scenario integration tests.
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

mod assertj_chain_tests;
mod builder_pattern_tests;
mod complex_flows_tests;
mod dao_repository_tests;
mod edge_case_fuzzing;
mod edge_case_tests;
mod find_actual_limitations_tests;
mod inline_variable_safety_tests;
mod jpa_annotation_navigation_test;
mod jpa_relationship_hover_tests;
mod lombok_hover_tests;
mod lombok_tests;
mod spring_jpa_tests;
mod stress_test_scenarios;
