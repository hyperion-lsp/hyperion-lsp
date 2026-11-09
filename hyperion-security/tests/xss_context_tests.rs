// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for XSS context-aware detection
//!
//! These tests verify that the scanner correctly identifies when XSS checks
//! should be skipped in REST controller contexts (JSON responses).

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

/// Helper to filter XSS findings
fn xss_findings(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Xss)
        .collect()
}

#[test]
fn test_rest_controller_response_entity_no_xss() {
    let source = r#"
        package com.example;

        import org.springframework.web.bind.annotation.RestController;
        import org.springframework.web.bind.annotation.GetMapping;
        import org.springframework.web.bind.annotation.RequestParam;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @GetMapping("/user")
            public ResponseEntity<String> getUser(@RequestParam String input) {
                // This should NOT trigger XSS - JSON response
                return ResponseEntity.ok(input);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert_eq!(
        xss.len(),
        0,
        "Expected no XSS findings in @RestController, got: {:?}",
        xss
    );
}

#[test]
fn test_controller_with_json_produces_no_xss() {
    let source = r#"
        package com.example;

        import org.springframework.stereotype.Controller;
        import org.springframework.web.bind.annotation.RequestMapping;
        import org.springframework.web.bind.annotation.RequestParam;
        import org.springframework.http.ResponseEntity;

        @Controller
        public class UserController {
            @RequestMapping(produces = "application/json")
            public ResponseEntity<String> getUser(@RequestParam String input) {
                // This should NOT trigger XSS - JSON produces
                return ResponseEntity.ok(input);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert_eq!(
        xss.len(),
        0,
        "Expected no XSS findings in @Controller with JSON produces, got: {:?}",
        xss
    );
}

#[test]
fn test_controller_with_html_produces_triggers_xss() {
    let source = r#"
        package com.example;

        import org.springframework.stereotype.Controller;
        import org.springframework.web.bind.annotation.RequestMapping;
        import org.springframework.web.bind.annotation.RequestParam;
        import org.springframework.http.ResponseEntity;

        @Controller
        public class UserController {
            @RequestMapping(produces = "text/html")
            public ResponseEntity<String> getUser(@RequestParam String input) {
                // This SHOULD trigger XSS - HTML response
                return ResponseEntity.ok(input);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert!(
        !xss.is_empty(),
        "Expected XSS finding for HTML response in @Controller"
    );
}

#[test]
fn test_plain_class_response_entity_triggers_xss() {
    let source = r#"
        package com.example;

        import org.springframework.http.ResponseEntity;
        import org.springframework.web.bind.annotation.RequestParam;

        public class UserService {
            public ResponseEntity<String> getUser(@RequestParam String input) {
                // This SHOULD trigger XSS - unknown context (not a REST controller)
                return ResponseEntity.ok(input);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert!(
        !xss.is_empty(),
        "Expected XSS finding for ResponseEntity in non-REST class"
    );
}

#[test]
fn test_rest_controller_with_request_param() {
    let source = r#"
        package com.example;

        import org.springframework.web.bind.annotation.RestController;
        import org.springframework.web.bind.annotation.GetMapping;
        import org.springframework.web.bind.annotation.RequestParam;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @GetMapping("/user")
            public ResponseEntity<String> getUser(@RequestParam String name) {
                // This should NOT trigger XSS - JSON response in @RestController
                return ResponseEntity.ok(name);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert_eq!(
        xss.len(),
        0,
        "Expected no XSS findings in @RestController with @RequestParam, got: {:?}",
        xss
    );
}

#[test]
fn test_rest_controller_body_method() {
    let source = r#"
        package com.example;

        import org.springframework.web.bind.annotation.RestController;
        import org.springframework.web.bind.annotation.PostMapping;
        import org.springframework.web.bind.annotation.RequestBody;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @PostMapping("/user")
            public ResponseEntity<String> createUser(@RequestBody String userData) {
                // This should NOT trigger XSS - JSON response using body() method
                return ResponseEntity.status(201).body(userData);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert_eq!(
        xss.len(),
        0,
        "Expected no XSS findings in @RestController with body() method, got: {:?}",
        xss
    );
}

#[test]
fn test_multiple_methods_in_rest_controller() {
    let source = r#"
        package com.example;

        import org.springframework.web.bind.annotation.RestController;
        import org.springframework.web.bind.annotation.GetMapping;
        import org.springframework.web.bind.annotation.PostMapping;
        import org.springframework.web.bind.annotation.RequestParam;
        import org.springframework.web.bind.annotation.RequestBody;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @GetMapping("/user")
            public ResponseEntity<String> getUser(@RequestParam String input) {
                return ResponseEntity.ok(input);
            }

            @PostMapping("/user")
            public ResponseEntity<String> createUser(@RequestBody String userData) {
                return ResponseEntity.ok(userData);
            }
        }
    "#;

    let findings = scan_java_code(source);
    let xss = xss_findings(&findings);

    assert_eq!(
        xss.len(),
        0,
        "Expected no XSS findings in @RestController with multiple methods, got: {:?}",
        xss
    );
}
