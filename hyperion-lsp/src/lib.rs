// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Language Server Protocol implementation for Hyperion's JVM-language tooling.
//!
//! This crate owns protocol lifecycle, request routing, document state and the
//! integration of the parser, semantic database, workspace index, formatter,
//! security scanner and debug adapter. Java is the primary implementation;
//! Kotlin, Groovy and Scala have deliberately different capability levels.
//!
//! The repository README is the source of truth for supported features and
//! known limitations. Crate-level documentation focuses on implementation
//! boundaries and does not make performance or compatibility promises.

// Lint: Allow unwrap/expect - these are used judiciously throughout the codebase
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
//!
//! # Main boundaries
//!
//! - [`server`] owns lifecycle, capabilities and request dispatch.
//! - [`completion`], [`hover`] and [`navigation`] implement code intelligence.
//! - [`diagnostic_engine`] coordinates parse, type, inspection, nullability and
//!   security diagnostics.
//! - [`gradle_integration`] and [`maven_integration`] build project classpaths.
//! - [`position_utils`] is the boundary between LSP UTF-16 positions and
//!   internal UTF-8 byte offsets.
//!
//! # Example Usage
//!
//! To run the LSP server as a standalone binary:
//!
//! ```bash
//! cargo run --bin hyperion-lsp
//! ```
//!
//! To use the server programmatically:
//!
//! ```rust,no_run
//! use hyperion_lsp::server::create_lsp_server;
//! use tokio::io::{stdin, stdout};
//!
//! #[tokio::main]
//! async fn main() {
//!     let (service, socket) = create_lsp_server();
//!     tower_lsp::Server::new(stdin(), stdout(), socket)
//!         .serve(service)
//!         .await;
//! }
//! ```
//!

pub mod allocator;
pub mod build_tool_classpath; // Opt-in: classpaths straight from Gradle/Maven
pub mod call_hierarchy;
pub mod call_hierarchy_kotlin; // Kotlin call hierarchy (walks the Kotlin CST, not the Java one)
pub mod code_actions;
pub mod code_lens;
pub mod commands;
pub mod completion;
pub mod config;
pub mod diagnostic_engine;
pub mod diagnostics;
pub mod document_cache;
pub mod document_links;
pub mod file_move; // Package-aware source file moves (code action, willRenameFiles)
pub mod file_registry;
pub mod file_watcher; // File system watcher for external change detection
pub(crate) mod find_references; // Find references implementation
pub(crate) mod find_references_constructors; // Constructor reference finding
pub(crate) mod find_references_kotlin; // Kotlin-specific find references
pub mod folding;
pub mod formatting;
pub mod gradle_integration;
pub mod groovy_support;
pub mod handlers; // LSP request handlers extracted from server.rs
pub mod hover;
pub mod import_manager;
pub mod inherited_members; // One hierarchy walk for every "override"/"implement" feature
pub mod inlay_hints;
pub mod inlay_hints_kotlin; // Kotlin inlay hints (walks the Kotlin CST, not the Java one)
pub mod inspections;
pub mod jdk_cache;
pub mod jdk_indexer;
pub(crate) mod kotlin_calls; // Shared Kotlin call-site analysis (signature help + inlay hints)
pub(crate) mod language_handler; // Unified language dispatch for hover, navigation, find-references
pub mod launch; // hyperion.resolveLaunch: run/debug/test launch descriptions
pub mod maven_integration;
pub mod navigation; // New modular navigation provider + helpers
pub(crate) mod navigation_util; // Utility functions for navigation
pub mod position_utils;
pub mod progress; // Progress reporting for LSP operations
pub mod project_classpath; // Cache of Gradle/Maven-resolved dependency JARs for run_executor
pub(crate) mod reference_results;
pub mod rename;
pub mod run_config;
pub mod run_executor; // Compiles + runs "Run" code lens targets in the VM
pub mod search_cli; // Headless semantic search for the CLI (sym/refs/def/impls/grep)
pub mod security_diagnostics; // Security vulnerability scanner integration
pub mod semantic_host; // The one owner of the semantic DB: ReadTxn / WriteTxn gate
#[cfg(test)]
mod semantic_host_stress_tests;
pub mod semantic_tokens;
pub mod server;
pub mod signature_help;
pub mod signature_help_kotlin; // Kotlin signature help (walks the Kotlin CST, not the Java one)
pub(crate) mod signature_matching; // Signature extraction, matching, and method call inference
pub mod source_provider; // Source provider system for JDK/external navigation
pub(crate) mod source_publication;
pub mod symbols;
pub mod symbols_kotlin; // Kotlin document symbols (walks the Kotlin CST, not the Java one)
pub mod type_hierarchy;
pub mod type_hierarchy_kotlin; // Kotlin type hierarchy (walks the Kotlin CST, not the Java one)
pub mod type_resolution; // Shared type resolution logic for hover and navigation

// Re-export commonly used types
pub use server::{Backend, DocumentState};
