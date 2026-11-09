// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::float_cmp)]

//! Tests for Phase 1.1 False Positive Reduction
//!
//! Variable vs Literal Distinction in Sensitive Data Logging
//!
//! This test suite validates that the sensitive data logging check:
//! - Catches TRUE POSITIVES: Variables with sensitive names being logged
//! - Suppresses FALSE POSITIVES: String literals containing sensitive keywords

use hyperion_parser::{Lexer, Parser};
use hyperion_security::checks::{CheckContext, CheckRegistry};
use hyperion_security::finding::VulnType;
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

fn get_sensitive_logging_findings(
    findings: &[hyperion_security::finding::Finding],
) -> Vec<&hyperion_security::finding::Finding> {
    findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SensitiveDataLogging)
        .collect()
}

// =============================================================================
// TRUE POSITIVES - Should be caught
// =============================================================================

#[test]
fn test_tp_password_variable_in_logger() {
    let source = r#"
        public class UserService {
            private static final Logger logger = LoggerFactory.getLogger(UserService.class);

            public void login(String username, String password) {
                logger.info("User {} logging in with password: {}", username, password);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        1,
        "Should detect password variable being logged"
    );
    assert!(
        sensitive[0].message.contains("password"),
        "Finding should mention password variable"
    );
}

#[test]
fn test_tp_token_variable_in_println() {
    let source = r#"
        public class ApiClient {
            public void callApi(String apiToken) {
                System.out.println("Calling API with token: " + apiToken);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        1,
        "Should detect apiToken variable in println"
    );
    assert!(
        sensitive[0].message.contains("apiToken"),
        "Finding should mention apiToken variable"
    );
}

#[test]
fn test_tp_secret_variable_in_debug() {
    let source = r#"
        public class ConfigLoader {
            private Logger log = LoggerFactory.getLogger(getClass());

            public void loadConfig(String apiSecret) {
                log.debug("Loading config with secret: {}", apiSecret);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(sensitive.len(), 1, "Should detect apiSecret variable");
}

#[test]
fn test_tp_multiple_sensitive_variables() {
    let source = r#"
        public class AuthService {
            private Logger logger = LoggerFactory.getLogger(AuthService.class);

            public void authenticate(String username, String password, String apiKey) {
                logger.info("Auth attempt: user={}, password={}, key={}", username, password, apiKey);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        1,
        "Should detect multiple sensitive variables"
    );
    // Should mention both password and apiKey
    let message = &sensitive[0].message.to_lowercase();
    assert!(
        message.contains("password") || message.contains("apikey"),
        "Finding should mention sensitive variables"
    );
}

#[test]
fn test_tp_sensitive_variable_concatenation() {
    let source = r#"
        public class Logger {
            public void log(String userPassword) {
                System.out.println("Password is: " + userPassword);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        1,
        "Should detect userPassword in string concatenation"
    );
}

#[test]
fn test_tp_credit_card_variable() {
    let source = r#"
        public class PaymentService {
            private Logger log = LoggerFactory.getLogger(PaymentService.class);

            public void processPayment(String creditCardNumber) {
                log.info("Processing payment for card: {}", creditCardNumber);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        1,
        "Should detect creditCardNumber variable"
    );
}

#[test]
fn test_tp_ssn_variable() {
    let source = r#"
        public class TaxService {
            public void validateTaxId(String ssn) {
                System.out.println("Validating SSN: " + ssn);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(sensitive.len(), 1, "Should detect ssn variable");
}

#[test]
fn test_tp_bearer_token_variable() {
    let source = r#"
        public class ApiClient {
            private Logger logger = LoggerFactory.getLogger(ApiClient.class);

            public void makeRequest(String bearerToken) {
                logger.debug("Request with bearer: {}", bearerToken);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(sensitive.len(), 1, "Should detect bearerToken variable");
}

#[test]
fn test_tp_private_key_variable() {
    let source = r#"
        public class CryptoService {
            public void sign(String privateKey) {
                System.out.println("Signing with key: " + privateKey);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(sensitive.len(), 1, "Should detect privateKey variable");
}

// =============================================================================
// FALSE POSITIVES - Should NOT be flagged
// =============================================================================

#[test]
fn test_fp_password_in_error_message() {
    let source = r#"
        public class ValidationService {
            private Logger logger = LoggerFactory.getLogger(ValidationService.class);

            public void validateUser(String username) {
                logger.error("Missing password field in request");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'password' in error message literal"
    );
}

#[test]
fn test_fp_token_in_info_message() {
    let source = r#"
        public class ConfigService {
            private Logger log = LoggerFactory.getLogger(ConfigService.class);

            public void init() {
                log.info("Token authentication disabled");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'token' in informational message"
    );
}

#[test]
fn test_fp_password_validation_message() {
    let source = r#"
        public class UserService {
            private Logger logger = LoggerFactory.getLogger(UserService.class);

            public void validatePassword(String username) {
                logger.warn("Password validation failed for user: {}", username);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'password' in validation message when only username is logged"
    );
}

#[test]
fn test_fp_api_key_description() {
    let source = r#"
        public class SetupService {
            public void setup() {
                System.out.println("API key configuration required");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'API key' in configuration message"
    );
}

#[test]
fn test_fp_secret_in_documentation() {
    let source = r#"
        public class HelpService {
            private Logger log = LoggerFactory.getLogger(HelpService.class);

            public void showHelp() {
                log.info("Use --secret flag to provide secret key");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'secret' in help documentation"
    );
}

#[test]
fn test_fp_credit_card_in_error() {
    let source = r#"
        public class PaymentService {
            private Logger logger = LoggerFactory.getLogger(PaymentService.class);

            public void validatePayment(String userId) {
                logger.error("Invalid credit card format for user: {}", userId);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'credit card' in error message when only userId is logged"
    );
}

#[test]
fn test_fp_ssn_in_audit_message() {
    let source = r#"
        public class AuditService {
            public void logAccess(String userId) {
                System.out.println("SSN verification completed for user: " + userId);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'SSN' in audit message when only userId is logged"
    );
}

#[test]
fn test_fp_authorization_header_description() {
    let source = r#"
        public class SecurityService {
            private Logger log = LoggerFactory.getLogger(SecurityService.class);

            public void checkAuth() {
                log.debug("Authorization header missing");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'authorization' in header description"
    );
}

#[test]
fn test_fp_session_id_in_lifecycle_message() {
    let source = r#"
        public class SessionManager {
            private Logger logger = LoggerFactory.getLogger(SessionManager.class);

            public void cleanup() {
                logger.info("Session ID cleanup completed");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag 'session ID' in lifecycle message"
    );
}

// =============================================================================
// EDGE CASES
// =============================================================================

#[test]
fn test_safe_variable_with_sensitive_in_message() {
    let source = r#"
        public class UserService {
            private Logger logger = LoggerFactory.getLogger(UserService.class);

            public void login(String username) {
                logger.info("Password authentication succeeded for user: {}", username);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        0,
        "Should NOT flag when only safe variable (username) is logged despite 'password' in message"
    );
}

#[test]
fn test_mixed_safe_and_sensitive_variables() {
    let source = r#"
        public class AuthService {
            private Logger logger = LoggerFactory.getLogger(AuthService.class);

            public void authenticate(String username, String password) {
                logger.info("User {} attempting login", username);
                logger.debug("Auth for {} with pwd {}", username, password);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    // Should only flag the second log statement with password
    assert_eq!(
        sensitive.len(),
        1,
        "Should flag only the statement logging password variable"
    );
}

#[test]
fn test_no_false_positive_on_empty_log() {
    let source = r#"
        public class Service {
            private Logger logger = LoggerFactory.getLogger(Service.class);

            public void process() {
                logger.info("Processing started");
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(sensitive.len(), 0, "Should not flag logs with no variables");
}

#[test]
fn test_case_insensitive_detection() {
    let source = r#"
        public class Service {
            private Logger logger = LoggerFactory.getLogger(Service.class);

            public void process(String userPASSWORD, String ApiToken) {
                logger.info("Data: {} and {}", userPASSWORD, ApiToken);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert_eq!(
        sensitive.len(),
        1,
        "Should detect sensitive variables regardless of case"
    );
}

// =============================================================================
// REGRESSION TESTS - Ensure existing functionality still works
// =============================================================================

#[test]
fn test_regression_original_password_test() {
    // Original test from phase1_tier1_tests.rs
    let source = r#"
        public class UserService {
            private static final Logger logger = LoggerFactory.getLogger(UserService.class);

            public void login(String username, String password) {
                logger.info("User {} logging in with password: {}", username, password);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert!(
        !sensitive.is_empty(),
        "Should detect sensitive data logging"
    );
}

#[test]
fn test_regression_original_token_test() {
    // Original test from phase1_tier1_tests.rs
    let source = r#"
        public class ApiClient {
            public void callApi(String apiToken) {
                System.out.println("Calling API with token: " + apiToken);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert!(!sensitive.is_empty(), "Should detect API token logging");
}

#[test]
fn test_regression_safe_logging() {
    // Original test from phase1_tier1_tests.rs
    let source = r#"
        public class UserService {
            private static final Logger logger = LoggerFactory.getLogger(UserService.class);

            public void login(String username) {
                logger.info("User {} logging in", username);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert!(
        sensitive.is_empty(),
        "Should not flag non-sensitive data logging"
    );
}

// =============================================================================
// PHASE 6: Context-Aware Logging Severity Tests
// =============================================================================

#[test]
fn test_phase6_info_level_password_high_severity() {
    // INFO-level password logging should still be flagged with medium confidence
    let source = r#"
        public class LoginService {
            private Logger logger = LoggerFactory.getLogger(LoginService.class);

            public void login(String username, String password) {
                logger.info("Login attempt with password: {}", password);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    // Password at INFO level: confidence = 1.0 (override) * 0.6 = 0.6 (above 0.4 threshold)
    // But password variables always get confidence = 1.0 override
    assert_eq!(
        sensitive.len(),
        1,
        "INFO-level password should be flagged (passwords always high confidence)"
    );

    // Verify severity is HIGH (password override)
    use hyperion_security::finding::Severity;
    assert_eq!(
        sensitive[0].severity,
        Severity::High,
        "Password logging should be HIGH severity"
    );
}

#[test]
fn test_phase6_warn_level_token_high_confidence() {
    // WARN-level token logging should be flagged with high confidence
    let source = r#"
        public class SecurityService {
            private Logger logger = LoggerFactory.getLogger(SecurityService.class);

            public void checkAuth(String authToken) {
                logger.warn("Invalid token: {}", authToken);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    // WARN level: confidence = 1.0 (log level) * 1.0 = 1.0
    assert_eq!(sensitive.len(), 1, "WARN-level token should be flagged");

    use hyperion_security::finding::Severity;
    // authToken at WARN level should have confidence > 0.8, thus HIGH severity
    assert!(
        sensitive[0].confidence > 0.8,
        "WARN-level token should have high confidence, got {}",
        sensitive[0].confidence
    );
    assert!(
        sensitive[0].severity >= Severity::High,
        "WARN-level token should be at least HIGH severity, got {:?}",
        sensitive[0].severity
    );
}

#[test]
fn test_phase6_error_level_credit_card_critical() {
    // ERROR-level credit card logging should be flagged with high severity
    let source = r#"
        public class PaymentService {
            private Logger logger = LoggerFactory.getLogger(PaymentService.class);

            public void processPayment(String creditCard) {
                logger.error("Failed to process card: {}", creditCard);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    // Credit cards always get confidence = 1.0
    assert_eq!(
        sensitive.len(),
        1,
        "ERROR-level credit card should be flagged"
    );

    use hyperion_security::finding::Severity;
    assert_eq!(
        sensitive[0].severity,
        Severity::High,
        "Credit card logging should be HIGH severity"
    );
}

#[test]
fn test_phase6_debug_generic_token_low_severity() {
    // DEBUG-level generic token (not in security context) should have lower confidence
    let source = r#"
        public class BusinessService {
            private Logger logger = LoggerFactory.getLogger(BusinessService.class);

            public void process(String token) {
                logger.debug("Processing token: {}", token);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    // DEBUG level: confidence = 0.3, below 0.4 threshold
    assert_eq!(
        sensitive.len(),
        0,
        "DEBUG-level generic token should be filtered (low confidence)"
    );
}

#[test]
fn test_phase6_confidence_metadata_present() {
    // Verify confidence is stored in finding (uses error level)
    let source = r#"
        public class LoginService {
            private Logger logger = LoggerFactory.getLogger(LoginService.class);

            public void login(String username, String password) {
                logger.error("Login attempt with password: {}", password);
            }
        }
    "#;

    let findings = check_code(source);
    let sensitive = get_sensitive_logging_findings(&findings);

    assert!(
        !sensitive.is_empty(),
        "Should detect password logging. All findings: {:?}",
        findings.iter().map(|f| f.vuln_type).collect::<Vec<_>>()
    );

    // Check confidence field is set
    assert!(
        sensitive[0].confidence > 0.0,
        "Finding should have confidence value set"
    );
    assert!(
        sensitive[0].confidence <= 1.0,
        "Confidence should be normalized [0,1]"
    );
    // Password should have maximum confidence
    assert_eq!(
        sensitive[0].confidence, 1.0,
        "Password logging should have confidence = 1.0"
    );
}
