// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method summary generation for interprocedural taint analysis.
//!
//! Generates `MethodSummary` instances by analyzing method bodies using
//! symbolic taint tracking. These summaries capture:
//! - Return value dependencies (which params affect the return)
//! - Call edges (which params flow to which callee arguments)
//! - Source detection (annotations and in-method sources)
//! - Constant return detection

use hyperion_parser::{SyntaxKind, SyntaxNode};

use crate::analysis_context::AnalysisContext;
use crate::call_target::{call_target_class, TypeScope};
use crate::constant_eval::{ConstantEvaluator, ConstantValue, ReturnValueKind};
use crate::interprocedural::MethodSummary;
use crate::symbolic_taint::{SymbolicTaint, TaintOrigins};
use crate::symbolic_tracker::SymbolicTracker;
use crate::taint::VulnTypes;

/// Generates method summaries for interprocedural analysis.
pub struct SummaryGenerator<'a> {
    /// Reference to the analysis context.
    context: &'a AnalysisContext,
    /// Constant evaluator for detecting constant return values.
    constant_eval: ConstantEvaluator,
}

/// Summary of a call edge from caller to callee.
#[derive(Debug, Clone)]
pub struct CallEdgeSummary {
    /// FQN of the called method.
    pub callee_fqn: String,
    /// Mapping from caller param index to callee param index.
    /// If caller param 0 flows to callee param 1, this would contain (0, 1).
    pub arg_flows: Vec<(usize, usize)>,
}

impl<'a> SummaryGenerator<'a> {
    /// Create a new summary generator.
    pub fn new(context: &'a AnalysisContext) -> Self {
        // Create a constant evaluator with the context's constant pool
        let mut constant_eval = ConstantEvaluator::new();
        for (fqn, value) in &context.constant_pool {
            constant_eval.add_constant(fqn.clone(), value.clone());
        }

        Self {
            context,
            constant_eval,
        }
    }

    /// Generate a summary for a method.
    pub fn generate_summary(
        &self,
        method: &SyntaxNode,
        method_fqn: &str,
        class_fqn: &str,
    ) -> MethodSummary {
        let mut summary = MethodSummary::new(method_fqn.to_string());

        // Extract parameter names
        let param_names = extract_param_names(method);

        // Check for source annotations on parameters
        if let Some(annotations) = self.context.get_source_annotations(method_fqn) {
            for (param_idx, _source_kind) in annotations {
                summary.add_tainted_param(*param_idx, VulnTypes::all_vulns());
            }
        }

        // Create symbolic tracker and analyze the method body
        let mut tracker = SymbolicTracker::new(param_names.clone());

        // Find the method body and analyze it
        if let Some(body) = find_method_body(method) {
            let scope = TypeScope::for_node(&self.context.workspace, method);
            self.analyze_body(&body, &mut tracker, &mut summary, class_fqn, &scope);
        }

        // Analyze return value
        let return_kind = self.analyze_return_kind(method);
        summary.return_kind = return_kind;

        // Analyze return dependencies
        let return_depends_on = Self::analyze_return_dependencies(method, &param_names);
        summary.return_depends_on = return_depends_on;

        // Set return taint based on dependencies
        let influenced_params = summary.return_depends_on.influenced_by_params();
        if !influenced_params.is_empty() {
            summary.set_return_taint(influenced_params);
        }

        summary
    }

    /// Analyze the return value kind (constant vs dynamic).
    fn analyze_return_kind(&self, method: &SyntaxNode) -> ReturnValueKind {
        let mut all_return_exprs = Vec::new();
        collect_return_expressions(method, &mut all_return_exprs);

        if all_return_exprs.is_empty() {
            return ReturnValueKind::Unknown;
        }

        // Check if all return expressions are constants
        let mut all_constant_strings = true;
        let mut all_constant_string_lists = true;

        for expr in &all_return_exprs {
            match self.constant_eval.evaluate(expr) {
                Some(ConstantValue::String(_)) => {
                    all_constant_string_lists = false;
                }
                Some(ConstantValue::StringList(_)) => {
                    all_constant_strings = false;
                }
                _ => {
                    all_constant_strings = false;
                    all_constant_string_lists = false;
                }
            }
        }

        if all_constant_strings && !all_return_exprs.is_empty() {
            ReturnValueKind::ConstString
        } else if all_constant_string_lists && !all_return_exprs.is_empty() {
            ReturnValueKind::ConstStringList
        } else {
            ReturnValueKind::Unknown
        }
    }

    /// Analyze which parameters the return value depends on.
    fn analyze_return_dependencies(method: &SyntaxNode, param_names: &[String]) -> TaintOrigins {
        let mut tracker = SymbolicTracker::new(param_names.to_vec());
        let mut combined_origins = TaintOrigins::empty();

        // Find method body and analyze return statements
        if let Some(body) = find_method_body(method) {
            // Track assignments in the body
            for node in body.descendants() {
                if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                    if let Some((name, expr)) = extract_var_decl_info(&node) {
                        tracker.track_assignment(&name, &expr);
                    }
                } else if node.kind() == SyntaxKind::ASSIGNMENT_EXPR {
                    if let Some((name, expr)) = extract_assignment_info(&node) {
                        tracker.track_assignment(&name, &expr);
                    }
                }
            }

            // Collect return expression taints
            let mut return_exprs = Vec::new();
            collect_return_expressions(&body, &mut return_exprs);

            for return_expr in return_exprs {
                let taint = tracker.get_return_taint(&return_expr);
                combined_origins |= taint.origins;
            }
        }

        combined_origins
    }

    /// Analyze a method body, tracking taint flow.
    #[allow(clippy::unused_self)]
    fn analyze_body(
        &self,
        body: &SyntaxNode,
        tracker: &mut SymbolicTracker,
        summary: &mut MethodSummary,
        class_fqn: &str,
        scope: &TypeScope<'_>,
    ) {
        for node in body.descendants() {
            match node.kind() {
                SyntaxKind::LOCAL_VAR_DECL => {
                    if let Some((name, expr)) = extract_var_decl_info(&node) {
                        // Check for source detection
                        if is_source_expression(&expr) {
                            tracker.add_source_origin(&name, TaintOrigins::SOURCE_HTTP);
                            summary.has_body_sources = true;
                        } else {
                            tracker.track_assignment(&name, &expr);
                        }
                    }
                }
                SyntaxKind::ASSIGNMENT_EXPR => {
                    if let Some((name, expr)) = extract_assignment_info(&node) {
                        if is_source_expression(&expr) {
                            tracker.add_source_origin(&name, TaintOrigins::SOURCE_HTTP);
                            summary.has_body_sources = true;
                        } else {
                            tracker.track_assignment(&name, &expr);
                        }
                    }
                }
                SyntaxKind::METHOD_CALL_EXPR => {
                    // Check for sink calls
                    if let Some(sink_info) = Self::check_for_sink(&node, tracker) {
                        summary.add_sink_call(sink_info.0, sink_info.1);
                    }

                    // Extract call edges and track source-tainted args
                    if let Some(edge) = Self::extract_call_edge(&node, tracker, class_fqn, scope) {
                        // Record the call in the summary
                        summary
                            .calls_sinks
                            .entry(edge.callee_fqn.clone())
                            .or_insert(VulnTypes::empty());
                    }

                    // Track args tainted from body sources flowing to callees
                    if summary.has_body_sources {
                        if let Some(callee_fqn) = Self::extract_callee_fqn(&node, class_fqn, scope)
                        {
                            if let Some(args) = find_arg_list(&node) {
                                for (callee_idx, arg) in args
                                    .children()
                                    .filter(|c| is_expression_kind(c.kind()))
                                    .enumerate()
                                {
                                    let taint = evaluate_expr_taint(&arg, tracker);
                                    if taint.origins.has_any_source() {
                                        summary
                                            .source_tainted_calls
                                            .push((callee_fqn.clone(), callee_idx));
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Check if a method call is a sink with tainted arguments.
    fn check_for_sink(call: &SyntaxNode, tracker: &SymbolicTracker) -> Option<(String, VulnTypes)> {
        let method_name = extract_method_name(call)?;

        // Check for SQL injection sinks
        let sql_sinks = [
            "executeQuery",
            "executeUpdate",
            "execute",
            "prepareStatement",
            "prepareCall",
        ];
        if sql_sinks.contains(&method_name.as_str()) {
            // Check if any argument is tainted
            if let Some(args) = find_arg_list(call) {
                for (idx, arg) in args
                    .children()
                    .filter(|c| is_expression_kind(c.kind()))
                    .enumerate()
                {
                    let taint = evaluate_expr_taint(&arg, tracker);
                    if taint.is_tainted() && idx == 0 {
                        return Some((
                            format!("Connection.{method_name}"),
                            VulnTypes::SQL_INJECTION,
                        ));
                    }
                }
            }
        }

        None
    }

    /// Resolve the callee FQN for a method call node. `None` when the call's class does not resolve.
    fn extract_callee_fqn(
        call: &SyntaxNode,
        class_fqn: &str,
        scope: &TypeScope<'_>,
    ) -> Option<String> {
        let method_name = extract_method_name(call)?;
        let target_class = call_target_class(call, class_fqn, scope)?;
        Some(format!("{target_class}.{method_name}"))
    }

    /// Extract call edge information from a method call.
    fn extract_call_edge(
        call: &SyntaxNode,
        tracker: &SymbolicTracker,
        class_fqn: &str,
        scope: &TypeScope<'_>,
    ) -> Option<CallEdgeSummary> {
        let method_name = extract_method_name(call)?;

        // Resolve callee FQN
        let target_class = call_target_class(call, class_fqn, scope)?;
        let callee_fqn = format!("{target_class}.{method_name}");

        // Analyze argument flows
        let mut arg_flows = Vec::new();
        if let Some(args) = find_arg_list(call) {
            for (callee_idx, arg) in args
                .children()
                .filter(|c| is_expression_kind(c.kind()))
                .enumerate()
            {
                let taint = evaluate_expr_taint(&arg, tracker);
                for param_idx in taint.influenced_params() {
                    arg_flows.push((param_idx, callee_idx));
                }
            }
        }

        Some(CallEdgeSummary {
            callee_fqn,
            arg_flows,
        })
    }

    /// Generate summaries for all methods in a file.
    pub fn generate_summaries_for_file(
        &self,
        root: &SyntaxNode,
        package_name: Option<&str>,
    ) -> Vec<MethodSummary> {
        let mut summaries = Vec::new();

        for class_node in root.descendants() {
            if !matches!(
                class_node.kind(),
                SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL
            ) {
                continue;
            }

            let class_name = extract_class_simple_name(&class_node);
            let Some(class_name) = class_name else {
                continue;
            };

            let class_fqn = package_name
                .map_or_else(|| class_name.clone(), |pkg| format!("{pkg}.{class_name}"));

            // Find methods in this class
            for method_node in class_node.descendants() {
                if !matches!(
                    method_node.kind(),
                    SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
                ) {
                    continue;
                }

                let method_name = extract_method_simple_name(&method_node);
                let Some(method_name) = method_name else {
                    continue;
                };

                let method_fqn = format!("{class_fqn}.{method_name}");
                let summary = self.generate_summary(&method_node, &method_fqn, &class_fqn);
                summaries.push(summary);
            }
        }

        summaries
    }
}

// Helper functions

fn extract_param_names(method: &SyntaxNode) -> Vec<String> {
    let mut params = Vec::new();

    for child in method.children() {
        if child.kind() == SyntaxKind::PARAM_LIST {
            for param in child.children() {
                if param.kind() == SyntaxKind::PARAM {
                    // Find the parameter name (IDENT)
                    for token in param.children_with_tokens() {
                        if let Some(t) = token.as_token() {
                            if t.kind() == SyntaxKind::IDENT {
                                params.push(t.text().to_string());
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    params
}

fn find_method_body(method: &SyntaxNode) -> Option<SyntaxNode> {
    method
        .children()
        .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
}

fn collect_return_expressions(node: &SyntaxNode, exprs: &mut Vec<SyntaxNode>) {
    for descendant in node.descendants() {
        if descendant.kind() == SyntaxKind::RETURN_STMT {
            for child in descendant.children() {
                if is_expression_kind(child.kind()) {
                    exprs.push(child);
                    break;
                }
            }
        }
    }
}

fn extract_var_decl_info(node: &SyntaxNode) -> Option<(String, SyntaxNode)> {
    let mut var_name = None;
    let mut init_expr = None;

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT && var_name.is_none() {
                    var_name = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(n) => {
                if is_expression_kind(n.kind()) {
                    init_expr = Some(n);
                }
            }
        }
    }

    match (var_name, init_expr) {
        (Some(name), Some(expr)) => Some((name, expr)),
        _ => None,
    }
}

fn extract_assignment_info(node: &SyntaxNode) -> Option<(String, SyntaxNode)> {
    let mut found_eq = false;
    let mut lhs_name = None;
    let mut rhs_expr = None;

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::EQ {
                    found_eq = true;
                }
            }
            rowan::NodeOrToken::Node(n) => {
                if !found_eq {
                    // Left-hand side
                    if n.kind() == SyntaxKind::NAME_EXPR {
                        for t in n.children_with_tokens() {
                            if let Some(token) = t.as_token() {
                                if token.kind() == SyntaxKind::IDENT {
                                    lhs_name = Some(token.text().to_string());
                                }
                            }
                        }
                    }
                } else if is_expression_kind(n.kind()) {
                    rhs_expr = Some(n);
                }
            }
        }
    }

    match (lhs_name, rhs_expr) {
        (Some(name), Some(expr)) => Some((name, expr)),
        _ => None,
    }
}

fn is_source_expression(expr: &SyntaxNode) -> bool {
    // Check for HTTP source patterns like request.getParameter()
    if expr.kind() == SyntaxKind::METHOD_CALL_EXPR {
        let text = expr.text().to_string();
        if text.contains("getParameter")
            || text.contains("getHeader")
            || text.contains("getCookie")
            || text.contains("getInputStream")
            || text.contains("getReader")
        {
            return true;
        }
    }
    false
}

fn extract_method_name(call: &SyntaxNode) -> Option<String> {
    let mut last_ident = None;

    for child in call.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT {
                    last_ident = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::ARG_LIST {
                    return last_ident;
                }
                if node.kind() == SyntaxKind::NAME_EXPR {
                    for inner in node.children_with_tokens() {
                        if let Some(token) = inner.as_token() {
                            if token.kind() == SyntaxKind::IDENT {
                                last_ident = Some(token.text().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    last_ident
}

fn find_arg_list(call: &SyntaxNode) -> Option<SyntaxNode> {
    call.children()
        .find(|child| child.kind() == SyntaxKind::ARG_LIST)
}

fn evaluate_expr_taint(expr: &SyntaxNode, tracker: &SymbolicTracker) -> SymbolicTaint {
    // Delegate to tracker for taint evaluation
    tracker.get_return_taint(expr)
}

fn extract_class_simple_name(class: &SyntaxNode) -> Option<String> {
    for child in class.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_method_simple_name(method: &SyntaxNode) -> Option<String> {
    if method.kind() == SyntaxKind::CONSTRUCTOR_DECL {
        return Some("<init>".to_string());
    }

    for child in method.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

const fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};
    use hyperion_workspace::WorkspaceIndex;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn parse_java(code: &str) -> SyntaxNode {
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green)
    }

    fn create_empty_context() -> AnalysisContext {
        let workspace = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp")));
        AnalysisContext::from_workspace(workspace)
    }

    #[test]
    fn test_extract_param_names() {
        let code = r#"
            class Test {
                void foo(String a, int b, List<String> c) {}
            }
        "#;
        let root = parse_java(code);
        let method = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::METHOD_DECL)
            .unwrap();
        let params = extract_param_names(&method);
        assert_eq!(params, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_generate_summary_constant_return() {
        let code = r#"
            class Test {
                String getQuery() {
                    return "SELECT * FROM users";
                }
            }
        "#;
        let root = parse_java(code);
        let context = create_empty_context();
        let generator = SummaryGenerator::new(&context);

        let summaries = generator.generate_summaries_for_file(&root, None);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].return_kind, ReturnValueKind::ConstString);
    }

    #[test]
    fn test_generate_summary_param_propagation() {
        let code = r#"
            class Test {
                String process(String input) {
                    return input.toLowerCase();
                }
            }
        "#;
        let root = parse_java(code);
        let context = create_empty_context();
        let generator = SummaryGenerator::new(&context);

        let summaries = generator.generate_summaries_for_file(&root, None);
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0]
            .return_depends_on
            .contains(TaintOrigins::PARAM_0));
    }

    #[test]
    fn test_generate_summary_detects_sink() {
        let code = r#"
            class Test {
                void query(String sql, Connection conn) throws Exception {
                    conn.executeQuery(sql);
                }
            }
        "#;
        let root = parse_java(code);
        let context = create_empty_context();
        let generator = SummaryGenerator::new(&context);

        let summaries = generator.generate_summaries_for_file(&root, None);
        assert_eq!(summaries.len(), 1);
        // The sink should be detected
        assert!(!summaries[0].calls_sinks.is_empty());
    }
}
