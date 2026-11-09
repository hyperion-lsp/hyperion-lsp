// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_field_assignment_with_tainted_value() {
    let source = r#"
public class UserService {
    private String userId;

    public void setUserId(HttpServletRequest request) {
        this.userId = request.getParameter("id");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    // The field assignment itself is not a vulnerability, but it should be tracked
    // We'll verify in the next test that the field taint propagates
}

#[test]
fn test_field_access_propagates_taint() {
    let source = r#"
public class UserService {
    private String userId;

    public void setUserId(HttpServletRequest request) {
        this.userId = request.getParameter("id");
    }

    public void search() {
        String query = "SELECT * FROM users WHERE id = " + this.userId;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through field"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_field_without_this_keyword() {
    let source = r#"
public class UserService {
    private String userId;

    public void setUserId(HttpServletRequest request) {
        userId = request.getParameter("id");
    }

    public void search() {
        String query = "SELECT * FROM users WHERE id = " + userId;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through field (without this.)"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_cross_method_field_taint() {
    let source = r#"
public class OrderService {
    private String orderId;

    public void initialize(HttpServletRequest request) {
        this.orderId = request.getParameter("orderId");
    }

    public void processOrder() {
        String cmd = "process_order " + this.orderId;
        Runtime.getRuntime().exec(cmd);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OrderService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through field"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_multiple_fields() {
    let source = r#"
public class UserService {
    private String username;
    private String email;

    public void setUser(HttpServletRequest request) {
        this.username = request.getParameter("username");
        this.email = request.getParameter("email");
    }

    public void logUser() {
        logger.info("User: " + this.username + ", Email: " + this.email);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    // Both fields should be tainted, and log injection should be detected
    assert!(
        !findings.is_empty(),
        "Should detect log injection through fields"
    );
}

#[test]
fn test_field_taint_persists_across_methods() {
    let source = r#"
public class SessionService {
    private String sessionId;

    public void login(HttpServletRequest request) {
        this.sessionId = request.getParameter("sessionId");
    }

    public void query1() {
        String sql = "SELECT * FROM sessions WHERE id = '" + this.sessionId + "'";
        stmt.executeQuery(sql);
    }

    public void query2() {
        String sql = "UPDATE sessions SET active = 1 WHERE id = '" + this.sessionId + "'";
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SessionService.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect SQL injection in both query1() and query2()
    assert!(
        findings.len() >= 2,
        "Should detect SQL injection in both methods, got {}",
        findings.len()
    );
    assert!(
        findings
            .iter()
            .filter(|f| f.vuln_type == VulnType::SqlInjection)
            .count()
            >= 2
    );
}

#[test]
fn test_field_not_tainted_if_clean() {
    let source = r#"
public class ConstantService {
    private String tableName = "users";

    public void query() {
        String sql = "SELECT * FROM " + this.tableName;
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConstantService.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect SQL injection because field is initialized with constant
    assert!(findings.is_empty(), "Should not flag constant field");
}

#[test]
fn test_field_sanitized_with_numeric_conversion() {
    let source = r#"
public class UserService {
    private int userId;

    public void setUserId(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        this.userId = Integer.parseInt(idParam);
    }

    public void search() {
        String query = "SELECT * FROM users WHERE id = " + this.userId;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect SQL injection because numeric conversion sanitizes
    assert!(findings.is_empty(), "Should not flag numeric field");
}

#[test]
fn test_field_path_traversal() {
    let source = r#"
public class FileService {
    private String filename;

    public void setFilename(HttpServletRequest request) {
        this.filename = request.getParameter("file");
    }

    public void readFile() throws IOException {
        FileInputStream fis = new FileInputStream(this.filename);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal through field"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}

#[test]
fn test_mixed_field_and_variable_taint() {
    let source = r#"
public class MixedService {
    private String fieldData;

    public void process(HttpServletRequest request) {
        String localData = request.getParameter("local");
        this.fieldData = request.getParameter("field");

        // Local variable taint
        String query1 = "SELECT * FROM t1 WHERE data = " + localData;
        stmt.executeQuery(query1);

        // Field taint
        String query2 = "SELECT * FROM t2 WHERE data = " + this.fieldData;
        stmt.executeQuery(query2);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MixedService.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect both SQL injections
    assert!(
        findings.len() >= 2,
        "Should detect both SQL injections, got {}",
        findings.len()
    );
    assert!(
        findings
            .iter()
            .filter(|f| f.vuln_type == VulnType::SqlInjection)
            .count()
            >= 2
    );
}

#[test]
fn test_field_reassignment() {
    let source = r#"
public class ReassignService {
    private String data;

    public void taint(HttpServletRequest request) {
        this.data = request.getParameter("data");
    }

    public void clean() {
        this.data = "clean_constant";
    }

    public void query() {
        String sql = "SELECT * FROM t WHERE data = " + this.data;
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ReassignService.java"));

    eprintln!("Findings: {:?}", findings);

    // After clean() is called, field should be clean, but we don't have
    // control flow analysis yet, so this should still detect SQL injection
    // This is a conservative approach - it's better to flag potential issues
    assert!(
        !findings.is_empty(),
        "Should conservatively flag SQL injection"
    );
}
