// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code completion integration tests.
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

mod advanced_completion_tests;
mod basic_completion_tests;
mod completion_overload_regression_tests;
mod completion_regression_tests;
mod completion_tests;
mod constructor_completion_tests;
mod contextual_keyword_tests;
mod cross_file_completion_tests;
mod generic_completion_tests;
mod ide_completion_tests;
mod lambda_completion_tests;
mod large_file_completion_tests;
mod override_completion_tests;
mod smart_cast_completion_tests;
mod wildcard_completion_tests;
