// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_stringbuilder_append_sql_injection() {
    // StringBuilder.append() with tainted data should be detected
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        StringBuilder query = new StringBuilder("SELECT * FROM users WHERE id = ");
        query.append(id);
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through StringBuilder.append()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));

    // Verify the data flow is tracked
    let finding = &findings[0];
    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    assert!(
        finding.dataflow.len() >= 2,
        "Should track source and append steps"
    );
}

#[test]
fn test_stringbuffer_append_sql_injection() {
    // StringBuffer should also be tracked (same as StringBuilder)
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        StringBuffer query = new StringBuffer("SELECT * FROM users WHERE id = ");
        query.append(id);
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through StringBuffer.append()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_stringbuilder_multiple_appends() {
    // Multiple append() calls with tainted data
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        String name = request.getParameter("name");
        StringBuilder query = new StringBuilder("SELECT * FROM users WHERE ");
        query.append("id = ");
        query.append(id);
        query.append(" AND name = '");
        query.append(name);
        query.append("'");
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through multiple StringBuilder.append() calls"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_stringbuilder_safe_usage() {
    // StringBuilder with safe data should not trigger false positive
    let source = r#"
public class UserController {
    public void getUser() {
        StringBuilder query = new StringBuilder("SELECT * FROM users WHERE id = ");
        query.append("123");  // Literal string, not tainted
        query.append(" AND status = 'active'");
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for safe StringBuilder usage"
    );
}

#[test]
fn test_stringbuilder_command_injection() {
    // StringBuilder should work for command injection too
    let source = r#"
public class ProcessController {
    public void runProcess(@RequestParam String input) {
        StringBuilder command = new StringBuilder("ping ");
        command.append(input);
        Runtime.getRuntime().exec(command.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProcessController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through StringBuilder"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_stringbuilder_without_tostring_not_tainted() {
    // If toString() is never called, the StringBuilder itself shouldn't flow to sinks
    // However, in this test we're passing the builder directly which should not be tainted
    // unless we call toString() on it
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        StringBuilder query = new StringBuilder("SELECT * FROM users WHERE id = ");
        query.append(id);
        // StringBuilder itself is not a String, so this should be a type error in real code
        // But for testing purposes, we want to verify that append() marks the builder as tainted
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // No sink is called, so no findings expected
    assert!(
        findings.is_empty(),
        "Should not detect vulnerability when no sink is called"
    );
}

#[test]
fn test_stringbuilder_intermediate_variable() {
    // StringBuilder result assigned to variable before sink
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        StringBuilder builder = new StringBuilder("SELECT * FROM users WHERE id = ");
        builder.append(id);
        String query = builder.toString();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through StringBuilder with intermediate variable"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));

    // Verify the full data flow is tracked
    let finding = &findings[0];
    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    // Should have: source → append → toString → assignment → sink
    assert!(
        finding.dataflow.len() >= 3,
        "Should track source, append, toString, and assignment"
    );
}

#[test]
fn test_stringbuilder_mixed_tainted_safe() {
    // StringBuilder with both tainted and safe data
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        String userInput = request.getParameter("search");
        StringBuilder query = new StringBuilder("SELECT * FROM products WHERE ");
        query.append("category = 'electronics' AND ");  // Safe
        query.append("name LIKE '%");
        query.append(userInput);  // Tainted
        query.append("%'");
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection even when mixed with safe data"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_stringbuilder_chained_initialization() {
    // StringBuilder created and used in expression
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        String query = new StringBuilder("SELECT * FROM users WHERE id = ")
            .append(id)
            .toString();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_stringbuilder_append_safe_after_numeric_conversion() {
    // StringBuilder with numerically converted data should be safe
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        int id = Integer.parseInt(idParam);
        StringBuilder query = new StringBuilder("SELECT * FROM users WHERE id = ");
        query.append(id);
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection when appending numerically converted data"
    );
}

#[test]
fn test_stringbuilder_in_loop() {
    // StringBuilder used in a loop with tainted data
    // Array taint tracking is implemented, but foreach loop variable taint propagation is not
    // The array `ids` is tainted, but the loop variable `id` doesn't inherit taint yet
    let source = r#"
public class UserController {
    public void searchUsers(HttpServletRequest request) {
        String[] ids = request.getParameterValues("ids");
        StringBuilder query = new StringBuilder("SELECT * FROM users WHERE id IN (");
        for (String id : ids) {
            query.append(id);
            query.append(",");
        }
        query.append(")");
        stmt.executeQuery(query.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Known limitation: foreach loop variable taint propagation is not implemented yet"
    );
}
