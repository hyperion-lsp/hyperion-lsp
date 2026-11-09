// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for array taint tracking (Phase 3, Task 3.2)

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_array_initialization_with_tainted_element() {
    // Test that arrays initialized with tainted data are tracked
    let source = r#"
public class UserController {
    public void searchUsers(HttpServletRequest request) {
        String id = request.getParameter("id");
        String[] params = new String[] { id };
        String query = "SELECT * FROM users WHERE id = " + params[0];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through array element"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_array_initialization_inline() {
    // Test inline array initialization: new String[] { tainted }
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        String[] params = new String[] { request.getParameter("id") };
        String query = "SELECT * FROM users WHERE id = " + params[0];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through inline array initialization"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_array_access_propagates_taint() {
    // Test that accessing a tainted array element propagates taint
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String[] ids = new String[] { request.getParameter("id") };
        String userId = ids[0];
        String query = "SELECT * FROM users WHERE id = " + userId;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through array access"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_array_unknown_index_conservative() {
    // When we don't know the index, treat access as tainted if any element is tainted
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request, int idx) {
        String[] params = new String[] { request.getParameter("id"), "safe" };
        String value = params[idx];  // Unknown index
        String query = "SELECT * FROM users WHERE id = " + value;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should conservatively detect SQL injection with unknown array index"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_command_injection_through_array() {
    // Test command injection through array element
    let source = r#"
public class CommandRunner {
    public void runCommand(@RequestParam String cmd) {
        String[] commands = new String[] { "ls", cmd };
        String command = commands[1];
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CommandRunner.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through array"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_clean_array_no_false_positive() {
    // Arrays with clean data should not trigger findings
    let source = r#"
public class UserController {
    public void getUser() {
        String[] ids = new String[] { "1", "2", "3" };
        String query = "SELECT * FROM users WHERE id = " + ids[0];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for clean array"
    );
}

#[test]
fn test_array_multiple_tainted_elements() {
    // Multiple tainted elements in an array
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        String id1 = request.getParameter("id1");
        String id2 = request.getParameter("id2");
        String[] ids = new String[] { id1, id2 };
        String query = "SELECT * FROM users WHERE id IN (" + ids[0] + "," + ids[1] + ")";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection with multiple tainted array elements"
    );
}

#[test]
fn test_array_direct_access_in_sink() {
    // Direct array access in sink without intermediate assignment
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String[] params = new String[] { request.getParameter("id") };
        String query = "SELECT * FROM users WHERE id = " + params[0];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection with direct array access in concatenation"
    );
}

#[test]
fn test_getparameter_values_array() {
    // Test request.getParameterValues() which returns String[]
    let source = r#"
public class UserController {
    public void searchUsers(HttpServletRequest request) {
        String[] ids = request.getParameterValues("ids");
        String query = "SELECT * FROM users WHERE id = " + ids[0];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from getParameterValues array"
    );
}

#[test]
fn test_array_sanitization() {
    // Array with sanitized element should not trigger finding
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        int id = Integer.parseInt(idParam);
        String[] params = new String[] { String.valueOf(id) };
        String query = "SELECT * FROM users WHERE id = " + params[0];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for array with sanitized element"
    );
}

#[test]
fn test_mixed_tainted_clean_array() {
    // Array with both tainted and clean elements
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        String tainted = request.getParameter("id");
        String[] mixed = new String[] { "clean", tainted, "also_clean" };
        String query = "SELECT * FROM users WHERE id = " + mixed[1];
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection with mixed tainted/clean array"
    );
}

#[test]
fn test_path_traversal_through_array() {
    // Test path traversal vulnerability through array
    let source = r#"
public class FileController {
    public void readFile(HttpServletRequest request) {
        String[] paths = new String[] { request.getParameter("file") };
        String path = paths[0];
        File file = new File(path);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal through array"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}
