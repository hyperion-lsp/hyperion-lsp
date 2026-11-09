// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for collection taint tracking (Phase 4.1)
//!
//! Tests that tainted data flowing through List, Map, and Set collections is properly tracked.
//! Uses a conservative approach: if ANY tainted element is added to a collection,
//! the ENTIRE collection is considered tainted.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

// =============================================================================
// List Taint Tests
// =============================================================================

#[test]
fn test_list_add_tainted_element() {
    // Test that List.add() with tainted element marks the list as tainted
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        List<String> params = new ArrayList<>();
        params.add(request.getParameter("id"));
        String value = params.get(0);
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through List taint flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_list_add_multiple_elements_one_tainted() {
    // Test that adding a tainted element after clean elements still marks list as tainted
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        List<String> params = new ArrayList<>();
        params.add("safe");
        params.add(request.getParameter("id"));  // Now list is tainted
        stmt.executeQuery(params.get(0));  // Should detect - list is tainted
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection even when accessing safe index of tainted list"
    );
}

#[test]
fn test_list_set_tainted_element() {
    // Test that List.set() with tainted element marks the list as tainted
    let source = r#"
public class UserController {
    public void update(HttpServletRequest request) {
        String taintedValue = request.getParameter("id");
        List<String> params = new ArrayList<>();
        params.add("placeholder");
        params.set(0, taintedValue);
        String value = params.get(0);
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through List.set() taint flow"
    );
}

#[test]
fn test_list_clean_elements_no_false_positive() {
    // Lists with only clean data should not trigger findings
    let source = r#"
public class UserController {
    public void search() {
        List<String> params = new ArrayList<>();
        params.add("1");
        params.add("2");
        String value = params.get(0);
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should be empty - no tainted data
    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for clean list"
    );
}

// =============================================================================
// Map Taint Tests
// =============================================================================

#[test]
fn test_map_put_tainted_value() {
    // Test that Map.put() with tainted value marks the map as tainted
    // Note: Using intermediate variable for tainted value to match working patterns
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedValue = request.getParameter("value");
        Map<String, String> params = new HashMap<>();
        params.put("key", taintedValue);
        String value = params.get("key");
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through Map taint flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_map_put_multiple_entries_one_tainted() {
    // Test that putting a tainted value marks the entire map as tainted
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedValue = request.getParameter("value");
        Map<String, String> params = new HashMap<>();
        params.put("safe_key", "safe_value");
        params.put("tainted_key", taintedValue);
        String value = params.get("safe_key");  // Get different key
        stmt.executeQuery(value);  // Should still detect - map is tainted
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection when map contains any tainted value"
    );
}

#[test]
fn test_map_clean_entries_no_false_positive() {
    // Maps with only clean data should not trigger findings
    let source = r#"
public class UserController {
    public void process() {
        Map<String, String> params = new HashMap<>();
        params.put("key1", "safe1");
        params.put("key2", "safe2");
        String value = params.get("key1");
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for clean map"
    );
}

// =============================================================================
// Set Taint Tests (Set doesn't have get(), but taint should propagate through iteration)
// =============================================================================

#[test]
fn test_set_add_tainted_element_to_string() {
    // Test that Set.add() with tainted element marks the set as tainted
    // and taint propagates through toString()
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        Set<String> params = new HashSet<>();
        params.add(request.getParameter("id"));
        String query = "SELECT * FROM users WHERE id IN " + params.toString();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through Set taint flow via toString()"
    );
}

// =============================================================================
// Command Injection Through Collections
// =============================================================================

#[test]
fn test_command_injection_through_list() {
    // Test command injection vulnerability through list
    let source = r#"
public class CommandRunner {
    public void runCommand(@RequestParam String cmd) {
        List<String> commands = new ArrayList<>();
        commands.add(cmd);
        String command = commands.get(0);
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CommandRunner.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through list"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

// =============================================================================
// Path Traversal Through Collections
// =============================================================================

#[test]
fn test_path_traversal_through_list() {
    // Test path traversal vulnerability through list
    let source = r#"
public class FileController {
    public void readFile(HttpServletRequest request) {
        List<String> paths = new ArrayList<>();
        paths.add(request.getParameter("file"));
        String path = paths.get(0);
        File file = new File(path);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal through list"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}

// =============================================================================
// Different Collection Types
// =============================================================================

#[test]
fn test_linkedlist_taint() {
    // Test that LinkedList is also tracked
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        LinkedList<String> params = new LinkedList<>();
        params.add(request.getParameter("id"));
        String value = params.get(0);
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through LinkedList"
    );
}

#[test]
fn test_treemap_taint() {
    // Test that TreeMap is also tracked
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedValue = request.getParameter("value");
        TreeMap<String, String> params = new TreeMap<>();
        params.put("key", taintedValue);
        String value = params.get("key");
        stmt.executeQuery(value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through TreeMap"
    );
}

// =============================================================================
// Complex Flow Tests
// =============================================================================

#[test]
fn test_collection_through_variable_chain() {
    // Test taint flow through multiple variable assignments
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        String input = request.getParameter("id");
        List<String> list = new ArrayList<>();
        list.add(input);
        String fromList = list.get(0);
        String query = "SELECT * FROM users WHERE id = " + fromList;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through variable chain with collection"
    );
}

#[test]
fn test_collection_direct_get_in_sink() {
    // Test direct collection.get() usage in sink without intermediate variable
    let source = r#"
public class UserController {
    public void search(HttpServletRequest request) {
        List<String> params = new ArrayList<>();
        params.add(request.getParameter("id"));
        String query = "SELECT * FROM users WHERE id = " + params.get(0);
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection with direct collection.get() in concatenation"
    );
}
