// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

#[test]
fn test_unused_private_method_severity_downgrade() {
    let source = r#"
public class Repo {
    public void safe() { }

    private void unused(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        em.createQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    let findings = scanner.scan_source(source, Path::new("src/main/java/Repo.java"));

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection)
        .expect("Expected SQL injection finding");
    assert_eq!(finding.severity, Severity::Info);
}

#[test]
fn test_called_private_method_keeps_severity() {
    let source = r#"
public class Repo {
    public void query() {
        unsafe("name");
    }

    private void unsafe(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        em.createQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    let findings = scanner.scan_source(source, Path::new("src/main/java/Repo.java"));

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection)
        .expect("Expected SQL injection finding");
    assert_ne!(finding.severity, Severity::Info);
}

#[test]
fn a_public_method_after_a_private_field_keeps_its_severity() {
    // The field's "private" is on the line above the method. It does not make the method
    // private, so the method's finding must not be downgraded as dead code.
    let source = r#"
public class Repo {
    private final Map<String, String> cache = new HashMap<>();

    public void lookup(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        em.createQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    let findings = scanner.scan_source(source, Path::new("src/main/java/Repo.java"));

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection)
        .expect("Expected SQL injection finding");
    assert_ne!(finding.severity, Severity::Info);
}
