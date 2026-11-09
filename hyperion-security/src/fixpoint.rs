// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Fixpoint analysis for interprocedural taint propagation.
//!
//! Implements a worklist algorithm that propagates taint across the call graph
//! until a stable state is reached. This is the core of cross-file taint analysis.
//!
//! ## Algorithm
//!
//! 1. Initialize worklist with methods having source annotations
//! 2. Generate initial summaries for all methods
//! 3. While worklist is not empty:
//!    a. Pop method M from worklist
//!    b. For each call edge M → N with argument flows:
//!       - If caller param P is tainted AND P flows to callee param Q
//!       - Mark Q as tainted in N's entry state
//!       - If N's entry state changed, add N to worklist
//! 4. Max 100 iterations to prevent divergence

use std::collections::{HashSet, VecDeque};

use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};

use crate::analysis_context::AnalysisContext;
use crate::call_target::{call_target_class, TypeScope};
use crate::summary_generator::{CallEdgeSummary, SummaryGenerator};
use crate::taint::VulnTypes;

/// Maximum iterations before assuming divergence.
const MAX_ITERATIONS: usize = 100;

/// Result of fixpoint analysis.
#[derive(Debug, Clone)]
pub struct FixpointResult {
    /// Method FQN → set of tainted parameter indices at entry.
    pub tainted_params: std::collections::HashMap<String, HashSet<usize>>,
    /// Methods that may reach sinks with tainted data.
    pub reachable_sinks: std::collections::HashMap<String, Vec<SinkInfo>>,
    /// Number of iterations until stable.
    pub iterations: usize,
    /// Whether the analysis converged within the limit.
    pub converged: bool,
}

/// Information about a reachable sink.
#[derive(Debug, Clone)]
pub struct SinkInfo {
    /// FQN of the sink method.
    pub sink_fqn: String,
    /// Vulnerability types the sink is sensitive to.
    pub vuln_types: VulnTypes,
    /// Path from source to sink (method FQNs).
    pub path: Vec<String>,
}

/// Fixpoint analysis for interprocedural taint propagation.
pub struct FixpointAnalysis<'a> {
    /// Reference to the analysis context.
    context: &'a mut AnalysisContext,
    /// Worklist of methods to process.
    worklist: VecDeque<String>,
    /// Methods already in the worklist (for deduplication).
    in_worklist: HashSet<String>,
    /// Call edges extracted during summary generation.
    call_edges: std::collections::HashMap<String, Vec<CallEdgeSummary>>,
    /// Iteration counter.
    iteration_count: usize,
}

impl<'a> FixpointAnalysis<'a> {
    /// Create a new fixpoint analysis.
    pub fn new(context: &'a mut AnalysisContext) -> Self {
        Self {
            context,
            worklist: VecDeque::new(),
            in_worklist: HashSet::new(),
            call_edges: std::collections::HashMap::new(),
            iteration_count: 0,
        }
    }

    /// Run the fixpoint analysis.
    pub fn run(&mut self) -> FixpointResult {
        // Phase 1: Generate initial summaries for all methods
        self.generate_all_summaries();

        // Phase 2: Seed worklist from source annotations
        self.seed_from_annotations();

        // Phase 3: Seed from in-method sources (request.getParameter, etc.)
        self.seed_from_method_sources();

        // Phase 4: Propagate taint until stable
        let converged = self.propagate_until_stable();

        // Phase 5: Compute reachable sinks
        let reachable_sinks = self.compute_reachable_sinks();

        FixpointResult {
            tainted_params: self.context.tainted_params.clone(),
            reachable_sinks,
            iterations: self.iteration_count,
            converged,
        }
    }

    /// Generate summaries for all methods in the workspace.
    fn generate_all_summaries(&mut self) {
        use crate::interprocedural::MethodSummary;

        // Collect all file data first (immutable borrow)
        let file_data: Vec<_> = self
            .context
            .workspace
            .all_file_ids()
            .into_iter()
            .filter_map(|file_id| {
                let contents = self.context.workspace.get_file_contents(file_id)?;
                let path = self.context.workspace.get_file_path(file_id)?;

                if path.extension().is_none_or(|ext| ext != "java") {
                    return None;
                }

                Some((file_id, contents, path))
            })
            .collect();

        // Now process each file and collect summaries and edges
        let mut summaries_to_add: Vec<MethodSummary> = Vec::new();
        let mut edges_to_add: Vec<(String, Vec<CallEdgeSummary>)> = Vec::new();

        // The workspace resolves the classes that written types name. A clone lets the
        // generator borrow the context while the scope is in use.
        let workspace = std::sync::Arc::clone(&self.context.workspace);

        // Create a generator (temporarily borrows context)
        {
            let generator = SummaryGenerator::new(self.context);

            for (_file_id, contents, _path) in &file_data {
                let tokens: Vec<_> = Lexer::new(contents).collect();
                let (green, _errors) = Parser::parse(tokens);
                let root = SyntaxNode::new_root(green);

                let package_name = extract_package_name(&root);
                let scope = TypeScope::for_node(&workspace, &root);

                for class_node in root.descendants() {
                    if !matches!(
                        class_node.kind(),
                        SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL
                    ) {
                        continue;
                    }

                    let class_name = extract_class_name(&class_node);
                    let Some(class_name) = class_name else {
                        continue;
                    };

                    let class_fqn = package_name
                        .as_ref()
                        .map_or_else(|| class_name.clone(), |pkg| format!("{pkg}.{class_name}"));

                    // Generate summaries for each method
                    for method_node in class_node.descendants() {
                        if !matches!(
                            method_node.kind(),
                            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
                        ) {
                            continue;
                        }

                        let method_name = extract_method_name(&method_node);
                        let Some(method_name) = method_name else {
                            continue;
                        };

                        let method_fqn = format!("{class_fqn}.{method_name}");
                        let summary =
                            generator.generate_summary(&method_node, &method_fqn, &class_fqn);

                        // Extract call edges for propagation
                        let edges = extract_call_edges_static(&method_node, &class_fqn, &scope);

                        summaries_to_add.push(summary);
                        edges_to_add.push((method_fqn, edges));
                    }
                }
            }
        }

        // Now apply the collected changes (mutable borrow)
        for summary in summaries_to_add {
            self.context.add_summary(summary);
        }
        for (method_fqn, edges) in edges_to_add {
            self.call_edges.insert(method_fqn, edges);
        }
    }

    /// Seed the worklist from source annotations.
    fn seed_from_annotations(&mut self) {
        for (method_fqn, annotations) in &self.context.source_annotations.clone() {
            for (param_idx, _source_kind) in annotations {
                self.context.mark_param_tainted(method_fqn, *param_idx);
            }
            self.add_to_worklist(method_fqn.clone());
        }
    }

    /// Seed the worklist from in-method sources.
    fn seed_from_method_sources(&mut self) {
        // Collect method FQNs and their tainted params first
        let method_fqns: Vec<_> = self.call_edges.keys().cloned().collect();

        for method_fqn in method_fqns {
            let tainted_params: Vec<usize> = self
                .context
                .get_summary(&method_fqn)
                .map(|summary| summary.tainted_params.keys().copied().collect())
                .unwrap_or_default();

            if !tainted_params.is_empty() {
                for param_idx in tainted_params {
                    self.context.mark_param_tainted(&method_fqn, param_idx);
                }
                self.add_to_worklist(method_fqn.clone());
            }

            // Seed from methods with in-body sources (e.g., request.getParameter())
            // These taint callee params directly without going through caller params.
            if let Some(summary) = self.context.get_summary(&method_fqn) {
                if summary.has_body_sources {
                    let source_calls = summary.source_tainted_calls.clone();
                    for (callee_fqn, callee_param_idx) in source_calls {
                        let changed = self
                            .context
                            .mark_param_tainted(&callee_fqn, callee_param_idx);
                        if changed {
                            self.add_to_worklist(callee_fqn);
                        }
                    }
                }
            }
        }
    }

    /// Propagate taint across the call graph until stable.
    fn propagate_until_stable(&mut self) -> bool {
        while let Some(caller_fqn) = self.worklist.pop_front() {
            self.in_worklist.remove(&caller_fqn);
            self.iteration_count += 1;

            if self.iteration_count > MAX_ITERATIONS {
                tracing::warn!(
                    "Fixpoint analysis reached {} iterations, assuming divergence",
                    MAX_ITERATIONS
                );
                return false;
            }

            // Get call edges from this method
            let Some(edges) = self.call_edges.get(&caller_fqn).cloned() else {
                continue;
            };

            // Get tainted params for the caller
            let caller_tainted = self
                .context
                .tainted_params
                .get(&caller_fqn)
                .cloned()
                .unwrap_or_default();

            // Propagate taint across each call edge
            for edge in edges {
                for (caller_param, callee_param) in edge.arg_flows {
                    if caller_tainted.contains(&caller_param) {
                        // Propagate taint to callee
                        let changed = self
                            .context
                            .mark_param_tainted(&edge.callee_fqn, callee_param);

                        if changed {
                            self.add_to_worklist(edge.callee_fqn.clone());
                        }
                    }
                }
            }
        }

        true
    }

    /// Compute which methods can reach sinks with tainted data.
    fn compute_reachable_sinks(&self) -> std::collections::HashMap<String, Vec<SinkInfo>> {
        let mut reachable = std::collections::HashMap::new();

        // For each method with tainted params, check if it has sink calls
        for (method_fqn, tainted_params) in &self.context.tainted_params {
            if tainted_params.is_empty() {
                continue;
            }

            // Get the summary to check for sink calls
            if let Some(summary) = self.context.get_summary(method_fqn) {
                for (sink_fqn, vuln_types) in &summary.calls_sinks {
                    reachable
                        .entry(method_fqn.clone())
                        .or_insert_with(Vec::new)
                        .push(SinkInfo {
                            sink_fqn: sink_fqn.clone(),
                            vuln_types: *vuln_types,
                            path: vec![method_fqn.clone()],
                        });
                }
            }
        }

        reachable
    }

    /// Add a method to the worklist if not already present.
    fn add_to_worklist(&mut self, method_fqn: String) {
        if !self.in_worklist.contains(&method_fqn) {
            self.in_worklist.insert(method_fqn.clone());
            self.worklist.push_back(method_fqn);
        }
    }
}

// Helper functions

fn extract_package_name(root: &SyntaxNode) -> Option<String> {
    for child in root.children() {
        if child.kind() == SyntaxKind::PACKAGE_DECL {
            let mut parts = Vec::new();
            collect_name_parts(&child, &mut parts);
            if !parts.is_empty() {
                return Some(parts.join("."));
            }
        }
    }
    None
}

fn collect_name_parts(node: &SyntaxNode, parts: &mut Vec<String>) {
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT {
                    parts.push(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                collect_name_parts(&node, parts);
            }
        }
    }
}

fn extract_class_name(class_node: &SyntaxNode) -> Option<String> {
    for child in class_node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_method_name(method_node: &SyntaxNode) -> Option<String> {
    if method_node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
        return Some("<init>".to_string());
    }

    for child in method_node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

/// Extract call edges from a method body (static version).
fn extract_call_edges_static(
    method: &SyntaxNode,
    class_fqn: &str,
    scope: &TypeScope<'_>,
) -> Vec<CallEdgeSummary> {
    let mut edges = Vec::new();

    for node in method.descendants() {
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            if let Some(edge) = extract_single_call_edge_static(&node, class_fqn, scope) {
                edges.push(edge);
            }
        }
    }

    edges
}

/// Extract a single call edge from a method call (static version). A call whose target
/// class does not resolve has no edge, rather than an edge to a name no class has.
fn extract_single_call_edge_static(
    call: &SyntaxNode,
    class_fqn: &str,
    scope: &TypeScope<'_>,
) -> Option<CallEdgeSummary> {
    let method_name = extract_called_method_name(call)?;
    let target_class = call_target_class(call, class_fqn, scope)?;
    let arg_flows = extract_arg_flows(call);

    Some(CallEdgeSummary {
        callee_fqn: format!("{target_class}.{method_name}"),
        arg_flows,
    })
}

fn extract_called_method_name(call: &SyntaxNode) -> Option<String> {
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
                // Unqualified call: dao(id) → NAME_EXPR("dao")
                if node.kind() == SyntaxKind::NAME_EXPR {
                    for inner in node.children_with_tokens() {
                        if let Some(token) = inner.as_token() {
                            if token.kind() == SyntaxKind::IDENT {
                                last_ident = Some(token.text().to_string());
                            }
                        }
                    }
                }
                // Qualified call: request.getParameter("id") → FIELD_ACCESS_EXPR
                if node.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                    // The method name is the last IDENT in the field access
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

fn extract_arg_flows(call: &SyntaxNode) -> Vec<(usize, usize)> {
    let mut flows = Vec::new();

    // Find ARG_LIST
    for child in call.children() {
        if child.kind() == SyntaxKind::ARG_LIST {
            for (callee_idx, arg) in child
                .children()
                .filter(|c| is_expression_kind(c.kind()))
                .enumerate()
            {
                // Check if the argument is a simple name reference to a parameter
                if arg.kind() == SyntaxKind::NAME_EXPR {
                    // We'll track this as a potential flow
                    // The actual param index will be determined by the caller's analysis
                    // For now, assume the argument position maps directly
                    flows.push((callee_idx, callee_idx));
                }
            }
        }
    }

    flows
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
    use crate::analysis_context::AnalysisContext;
    use hyperion_workspace::WorkspaceIndex;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn create_test_context(files: &[(&str, &str)]) -> (TempDir, AnalysisContext) {
        let temp_dir = TempDir::new().unwrap();

        let mut java_files = Vec::new();
        for (name, content) in files {
            let path = temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();

            if std::path::Path::new(name)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            {
                java_files.push((path, content.to_string()));
            }
        }

        let workspace = Arc::new(WorkspaceIndex::new(temp_dir.path().to_path_buf()));

        // Index files sequentially to avoid rayon thread pool issues in tests
        for (path, source) in &java_files {
            workspace
                .index_file_with_source(path, source)
                .unwrap_or_else(|e| panic!("Failed to index {}: {e}", path.display()));
        }

        let context = AnalysisContext::from_workspace(workspace);
        (temp_dir, context)
    }

    #[test]
    fn test_fixpoint_simple_propagation() {
        let (_temp, mut context) = create_test_context(&[(
            "Test.java",
            r#"
            package com.example;

            import javax.servlet.http.HttpServletRequest;

            public class Test {
                public void controller(HttpServletRequest request) throws Exception {
                    String id = request.getParameter("id");
                    dao(id);
                }

                private void dao(String id) throws Exception {
                    executeQuery("SELECT * FROM users WHERE id = " + id);
                }
            }
            "#,
        )]);

        let mut analysis = FixpointAnalysis::new(&mut context);
        let result = analysis.run();

        assert!(result.converged);
        // The taint should propagate from controller to dao
        assert!(result.tainted_params.contains_key("com.example.Test.dao"));
    }

    #[test]
    fn test_fixpoint_follows_a_call_through_a_variable_to_its_declared_class() {
        let (_temp, mut context) = create_test_context(&[
            (
                "Controller.java",
                r#"
            package app;

            import dao.UserDao;
            import javax.servlet.http.HttpServletRequest;

            public class Controller {
                public void handle(HttpServletRequest request) throws Exception {
                    String id = request.getParameter("id");
                    UserDao dao = new UserDao();
                    dao.find(id);
                }
            }
            "#,
            ),
            (
                "UserDao.java",
                r#"
            package dao;

            public class UserDao {
                public void find(String id) throws Exception {}
            }
            "#,
            ),
        ]);

        let mut analysis = FixpointAnalysis::new(&mut context);
        let result = analysis.run();

        assert!(result.tainted_params.contains_key("dao.UserDao.find"));
        assert!(!result.tainted_params.contains_key("app.Controller.find"));
    }

    #[test]
    fn test_fixpoint_does_not_invent_an_edge_for_an_unresolved_receiver() {
        let (_temp, mut context) = create_test_context(&[(
            "Controller.java",
            r#"
            package app;

            import javax.servlet.http.HttpServletRequest;

            public class Controller {
                public void handle(HttpServletRequest request) throws Exception {
                    String id = request.getParameter("id");
                    helper.find(id);
                }

                public void find(String id) throws Exception {}
            }
            "#,
        )]);

        let mut analysis = FixpointAnalysis::new(&mut context);
        let result = analysis.run();

        // `helper` names no variable and no workspace class, so the call reaches nothing.
        assert!(!result.tainted_params.contains_key("app.Controller.find"));
    }

    #[test]
    fn test_fixpoint_resolves_a_class_named_directly_as_a_static_call_target() {
        let (_temp, mut context) = create_test_context(&[
            (
                "Controller.java",
                r#"
            package app;

            import javax.servlet.http.HttpServletRequest;

            public class Controller {
                public void handle(HttpServletRequest request) throws Exception {
                    String id = request.getParameter("id");
                    UserDao.find(id);
                }
            }
            "#,
            ),
            (
                "UserDao.java",
                r#"
            package app;

            public class UserDao {
                public static void find(String id) throws Exception {}
            }
            "#,
            ),
        ]);

        let mut analysis = FixpointAnalysis::new(&mut context);
        let result = analysis.run();

        assert!(result.tainted_params.contains_key("app.UserDao.find"));
        assert!(!result.tainted_params.contains_key("app.Controller.find"));
    }

    #[test]
    fn test_fixpoint_terminates() {
        let (_temp, mut context) = create_test_context(&[(
            "Test.java",
            r#"
            public class Test {
                void a() { b(); }
                void b() { c(); }
                void c() { a(); }  // Cycle
            }
            "#,
        )]);

        let mut analysis = FixpointAnalysis::new(&mut context);
        let result = analysis.run();

        // Should terminate even with cycles
        assert!(result.iterations <= MAX_ITERATIONS);
    }
}
