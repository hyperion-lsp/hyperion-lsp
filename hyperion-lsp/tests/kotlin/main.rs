// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin language support integration tests.
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

#[path = "../support/kotlin_hierarchy_harness.rs"]
mod kotlin_hierarchy_harness;

mod kotlin_call_hierarchy_tests;
mod kotlin_certain_error_tests;
mod kotlin_clean_code_tests;
mod kotlin_code_intel_dispatch_tests;
mod kotlin_comment_filtering_tests;
mod kotlin_completion_tests;
mod kotlin_diagnostic_pipeline_tests;
mod kotlin_document_symbols_tests;
mod kotlin_expression_form_hover_tests;
mod kotlin_find_references_enhanced_tests;
mod kotlin_goto_tests;
mod kotlin_hover_tests;
mod kotlin_hover_type_tests;
mod kotlin_ide_completion_tests;
mod kotlin_import_tests;
mod kotlin_inlay_hints_tests;
mod kotlin_inspections_tests;
mod kotlin_java_interop_tests;
mod kotlin_local_scope_tests;
mod kotlin_production_ready_tests;
mod kotlin_references_tests;
mod kotlin_script_tests;
mod kotlin_semantic_completion_tests;
mod kotlin_signature_help_tests;
mod kotlin_smart_cast_tests;
mod kotlin_string_interpolation_tests;
mod kotlin_type_hierarchy_tests;
mod kotlin_type_inference_tests;
mod kotlin_type_reference_tests;
mod kotlin_user_defined_generics_tests;
