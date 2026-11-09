// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! SecurityVisitor - AST visitor that walks the syntax tree looking for security issues.
//!
//! This module contains the `SecurityVisitor` struct and its implementation,
//! which performs taint analysis across methods, constructors, and expressions.

use crate::finding::{Finding, Location};
use crate::taint::TaintTracker;
use crate::type_resolver::TypeAwareAnalyzer;

use hyperion_parser::syntax::{AstNode, ConstructorDecl, MethodDecl, SyntaxKind, SyntaxNode};
use std::path::PathBuf;
use tracing::debug;

use super::utilities::{offset_to_line_col, SecurityScanner};

pub(super) struct SecurityVisitor<'a> {
    pub(super) scanner: &'a SecurityScanner,
    pub(super) source: &'a str,
    pub(super) file: PathBuf,
    pub(super) findings: Vec<Finding>,

    // Taint tracker handles all taint analysis
    pub(super) taint_tracker: TaintTracker<'a>,

    // Type-aware analyzer for reducing false positives
    pub(super) type_analyzer: TypeAwareAnalyzer,
}

impl<'a> SecurityVisitor<'a> {
    pub(super) fn new(scanner: &'a SecurityScanner, source: &'a str, file: PathBuf) -> Self {
        let type_analyzer = TypeAwareAnalyzer::new();
        let taint_tracker = TaintTracker::new(
            &scanner.source_registry,
            TypeAwareAnalyzer::new(),
            scanner.sanitizer_registry.clone(),
        );
        Self {
            scanner,
            source,
            file,
            findings: Vec::new(),
            taint_tracker,
            type_analyzer,
        }
    }

    pub(super) fn visit_type_decl(&mut self, node: &SyntaxNode) {
        // Collect method nodes for this class
        let methods: Vec<SyntaxNode> = node
            .children()
            .filter(|child| MethodDecl::can_cast(child.kind()))
            .collect();

        // Collect constructor nodes for this class
        let constructors: Vec<SyntaxNode> = node
            .children()
            .filter(|child| ConstructorDecl::can_cast(child.kind()))
            .collect();

        // Clear field taint before analyzing this class
        self.taint_tracker.clear_field_taint();
        self.collect_default_deserialized_fields(node, &methods);

        // ========================================================================
        // PASS 0: Build method parameter map for interprocedural analysis
        // ========================================================================
        debug!("Pass 0: Building method parameter map");
        let method_params = Self::build_method_param_map(&methods);

        // ========================================================================
        // PASS 1: Collect field taint sources and method call taint edges
        // ========================================================================
        // This pass identifies which fields receive tainted data from user input
        // and tracks method calls with tainted arguments for interprocedural analysis
        debug!("Pass 1: Collecting field taint sources and method call edges");

        // Track method calls: (callee_method_name, param_index) -> is_tainted
        let mut tainted_method_params: std::collections::HashSet<(String, usize)> =
            std::collections::HashSet::new();

        for method_node in &methods {
            let method_call_edges =
                self.visit_method_for_field_taint_collection_with_calls(method_node);
            for (callee, param_idx) in method_call_edges {
                tainted_method_params.insert((callee, param_idx));
            }
        }

        for constructor_node in &constructors {
            self.visit_constructor_for_field_taint_collection(constructor_node);
        }

        // Capture the collected field taint state
        let field_taint = self.taint_tracker.get_field_taint();

        #[cfg(test)]
        if !field_taint.is_empty() {
            eprintln!("=== Collected field taint after Pass 1 ===");
            for field in field_taint.keys() {
                eprintln!("  Field '{}' is tainted", field);
            }
        }

        #[cfg(test)]
        if !tainted_method_params.is_empty() {
            eprintln!("=== Tainted method params after Pass 1 ===");
            for (method, idx) in &tainted_method_params {
                eprintln!("  Method '{}' param {} is tainted", method, idx);
            }
        }

        // ========================================================================
        // PASS 1.5: Propagate taint across method calls using fixpoint
        // ========================================================================
        debug!("Pass 1.5: Propagating taint across method calls");
        let propagated_taint =
            self.propagate_method_taint(&methods, &method_params, &tainted_method_params);

        #[cfg(test)]
        if !propagated_taint.is_empty() {
            eprintln!("=== Propagated taint after Pass 1.5 ===");
            for (method, idx) in &propagated_taint {
                eprintln!(
                    "  Method '{}' param {} is tainted (propagated)",
                    method, idx
                );
            }
        }

        // ========================================================================
        // PASS 2: Analyze methods and constructors with field taint pre-initialized
        // ========================================================================
        // This pass reports actual vulnerabilities, using field taint state from pass 1
        // and interprocedural taint from pass 1.5
        debug!("Pass 2: Analyzing methods with cross-method field taint");

        // Re-initialize with collected field taint
        self.taint_tracker.initialize_field_taint(field_taint);

        for method_node in &methods {
            self.visit_method_with_propagated_taint(method_node, &method_params, &propagated_taint);
        }

        for constructor_node in &constructors {
            self.visit_constructor(constructor_node);
        }

        // Handle nested types (recursively)
        for child in node.children() {
            if child.kind() == SyntaxKind::CLASS_DECL
                || child.kind() == SyntaxKind::INTERFACE_DECL
                || child.kind() == SyntaxKind::ENUM_DECL
            {
                // Nested type - process as a separate class
                self.visit_type_decl(&child);
            }
        }
    }

    pub(super) fn node_location(&self, node: &SyntaxNode) -> Location {
        let range = node.text_range();
        let start_offset = range.start().into();
        let end_offset = range.end().into();

        let (start_line, start_col) = self.offset_to_line_col(start_offset);
        let (end_line, end_col) = self.offset_to_line_col(end_offset);

        Location {
            file: self.file.clone(),
            line: start_line,
            column: start_col,
            end_line,
            end_column: end_col,
        }
    }

    fn offset_to_line_col(&self, offset: usize) -> (u32, u32) {
        offset_to_line_col(self.source, offset)
    }

    pub(super) fn get_code_snippet(node: &SyntaxNode) -> String {
        node.text().to_string()
    }
}

#[cfg(test)]
#[path = "visitor_tests.rs"]
mod tests;
