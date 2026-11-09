// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for JDBC sink detection in chained calls and field receivers.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_sql_injection_chained_create_statement_execute_query_is_detected() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    getConnection().createStatement().executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding for chained createStatement().executeQuery(...). Findings: {findings:?}"
    );
}

#[test]
fn test_sql_injection_chained_get_connection_prepare_statement_is_detected() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    dataSource.getConnection().prepareStatement("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding for chained getConnection().prepareStatement(...). Findings: {findings:?}"
    );
}

#[test]
fn test_sql_injection_field_receiver_statement_execute_query_is_detected() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;

public class Test {
  private Statement stmt;

  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    this.stmt.executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding for field receiver this.stmt.executeQuery(...). Findings: {findings:?}"
    );
}
