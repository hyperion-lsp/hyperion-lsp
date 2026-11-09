// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Integration test verifying sanitizer registry is properly wired
//!
//! This test verifies the fix for the critical bug where the sanitizer registry
//! from framework detection was being discarded, causing 10-30% false positive inflation.

use hyperion_security::{FrameworkRegistry, ScanConfig, SecurityScanner, VulnType};

#[test]
fn test_sanitizer_registry_prevents_false_positives() {
    // Test case: Spring's HtmlUtils.htmlEscape() should prevent XSS detection
    let code = r#"
        package com.example;

        import org.springframework.web.util.HtmlUtils;
        import javax.servlet.http.HttpServletRequest;

        public class UserController {
            public String display(HttpServletRequest request) {
                String userInput = request.getParameter("name");
                String safe = HtmlUtils.htmlEscape(userInput);
                return "<div>" + safe + "</div>";  // Should NOT be flagged - sanitized
            }
        }
    "#;

    // Create scanner with framework detection (includes sanitizers)
    let mut framework_registry = FrameworkRegistry::with_builtin();
    framework_registry.enable_frameworks(&["spring", "servlet"]);
    let (source_registry, sink_registry, sanitizer_registry) = framework_registry.into_registries();

    let scanner = SecurityScanner::with_frameworks(
        ScanConfig::default(),
        source_registry,
        sink_registry,
        sanitizer_registry,
    );

    // Parse and scan
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("UserController.java");
    std::fs::write(&file_path, code).unwrap();

    let findings = scanner.scan_file(&file_path);

    // Verify: Should NOT detect XSS because HtmlUtils.htmlEscape() sanitizes the input
    let xss_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::Xss))
        .collect();

    assert_eq!(
        xss_findings.len(),
        0,
        "HtmlUtils.htmlEscape() should prevent XSS detection (sanitizer registry working)"
    );
}

#[test]
fn test_uuid_from_string_sanitizer_prevents_sql_injection() {
    // Test case: UUID.fromString() enforces a strict format and should prevent SQL injection.
    let code = r#"
        package com.example;

        import java.util.UUID;
        import javax.servlet.http.HttpServletRequest;
        import java.sql.Statement;

        public class UserRepository {
            public void find(HttpServletRequest request, Statement stmt) throws Exception {
                String userId = request.getParameter("id");
                UUID safe = UUID.fromString(userId);
                String query = "SELECT * FROM users WHERE id = '" + safe + "'";
                stmt.executeQuery(query);  // Should NOT be flagged - sanitized
            }
        }
    "#;

    // Create scanner with framework detection
    let mut framework_registry = FrameworkRegistry::with_builtin();
    framework_registry.enable_frameworks(&["servlet"]);
    let (source_registry, sink_registry, sanitizer_registry) = framework_registry.into_registries();

    let scanner = SecurityScanner::with_frameworks(
        ScanConfig::default(),
        source_registry,
        sink_registry,
        sanitizer_registry,
    );

    // Parse and scan
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("UserRepository.java");
    std::fs::write(&file_path, code).unwrap();

    let findings = scanner.scan_file(&file_path);

    // Verify: Should NOT detect SQL injection because StringEscapeUtils.escapeSql() sanitizes
    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::SqlInjection))
        .collect();

    assert_eq!(
        sql_findings.len(),
        0,
        "UUID.fromString() should prevent SQL injection detection"
    );
}

#[test]
fn test_builtin_sanitizers_still_work() {
    // Test case: Built-in sanitizers (Integer.parseInt) should still work
    let code = r#"
        package com.example;

        import javax.servlet.http.HttpServletRequest;
        import java.sql.Statement;

        public class UserRepository {
            public void find(HttpServletRequest request, Statement stmt) throws Exception {
                String idParam = request.getParameter("id");
                int id = Integer.parseInt(idParam);  // Built-in sanitizer
                String query = "SELECT * FROM users WHERE id = " + id;
                stmt.executeQuery(query);  // Should NOT be flagged - integer conversion sanitizes
            }
        }
    "#;

    // Create scanner with framework detection
    let mut framework_registry = FrameworkRegistry::with_builtin();
    framework_registry.enable_frameworks(&["servlet"]);
    let (source_registry, sink_registry, sanitizer_registry) = framework_registry.into_registries();

    let scanner = SecurityScanner::with_frameworks(
        ScanConfig::default(),
        source_registry,
        sink_registry,
        sanitizer_registry,
    );

    // Parse and scan
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("UserRepository.java");
    std::fs::write(&file_path, code).unwrap();

    let findings = scanner.scan_file(&file_path);

    // Verify: Should NOT detect SQL injection because Integer.parseInt() sanitizes
    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::SqlInjection))
        .collect();

    assert_eq!(
        sql_findings.len(),
        0,
        "Integer.parseInt() should prevent SQL injection detection"
    );
}
