// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Sensitive data logging detection
//!
//! Detects when sensitive data (passwords, tokens, secrets, SSNs, credit cards)
//! are logged through logger methods.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for logging sensitive data (CWE-532)
///
/// Detects:
/// - Logger method calls (info, debug, warn, error) that log sensitive variables
/// - String literals containing sensitive patterns
/// - Formatted string logging with sensitive placeholders
pub struct SensitiveDataLoggingCheck;

impl SensitiveDataLoggingCheck {
    /// Sensitive variable name patterns (case-insensitive)
    const SENSITIVE_PATTERNS: &'static [&'static str] = &[
        "password",
        "passwd",
        "pwd",
        "secret",
        "token",
        "apikey",
        "api_key",
        "privatekey",
        "private_key",
        "accesskey",
        "access_key",
        "secretkey",
        "secret_key",
        "ssn",
        "socialsecurity",
        "creditcard",
        "cardnumber",
        "cvv",
        "pin",
        "authorization",
        "bearer",
        "oauth",
        "sessionid",
        "session_id",
    ];

    /// Logger method names that output to logs
    const LOGGER_METHODS: &'static [&'static str] = &[
        "info", "debug", "warn", "error", "trace", "log", "println", // System.out.println
        "print",   // System.out.print
    ];

    /// Check if a variable name appears to contain sensitive data
    fn is_sensitive_variable(name: &str) -> bool {
        let lower = name.to_lowercase();

        Self::SENSITIVE_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Extract variable names from logging arguments
    ///
    /// Returns a list of variable names found in the arguments.
    /// Skips string literals - only extracts NAME_EXPR nodes (variable references).
    fn extract_variable_names(arg_list: &SyntaxNode) -> Vec<String> {
        let mut variables = Vec::new();

        for arg in arg_list.descendants() {
            if arg.kind() == SyntaxKind::NAME_EXPR {
                // Extract identifier from NAME_EXPR
                if let Some(ident) = arg
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|t| t.kind() == SyntaxKind::IDENT)
                {
                    variables.push(ident.text().to_string());
                }
            }
        }

        variables
    }

    /// Extract the format string (first argument) from logging call
    fn extract_format_string(arg_list: &SyntaxNode) -> Option<String> {
        // First argument is usually the format string
        for arg in arg_list.descendants() {
            if arg.kind() == SyntaxKind::STRING_LITERAL {
                return Some(arg.text().to_string());
            }
        }
        None
    }

    /// Check if actually logging sensitive data (not just mentioning it)
    ///
    /// Filters out common false positives:
    /// 1. Variables ending with Id, Type, Event, Flag, Status (not sensitive data)
    /// 2. Format strings that just mention sensitive words without logging variables
    /// 3. Hardcoded messages with no variables
    fn is_actually_logging_sensitive_data(_format_string: &str, logged_vars: &[String]) -> bool {
        // If no variables are logged, it's just a hardcoded message
        if logged_vars.is_empty() {
            return false;
        }

        // Check each logged variable
        for var in logged_vars {
            let lower = var.to_lowercase();

            // Safe patterns: IDs, flags, types, events, names, counts
            if lower.ends_with("id")
                || lower.ends_with("type")
                || lower.ends_with("event")
                || lower.ends_with("flag")
                || lower.ends_with("status")
                || lower.ends_with("name")
                || lower.ends_with("count")
            {
                continue; // Not sensitive
            }

            // Check for actual sensitive data variable names (exact matches)
            if lower == "password"
                || lower == "secret"
                || lower == "token"
                || lower == "apikey"
                || lower == "accesstoken"
                || lower == "refreshtoken"
                || lower == "privatekey"
                || lower == "credential"
                || lower == "ssn"
                || lower == "creditcard"
                || lower == "cvv"
                || lower == "pin"
            {
                return true; // TRUE POSITIVE
            }

            // Check for sensitive patterns with common suffixes (creditCardNumber, ssnValue, etc.)
            if lower.contains("creditcard")
                || lower.contains("cardnumber")
                || lower.contains("socialsecurity")
                || lower.contains("bearer")
            {
                return true; // TRUE POSITIVE
            }

            // Check if ends with sensitive suffix (excluding safe patterns above)
            if (lower.ends_with("password")
                || lower.ends_with("secret")
                || lower.ends_with("token")
                || lower.ends_with("key"))
                && !lower.ends_with("id")
            // Not tokenId, keyId, etc.
            {
                return true;
            }
        }

        false
    }

    /// Find the argument list node within a method call
    fn find_arg_list(node: &SyntaxNode) -> Option<SyntaxNode> {
        node.descendants()
            .find(|descendant| descendant.kind() == SyntaxKind::ARG_LIST)
    }

    /// Extract the logger method name (debug, info, warn, error, trace)
    fn extract_log_method(node: &SyntaxNode) -> String {
        // Get the text and search for the log method pattern
        let text = node.text().to_string();

        // Try to match logger.METHOD( or System.out.METHOD(
        for method in ["debug", "info", "warn", "error", "trace", "log"] {
            if text.contains(&format!(".{method}(")) || text.contains(&format!(".{method} (")) {
                return method.to_string();
            }
        }

        // Check for println/print
        if text.contains(".println(") || text.contains(".println (") {
            return "info".to_string(); // Treat println as info level
        }
        if text.contains(".print(") || text.contains(".print (") {
            return "info".to_string();
        }

        "info".to_string() // Default
    }

    /// Calculate confidence that this is a real security issue based on context
    ///
    /// Factors:
    /// 1. Log level (DEBUG/TRACE in production code are concerning, but in security frameworks are expected)
    /// 2. Security framework context (security classes logging tokens for debugging)
    /// 3. Variable type (passwords/credit cards/secrets always high confidence)
    fn calculate_logging_confidence(log_method: &str, class_name: &str, var_name: &str) -> f32 {
        let class_lower = class_name.to_lowercase();
        let var_lower = var_name.to_lowercase();

        // Factor 1: Actual passwords should ALWAYS be high confidence (check first)
        if var_lower == "password" || var_lower == "passwd" || var_lower == "pwd" {
            return 1.0;
        }

        // Factor 2: Credit cards, SSNs should ALWAYS be high confidence
        if var_lower.contains("creditcard")
            || var_lower.contains("ssn")
            || var_lower.contains("socialsecurity")
            || var_lower.contains("cvv")
        {
            return 1.0;
        }

        // Factor 3: Actual secrets (not just tokens) should be HIGH confidence
        // apiSecret, clientSecret, etc. are real secrets, not framework debugging
        if var_lower.contains("secret") && !var_lower.ends_with("status") {
            return 1.0;
        }

        // Factor 4: Bearer tokens are always concerning
        if var_lower.contains("bearer") {
            return 1.0;
        }

        let mut confidence = 1.0;

        // Factor 5: Log level - apply discount primarily for tokens in security framework context
        // For regular application code, DEBUG/TRACE of tokens is still concerning
        let is_security_framework = class_lower.contains("csrf")
            || class_lower.contains("oauth")
            || class_lower.contains("auth")
            || class_lower.contains("authorizer")
            || class_lower.contains("authenticator");

        if is_security_framework {
            // In security frameworks, DEBUG/TRACE logging is expected
            match log_method {
                "debug" | "trace" => confidence *= 0.3,
                "info" => confidence *= 0.6,
                "warn" | "error" => confidence *= 1.0,
                _ => {}
            }
        } else {
            // In regular application code at DEBUG/TRACE, generic "token" is likely not sensitive
            // But specific tokens (apiToken, accessToken, etc.) are concerning
            let is_generic_token = var_lower == "token";

            match log_method {
                "debug" | "trace" => {
                    if is_generic_token {
                        confidence *= 0.3; // Filter generic token at DEBUG
                    } else {
                        confidence *= 0.7; // Still flag specific tokens like apiToken
                    }
                }
                "info" => confidence *= 0.8,
                "warn" | "error" => confidence *= 1.0,
                _ => {}
            }
        }

        // Factor 5: Specific security framework patterns - further reduce confidence
        // CSRF tokens in CSRF-related classes
        if class_lower.contains("csrf") && var_lower.contains("csrf") {
            confidence *= 0.2;
        }

        // OAuth/Auth tokens in OAuth/Auth classes (at debug level)
        if (class_lower.contains("oauth") || class_lower.contains("auth"))
            && (var_lower.contains("token") || var_lower.contains("oauth"))
            && (log_method == "debug" || log_method == "trace")
        {
            confidence *= 0.3;
        }

        confidence
    }
}

impl Check for SensitiveDataLoggingCheck {
    fn name(&self) -> &'static str {
        "sensitive-data-logging"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        // Target only the call node itself.
        //
        // Logger calls typically appear both as a `METHOD_CALL_EXPR` and wrapped in an
        // `EXPR_STMT` (expression statement). Targeting both causes duplicate findings.
        &[SyntaxKind::METHOD_CALL_EXPR]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        if node.kind() != SyntaxKind::METHOD_CALL_EXPR {
            return vec![];
        }

        // Skip demo/test/seeder contexts - these often log test credentials intentionally
        let class_name = crate::context::get_containing_class_name(node).unwrap_or_default();
        let method_name = crate::context::get_containing_method_name(node);

        if crate::context::is_demo_or_test_context(
            context.file,
            &class_name,
            method_name.as_deref(),
        ) {
            // Don't report for demo/test code - logging test credentials is often intentional
            return vec![];
        }

        let text = node.text().to_string();

        // Find logger method call
        let logger_method = Self::LOGGER_METHODS.iter().find(|&method| {
            text.contains(&format!(".{method}(")) || text.contains(&format!(".{method} ("))
        });

        if logger_method.is_none() {
            return vec![];
        }

        // Find the argument list node
        let Some(arg_list) = Self::find_arg_list(node) else {
            return vec![];
        };

        // Extract variable names from arguments
        let variable_names = Self::extract_variable_names(&arg_list);

        // Extract format string to check context
        let format_string = Self::extract_format_string(&arg_list).unwrap_or_default();

        // Apply context-aware filtering
        if !Self::is_actually_logging_sensitive_data(&format_string, &variable_names) {
            return vec![];
        }

        // Check if any variable name is sensitive (for the finding message)
        let sensitive_vars: Vec<String> = variable_names
            .iter()
            .filter(|var| Self::is_sensitive_variable(var))
            .cloned()
            .collect();

        if sensitive_vars.is_empty() {
            return vec![];
        }

        // Extract log method for context-aware confidence scoring
        let log_method = Self::extract_log_method(node);

        // Calculate confidence for each sensitive variable (use highest)
        let mut max_confidence = 0.0_f32;
        for var in &sensitive_vars {
            let conf = Self::calculate_logging_confidence(&log_method, &class_name, var);
            if conf > max_confidence {
                max_confidence = conf;
            }
        }

        // Filter out very low confidence findings (likely false positives)
        if max_confidence < 0.4 {
            return vec![];
        }

        // Create finding with context-aware severity
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);

        let message = format!(
            "Sensitive data logged via variables: {}. Logging passwords, tokens, or secrets can expose them in log files, \
             log aggregation systems, and to unauthorized personnel.",
            sensitive_vars.join(", ")
        );

        let mut finding = Finding::new(VulnType::SensitiveDataLogging, message, location, code);

        // Set confidence score
        finding.confidence = max_confidence;

        // Adjust severity based on confidence
        finding.severity = if max_confidence > 0.8 {
            Severity::High
        } else if max_confidence > 0.5 {
            Severity::Medium
        } else {
            Severity::Low
        };

        finding.remediation = "Remove sensitive data from logs or use masking/redaction. \
            For debugging, log only non-sensitive identifiers or use structured logging with field filtering."
            .to_string();

        vec![finding]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sensitive_variable() {
        assert!(SensitiveDataLoggingCheck::is_sensitive_variable("password"));
        assert!(SensitiveDataLoggingCheck::is_sensitive_variable(
            "userPassword"
        ));
        assert!(SensitiveDataLoggingCheck::is_sensitive_variable("apiKey"));
        assert!(SensitiveDataLoggingCheck::is_sensitive_variable(
            "secretToken"
        ));
        assert!(SensitiveDataLoggingCheck::is_sensitive_variable(
            "creditCardNumber"
        ));

        assert!(!SensitiveDataLoggingCheck::is_sensitive_variable(
            "username"
        ));
        assert!(!SensitiveDataLoggingCheck::is_sensitive_variable("userId"));
        assert!(!SensitiveDataLoggingCheck::is_sensitive_variable("count"));
    }

    #[test]
    fn test_is_actually_logging_sensitive_data_true_positives() {
        // TRUE POSITIVE: Actually logging password
        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "User logged in with password: {}",
                &["password".to_string()]
            )
        );

        // TRUE POSITIVE: Logging API key value
        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Generated API key: {}",
                &["apiKey".to_string()]
            )
        );

        // TRUE POSITIVE: Logging token
        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Access token: {}",
                &["accessToken".to_string()]
            )
        );

        // TRUE POSITIVE: Logging user password variable
        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "User data: {}",
                &["userPassword".to_string()]
            )
        );

        // TRUE POSITIVE: Logging secret
        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Secret value: {}",
                &["secret".to_string()]
            )
        );
    }

    #[test]
    fn test_is_actually_logging_sensitive_data_false_positives() {
        // FALSE POSITIVE: Logging about password, not password itself
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Password reset email sent to user: {}",
                &["userId".to_string()]
            )
        );

        // FALSE POSITIVE: API key reference (ID)
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Validating API key for merchant: {}",
                &["merchantId".to_string()]
            )
        );

        // FALSE POSITIVE: Token type (not token value)
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Processing token type: {}",
                &["tokenType".to_string()]
            )
        );

        // FALSE POSITIVE: Hardcoded message (no variables)
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Password must be at least 8 characters",
                &[]
            )
        );

        // FALSE POSITIVE: Password ID reference
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Password reset requested for: {}",
                &["passwordId".to_string()]
            )
        );

        // FALSE POSITIVE: Token event type
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Token event occurred: {}",
                &["tokenEvent".to_string()]
            )
        );

        // FALSE POSITIVE: API key flag
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "API key validation flag: {}",
                &["apiKeyFlag".to_string()]
            )
        );

        // FALSE POSITIVE: Username (not sensitive)
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "User logged in: {}",
                &["username".to_string()]
            )
        );
    }

    #[test]
    fn test_safe_suffix_patterns() {
        // Variables ending with safe suffixes should not trigger
        let safe_vars = vec![
            "userId".to_string(),
            "tokenType".to_string(),
            "passwordEvent".to_string(),
            "apiKeyFlag".to_string(),
            "secretStatus".to_string(),
            "tokenName".to_string(),
            "passwordCount".to_string(),
        ];

        for var in safe_vars {
            assert!(
                !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                    "Log message: {}",
                    std::slice::from_ref(&var)
                ),
                "Variable '{}' should not trigger as sensitive",
                var
            );
        }
    }

    #[test]
    fn test_exact_sensitive_matches() {
        // Exact matches should trigger
        let sensitive_exact = vec![
            "password",
            "secret",
            "token",
            "apikey",
            "accesstoken",
            "refreshtoken",
            "privatekey",
            "credential",
            "ssn",
            "creditcard",
            "cvv",
            "pin",
        ];

        for var in sensitive_exact {
            assert!(
                SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                    "Value: {}",
                    &[var.to_string()]
                ),
                "Variable '{}' should trigger as sensitive",
                var
            );
        }
    }

    #[test]
    fn test_sensitive_suffix_patterns() {
        // Variables ending with sensitive suffixes (but not safe suffixes) should trigger
        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Value: {}",
                &["userPassword".to_string()]
            )
        );

        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Value: {}",
                &["appSecret".to_string()]
            )
        );

        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Value: {}",
                &["authToken".to_string()]
            )
        );

        assert!(
            SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Value: {}",
                &["privateKey".to_string()]
            )
        );

        // But not if they end with "Id"
        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Value: {}",
                &["tokenId".to_string()]
            )
        );

        assert!(
            !SensitiveDataLoggingCheck::is_actually_logging_sensitive_data(
                "Value: {}",
                &["keyId".to_string()]
            )
        );
    }
}
