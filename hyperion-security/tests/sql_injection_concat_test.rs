// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test SQL injection detection via string concatenation

use hyperion_security::{Finding, ScanConfig, SecurityScanner, VulnType};
use tempfile::TempDir;

/// Helper to scan a Java file and return findings
fn scan_java_code(source: &str) -> Vec<Finding> {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("Test.java");
    std::fs::write(&file_path, source).unwrap();

    let config = ScanConfig::default();
    let scanner = SecurityScanner::new(config);
    scanner.scan_file(&file_path)
}

#[test]
fn test_sql_injection_simple_concatenation() {
    let code = r#"
        package com.example;

        import javax.servlet.http.HttpServletRequest;
        import java.sql.*;

        public class UserController {
            public List<User> getUsers(HttpServletRequest request) {
                String id = request.getParameter("id");
                String query = "SELECT * FROM users WHERE id = " + id;

                Connection conn = getConnection();
                Statement stmt = conn.createStatement();
                ResultSet rs = stmt.executeQuery(query);

                return processResults(rs);
            }
        }
    "#;

    let findings = scan_java_code(code);

    // Should detect SQL injection
    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::SqlInjection))
        .collect();

    println!("Found {} SQL injection findings", sql_findings.len());
    for finding in &sql_findings {
        println!("  - {}", finding.message);
    }

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection via string concatenation"
    );
}

#[test]
fn test_sql_injection_string_builder() {
    let code = r#"
        package com.example;

        import javax.servlet.http.HttpServletRequest;
        import java.sql.*;

        public class ProductController {
            public List<Product> search(HttpServletRequest request) {
                String term = request.getParameter("search");

                StringBuilder query = new StringBuilder("SELECT * FROM products WHERE ");
                query.append("name LIKE '%" + term + "%'");

                Connection conn = getConnection();
                Statement stmt = conn.createStatement();
                ResultSet rs = stmt.executeQuery(query.toString());

                return processResults(rs);
            }
        }
    "#;

    let findings = scan_java_code(code);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::SqlInjection))
        .collect();

    println!("Found {} SQL injection findings", sql_findings.len());
    for finding in &sql_findings {
        println!("  - {}", finding.message);
    }

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection via StringBuilder"
    );
}

#[test]
fn test_sql_injection_string_format() {
    let code = r#"
        package com.example;

        import javax.servlet.http.HttpServletRequest;
        import java.sql.*;

        public class OrderController {
            public List<Order> getOrders(HttpServletRequest request) {
                String status = request.getParameter("status");
                String query = String.format("SELECT * FROM orders WHERE status = '%s'", status);

                Connection conn = getConnection();
                Statement stmt = conn.createStatement();
                ResultSet rs = stmt.executeQuery(query);

                return processResults(rs);
            }
        }
    "#;

    let findings = scan_java_code(code);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::SqlInjection))
        .collect();

    println!("Found {} SQL injection findings", sql_findings.len());
    for finding in &sql_findings {
        println!("  - {}", finding.message);
    }

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection via String.format"
    );
}

#[test]
fn test_no_false_positive_prepared_statement() {
    let code = r#"
        package com.example;

        import javax.servlet.http.HttpServletRequest;
        import java.sql.*;

        public class UserController {
            public List<User> getUsers(HttpServletRequest request) {
                String id = request.getParameter("id");
                String query = "SELECT * FROM users WHERE id = ?";

                Connection conn = getConnection();
                PreparedStatement ps = conn.prepareStatement(query);
                ps.setString(1, id);
                ResultSet rs = ps.executeQuery();

                return processResults(rs);
            }
        }
    "#;

    let findings = scan_java_code(code);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.vuln_type, VulnType::SqlInjection))
        .collect();

    println!("Found {} SQL injection findings", sql_findings.len());
    for finding in &sql_findings {
        println!("  - {}", finding.message);
    }

    // Should NOT flag PreparedStatement usage
    assert!(
        sql_findings.is_empty(),
        "PreparedStatement should be safe - no SQL injection"
    );
}
