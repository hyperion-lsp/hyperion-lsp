// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Spring Security misconfigurations and vulnerabilities

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for CSRF protection disabled
pub struct CsrfDisabledCheck;

impl Check for CsrfDisabledCheck {
    fn name(&self) -> &'static str {
        "csrf-disabled"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::METHOD_DECL,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Check if CSRF is disabled
        let csrf_disabled = (node_text.contains("csrf()") && node_text.contains("disable()"))
            || (node_text.contains("csrf(")
                && node_text.contains("->")
                && node_text.contains("disable()"))
            || (node_text.contains("csrf(") && node_text.contains("::disable"));

        if !csrf_disabled {
            return vec![];
        }

        // Find the containing method or class to get full context
        let context_text = Self::get_security_config_context(node);

        // Check if this is a stateless API where CSRF is not applicable
        if !Self::csrf_protection_needed(&context_text) {
            // Stateless JWT/API key authentication - CSRF not applicable
            return vec![];
        }

        // CSRF disabled in a session-based or cookie-based application
        vec![Self::create_finding(node, context)]
    }
}

impl CsrfDisabledCheck {
    /// Get the full security configuration context (method or class)
    ///
    /// We need to see the full config to determine if it's stateless
    fn get_security_config_context(node: &SyntaxNode) -> String {
        // Walk up to find the containing method or class
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                SyntaxKind::METHOD_DECL | SyntaxKind::CLASS_DECL => {
                    return parent.text().to_string();
                }
                _ => current = parent.parent(),
            }
        }

        // Fallback to node text if we can't find parent
        node.text().to_string()
    }

    /// Check if the security config uses stateless session management
    fn has_stateless_session_management(text: &str) -> bool {
        text.contains("SessionCreationPolicy.STATELESS")
            || text.contains("sessionCreationPolicy(SessionCreationPolicy.STATELESS)")
            || text.contains("sessionCreationPolicy(STATELESS)")
    }

    /// Check if the config uses JWT authentication
    fn has_jwt_authentication(text: &str) -> bool {
        text.contains("JwtAuthenticationFilter")
            || text.contains("jwtFilter")
            || text.contains("JwtTokenProvider")
            || text.contains("JwtDecoder")
            || text.contains("addFilterBefore(jwt")
            || text.contains("BearerTokenAuthenticationFilter")
            || text.contains("oauth2ResourceServer")
    }

    /// Check if the config uses API key authentication
    fn has_api_key_authentication(text: &str) -> bool {
        text.contains("ApiKeyAuthenticationFilter")
            || text.contains("apiKeyFilter")
            || text.contains("X-API-Key")
            || text.contains("Api-Key")
    }

    /// Check if the config uses session/cookie authentication
    fn has_cookie_authentication(text: &str) -> bool {
        // Don't flag UsernamePasswordAuthenticationFilter if it's just used as a reference point
        // in addFilterBefore (common in JWT configs)
        let has_form_login = text.contains("formLogin(")
            || text.contains("loginPage(")
            || text.contains("rememberMe(");

        let has_cookie_csrf = text.contains("CookieCsrfTokenRepository");

        // Only flag UsernamePasswordAuthenticationFilter if not used as a filter chain reference
        let has_username_password_auth = text.contains("UsernamePasswordAuthenticationFilter")
            && !text.contains("addFilterBefore(")
            && !text.contains("addFilterAfter(");

        has_form_login || has_cookie_csrf || has_username_password_auth
    }

    /// Determine if CSRF protection is actually needed
    fn csrf_protection_needed(text: &str) -> bool {
        // CSRF is NOT needed if:
        // 1. Using stateless session management AND
        // 2. Using token-based auth (JWT or API key) AND
        // 3. NOT using cookie-based auth

        let is_stateless = Self::has_stateless_session_management(text);
        let has_token_auth =
            Self::has_jwt_authentication(text) || Self::has_api_key_authentication(text);
        let has_cookie_auth = Self::has_cookie_authentication(text);

        // CSRF needed if using cookies OR if not truly stateless
        has_cookie_auth || !is_stateless || !has_token_auth
    }

    fn create_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "CSRF protection is disabled. This exposes the application to Cross-Site Request Forgery attacks where attackers can perform unauthorized actions on behalf of authenticated users. Only disable for stateless APIs with token-based authentication.".to_string();

        let mut finding = Finding::new(VulnType::CsrfDisabled, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Remove .csrf().disable() to enable CSRF protection, or ensure your API is truly stateless and uses secure token authentication (JWT or API keys, not cookies).".to_string();
        finding
    }
}

/// Check for permitAll() on sensitive paths
pub struct PermitAllCheck;

impl Check for PermitAllCheck {
    fn name(&self) -> &'static str {
        "permit-all-sensitive"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::METHOD_DECL,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Must contain permitAll()
        if !node_text.contains("permitAll()") {
            return vec![];
        }

        // Check for sensitive path patterns
        let sensitive_patterns = [
            "/admin",
            "/internal",
            "/api/private",
            "/management",
            "/actuator",
            "/console",
            "/config",
            "/system",
        ];

        for pattern in &sensitive_patterns {
            if node_text.contains(pattern) {
                return vec![Self::create_finding(node, context, pattern)];
            }
        }

        vec![]
    }
}

impl PermitAllCheck {
    fn create_finding(node: &SyntaxNode, context: &CheckContext, path: &str) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "permitAll() allows unauthenticated access to sensitive path '{path}'. This could expose administrative or internal functionality to unauthorized users."
        );

        let mut finding = Finding::new(VulnType::InsecurePermitAll, message, location, code);
        finding.severity = Severity::High;
        finding.remediation =
            format!("Replace permitAll() with authenticated() or hasRole() for path '{path}'");
        finding
    }
}

/// Check for weak password encoders
pub struct WeakPasswordEncoderCheck;

impl Check for WeakPasswordEncoderCheck {
    fn name(&self) -> &'static str {
        "weak-password-encoder"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::NEW_EXPR,
            SyntaxKind::METHOD_DECL,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // NoOpPasswordEncoder - stores passwords in plaintext
        if node_text.contains("NoOpPasswordEncoder") {
            return vec![Self::create_finding(
                node,
                context,
                "NoOpPasswordEncoder stores passwords in plaintext with no hashing",
            )];
        }

        // StandardPasswordEncoder - uses SHA-256, which is too fast
        if node_text.contains("StandardPasswordEncoder") {
            return vec![Self::create_finding(
                node,
                context,
                "StandardPasswordEncoder uses SHA-256, which is too fast for password hashing and vulnerable to rainbow table attacks",
            )];
        }

        // MessageDigestPasswordEncoder with MD5
        if node_text.contains("MessageDigestPasswordEncoder") && node_text.contains("\"MD5\"") {
            return vec![Self::create_finding(
                node,
                context,
                "MessageDigestPasswordEncoder with MD5 is cryptographically broken",
            )];
        }

        // Pbkdf2PasswordEncoder with low iterations (less than 100000)
        if node_text.contains("Pbkdf2PasswordEncoder") {
            let marker = "Pbkdf2PasswordEncoder(";
            if let Some(start) = node_text.find(marker) {
                if let Some(iterations) =
                    pbkdf2_iteration_argument(&node_text[start + marker.len()..])
                        .and_then(|argument| argument.replace('_', "").parse::<u32>().ok())
                {
                    if iterations < 100_000 && iterations > 0 {
                        return vec![Self::create_finding(
                            node,
                            context,
                            &format!(
                                "Pbkdf2PasswordEncoder configured with only {iterations} iterations (minimum 100,000 recommended)"
                            ),
                        )];
                    }
                }
            }
        }

        vec![]
    }
}

impl WeakPasswordEncoderCheck {
    fn create_finding(node: &SyntaxNode, context: &CheckContext, reason: &str) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Weak password encoder detected: {reason}. This makes user passwords vulnerable to offline attacks."
        );

        let mut finding = Finding::new(VulnType::WeakPasswordEncoding, message, location, code);
        finding.severity = Severity::High;
        finding.remediation =
            "Use BCryptPasswordEncoder or Argon2PasswordEncoder for secure password hashing"
                .to_string();
        finding
    }
}

/// The iteration-count argument of a `Pbkdf2PasswordEncoder` constructor call, given the
/// text after its opening parenthesis. Spring Security has two constructor shapes:
/// `(secret, iterations, hashWidth)` in 5.x, and
/// `(secret, saltLength, iterations, hashWidth | SecretKeyFactoryAlgorithm)` in 5.8 and 6.x.
fn pbkdf2_iteration_argument(after_open: &str) -> Option<String> {
    let arguments = top_level_arguments(after_open)?;
    let index = match arguments.len() {
        3 => 1,
        4 => 2,
        _ => return None,
    };
    Some(arguments[index].clone())
}

/// The top-level arguments of a call, given the text after its opening parenthesis.
/// Commas and brackets inside string and char literals do not count.
/// Returns `None` when the closing parenthesis is missing.
fn top_level_arguments(after_open: &str) -> Option<Vec<String>> {
    let mut depth = 0_i32;
    let mut argument_start = 0;
    let mut arguments = Vec::new();
    let mut open_quote: Option<char> = None;
    let mut escaped = false;
    for (index, character) in after_open.char_indices() {
        if let Some(quote) = open_quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                open_quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => open_quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => {
                let last = after_open[argument_start..index].trim();
                if !last.is_empty() || !arguments.is_empty() {
                    arguments.push(last.to_string());
                }
                return Some(arguments);
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                arguments.push(after_open[argument_start..index].trim().to_string());
                argument_start = index + 1;
            }
            _ => {}
        }
    }
    None
}
