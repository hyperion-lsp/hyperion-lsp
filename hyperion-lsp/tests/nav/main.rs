// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation and goto-definition integration tests.
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

mod additional_navigation_tests;
mod annotation_definition_tests;
mod cast_and_array_receiver_tests;
mod chain_parsing_tests;
mod complex_navigation_references_hover_tests;
mod cross_file_gd_tests;
mod cross_module_enum_tests;
mod cursor_position_accuracy_tests;
mod enum_annotation_test;
mod enum_reference_disambiguation_tests;
mod field_resolution_tests;
mod foreach_loop_variable_test;
mod goto_implementation_tests;
mod import_auto_insertion_tests;
mod import_navigation_tests;
mod import_resolution_edge_case_tests;
mod inner_class_tests;
mod interface_tests;
mod jdk_shadowing_bug_test;
mod lambda_constructor_tests;
mod lambda_navigation_tests;
mod lambda_overload_navigation_tests;
mod macro_examples;
mod method_navigation_tests;
mod method_reference_tests;
mod navigation_revision_provenance_tests;
mod negative_test_cases;
mod pattern_variable_gd_tests;
mod qualified_access_tests;
mod qualified_access_tests_v2;
mod repro_override_decl_nav;
mod same_file_navigation_tests;
mod same_package_navigation_tests;
mod same_package_resolution;
mod second_gd_cursor_position_test;
mod static_argument_navigation_tests;
mod static_import_tests;
mod static_member_navigation_tests;
mod static_method_tests;
mod static_properties_cross_file_tests;
mod strict_macro_use_cases_tests;
mod ternary_navigation_tests;
mod test_param_gd;
mod this_field_navigation_tests;
mod try_with_resources_navigation_tests;
mod type_param_bound_navigation_tests;
mod variable_chain_tests;
