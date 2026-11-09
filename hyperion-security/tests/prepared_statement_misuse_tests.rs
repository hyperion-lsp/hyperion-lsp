// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for PreparedStatement misuse.
//!
//! `PreparedStatement` inherits `Statement.execute*(String sql)` overloads. Passing attacker-controlled
//! SQL to those overloads is still SQL injection and must be detected.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_prepared_statement_execute_query_string_overload_is_flagged() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection conn = getConnection();
    PreparedStatement ps = conn.prepareStatement("SELECT foo FROM bar WHERE id = ?");
    ps.executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding for PreparedStatement.executeQuery(String) misuse. Findings: {findings:?}"
    );
}

#[test]
fn test_prepared_statement_execute_string_overload_is_flagged() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection conn = getConnection();
    PreparedStatement ps = conn.prepareStatement("SELECT foo FROM bar WHERE id = ?");
    ps.execute("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding for PreparedStatement.execute(String) misuse. Findings: {findings:?}"
    );
}
