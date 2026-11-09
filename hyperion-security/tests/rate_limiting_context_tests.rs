// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Tests for context-aware rate limiting detection (Phase 2.2)
//!
//! This test suite verifies that the rate limiting check correctly:
//! - Skips health check and monitoring endpoints
//! - Flags authentication endpoints as Critical
//! - Flags payment endpoints as Critical
//! - Flags registration endpoints as High
//! - Assigns appropriate severity based on endpoint type

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

#[test]
fn test_rate_limit_diagnostic_highlights_only_class_name() {
    for name in ["AuthController", "CaféController", "module"] {
        for newline in ["\n", "\r\n"] {
            let source = format!(
                "@RestController\n@SuppressWarnings(\"😀\") public class {name}<T> {{\n\
                 @PostMapping(\"/auth/login\")\n\
                 public String login() {{\n\
                     return authenticate();\n\
                 }}\n\
                 public void unrelated() {{}}\n\
                 }}\n"
            )
            .replace('\n', newline);
            let scanner = SecurityScanner::new(ScanConfig::default());
            let findings = scanner.scan_source(&source, Path::new("Controller.java"));
            let findings: Vec<_> = findings
                .iter()
                .filter(|finding| finding.vuln_type == VulnType::MissingRateLimiting)
                .collect();
            assert_eq!(findings.len(), 1, "{source}");
            let finding = findings[0];
            let diagnostic = hyperion_security::finding_to_diagnostic(finding, &source, None);
            let prefix = "@SuppressWarnings(\"😀\") public class ";
            let start = u32::try_from(prefix.encode_utf16().count()).unwrap();
            let end = start + u32::try_from(name.encode_utf16().count()).unwrap();
            assert_eq!(
                diagnostic.range,
                lsp_types::Range::new(
                    lsp_types::Position::new(1, start),
                    lsp_types::Position::new(1, end),
                ),
                "{source}"
            );
            assert!(finding.code_snippet.contains("return authenticate();"));
            assert!(finding.code_snippet.contains("public void unrelated()"));
        }
    }
}

#[test]
fn test_health_endpoint_not_flagged() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class HealthController {
    @GetMapping("/health")
    public ResponseEntity<Map<String, String>> health() {
        return ResponseEntity.ok(Map.of("status", "UP"));
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("HealthController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        0,
        "Health check endpoints should not be flagged for missing rate limiting"
    );
}

#[test]
fn test_actuator_endpoint_not_flagged() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class ActuatorController {
    @GetMapping("/actuator/health")
    public ResponseEntity<?> actuatorHealth() {
        return ResponseEntity.ok(Map.of("status", "UP"));
    }

    @GetMapping("/actuator/metrics")
    public ResponseEntity<?> metrics() {
        return ResponseEntity.ok(Map.of());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ActuatorController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        0,
        "Actuator endpoints should not be flagged for missing rate limiting"
    );
}

#[test]
fn test_login_endpoint_critical_severity() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class AuthController {
    @PostMapping("/auth/login")
    public ResponseEntity<?> login(@RequestBody LoginRequest req) {
        return authService.login(req);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AuthController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        1,
        "Login endpoint should be flagged for missing rate limiting"
    );

    let finding = rate_limit_findings[0];
    assert_eq!(
        finding.severity,
        Severity::High,
        "Login endpoint should have HIGH severity"
    );
    assert!(
        finding.confidence >= 90.0,
        "Login endpoint should have high confidence ({})",
        finding.confidence
    );
    assert!(
        finding.message.contains("CRITICAL"),
        "Login endpoint message should indicate CRITICAL priority"
    );
}

#[test]
fn test_password_reset_endpoint_critical() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class PasswordController {
    @PostMapping("/auth/reset-password")
    public ResponseEntity<?> resetPassword(@RequestBody ResetRequest req) {
        return passwordService.reset(req);
    }

    @PostMapping("/auth/forgot-password")
    public ResponseEntity<?> forgotPassword(@RequestBody String email) {
        return passwordService.sendResetEmail(email);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("PasswordController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        1,
        "Password endpoints should be flagged (class-level check)"
    );

    let finding = rate_limit_findings[0];
    assert_eq!(
        finding.severity,
        Severity::High,
        "Password endpoint should have HIGH severity"
    );
    assert!(
        finding.confidence >= 90.0,
        "Password endpoint should have high confidence"
    );
}

#[test]
fn test_payment_endpoint_critical() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class PaymentController {
    @PostMapping("/payment/charge")
    public ResponseEntity<?> charge(@RequestBody ChargeRequest req) {
        return paymentService.charge(req);
    }

    @PostMapping("/checkout")
    public ResponseEntity<?> checkout(@RequestBody CheckoutRequest req) {
        return paymentService.checkout(req);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("PaymentController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        1,
        "Payment endpoints should be flagged (class-level check)"
    );

    let finding = rate_limit_findings[0];
    assert_eq!(
        finding.severity,
        Severity::High,
        "Payment endpoint should have HIGH severity"
    );
}

#[test]
fn test_registration_endpoint_high_priority() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class RegistrationController {
    @PostMapping("/register")
    public ResponseEntity<?> register(@RequestBody RegisterRequest req) {
        return userService.register(req);
    }

    @PostMapping("/signup")
    public ResponseEntity<?> signup(@RequestBody SignupRequest req) {
        return userService.signup(req);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RegistrationController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        1,
        "Registration endpoints should be flagged (class-level check)"
    );

    let finding = rate_limit_findings[0];
    assert_eq!(
        finding.severity,
        Severity::High,
        "Registration endpoint should have HIGH severity"
    );
    assert!(
        finding.confidence >= 80.0,
        "Registration endpoint should have high confidence"
    );
}

#[test]
fn test_internal_endpoint_low_priority() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class InternalController {
    @GetMapping("/internal/stats")
    public ResponseEntity<?> getStats() {
        return ResponseEntity.ok(statsService.getStats());
    }

    @PostMapping("/internal/refresh")
    public ResponseEntity<?> refresh() {
        cacheService.refresh();
        return ResponseEntity.ok().build();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InternalController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        1,
        "the class-level check should emit one consolidated internal-endpoint finding"
    );
    let finding = rate_limit_findings[0];
    assert_eq!(finding.severity, Severity::Low);
    assert!((finding.confidence - 30.0).abs() < f32::EPSILON);
}

#[test]
fn test_get_endpoint_low_priority() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class UserController {
    @GetMapping("/users")
    public ResponseEntity<?> getUsers() {
        return ResponseEntity.ok(userService.findAll());
    }

    @GetMapping("/users/{id}")
    public ResponseEntity<?> getUser(@PathVariable Long id) {
        return userService.findById(id)
            .map(ResponseEntity::ok)
            .orElse(ResponseEntity.notFound().build());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    // GET endpoints should not be flagged or have very low severity
    assert_eq!(
        rate_limit_findings.len(),
        0,
        "GET endpoints should not be flagged for rate limiting"
    );
}

#[test]
fn test_post_endpoint_medium_priority() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class UserController {
    @PostMapping("/users")
    public ResponseEntity<?> createUser(@RequestBody UserRequest req) {
        return ResponseEntity.ok(userService.create(req));
    }

    @PutMapping("/users/{id}")
    public ResponseEntity<?> updateUser(@PathVariable Long id, @RequestBody UserRequest req) {
        return ResponseEntity.ok(userService.update(id, req));
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        1,
        "State-changing endpoints should be flagged"
    );

    let finding = rate_limit_findings[0];
    assert_eq!(
        finding.severity,
        Severity::Medium,
        "State-changing POST endpoint should have MEDIUM severity"
    );
    assert!(
        finding.confidence >= 50.0 && finding.confidence <= 70.0,
        "State-changing endpoint should have medium confidence"
    );
}

#[test]
fn test_metrics_endpoint_not_flagged() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class MetricsController {
    @GetMapping("/metrics")
    public ResponseEntity<?> metrics() {
        return ResponseEntity.ok(metricsService.getAll());
    }

    @GetMapping("/prometheus")
    public ResponseEntity<?> prometheus() {
        return ResponseEntity.ok(prometheusService.export());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MetricsController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        0,
        "Metrics endpoints should not be flagged for missing rate limiting"
    );
}

#[test]
fn test_swagger_endpoint_not_flagged() {
    let source = r#"
import org.springframework.web.bind.annotation.*;

public class SwaggerController {
    @GetMapping("/swagger-ui")
    public ResponseEntity<?> swaggerUI() {
        return ResponseEntity.ok(swaggerService.getUI());
    }

    @GetMapping("/api-docs")
    public ResponseEntity<?> apiDocs() {
        return ResponseEntity.ok(apiService.getDocs());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SwaggerController.java"));

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert_eq!(
        rate_limit_findings.len(),
        0,
        "Swagger/API documentation endpoints should not be flagged"
    );
}
