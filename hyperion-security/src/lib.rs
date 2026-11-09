// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Security tests intentionally materialize finding sets for readable assertions,
// and some verify deterministic confidence constants exactly. Production code
// remains subject to the workspace's strict lint policy.
#![cfg_attr(test, allow(clippy::float_cmp, clippy::needless_collect))]

//! Hyperion Security Scanner
//!
//! Type-aware taint analysis for JVM languages.
//! Uses hyperion-parser's AST to find security vulnerabilities.

pub mod analysis_context;
pub mod annotation_utils;
pub mod cache;
mod call_target;
pub mod callgraph;
pub mod checks;
pub mod config;
pub mod constant_eval;
pub mod context;
pub mod finding;
pub mod fixpoint;
pub mod frameworks;
pub mod highlighter;
pub mod interprocedural;
pub mod logging_context;
pub mod lsp_integration;
pub mod method_summaries;
pub mod response_context;
pub mod sanitizers;
pub mod scanner;
pub mod sinks;
pub mod sources;
pub mod summary_generator;
pub mod symbolic_taint;
pub mod symbolic_tracker;
pub mod taint;
pub mod type_resolver;

pub use analysis_context::AnalysisContext;
pub use annotation_utils::{
    get_annotation_attribute, has_annotation, has_produces_media_type, is_rest_controller,
};
pub use cache::ScanCache;
pub use callgraph::{CallGraph as AstCallGraph, MethodId, MethodInfo};
pub use config::ScanConfig;
pub use constant_eval::{
    extract_constant_fields, ConstantEvaluator, ConstantValue, ReturnValueKind,
};
pub use context::{get_containing_class_name, get_containing_method_name, is_demo_or_test_context};
pub use finding::{
    diff_findings, group_findings, Finding, FindingGroup, FindingPattern, FlowStep, Severity,
    VulnType,
};
pub use fixpoint::{FixpointAnalysis, FixpointResult, SinkInfo};
pub use frameworks::{Framework, FrameworkRegistry};
pub use highlighter::CodeHighlighter;
pub use interprocedural::{CallGraph, MethodSummary, SummaryDatabase};
pub use logging_context::{LoggingContext, LoggingContextSource, LoggingSafety};
pub use lsp_integration::{finding_to_code_actions, finding_to_diagnostic, finding_to_hover};
pub use method_summaries::{MethodSummaryRegistry, TaintBehavior};
pub use response_context::{ResponseContext, ResponseContextAnalyzer};
pub use sanitizers::{SanitizerPattern, SanitizerRegistry};
pub use scanner::{ScanStats, SecurityScanner};
pub use sinks::{SinkPattern, SinkRegistry};
pub use sources::{SourceKind, SourcePattern, SourceRegistry};
pub use summary_generator::{CallEdgeSummary, SummaryGenerator};
pub use symbolic_taint::{SymbolicTaint, TaintOrigins};
pub use symbolic_tracker::SymbolicTracker;
pub use taint::{TaintState, VulnTypes};
pub use type_resolver::TypeAwareAnalyzer;
