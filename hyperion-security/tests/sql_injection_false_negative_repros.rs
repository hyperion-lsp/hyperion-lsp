// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Repros for SQL injection false negatives.
//!
//! These tests document patterns that *should* trigger `SqlInjection` findings.
//! If they don't, we have a detection gap (often caused by missing type-aware source
//! detection, missing constructor scanning, or missing interprocedural taint flow).
//!
//! Notes:
//! - These are intentionally written as "should detect" assertions.
//! - If any of these fail, it means the scanner is missing a real-world JDBC + string
//!   concatenation injection pattern.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

fn scan(code: &str) -> Vec<hyperion_security::Finding> {
    let scanner = SecurityScanner::new(ScanConfig::default());
    scanner.scan_source(code, Path::new("Test.java"))
}

fn has_sql_injection(findings: &[hyperion_security::Finding]) -> bool {
    findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection)
}

#[test]
fn repro_request_source_detection_should_use_declared_type_not_variable_name() {
    // The request parameter is named `r` (not `request`/`req`), but its *type* is
    // HttpServletRequest, so r.getParameter(..) is still a taint source.
    //
    // Today, sources are primarily detected by receiver *name aliases*, which can
    // miss real codebases with different parameter naming.
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.Statement;

public class Test {
  public void test(HttpServletRequest r) throws Exception {
    String whoops = r.getParameter("id");
    Connection con = getConnection();
    Statement stmt = con.createStatement();
    stmt.executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let findings = scan(source);
    assert!(
        has_sql_injection(&findings),
        "Expected SqlInjection when using r.getParameter(..) + string concatenation into JDBC. Findings: {findings:?}"
    );
}

#[test]
fn repro_sql_injection_taint_lost_across_methods_same_class() {
    // Controller -> service/DAO split inside the same class. The tainted value is
    // passed as a method argument.
    //
    // Without interprocedural taint propagation, `dao(String id)` won't see `id` as tainted.
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.Statement;

public class Test {
  public void controller(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    dao(whoops);
  }

  private void dao(String id) throws Exception {
    Connection con = getConnection();
    Statement stmt = con.createStatement();
    stmt.executeQuery("SELECT foo FROM bar WHERE id = " + id);
  }
}
"#;

    let findings = scan(source);
    assert!(
        has_sql_injection(&findings),
        "Expected SqlInjection to propagate across method call controller(..)->dao(..). Findings: {findings:?}"
    );
}

#[test]
fn repro_sql_injection_in_constructor_should_be_scanned() {
    // Constructors are executable code and can contain sinks.
    // If constructors aren't visited, we miss vulnerabilities in initialization logic.
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.Statement;

public class Test {
  public Test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection con = getConnection();
    Statement stmt = con.createStatement();
    stmt.executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let findings = scan(source);
    assert!(
        has_sql_injection(&findings),
        "Expected SqlInjection to be detected in constructor bodies. Findings: {findings:?}"
    );
}

#[test]
fn repro_var_connection_named_con_prepare_statement_should_be_a_sink() {
    // `var con = getConnection()` is idiomatic modern Java.
    // Even without full type inference, we should avoid relying on fragile name aliases.
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    var con = getConnection();
    con.prepareStatement("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let findings = scan(source);
    assert!(
        has_sql_injection(&findings),
        "Expected SqlInjection for var con + Connection.prepareStatement(taintedSql). Findings: {findings:?}"
    );
}

#[test]
fn repro_sql_injection_when_sink_is_nested_in_an_argument_expression() {
    // The JDBC sink is nested inside another call's argument list:
    //   consume(stmt.executeQuery("... " + whoops))
    //
    // If sink detection only looks at top-level statement siblings (and not nested ARG_LIST
    // structures), it will miss this.
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.Statement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    Connection con = getConnection();
    Statement stmt = con.createStatement();
    consume(stmt.executeQuery("SELECT foo FROM bar WHERE id = " + whoops));
  }

  private void consume(Object o) {}
}
"#;

    let findings = scan(source);
    assert!(
        has_sql_injection(&findings),
        "Expected SqlInjection even when sink call is nested inside another call's args. Findings: {findings:?}"
    );
}
