// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for PreparedStatement receiver-taint SQL injection.
//!
//! If a PreparedStatement is created from tainted SQL (e.g., string concatenation),
//! the subsequent no-arg executeQuery()/executeUpdate()/execute() should be flagged.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

fn has_sql_injection_at_execute_query(findings: &[hyperion_security::Finding]) -> bool {
    findings.iter().any(|f| {
        f.vuln_type == VulnType::SqlInjection
            && f.sink_kind
                .as_deref()
                .unwrap_or("")
                .contains("executeQuery")
    })
}

#[test]
fn test_prepared_statement_execute_query_no_args_receiver_taint_is_detected() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection conn = getConnection();
    PreparedStatement ps = conn.prepareStatement("SELECT foo FROM bar WHERE id = " + whoops);
    ps.executeQuery();
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        has_sql_injection_at_execute_query(&findings),
        "Expected SQL injection finding at PreparedStatement.executeQuery() (receiver-taint). Findings: {findings:?}"
    );
}

#[test]
fn test_prepared_statement_execute_query_no_args_receiver_taint_is_detected_in_try_with_resources()
{
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    try (Connection con = getConnection();
         PreparedStatement ps = con.prepareStatement("SELECT foo FROM bar WHERE id = " + whoops)) {
      ps.executeQuery();
    }
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        has_sql_injection_at_execute_query(&findings),
        "Expected SQL injection finding at executeQuery() for tainted PreparedStatement in try-with-resources. Findings: {findings:?}"
    );
}

#[test]
fn test_prepared_statement_execute_query_no_args_receiver_taint_is_detected_in_chain() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    getConnection().prepareStatement("SELECT foo FROM bar WHERE id = " + whoops).executeQuery();
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        has_sql_injection_at_execute_query(&findings),
        "Expected SQL injection finding at executeQuery() in chained prepareStatement(...).executeQuery(). Findings: {findings:?}"
    );
}

#[test]
fn test_var_stmt_prepare_statement_execute_query_no_args_receiver_taint_is_detected() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection conn = getConnection();
    var stmt = conn.prepareStatement("SELECT foo FROM bar WHERE id = " + whoops);
    stmt.executeQuery();
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        has_sql_injection_at_execute_query(&findings),
        "Expected SQL injection finding for `var stmt = conn.prepareStatement(tainted)` then stmt.executeQuery(). Findings: {findings:?}"
    );
}

#[test]
fn test_parameterized_prepared_statement_execute_query_no_args_is_not_flagged() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection conn = getConnection();
    PreparedStatement ps = conn.prepareStatement("SELECT foo FROM bar WHERE id = ?");
    ps.setString(1, whoops);
    ps.executeQuery();
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        !findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Did not expect SQL injection finding for parameterized PreparedStatement usage. Findings: {findings:?}"
    );
}
