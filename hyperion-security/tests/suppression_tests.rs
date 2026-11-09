// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

fn scan(source: &str) -> Vec<hyperion_security::Finding> {
    let scanner = SecurityScanner::new(ScanConfig::default().with_min_severity(Severity::Info));
    scanner.scan_source(source, Path::new("UserRepository.java"))
}

#[test]
fn test_inline_suppression() {
    let source = r#"
public class UserRepository {
    public void query(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        em.createQuery(sql); // hyperion-security: ignore sql_injection
    }
}
"#;

    let findings = scan(source);
    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Inline suppression should remove SQL injection finding"
    );
}

#[test]
fn test_previous_line_suppression() {
    let source = r#"
public class UserRepository {
    public void query(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        // hyperion-security: ignore CWE-89
        em.createQuery(sql);
    }
}
"#;

    let findings = scan(source);
    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Previous-line suppression should remove SQL injection finding"
    );
}

#[test]
fn test_ignore_next_line_suppression() {
    let source = r#"
public class UserRepository {
    public void query(@RequestParam String name) {
        String sql = "SELECT * FROM users WHERE name = '" + name + "'";
        // hyperion-security: ignore-next-line
        em.createQuery(sql);
    }
}
"#;

    let findings = scan(source);
    assert!(
        !findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "ignore-next-line suppression should remove SQL injection finding"
    );
}
