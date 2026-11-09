// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Security scanner implementation
//!
//! This module is organized as follows:
//! - `scanner.rs`: Main SecurityScanner implementation
//! - `interprocedural.rs`: Interprocedural (cross-method) analysis and AST extraction helpers
//! - `visitor.rs`: SecurityVisitor struct, constructor, visit_type_decl, and location helpers
//! - `pass2_analysis.rs`: Pass 2 AST walking (blocks, statements, expressions, sinks)
//! - `context_analysis.rs`: Context analysis (JDBC, exception logging, file output detection)
//! - `field_taint.rs`: Field taint collection and interprocedural taint propagation
//! - `source_detection.rs`: Source detection (annotations, known source types)
//! - `file_processing.rs`: File I/O, caching, parallel scanning, and exclusion filtering
//! - `utilities.rs`: Free utility functions (expression/statement checks, location helpers, etc.)
//! - `method_call_pattern.rs`: Pattern matching for receiver.method(args)
//! - `collection_taint.rs`: Taint tracking for collection mutations
//! - `sink_detection.rs`: Sink detection and finding creation

mod collection_taint;
mod context_analysis;
mod field_taint;
mod file_processing;
mod interprocedural;
pub(crate) mod method_call_pattern;
mod pass2_analysis;
mod sink_detection;
mod source_detection;
pub(crate) mod utilities;
mod visitor;

// Re-export the main scanner types
pub use self::utilities::{ScanStats, SecurityScanner};

// The main scanner implementation
#[allow(clippy::module_inception)]
mod scanner;
