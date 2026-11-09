// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Phase 1 (Tier 1) vulnerability detection tests
//!
//! Tests for the 5 high-value, low-false-positive checks:
//! 1. Sensitive data logging
//! 2. Mass assignment
//! 3. Stack trace exposure
//! 4. Hardcoded internal URLs
//! 5. Enhanced insecure random

use hyperion_parser::{Lexer, Parser};
use hyperion_security::checks::{CheckContext, CheckRegistry};
use std::path::Path;

fn check_code(source: &str) -> Vec<hyperion_security::finding::Finding> {
    let lexer = Lexer::new(source);
    let tokens = lexer.collect::<Vec<_>>();
    let (green_node, _errors) = Parser::parse(tokens);
    let tree = hyperion_parser::SyntaxNode::new_root(green_node);

    let registry = CheckRegistry::new();
    let file = Path::new("Test.java");
    let context = CheckContext::new(file, source);

    registry.check_tree(&tree, &context)
}

#[test]
fn test_sensitive_data_logging_password() {
    let source = r#"
        public class UserService {
            private static final Logger logger = LoggerFactory.getLogger(UserService.class);

            public void login(String username, String password) {
                logger.info("User {} logging in with password: {}", username, password);
            }
        }
    "#;

    let findings = check_code(source);

    // Should detect password in log
    let sensitive_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::SensitiveDataLogging)
        .collect();

    assert!(
        !sensitive_findings.is_empty(),
        "Should detect sensitive data logging"
    );
}

#[test]
fn test_sensitive_data_logging_token() {
    let source = r#"
        public class ApiClient {
            public void callApi(String apiToken) {
                System.out.println("Calling API with token: " + apiToken);
            }
        }
    "#;

    let findings = check_code(source);

    let sensitive_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::SensitiveDataLogging)
        .collect();

    assert!(
        !sensitive_findings.is_empty(),
        "Should detect API token logging"
    );
}

#[test]
fn test_mass_assignment_model_attribute() {
    let source = r#"
        @RestController
        public class UserController {
            @PostMapping("/user")
            public void updateUser(@ModelAttribute User user) {
                userRepository.save(user);
            }
        }
    "#;

    let findings = check_code(source);

    let mass_assignment_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::MassAssignment)
        .collect();

    assert!(
        !mass_assignment_findings.is_empty(),
        "Should detect mass assignment vulnerability"
    );
}

#[test]
fn test_mass_assignment_bean_utils() {
    let source = r#"
        public class UserService {
            public void updateUser(UserRequest request, User user) {
                BeanUtils.copyProperties(request, user);
                userRepository.save(user);
            }
        }
    "#;

    let findings = check_code(source);

    let mass_assignment_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::MassAssignment)
        .collect();

    assert!(
        !mass_assignment_findings.is_empty(),
        "Should detect BeanUtils.copyProperties mass assignment"
    );
}

#[test]
fn test_stack_trace_exposure_print_stack_trace() {
    let source = r#"
        @RestController
        public class ErrorController {
            @GetMapping("/data")
            public ResponseEntity<String> getData() {
                try {
                    return ResponseEntity.ok(fetchData());
                } catch (Exception e) {
                    e.printStackTrace();
                    return ResponseEntity.status(500).body("Error");
                }
            }
        }
    "#;

    let findings = check_code(source);

    let stack_trace_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::StackTraceExposure)
        .collect();

    assert!(
        !stack_trace_findings.is_empty(),
        "Should detect printStackTrace() in web handler"
    );
}

#[test]
fn test_stack_trace_exposure_return_message() {
    let source = r#"
        @RestController
        public class ErrorController {
            @ExceptionHandler
            public ResponseEntity<String> handleError(Exception e) {
                return ResponseEntity.status(500).body(e.getMessage());
            }
        }
    "#;

    let findings = check_code(source);

    let stack_trace_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::StackTraceExposure)
        .collect();

    assert!(
        !stack_trace_findings.is_empty(),
        "Should detect returning exception message"
    );
}

#[test]
fn test_hardcoded_internal_url_private_ip() {
    let source = r#"
        public class DatabaseConfig {
            private String dbUrl = "jdbc:mysql://10.0.0.5:3306/production";
        }
    "#;

    let findings = check_code(source);

    let internal_url_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::HardcodedInternalUrl)
        .collect();

    assert!(
        !internal_url_findings.is_empty(),
        "Should detect hardcoded private IP"
    );
}

#[test]
fn test_hardcoded_internal_url_localhost() {
    let source = r#"
        public class ApiClient {
            private String apiUrl = "http://localhost:8080/api";
        }
    "#;

    let findings = check_code(source);

    let internal_url_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::HardcodedInternalUrl)
        .collect();

    assert!(
        !internal_url_findings.is_empty(),
        "Should detect hardcoded localhost URL"
    );
}

#[test]
fn test_hardcoded_internal_url_internal_domain() {
    let source = r#"
        public class ServiceClient {
            private String serviceUrl = "http://api.internal/v1/users";
        }
    "#;

    let findings = check_code(source);

    let internal_url_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::HardcodedInternalUrl)
        .collect();

    assert!(
        !internal_url_findings.is_empty(),
        "Should detect hardcoded internal domain"
    );
}

#[test]
fn test_insecure_random_java_util_random() {
    let source = r#"
        public class TokenGenerator {
            public String generateToken() {
                Random random = new Random();
                byte[] token = new byte[32];
                random.nextBytes(token);
                return Base64.getEncoder().encodeToString(token);
            }
        }
    "#;

    let findings = check_code(source);

    let insecure_random_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::InsecureRandom)
        .collect();

    assert!(
        !insecure_random_findings.is_empty(),
        "Should detect java.util.Random for token generation"
    );
}

#[test]
fn test_insecure_random_math_random() {
    let source = r#"
        public class SessionIdGenerator {
            public String generateSessionId() {
                return String.valueOf(Math.random() * 1000000);
            }
        }
    "#;

    let findings = check_code(source);

    let insecure_random_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::InsecureRandom)
        .collect();

    assert!(
        !insecure_random_findings.is_empty(),
        "Should detect Math.random() for session ID"
    );
}

#[test]
fn test_no_false_positive_safe_logging() {
    let source = r#"
        public class UserService {
            private static final Logger logger = LoggerFactory.getLogger(UserService.class);

            public void login(String username) {
                logger.info("User {} logging in", username);
            }
        }
    "#;

    let findings = check_code(source);

    // Should not flag username logging
    let sensitive_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::SensitiveDataLogging)
        .collect();

    assert!(
        sensitive_findings.is_empty(),
        "Should not flag non-sensitive data logging"
    );
}

#[test]
fn test_no_false_positive_public_url() {
    let source = r#"
        public class ApiClient {
            private String apiUrl = "https://api.example.com/v1";
        }
    "#;

    let findings = check_code(source);

    let internal_url_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == hyperion_security::finding::VulnType::HardcodedInternalUrl)
        .collect();

    assert!(
        internal_url_findings.is_empty(),
        "Should not flag public URL"
    );
}
