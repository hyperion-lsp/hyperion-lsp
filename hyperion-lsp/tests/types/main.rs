// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java type system: generics, records, enums and inference integration tests.
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

mod advanced_enum_tests;
mod advanced_generics_tests;
mod advanced_record_tests;
mod enum_tests;
mod field_type_tests;
mod inheritance_method_resolution_tests;
mod inheritance_tests;
mod inheritance_traversal_tests;
mod inherited_member_fallback_tests;
mod lambda_with_real_jdk_types_test;
mod property_vs_param_tests;
mod record_pattern_switch_tests;
mod record_tests;
mod sealed_class_tests;
mod stream_collect_test;
mod stream_lambda_inference_tests;
mod stream_lambda_tests;
mod switch_expression_tests;
mod type_inference_integration_tests;
mod type_param_detection_tests;
mod var_type_inference_tests;
mod wildcard_generics_tests;
