// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Interprocedural analysis methods for the SecurityScanner.
//!
//! This module contains cross-method and cross-file analysis logic,
//! including fixpoint-driven taint propagation and simple AST-based
//! taint tracking for method bodies.

use crate::finding::{Finding, Location, VulnType};
use crate::logging_context::LoggingSafety;
use crate::taint::{TaintState, VulnTypes};

use hyperion_parser::SyntaxKind;
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};
use std::path::Path;
use tracing::{info, instrument, warn};

use super::utilities::{is_expr_kind, offset_to_line_col, SecurityScanner};

impl SecurityScanner {
    /// Scan a directory using interprocedural analysis.
    ///
    /// This method builds a workspace index, runs fixpoint analysis
    /// to propagate taint across method calls, and then scans each
    /// file with the pre-computed taint information.
    ///
    /// Use this method when `config.interprocedural` is true.
    #[instrument(skip(self), fields(dir = %dir.display()))]
    pub fn scan_with_context(&self, dir: &Path) -> Vec<Finding> {
        use crate::analysis_context::AnalysisContext;
        use crate::fixpoint::FixpointAnalysis;

        info!("Starting interprocedural analysis for {}", dir.display());

        // Build analysis context
        let mut context = match AnalysisContext::from_directory(dir) {
            Ok(ctx) => ctx,
            Err(e) => {
                // The per-file pass has already run, so there is no cross-file result to add.
                warn!(error = %e, "Failed to build analysis context, cross-file analysis skipped");
                return Vec::new();
            }
        };

        // Run fixpoint analysis. The fixpoint takes its call edges from its own
        // extraction, so no separate call graph is built here.
        let mut fixpoint = FixpointAnalysis::new(&mut context);
        let result = fixpoint.run();

        info!(
            iterations = result.iterations,
            converged = result.converged,
            tainted_methods = result.tainted_params.len(),
            "Fixpoint analysis complete"
        );

        // Now scan each file with the pre-computed taint information
        let mut all_findings = Vec::new();

        for file_id in context.workspace.all_file_ids() {
            let Some(path) = context.workspace.get_file_path(file_id) else {
                continue;
            };

            if path.extension().is_none_or(|ext| ext != "java") {
                continue;
            }

            if self.is_excluded(&path) {
                continue;
            }

            let Some(contents) = context.workspace.get_file_contents(file_id) else {
                continue;
            };

            // Scan the file with taint pre-seeded from fixpoint
            let findings = Self::scan_source_with_tainted_params(&contents, &path, &context);
            all_findings.extend(findings);
        }

        // Deduplicate findings by their unique id
        let mut seen_ids = std::collections::HashSet::new();
        all_findings.retain(|f| seen_ids.insert(f.id.clone()));
        all_findings
    }

    /// Scan source code with pre-computed tainted parameters from fixpoint analysis.
    fn scan_source_with_tainted_params(
        source: &str,
        path: &Path,
        context: &crate::analysis_context::AnalysisContext,
    ) -> Vec<Finding> {
        let tokens: Vec<_> = Lexer::new(source).collect();
        let (green, _errors) = Parser::parse(tokens);
        let root = RawSyntaxNode::new_root(green);

        let package_name = Self::extract_package_name_from_root(&root);
        let mut findings = Vec::new();

        for class_node in root.descendants() {
            if !matches!(
                class_node.kind(),
                SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL
            ) {
                continue;
            }

            let class_name = Self::extract_class_name_from_node(&class_node);
            let Some(class_name) = class_name else {
                continue;
            };

            let class_fqn = package_name
                .as_ref()
                .map_or_else(|| class_name.clone(), |pkg| format!("{pkg}.{class_name}"));

            // Scan each method
            for method_node in class_node.descendants() {
                if !matches!(
                    method_node.kind(),
                    SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
                ) {
                    continue;
                }

                let method_name = if method_node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
                    "<init>".to_string()
                } else {
                    Self::extract_method_name_from_node(&method_node).unwrap_or_default()
                };

                let method_fqn = format!("{class_fqn}.{method_name}");

                // Get pre-computed tainted params from fixpoint
                let tainted_params = context
                    .tainted_params
                    .get(&method_fqn)
                    .cloned()
                    .unwrap_or_default();

                // Scan the method body with pre-seeded taint
                let method_findings = Self::scan_method_with_tainted_params(
                    &method_node,
                    path,
                    source,
                    &tainted_params,
                );
                findings.extend(method_findings);
            }
        }

        findings
    }

    /// Scan a method body with pre-seeded tainted parameters.
    /// Uses a simple HashMap-based taint tracker for interprocedural analysis.
    fn scan_method_with_tainted_params(
        method: &RawSyntaxNode,
        path: &Path,
        source: &str,
        tainted_params: &std::collections::HashSet<usize>,
    ) -> Vec<Finding> {
        use std::collections::HashMap;

        // Simple taint tracker: variable name -> is_tainted
        let mut taint_map: HashMap<String, TaintState> = HashMap::new();
        let param_names = Self::extract_param_names_from_node(method);

        // Mark parameters as tainted based on fixpoint results
        for &param_idx in tainted_params {
            if param_idx < param_names.len() {
                let param_name = &param_names[param_idx];
                let location = Location {
                    file: path.to_path_buf(),
                    line: 1,
                    column: 1,
                    end_line: 1,
                    end_column: 1,
                };
                taint_map.insert(
                    param_name.clone(),
                    TaintState::tainted(
                        param_name.clone(),
                        location,
                        "Parameter from caller".to_string(),
                        VulnTypes::all_vulns(),
                    ),
                );
            }
        }

        // Scan the method body for sinks
        Self::scan_method_body_for_sinks_simple(method, path, source, &mut taint_map)
    }

    /// Scan a method body for sinks using simple taint tracking.
    fn scan_method_body_for_sinks_simple(
        method: &RawSyntaxNode,
        path: &Path,
        source: &str,
        taint_map: &mut std::collections::HashMap<String, TaintState>,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Find all statements in the method body
        for node in method.descendants() {
            // Track assignments
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                if let Some((name, expr)) = Self::extract_var_decl_from_node(&node) {
                    // Check if initializer is tainted
                    let taint =
                        Self::evaluate_expression_taint_simple(&expr, taint_map, path, source);
                    if taint.is_tainted() {
                        taint_map.insert(name, taint);
                    }
                }
            }

            // Check for sinks
            if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                if let Some(finding) =
                    Self::check_method_call_sink_simple(&node, taint_map, path, source)
                {
                    findings.push(finding);
                }
            }
        }

        findings
    }

    /// Evaluate expression taint state using simple taint tracking.
    fn evaluate_expression_taint_simple(
        expr: &RawSyntaxNode,
        taint_map: &std::collections::HashMap<String, TaintState>,
        path: &Path,
        source: &str,
    ) -> TaintState {
        match expr.kind() {
            SyntaxKind::NAME_EXPR => {
                Self::extract_ident_from_node(expr).map_or_else(TaintState::clean, |name| {
                    taint_map
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(TaintState::clean)
                })
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                // Check if this is a source
                let text = expr.text().to_string();
                if text.contains("getParameter")
                    || text.contains("getHeader")
                    || text.contains("getCookie")
                {
                    let location = Self::node_location_raw(expr, source, path);
                    return TaintState::tainted(
                        "request".to_string(),
                        location,
                        text,
                        VulnTypes::all_vulns(),
                    );
                }
                // Otherwise, propagate taint from the receiver and the arguments. The operands
                // are the children, not the descendants: `descendants()` includes this call.
                let mut operands = Vec::new();
                for child in expr.children() {
                    if child.kind() == SyntaxKind::ARG_LIST {
                        operands.extend(child.children());
                    } else {
                        operands.push(child);
                    }
                }
                for operand in operands {
                    if is_expr_kind(operand.kind()) {
                        let operand_taint = Self::evaluate_expression_taint_simple(
                            &operand, taint_map, path, source,
                        );
                        if operand_taint.is_tainted() {
                            return operand_taint;
                        }
                    }
                }
                TaintState::clean()
            }
            SyntaxKind::BINARY_EXPR => {
                // Check if either operand is tainted
                for child in expr.children() {
                    if is_expr_kind(child.kind()) {
                        let child_taint =
                            Self::evaluate_expression_taint_simple(&child, taint_map, path, source);
                        if child_taint.is_tainted() {
                            return child_taint;
                        }
                    }
                }
                TaintState::clean()
            }
            _ => TaintState::clean(),
        }
    }

    /// Check if a method call is a sink with tainted arguments (simple version).
    fn check_method_call_sink_simple(
        call: &RawSyntaxNode,
        taint_map: &std::collections::HashMap<String, TaintState>,
        path: &Path,
        source: &str,
    ) -> Option<Finding> {
        let method_name = Self::extract_called_method_name_from_node(call)?;

        // Check for SQL injection sinks
        let sql_sinks = [
            "executeQuery",
            "executeUpdate",
            "execute",
            "prepareStatement",
            "prepareCall",
        ];

        if sql_sinks.contains(&method_name.as_str()) {
            // Find the first argument
            for child in call.children() {
                if child.kind() == SyntaxKind::ARG_LIST {
                    for arg in child.children() {
                        if is_expr_kind(arg.kind()) {
                            let taint = Self::evaluate_expression_taint_simple(
                                &arg, taint_map, path, source,
                            );
                            if taint.is_tainted() {
                                let location = Self::node_location_raw(call, source, path);
                                let code = call.text().to_string();
                                return Some(Finding::new(
                                    VulnType::SqlInjection,
                                    format!("SQL injection: tainted data flows to {method_name}"),
                                    location,
                                    code,
                                ));
                            }
                            break; // Only check first argument for SQL
                        }
                    }
                }
            }
        }

        None
    }

    /// Extract package name from parsed root node.
    fn extract_package_name_from_root(root: &RawSyntaxNode) -> Option<String> {
        for child in root.children() {
            if child.kind() == SyntaxKind::PACKAGE_DECL {
                // Find the QUALIFIED_NAME child
                for name_child in child.children() {
                    if name_child.kind() == SyntaxKind::QUALIFIED_NAME {
                        return Some(name_child.text().to_string());
                    }
                }
            }
        }
        None
    }

    /// The text of the first identifier token that is a direct child of a node. Identifiers are
    /// tokens, so `children()`, which yields only nodes, never finds one.
    fn first_ident_token(node: &RawSyntaxNode) -> Option<String> {
        node.children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::IDENT)
            .map(|token| token.text().to_string())
    }

    /// Extract class name from class/interface declaration node.
    fn extract_class_name_from_node(node: &RawSyntaxNode) -> Option<String> {
        Self::first_ident_token(node)
    }

    /// Extract method name from method declaration node.
    fn extract_method_name_from_node(node: &RawSyntaxNode) -> Option<String> {
        Self::first_ident_token(node)
    }

    /// Extract parameter names from method node.
    fn extract_param_names_from_node(method: &RawSyntaxNode) -> Vec<String> {
        let mut params = Vec::new();
        for child in method.descendants() {
            if child.kind() == SyntaxKind::PARAM {
                // The parameter's name is its identifier token.
                if let Some(name) = Self::first_ident_token(&child) {
                    params.push(name);
                }
            }
        }
        params
    }

    /// Extract variable declaration (name and initializer).
    /// LOCAL_VAR_DECL structure: [modifiers] Type IDENT [= expression]
    fn extract_var_decl_from_node(node: &RawSyntaxNode) -> Option<(String, RawSyntaxNode)> {
        let mut name = None;
        let mut init = None;
        let mut found_eq = false;

        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT && name.is_none() {
                    name = Some(token.text().to_string());
                } else if token.kind() == SyntaxKind::EQ {
                    found_eq = true;
                }
            } else if let Some(expr) = child.as_node() {
                if found_eq && is_expr_kind(expr.kind()) && init.is_none() {
                    init = Some(expr.clone());
                }
            }
        }

        match (name, init) {
            (Some(n), Some(i)) => Some((n, i)),
            _ => None,
        }
    }

    /// Extract identifier from name expression.
    fn extract_ident_from_node(node: &RawSyntaxNode) -> Option<String> {
        if let Some(name) = Self::first_ident_token(node) {
            return Some(name);
        }
        // If no IDENT child, try the node text itself
        if node.kind() == SyntaxKind::NAME_EXPR {
            let text = node.text().to_string().trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
        None
    }

    /// Extract called method name from method call expression. The name is the last identifier
    /// before the argument list: a token in `receiver.name(...)`, or the name of an unqualified
    /// `name(...)`, which is a `NAME_EXPR`.
    fn extract_called_method_name_from_node(call: &RawSyntaxNode) -> Option<String> {
        let mut name = None;
        for child in call.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    name = Some(token.text().to_string());
                }
            } else if let Some(node) = child.as_node() {
                match node.kind() {
                    SyntaxKind::ARG_LIST => break,
                    SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR => {
                        if let Some(ident) = last_ident_token(node) {
                            name = Some(ident);
                        }
                    }
                    _ => {}
                }
            }
        }
        name
    }

    /// Get location from a raw syntax node.
    fn node_location_raw(node: &RawSyntaxNode, source: &str, path: &Path) -> Location {
        let range = node.text_range();
        let start_offset: usize = range.start().into();
        let end_offset: usize = range.end().into();

        let (start_line, start_col) = offset_to_line_col(source, start_offset);
        let (end_line, end_col) = offset_to_line_col(source, end_offset);

        Location {
            file: path.to_path_buf(),
            line: start_line,
            column: start_col,
            end_line,
            end_column: end_col,
        }
    }

    pub(crate) fn logging_safety(&self) -> LoggingSafety {
        self.logging_context
            .read()
            .map_or(LoggingSafety::Unknown, |context| context.safety())
    }
}

/// The text of the last identifier token that is a direct child of a node.
fn last_ident_token(node: &RawSyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(|child| child.into_token())
        .filter(|token| token.kind() == SyntaxKind::IDENT)
        .last()
        .map(|token| token.text().to_string())
}
