// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for try-with-resources scanning.
//!
//! Resource initializers live under RESOURCE_SPEC and must be scanned, otherwise
//! sinks inside them (JDBC, URL.openStream, etc.) are missed.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_sql_injection_in_try_with_resources_resource_initializer() {
    let source = r#"
import javax.servlet.http.HttpServletRequest;
import java.sql.Connection;
import java.sql.PreparedStatement;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String whoops = request.getParameter("id");
    try (Connection con = getConnection();
         PreparedStatement stmt = con.prepareStatement("SELECT foo FROM bar WHERE id = " + whoops)) {
      stmt.executeQuery();
    }
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding from tainted SQL in try-with-resources initializer. Findings: {findings:?}"
    );
}

#[test]
fn test_ssrf_in_try_with_resources_resource_initializer() {
    let source = r#"
import java.io.InputStream;
import java.net.URL;
import javax.servlet.http.HttpServletRequest;

public class Test {
  public void test(HttpServletRequest request) throws Exception {
    String url = request.getParameter("url");
    try (InputStream in = new URL(url).openStream()) {
      // body
    }
  }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Ssrf),
        "Expected SSRF finding from URL.openStream() in try-with-resources initializer. Findings: {findings:?}"
    );
}
