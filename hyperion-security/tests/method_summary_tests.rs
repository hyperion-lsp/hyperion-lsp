// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

/// Test String.concat() propagates taint
#[test]
fn test_string_concat_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        String query = "SELECT * FROM users WHERE id = ".concat(id);
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.concat()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.format() propagates taint from arguments
#[test]
fn test_string_format_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        String query = String.format("SELECT * FROM users WHERE id = %s", id);
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.format()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.replace() propagates taint
#[test]
fn test_string_replace_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String input = request.getParameter("input");
        String sanitized = input.replace("'", "''");
        String query = "SELECT * FROM users WHERE name = '" + sanitized + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.replace() (simple replace is not safe sanitization)"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.toLowerCase() propagates taint
#[test]
fn test_string_tolowercase_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String name = request.getParameter("name");
        String normalized = name.toLowerCase();
        String query = "SELECT * FROM users WHERE LOWER(name) = '" + normalized + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.toLowerCase()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.trim() propagates taint
#[test]
fn test_string_trim_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String name = request.getParameter("name");
        String trimmed = name.trim();
        String query = "SELECT * FROM users WHERE name = '" + trimmed + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.trim()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.substring() propagates taint
#[test]
fn test_string_substring_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String fullName = request.getParameter("name");
        String firstName = fullName.substring(0, 5);
        String query = "SELECT * FROM users WHERE name LIKE '" + firstName + "%'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.substring()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test StringBuilder.toString() propagates taint (already covered in stringbuilder_tests.rs)
#[test]
fn test_stringbuilder_tostring_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        StringBuilder builder = new StringBuilder();
        builder.append("SELECT * FROM users WHERE id = ");
        builder.append(id);
        String query = builder.toString();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through StringBuilder.toString()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test Optional.get() propagates taint
#[test]
fn test_optional_get_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        Optional<String> optId = Optional.ofNullable(request.getParameter("id"));
        String id = optId.get();
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test Integer.parseInt() does NOT propagate taint (sanitizer)
#[test]
fn test_integer_parseint_sanitizes() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        int id = Integer.parseInt(idParam);
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should NOT detect SQL injection when Integer.parseInt() sanitizes the input"
    );
}

/// Test method summary with chained calls
#[test]
fn test_chained_string_methods_propagate_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String name = request.getParameter("name");
        String normalized = name.trim().toLowerCase().substring(0, 10);
        String query = "SELECT * FROM users WHERE name = '" + normalized + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.join() propagates taint
#[test]
fn test_string_join_propagates_taint() {
    let source = r#"
public class UserController {
    public void searchUsers(HttpServletRequest request) {
        String term1 = request.getParameter("term1");
        String term2 = request.getParameter("term2");
        String combined = String.join(" OR ", term1, term2);
        String query = "SELECT * FROM users WHERE (" + combined + ")";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test String.replaceAll() propagates taint
#[test]
fn test_string_replaceall_propagates_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String input = request.getParameter("input");
        String cleaned = input.replaceAll("[^a-zA-Z0-9]", "");
        String query = "SELECT * FROM users WHERE name = '" + cleaned + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through String.replaceAll() (regex replace is not safe sanitization)"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test safe usage - no taint source
#[test]
fn test_string_methods_with_safe_data() {
    let source = r#"
public class UserController {
    public void getUser() {
        String safe = "admin";
        String normalized = safe.toLowerCase().trim();
        String query = "SELECT * FROM users WHERE role = '" + normalized + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should NOT detect SQL injection when using safe (non-tainted) data"
    );
}

/// Test multiple method summaries in sequence
#[test]
fn test_multiple_transformations_propagate_taint() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String rawName = request.getParameter("name");
        String step1 = rawName.trim();
        String step2 = step1.toLowerCase();
        String step3 = step2.substring(0, 5);
        String step4 = step3.replace("'", "");
        String query = "SELECT * FROM users WHERE name LIKE '" + step4 + "%'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should track taint through multiple string transformation methods"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));

    // Verify the data flow tracks all transformations
    let finding = &findings[0];
    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    // Should track: source → trim → toLowerCase → substring → replace → concat → sink
    assert!(
        finding.dataflow.len() >= 3,
        "Should track multiple transformation steps"
    );
}

/// Test that numeric parsing in the middle of a flow sanitizes
#[test]
fn test_numeric_parsing_breaks_taint_flow() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idStr = request.getParameter("id");
        String trimmed = idStr.trim();
        int id = Integer.parseInt(trimmed);
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should NOT detect SQL injection - Integer.parseInt sanitizes even after trim()"
    );
}

/// Test command injection with String methods
#[test]
fn test_string_methods_command_injection() {
    let source = r#"
public class ProcessController {
    public void runCommand(@RequestParam String input) {
        String normalized = input.toLowerCase().trim();
        String command = "echo ".concat(normalized);
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProcessController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through String methods"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

/// Test XSS with String methods
#[test]
fn test_string_methods_xss() {
    let source = r#"
public class TemplateController {
    public void render(HttpServletRequest request, HttpServletResponse response) throws IOException {
        String userContent = request.getParameter("content");
        String formatted = userContent.trim().toUpperCase();
        PrintWriter out = response.getWriter();
        out.write("<div>" + formatted + "</div>");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TemplateController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect XSS through String methods"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Xss));
}
