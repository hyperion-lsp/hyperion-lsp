// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

const fn sql_injection_source() -> &'static str {
    r#"
public class UserRepository {
    public void query(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        em.createQuery(sql);
    }
}
"#
}

#[test]
fn test_test_path_severity_downgrade() {
    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    let findings = scanner.scan_source(
        sql_injection_source(),
        Path::new("src/test/java/UserRepositoryTest.java"),
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection)
        .expect("Expected SQL injection finding");
    assert_eq!(finding.severity, Severity::Info);
}

#[test]
fn test_internal_path_severity_downgrade() {
    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    let findings = scanner.scan_source(
        sql_injection_source(),
        Path::new("src/internal/UserRepository.java"),
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection)
        .expect("Expected SQL injection finding");
    assert_eq!(finding.severity, Severity::High);
}

#[test]
fn test_deprecated_severity_downgrade() {
    let source = r#"
@Deprecated
public class UserRepository {
    public void query(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        em.createQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    let findings = scanner.scan_source(source, Path::new("UserRepository.java"));

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection)
        .expect("Expected SQL injection finding");
    assert_eq!(finding.severity, Severity::Info);
}
