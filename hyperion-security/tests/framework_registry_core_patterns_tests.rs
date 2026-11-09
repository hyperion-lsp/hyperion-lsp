// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for FrameworkRegistry-built registries.
//!
//! When scanning via the CLI we build registries from FrameworkRegistry. Core sinks like JDBC
//! `Statement.execute*(String)` and `Runtime.exec` must still be present even if no web framework
//! is enabled/detected.

use hyperion_security::{frameworks::FrameworkRegistry, ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

fn scanner_with_framework_registry(registry: FrameworkRegistry) -> SecurityScanner {
    let (sources, sinks, sanitizers) = registry.into_registries();
    SecurityScanner::with_frameworks(ScanConfig::default(), sources, sinks, sanitizers)
}

#[test]
fn test_core_jdbc_statement_sinks_present_with_framework_registry() {
    // Simulate "framework mode" by using FrameworkRegistry-built registries.
    let registry = FrameworkRegistry::with_builtin();
    let scanner = scanner_with_framework_registry(registry);

    let source = r#"
import java.sql.Statement;

public class Test {
  public void test(@RequestParam String whoops) throws Exception {
    Statement stmt = conn.createStatement();
    stmt.executeQuery("SELECT foo FROM bar WHERE id = " + whoops);
  }
}
"#;

    let findings = scanner.scan_source(source, Path::new("Test.java"));
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::SqlInjection),
        "Expected SQL injection finding when using FrameworkRegistry registries. Findings: {findings:?}"
    );
}

#[test]
fn test_core_runtime_exec_sink_present_with_framework_registry() {
    let registry = FrameworkRegistry::with_builtin();
    let scanner = scanner_with_framework_registry(registry);

    let source = r#"
public class Test {
  public void test(@RequestParam String cmd) throws Exception {
    Runtime.getRuntime().exec(cmd);
  }
}
"#;

    let findings = scanner.scan_source(source, Path::new("Test.java"));
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::CommandInjection),
        "Expected command injection finding when using FrameworkRegistry registries. Findings: {findings:?}"
    );
}
