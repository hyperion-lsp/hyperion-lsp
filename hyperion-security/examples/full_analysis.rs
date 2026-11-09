// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Complete example showing all three phases of hyperion-security working together
//!
//! Phase 1: Type-aware analysis (PreparedStatement, numeric conversions)
//! Phase 2: Taint tracking (TaintState, VulnTypes, data flow)
//! Phase 3: Cross-file analysis (MethodSummary, SummaryDatabase, CallGraph)

use hyperion_security::{
    interprocedural::{CallGraph, MethodSummary, SummaryDatabase},
    taint::VulnTypes,
    ScanConfig, SecurityScanner,
};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    println!("=== Hyperion Security Scanner - Full Analysis Example ===\n");

    // Phase 1 & 2: Basic scanning with type-aware analysis and taint tracking
    println!("Phase 1 & 2: Scanning with type-aware analysis...");
    let config = ScanConfig::default();
    let scanner = SecurityScanner::new(config);

    // Example: Scan a single file
    let test_code = r#"
public class UserController {
    public void vulnerable(HttpServletRequest request) {
        String id = request.getParameter("id");
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }

    public void safe(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        int id = Integer.parseInt(idParam);  // Phase 1: Numeric conversion
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);  // Safe - id is numeric
    }

    public void safePrepared(HttpServletRequest request) {
        String name = request.getParameter("name");
        PreparedStatement stmt = conn.prepareStatement(  // Phase 1: PreparedStatement
            "SELECT * FROM users WHERE name = ?"
        );
        stmt.setString(1, name);  // Safe - parameterized
        stmt.executeQuery();
    }
}
"#;

    let findings = scanner.scan_source(test_code, &PathBuf::from("UserController.java"));
    println!("Found {} issues:", findings.len());
    for finding in &findings {
        println!(
            "  - {} at line {} (severity: {:?})",
            finding.vuln_type.description(),
            finding.source_location.line,
            finding.severity
        );
        if !finding.dataflow.is_empty() {
            println!("    Data flow:");
            for step in &finding.dataflow {
                println!("      → {}", step.description);
            }
        }
    }
    println!();

    // Phase 3: Cross-file analysis with workspace integration
    println!("Phase 3: Setting up cross-file analysis...");

    // Create workspace index (in real usage, this would index the entire project)
    let workspace = Arc::new(WorkspaceIndex::new(PathBuf::from(".")));
    println!("  ✓ Workspace index created");

    // Create summary database with workspace context
    let mut summary_db = SummaryDatabase::new(Some(workspace.clone()));
    println!("  ✓ Summary database initialized");

    // Add known sanitization methods from common security libraries
    add_security_library_summaries(&mut summary_db);
    println!("  ✓ Added sanitization summaries for common libraries");

    // Add custom application sanitizers
    let mut custom_sanitizer =
        MethodSummary::new("com.example.security.InputValidator.sanitizeSql".to_string());
    custom_sanitizer.add_sanitization(VulnTypes::SQL_INJECTION);
    summary_db.add_summary(custom_sanitizer);
    println!("  ✓ Added custom application sanitizers");

    // Build call graph for the application
    println!("\nBuilding call graph...");
    let mut call_graph = CallGraph::new();
    call_graph.add_edge(
        "com.example.UserController.handleRequest".to_string(),
        "com.example.UserService.getUser".to_string(),
    );
    call_graph.add_edge(
        "com.example.UserService.getUser".to_string(),
        "com.example.UserRepository.findById".to_string(),
    );
    call_graph.add_edge(
        "com.example.UserRepository.findById".to_string(),
        "java.sql.Statement.executeQuery".to_string(),
    );
    println!("  ✓ Call graph constructed with {} methods", 4);

    // Analyze call paths
    println!("\nAnalyzing call paths...");
    let source = "com.example.UserController.handleRequest";
    let sink = "java.sql.Statement.executeQuery";

    if call_graph.has_path(source, sink) {
        println!("  ⚠ Found path from source to sink:");
        let callees = call_graph.transitive_callees(source);
        for method in callees {
            println!("    → {}", method);
        }
    }

    // Demonstrate reverse analysis
    println!("\nReverse analysis - finding callers of sink:");
    let callers = call_graph.transitive_callers(sink);
    for caller in callers {
        println!("  ← {}", caller);
    }

    // Demonstrate method resolution
    println!("\nMethod resolution through inheritance:");
    if let Some(resolved) = summary_db.resolve_method("com.example.User", "toString") {
        println!("  User.toString() → {}", resolved);
    }

    // Demonstrate sanitization checking
    println!("\nSanitization library coverage:");
    let sanitizers = [
        "org.owasp.esapi.Encoder.encodeForSQL",
        "org.owasp.esapi.Encoder.encodeForHTML",
        "org.apache.commons.text.StringEscapeUtils.escapeHtml4",
        "com.google.common.html.HtmlEscapers.htmlEscaper",
    ];

    for sanitizer in &sanitizers {
        if let Some(summary) = summary_db.get_summary(sanitizer) {
            let vuln_types = if summary.sanitizes.contains(VulnTypes::SQL_INJECTION) {
                "SQL"
            } else if summary.sanitizes.contains(VulnTypes::XSS) {
                "XSS"
            } else {
                "Other"
            };
            println!("  ✓ {} → sanitizes {}", sanitizer, vuln_types);
        }
    }

    println!("\n=== Analysis Complete ===");
}

/// Add summaries for common security libraries
fn add_security_library_summaries(db: &mut SummaryDatabase) {
    // OWASP ESAPI
    let mut encode_sql = MethodSummary::new("org.owasp.esapi.Encoder.encodeForSQL".to_string());
    encode_sql.add_sanitization(VulnTypes::SQL_INJECTION);
    db.add_summary(encode_sql);

    let mut encode_html = MethodSummary::new("org.owasp.esapi.Encoder.encodeForHTML".to_string());
    encode_html.add_sanitization(VulnTypes::XSS);
    db.add_summary(encode_html);

    let mut encode_js =
        MethodSummary::new("org.owasp.esapi.Encoder.encodeForJavaScript".to_string());
    encode_js.add_sanitization(VulnTypes::XSS);
    db.add_summary(encode_js);

    // Apache Commons Text
    let mut escape_html4 =
        MethodSummary::new("org.apache.commons.text.StringEscapeUtils.escapeHtml4".to_string());
    escape_html4.add_sanitization(VulnTypes::XSS);
    db.add_summary(escape_html4);

    let mut escape_java =
        MethodSummary::new("org.apache.commons.text.StringEscapeUtils.escapeJava".to_string());
    escape_java.add_sanitization(VulnTypes::LOG_INJECTION);
    db.add_summary(escape_java);

    // Guava
    let mut html_escaper =
        MethodSummary::new("com.google.common.html.HtmlEscapers.htmlEscaper".to_string());
    html_escaper.add_sanitization(VulnTypes::XSS);
    db.add_summary(html_escaper);

    // Spring Framework
    let mut html_utils =
        MethodSummary::new("org.springframework.web.util.HtmlUtils.htmlEscape".to_string());
    html_utils.add_sanitization(VulnTypes::XSS);
    db.add_summary(html_utils);

    // Java standard library - String methods that propagate taint
    let mut to_lower_case = MethodSummary::new("java.lang.String.toLowerCase".to_string());
    to_lower_case.set_return_taint(vec![0]);
    db.add_summary(to_lower_case);

    let mut to_upper_case = MethodSummary::new("java.lang.String.toUpperCase".to_string());
    to_upper_case.set_return_taint(vec![0]);
    db.add_summary(to_upper_case);

    let mut trim = MethodSummary::new("java.lang.String.trim".to_string());
    trim.set_return_taint(vec![0]);
    db.add_summary(trim);
}
