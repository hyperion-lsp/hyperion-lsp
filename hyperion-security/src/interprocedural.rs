// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Interprocedural analysis for cross-file taint tracking
//!
//! The analysis is built from:
//! - Method summaries capturing taint effects
//! - Cross-file method resolution via hyperion-workspace
//! - Call graph construction for inter-procedural analysis
//!
//! # Example Integration with Scanner
//!
//! ```rust,ignore
//! use hyperion_security::{SecurityScanner, SummaryDatabase};
//! use hyperion_workspace::WorkspaceIndex;
//! use std::sync::Arc;
//!
//! // Create workspace index
//! let workspace = Arc::new(WorkspaceIndex::new(root_path));
//! workspace.scan_workspace()?;
//!
//! // Create summary database with workspace context
//! let mut summary_db = SummaryDatabase::new(Some(workspace.clone()));
//!
//! // Add known sanitization methods
//! let mut sanitize_summary = MethodSummary::new("com.example.Utils.sanitizeSql".to_string());
//! sanitize_summary.add_sanitization(VulnTypes::SQL_INJECTION);
//! summary_db.add_summary(sanitize_summary);
//!
//! // Scanner can now resolve method calls across files
//! if let Some(method_fqn) = summary_db.resolve_method("com.example.User", "getName") {
//!     if let Some(summary) = summary_db.get_summary(&method_fqn) {
//!         // Apply summary to check if method propagates taint
//!         let result = summary.apply_to_call(&arg_taints, call_location);
//!     }
//! }
//!
//! // Build call graph for analysis
//! let call_graph = summary_db.build_call_graph(&files);
//! let all_callers = call_graph.transitive_callers("com.example.sink");
//! ```

use crate::constant_eval::ReturnValueKind;
use crate::finding::Location;
use crate::symbolic_taint::TaintOrigins;
use crate::taint::{TaintState, VulnTypes};
use hyperion_workspace::WorkspaceIndex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Summary of how a method propagates taint
///
/// This captures the taint behavior of a method without requiring
/// full re-analysis every time the method is called.
#[derive(Debug, Clone)]
pub struct MethodSummary {
    /// Fully qualified name of the method (e.g., "com.example.Utils.sanitize")
    pub fqn: String,

    /// Which parameters can carry taint (parameter index -> vulnerability types)
    /// Example: {0 -> SQL_INJECTION} means parameter 0 can carry SQL injection taint
    pub tainted_params: HashMap<usize, VulnTypes>,

    /// Whether the return value carries taint from any parameter
    /// None if return is always clean, Some(indices) if taint propagates from those params
    pub return_taint: Option<Vec<usize>>,

    /// Which sinks this method may call with tainted data
    /// Maps sink method FQN to vulnerability types
    pub calls_sinks: HashMap<String, VulnTypes>,

    /// Whether this method sanitizes specific vulnerability types
    /// Example: VulnTypes::SQL_INJECTION means this method sanitizes SQL injection
    pub sanitizes: VulnTypes,

    /// Location where this method is defined
    pub location: Option<Location>,

    /// Classification of the return value (constant, propagates params, etc.)
    pub return_kind: ReturnValueKind,

    /// Which origins (parameters/sources) the return value depends on
    pub return_depends_on: TaintOrigins,

    /// Whether this method body contains in-body source expressions
    /// (e.g., request.getParameter()) that produce tainted data.
    pub has_body_sources: bool,

    /// Call edges from this method to callees where tainted data from
    /// in-body sources flows into callee parameters.
    /// Vec of (callee_fqn, callee_param_idx).
    pub source_tainted_calls: Vec<(String, usize)>,
}

impl MethodSummary {
    /// Create a new method summary
    pub fn new(fqn: String) -> Self {
        Self {
            fqn,
            tainted_params: HashMap::new(),
            return_taint: None,
            calls_sinks: HashMap::new(),
            sanitizes: VulnTypes::empty(),
            location: None,
            return_kind: ReturnValueKind::Unknown,
            return_depends_on: TaintOrigins::empty(),
            has_body_sources: false,
            source_tainted_calls: Vec::new(),
        }
    }

    /// Mark a parameter as potentially tainted
    pub fn add_tainted_param(&mut self, param_idx: usize, vuln_types: VulnTypes) {
        self.tainted_params
            .entry(param_idx)
            .and_modify(|v| *v |= vuln_types)
            .or_insert(vuln_types);
    }

    /// Mark return value as propagating taint from parameters
    pub fn set_return_taint(&mut self, param_indices: Vec<usize>) {
        self.return_taint = Some(param_indices);
    }

    /// Record that this method calls a sink
    pub fn add_sink_call(&mut self, sink_fqn: String, vuln_types: VulnTypes) {
        self.calls_sinks
            .entry(sink_fqn)
            .and_modify(|v| *v |= vuln_types)
            .or_insert(vuln_types);
    }

    /// Mark this method as sanitizing specific vulnerability types
    pub fn add_sanitization(&mut self, vuln_types: VulnTypes) {
        self.sanitizes |= vuln_types;
    }

    /// Check if this method is a taint source (returns tainted data)
    pub fn is_source(&self) -> bool {
        self.return_taint.is_some() && self.tainted_params.is_empty()
    }

    /// Check if this method is a taint sink (consumes tainted data)
    pub fn is_sink(&self) -> bool {
        !self.calls_sinks.is_empty() || !self.tainted_params.is_empty()
    }

    /// Apply this summary to a method call with given argument taints
    ///
    /// Returns the taint state of the return value after applying the summary
    pub fn apply_to_call(&self, arg_taints: &[TaintState], call_location: Location) -> TaintState {
        // Check if any argument is tainted in a way that matters for this method
        let mut combined_taint = TaintState::clean();

        for param_idx in self.tainted_params.keys() {
            if let Some(arg_taint) = arg_taints.get(*param_idx) {
                // Check if this argument's taint matches what the parameter cares about
                if arg_taint.is_tainted() {
                    combined_taint = combined_taint.merge(arg_taint);
                }
            }
        }

        // If method sanitizes, apply sanitization
        if !self.sanitizes.is_empty() && combined_taint.is_tainted() {
            combined_taint = combined_taint.sanitize(self.sanitizes);
        }

        // If return propagates taint, create new taint state
        if let Some(ref param_indices) = self.return_taint {
            for &idx in param_indices {
                if let Some(arg_taint) = arg_taints.get(idx) {
                    if arg_taint.is_tainted() {
                        return arg_taint.propagate(
                            call_location,
                            format!("Returned from method {}", self.fqn),
                            String::new(),
                        );
                    }
                }
            }
        }

        combined_taint
    }
}

/// Database of method summaries for interprocedural analysis
pub struct SummaryDatabase {
    /// Method FQN -> Summary
    summaries: HashMap<String, Arc<MethodSummary>>,

    /// Reference to workspace index for symbol resolution
    workspace: Option<Arc<WorkspaceIndex>>,
}

impl SummaryDatabase {
    /// Create a new summary database
    pub fn new(workspace: Option<Arc<WorkspaceIndex>>) -> Self {
        Self {
            summaries: HashMap::new(),
            workspace,
        }
    }

    /// Add a method summary to the database
    pub fn add_summary(&mut self, summary: MethodSummary) {
        self.summaries
            .insert(summary.fqn.clone(), Arc::new(summary));
    }

    /// Get a method summary by FQN
    pub fn get_summary(&self, fqn: &str) -> Option<Arc<MethodSummary>> {
        self.summaries.get(fqn).cloned()
    }

    /// Resolve a method call across files using workspace index
    ///
    /// Given a receiver type and method name, finds the method's FQN
    /// by traversing the inheritance hierarchy.
    pub fn resolve_method(&self, receiver_type: &str, method_name: &str) -> Option<String> {
        let workspace = self.workspace.as_ref()?;

        // Try direct lookup first
        let method_fqn = format!("{receiver_type}.{method_name}");
        if workspace.find_symbol(&method_fqn).is_some() {
            return Some(method_fqn);
        }

        // Try through inheritance hierarchy
        self.resolve_through_inheritance(receiver_type, method_name)
    }

    /// Resolve method through inheritance hierarchy
    fn resolve_through_inheritance(
        &self,
        receiver_type: &str,
        method_name: &str,
    ) -> Option<String> {
        let workspace = self.workspace.as_ref()?;

        // Get the receiver class symbol
        let receiver_symbol = workspace.get_symbol(receiver_type)?;

        // Check parent class
        if let Some(ref parent_fqn) = receiver_symbol.parent_class {
            let method_fqn = format!("{parent_fqn}.{method_name}");
            if workspace.find_symbol(&method_fqn).is_some() {
                return Some(method_fqn);
            }
            // Recurse to parent's parent
            return self.resolve_through_inheritance(parent_fqn.as_ref(), method_name);
        }

        // Check interfaces
        for interface_fqn in &receiver_symbol.interfaces {
            let method_fqn = format!("{interface_fqn}.{method_name}");
            if workspace.find_symbol(&method_fqn).is_some() {
                return Some(method_fqn);
            }
        }

        None
    }

    /// Get all methods that call a given method (reverse call graph)
    pub fn get_callers(&self, method_fqn: &str) -> Vec<String> {
        self.summaries
            .iter()
            .filter(|(_, summary)| summary.calls_sinks.keys().any(|sink| sink == method_fqn))
            .map(|(fqn, _)| fqn.clone())
            .collect()
    }

    /// Build call graph for a set of files
    ///
    /// Returns a map of method FQN -> list of called method FQNs
    pub fn build_call_graph(&self, _file_paths: &[PathBuf]) -> CallGraph {
        let mut graph = CallGraph::new();

        // For each method in the summaries, add its call edges
        for (caller_fqn, summary) in &self.summaries {
            for callee_fqn in summary.calls_sinks.keys() {
                graph.add_edge(caller_fqn.clone(), callee_fqn.clone());
            }
        }

        graph
    }
}

/// Call graph for interprocedural analysis
///
/// Represents caller -> callee relationships for taint propagation
#[derive(Debug, Clone)]
pub struct CallGraph {
    /// Method FQN -> list of called method FQNs
    edges: HashMap<String, Vec<String>>,

    /// Reverse edges for efficient reverse traversal
    reverse_edges: HashMap<String, Vec<String>>,
}

impl CallGraph {
    /// Create a new empty call graph
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
        }
    }

    /// Add an edge from caller to callee
    pub fn add_edge(&mut self, caller: String, callee: String) {
        self.edges
            .entry(caller.clone())
            .or_default()
            .push(callee.clone());
        self.reverse_edges.entry(callee).or_default().push(caller);
    }

    /// Get all methods called by the given method
    pub fn get_callees(&self, method_fqn: &str) -> &[String] {
        self.edges
            .get(method_fqn)
            .map_or(&[], std::vec::Vec::as_slice)
    }

    /// Get all methods that call the given method
    pub fn get_callers(&self, method_fqn: &str) -> &[String] {
        self.reverse_edges
            .get(method_fqn)
            .map_or(&[], std::vec::Vec::as_slice)
    }

    /// Perform transitive closure to find all reachable methods
    ///
    /// Returns all methods that can be reached from the starting method
    /// via the call graph (including the starting method itself)
    pub fn transitive_callees(&self, start: &str) -> Vec<String> {
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![start.to_string()];
        let mut result = Vec::new();

        while let Some(method) = stack.pop() {
            if visited.insert(method.clone()) {
                result.push(method.clone());
                for callee in self.get_callees(&method) {
                    if !visited.contains(callee) {
                        stack.push(callee.clone());
                    }
                }
            }
        }

        result
    }

    /// Perform reverse transitive closure to find all callers
    ///
    /// Returns all methods that can reach the target method
    /// via the call graph (including the target method itself)
    pub fn transitive_callers(&self, target: &str) -> Vec<String> {
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![target.to_string()];
        let mut result = Vec::new();

        while let Some(method) = stack.pop() {
            if visited.insert(method.clone()) {
                result.push(method.clone());
                for caller in self.get_callers(&method) {
                    if !visited.contains(caller) {
                        stack.push(caller.clone());
                    }
                }
            }
        }

        result
    }

    /// Check if there's a path from caller to callee
    pub fn has_path(&self, from: &str, to: &str) -> bool {
        if from == to {
            return true;
        }

        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![from.to_string()];

        while let Some(method) = stack.pop() {
            if method == to {
                return true;
            }
            if visited.insert(method.clone()) {
                for callee in self.get_callees(&method) {
                    if !visited.contains(callee) {
                        stack.push(callee.clone());
                    }
                }
            }
        }

        false
    }
}

impl Default for CallGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_location() -> Location {
        Location {
            file: PathBuf::from("test.java"),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 10,
        }
    }

    #[test]
    fn test_method_summary_creation() {
        let mut summary = MethodSummary::new("com.example.Utils.sanitize".to_string());
        summary.add_tainted_param(0, VulnTypes::SQL_INJECTION);
        summary.set_return_taint(vec![0]);

        assert!(summary.is_sink());
        assert_eq!(summary.tainted_params.len(), 1);
        assert!(summary.return_taint.is_some());
    }

    #[test]
    fn test_method_summary_sanitization() {
        let mut summary = MethodSummary::new("com.example.Utils.sanitizeSql".to_string());
        summary.add_sanitization(VulnTypes::SQL_INJECTION);

        let tainted = TaintState::tainted(
            "user_input".to_string(),
            dummy_location(),
            "getInput()".to_string(),
            VulnTypes::SQL_INJECTION,
        );

        let result = summary.apply_to_call(&[tainted], dummy_location());
        assert!(result.is_clean());
    }

    #[test]
    fn test_method_summary_taint_propagation() {
        let mut summary = MethodSummary::new("java.lang.String.toLowerCase".to_string());
        summary.set_return_taint(vec![0]);

        let tainted = TaintState::tainted(
            "user_input".to_string(),
            dummy_location(),
            "getInput()".to_string(),
            VulnTypes::XSS,
        );

        let result = summary.apply_to_call(&[tainted], dummy_location());
        assert!(result.is_tainted());
    }

    #[test]
    fn test_call_graph_construction() {
        let mut graph = CallGraph::new();
        graph.add_edge("main".to_string(), "foo".to_string());
        graph.add_edge("foo".to_string(), "bar".to_string());
        graph.add_edge("main".to_string(), "baz".to_string());

        assert_eq!(graph.get_callees("main").len(), 2);
        assert_eq!(graph.get_callers("foo").len(), 1);
        assert!(graph.has_path("main", "bar"));
        assert!(!graph.has_path("bar", "main"));
    }

    #[test]
    fn test_transitive_closure() {
        let mut graph = CallGraph::new();
        graph.add_edge("main".to_string(), "foo".to_string());
        graph.add_edge("foo".to_string(), "bar".to_string());
        graph.add_edge("bar".to_string(), "baz".to_string());

        let callees = graph.transitive_callees("main");
        assert_eq!(callees.len(), 4); // main, foo, bar, baz

        let callers = graph.transitive_callers("baz");
        assert_eq!(callers.len(), 4); // baz, bar, foo, main
    }

    #[test]
    fn test_call_graph_cycle_detection() {
        let mut graph = CallGraph::new();
        graph.add_edge("a".to_string(), "b".to_string());
        graph.add_edge("b".to_string(), "c".to_string());
        graph.add_edge("c".to_string(), "a".to_string());

        // Should handle cycles without infinite loop
        let callees = graph.transitive_callees("a");
        assert_eq!(callees.len(), 3); // a, b, c (each visited once)
    }

    #[test]
    fn test_summary_database() {
        let mut db = SummaryDatabase::new(None);

        let mut summary = MethodSummary::new("com.example.Utils.sanitize".to_string());
        summary.add_sanitization(VulnTypes::SQL_INJECTION);
        db.add_summary(summary);

        let retrieved = db.get_summary("com.example.Utils.sanitize");
        assert!(retrieved.is_some());
        assert!(retrieved
            .unwrap()
            .sanitizes
            .contains(VulnTypes::SQL_INJECTION));
    }
}
