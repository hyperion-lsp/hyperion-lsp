// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for cross-file taint tracking
//!
//! These tests verify that the interprocedural analysis correctly
//! tracks taint across method calls in different files.

use hyperion_security::{
    interprocedural::{CallGraph, MethodSummary, SummaryDatabase},
    taint::{TaintState, VulnTypes},
    ScanConfig, SecurityScanner,
};

/// Test helper to create a dummy location
fn dummy_location() -> hyperion_security::finding::Location {
    hyperion_security::finding::Location {
        file: std::path::PathBuf::from("test.java"),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 10,
    }
}

#[test]
fn test_method_summary_basic() {
    let mut summary = MethodSummary::new("com.example.Utils.processInput".to_string());
    summary.add_tainted_param(0, VulnTypes::SQL_INJECTION);
    summary.set_return_taint(vec![0]);

    assert!(summary.is_sink());
    assert_eq!(summary.tainted_params.len(), 1);
    assert_eq!(summary.return_taint, Some(vec![0]));
}

#[test]
fn test_sanitization_summary() {
    let mut summary = MethodSummary::new("com.example.Security.sanitizeSql".to_string());
    summary.add_sanitization(VulnTypes::SQL_INJECTION);

    let tainted = TaintState::tainted(
        "user_input".to_string(),
        dummy_location(),
        "request.getParameter(\"id\")".to_string(),
        VulnTypes::SQL_INJECTION,
    );

    let result = summary.apply_to_call(&[tainted], dummy_location());
    assert!(
        result.is_clean(),
        "Sanitization should clean SQL injection taint"
    );
}

#[test]
fn test_taint_propagation_through_method() {
    let mut summary = MethodSummary::new("java.lang.String.toLowerCase".to_string());
    summary.set_return_taint(vec![0]);

    let tainted = TaintState::tainted(
        "user_input".to_string(),
        dummy_location(),
        "getUserInput()".to_string(),
        VulnTypes::XSS,
    );

    let result = summary.apply_to_call(&[tainted], dummy_location());
    assert!(result.is_tainted(), "toLowerCase should propagate taint");
}

#[test]
fn test_summary_database_resolution() {
    let mut db = SummaryDatabase::new(None);

    let mut summary = MethodSummary::new("com.example.Utils.sanitize".to_string());
    summary.add_sanitization(VulnTypes::SQL_INJECTION | VulnTypes::XSS);
    db.add_summary(summary);

    let retrieved = db.get_summary("com.example.Utils.sanitize");
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert!(retrieved.sanitizes.contains(VulnTypes::SQL_INJECTION));
    assert!(retrieved.sanitizes.contains(VulnTypes::XSS));
}

#[test]
fn test_call_graph_construction() {
    let mut graph = CallGraph::new();
    graph.add_edge("main".to_string(), "processRequest".to_string());
    graph.add_edge("processRequest".to_string(), "executeQuery".to_string());
    graph.add_edge("processRequest".to_string(), "sanitize".to_string());

    assert_eq!(graph.get_callees("main").len(), 1);
    assert_eq!(graph.get_callees("processRequest").len(), 2);
    assert_eq!(graph.get_callers("executeQuery").len(), 1);
}

#[test]
fn test_call_graph_transitive_closure() {
    let mut graph = CallGraph::new();
    graph.add_edge("main".to_string(), "foo".to_string());
    graph.add_edge("foo".to_string(), "bar".to_string());
    graph.add_edge("bar".to_string(), "sink".to_string());

    let callees = graph.transitive_callees("main");
    assert_eq!(callees.len(), 4, "Should include main, foo, bar, sink");
    assert!(callees.contains(&"main".to_string()));
    assert!(callees.contains(&"foo".to_string()));
    assert!(callees.contains(&"bar".to_string()));
    assert!(callees.contains(&"sink".to_string()));

    let callers = graph.transitive_callers("sink");
    assert_eq!(callers.len(), 4, "Should include sink, bar, foo, main");
}

#[test]
fn test_call_graph_path_checking() {
    let mut graph = CallGraph::new();
    graph.add_edge("a".to_string(), "b".to_string());
    graph.add_edge("b".to_string(), "c".to_string());
    graph.add_edge("d".to_string(), "e".to_string());

    assert!(graph.has_path("a", "c"), "Should have path a -> b -> c");
    assert!(graph.has_path("a", "b"), "Should have direct path a -> b");
    assert!(!graph.has_path("a", "e"), "Should not have path a to e");
    assert!(!graph.has_path("c", "a"), "Should not have reverse path");
}

#[test]
fn test_call_graph_cycle_handling() {
    let mut graph = CallGraph::new();
    graph.add_edge("a".to_string(), "b".to_string());
    graph.add_edge("b".to_string(), "c".to_string());
    graph.add_edge("c".to_string(), "a".to_string());

    // Should handle cycles without infinite loop
    let callees = graph.transitive_callees("a");
    assert_eq!(callees.len(), 3, "Should visit each node once");

    assert!(graph.has_path("a", "c"));
    assert!(graph.has_path("c", "a"));
}

#[test]
fn test_summary_database_callers() {
    let mut db = SummaryDatabase::new(None);

    let mut caller1 = MethodSummary::new("com.example.Service.process".to_string());
    caller1.add_sink_call("com.example.Db.query".to_string(), VulnTypes::SQL_INJECTION);
    db.add_summary(caller1);

    let mut caller2 = MethodSummary::new("com.example.Service.execute".to_string());
    caller2.add_sink_call("com.example.Db.query".to_string(), VulnTypes::SQL_INJECTION);
    db.add_summary(caller2);

    let callers = db.get_callers("com.example.Db.query");
    assert_eq!(callers.len(), 2);
    assert!(callers.contains(&"com.example.Service.process".to_string()));
    assert!(callers.contains(&"com.example.Service.execute".to_string()));
}

#[test]
fn test_multi_vulnerability_sanitization() {
    let mut summary = MethodSummary::new("com.example.Security.sanitizeAll".to_string());
    summary
        .add_sanitization(VulnTypes::SQL_INJECTION | VulnTypes::XSS | VulnTypes::COMMAND_INJECTION);

    let sql_tainted = TaintState::tainted(
        "user_input".to_string(),
        dummy_location(),
        "getInput()".to_string(),
        VulnTypes::SQL_INJECTION,
    );

    let result = summary.apply_to_call(&[sql_tainted], dummy_location());
    assert!(result.is_clean(), "Should sanitize SQL injection");

    let xss_tainted = TaintState::tainted(
        "user_input".to_string(),
        dummy_location(),
        "getInput()".to_string(),
        VulnTypes::XSS,
    );

    let result = summary.apply_to_call(&[xss_tainted], dummy_location());
    assert!(result.is_clean(), "Should sanitize XSS");
}

#[test]
fn test_partial_sanitization() {
    // Test that sanitization only removes the specific vulnerability types it targets
    let tainted = TaintState::tainted(
        "user_input".to_string(),
        dummy_location(),
        "getInput()".to_string(),
        VulnTypes::SQL_INJECTION | VulnTypes::XSS,
    );

    // Sanitize only SQL injection
    let result = tainted.sanitize(VulnTypes::SQL_INJECTION);
    assert!(result.is_tainted(), "Should still be tainted for XSS");
    assert!(!result.is_dangerous_for(hyperion_security::VulnType::SqlInjection));
    assert!(result.is_dangerous_for(hyperion_security::VulnType::Xss));
}

#[test]
fn test_method_call_with_multiple_args() {
    let mut summary = MethodSummary::new("com.example.Utils.concat".to_string());
    summary.add_tainted_param(0, VulnTypes::all_vulns());
    summary.add_tainted_param(1, VulnTypes::all_vulns());
    summary.set_return_taint(vec![0, 1]);

    let clean = TaintState::clean();
    let tainted = TaintState::tainted(
        "source".to_string(),
        dummy_location(),
        "getSource()".to_string(),
        VulnTypes::XSS,
    );

    // First arg clean, second arg tainted
    let result = summary.apply_to_call(&[clean.clone(), tainted.clone()], dummy_location());
    assert!(
        result.is_tainted(),
        "Should propagate taint from second argument"
    );

    // First arg tainted, second arg clean
    let result = summary.apply_to_call(&[tainted, clean], dummy_location());
    assert!(
        result.is_tainted(),
        "Should propagate taint from first argument"
    );
}

/// Integration test showing scanner could use summary database
/// (This is a conceptual test - actual integration would require modifying SecurityScanner)
#[test]
fn test_scanner_with_summary_database_concept() {
    // This demonstrates how the scanner would use the summary database
    // In actual implementation, SecurityScanner would be modified to accept
    // a SummaryDatabase and use it during analysis

    let _scanner = SecurityScanner::new(ScanConfig::default());
    let mut db = SummaryDatabase::new(None);

    // Add known sanitizers
    let mut html_escape =
        MethodSummary::new("org.apache.commons.text.StringEscapeUtils.escapeHtml4".to_string());
    html_escape.add_sanitization(VulnTypes::XSS);
    db.add_summary(html_escape);

    // Scanner would check if method is in summary database
    let summary = db.get_summary("org.apache.commons.text.StringEscapeUtils.escapeHtml4");
    assert!(summary.is_some());
    assert!(summary.unwrap().sanitizes.contains(VulnTypes::XSS));
}
