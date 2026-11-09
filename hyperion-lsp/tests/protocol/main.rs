// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP protocol surface, server lifecycle and CLI integration tests.
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

mod cache_invalidation_tests;
mod cli_integration_tests;
mod code_action_tests;
mod code_lens_tests;
mod concurrency_tests;
mod document_symbols_tests;
mod external_change_tests;
mod extract_interface_tests;
mod file_move_tests;
mod file_semantics_invalidation_tests;
mod folding_tests;
mod gradle_lsp_integration_tests;
mod hierarchy_member_actions_tests;
mod import_fix_tests;
mod java_only_request_gates;
mod lombok_generation_tests;
mod lsp_contract_tests;
mod lsp_test_framework_contract_tests;
mod position_conversion_tests;
mod refactor_harness;
mod request_resilience_tests;
mod rootless_initialization_tests;
mod run_command_tests;
mod run_executor_project_classpath_tests;
mod safe_delete_flow_tests;
mod semantic_tokens_tests;
mod type_hierarchy_object_tests;
