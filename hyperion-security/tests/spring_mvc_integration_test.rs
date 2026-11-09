// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_login_controller_views() {
    let code = r#"
package com.example.auth;

import org.springframework.stereotype.Controller;
import org.springframework.web.bind.annotation.GetMapping;

@Controller
public class LoginController {

    // View names - should NOT be flagged
    private String loginView = "login/login";
    private String logoutView = "logout/logout";
    private String loginPage = "auth/login.html";
    private String errorPage = "error/403.jsp";

    // Template paths - should NOT be flagged
    private String passwordResetTemplate = "templates/password-reset.ftl";
    private String tokenVerificationUrl = "forward:/verify/token";

    @GetMapping("/login")
    public String showLoginPage() {
        return loginView;
    }
}
    "#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(code, Path::new("LoginController.java"));

    let hardcoded_creds: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .collect();

    // Should find ZERO hardcoded credentials
    assert_eq!(
        hardcoded_creds.len(),
        0,
        "View names and template paths should not be flagged. Found: {:?}",
        hardcoded_creds
            .iter()
            .map(|f| &f.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_mixed_views_and_actual_secrets() {
    let code = r#"
package com.example;

import org.springframework.stereotype.Controller;

@Controller
public class MixedController {

    // View names - should NOT be flagged
    private String accountView = "account/settings";
    private String passwordChangeView = "account/change-password";

    // Actual secrets - SHOULD be flagged
    private String apiKey = "STRIPE_LIVE_1234567890abcdefghijklmnopqrstuvwxyz";
    private String dbPassword = "MyDatabaseP@ssw0rd!";

    // More view names - should NOT be flagged
    private String loginRedirect = "redirect:/login";
}
    "#;
    let code = &stripe_keys(code);

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(code, Path::new("MixedController.java"));

    let hardcoded_creds: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::HardcodedCredentials)
        .collect();

    // Should find exactly 2: apiKey and dbPassword (NOT the view names)
    assert_eq!(
        hardcoded_creds.len(),
        2,
        "Should detect apiKey and dbPassword but not view names. Found: {:?}",
        hardcoded_creds
            .iter()
            .map(|f| &f.code_snippet)
            .collect::<Vec<_>>()
    );

    // Verify findings are for the actual secrets
    let finding_codes: Vec<_> = hardcoded_creds
        .iter()
        .map(|f| f.code_snippet.as_str())
        .collect();

    assert!(
        finding_codes.iter().any(|c| c.contains("apiKey")),
        "Should detect apiKey"
    );
    assert!(
        finding_codes.iter().any(|c| c.contains("dbPassword")),
        "Should detect dbPassword"
    );
}

/// Expands `STRIPE_LIVE_` / `STRIPE_TEST_` placeholders into Stripe key prefixes, so the
/// repository itself never contains key-shaped strings that secret scanners would flag.
fn stripe_keys(source: &str) -> String {
    source
        .replace("STRIPE_LIVE_", concat!("sk", "_live_"))
        .replace("STRIPE_TEST_", concat!("sk", "_test_"))
}
