// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Authentication and authorization security checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

/// Check for empty password authentication bypass.
///
/// This check distinguishes between:
/// - **True positives**: Code that ACCEPTS authentication when password is empty
///   - `if (password.isEmpty()) { return true; }` - VULNERABLE
///   - `if (pwd.equals("")) { authenticated = true; }` - VULNERABLE
///
/// - **False positives to avoid**:
///   - Checking if credential STORE is empty: `if (credentialProperties.isEmpty()) { ... }`
///   - REJECTING empty passwords: `if (password.isEmpty()) { throw new AuthException(); }`
///   - REJECTING empty passwords: `if (password.isEmpty()) { return false; }`
pub struct EmptyPasswordCheck;

impl EmptyPasswordCheck {
    /// Password-related variable name patterns (lowercase for comparison)
    const PASSWORD_PATTERNS: &'static [&'static str] = &["password", "passwd", "pwd", "passphrase"];

    /// Statements that REJECT authentication (safe). These are control-flow forms
    /// only. Bare words such as "error" or "invalid" are not used because they also
    /// match log messages and string literals in branches that go on to grant access.
    const REJECTION_PATTERNS: &'static [&'static str] = &[
        "throw ",
        "throw(",
        "return false",
        "return null",
        "authenticated = false",
        "setAuthenticated(false)",
    ];

    /// Patterns indicating the code ACCEPTS authentication (potentially vulnerable)
    const ACCEPTANCE_PATTERNS: &'static [&'static str] = &[
        "return true",
        "authenticated = true",
        "setAuthenticated(true)",
        "grant",
    ];

    /// Variable name patterns that indicate a container/store, not a password value
    const CONTAINER_PATTERNS: &'static [&'static str] = &[
        "properties",
        "credentials",
        "store",
        "map",
        "list",
        "set",
        "collection",
        "cache",
        "config",
        "settings",
    ];

    /// Extract the receiver of a method call from a condition string.
    /// For example, from "password.isEmpty()" returns Some("password").
    fn extract_method_receiver(condition: &str, method: &str) -> Option<String> {
        // Find the method call pattern
        let method_pattern = format!(".{method}(");
        let method_pos = condition.find(&method_pattern)?;

        // Work backwards from the method call to find the receiver
        let before_method = &condition[..method_pos];

        // The receiver is the last "word" before the dot
        // Handle cases like: password.isEmpty(), user.getPassword().isEmpty()
        let mut receiver = String::new();
        let mut paren_depth = 0;

        for ch in before_method.chars().rev() {
            match ch {
                ')' => {
                    paren_depth += 1;
                    receiver.insert(0, ch);
                }
                '(' => {
                    if paren_depth > 0 {
                        paren_depth -= 1;
                        receiver.insert(0, ch);
                    } else {
                        break;
                    }
                }
                ' ' | '\t' | '\n' | '!' | '&' | '|' | '=' if paren_depth == 0 => {
                    break;
                }
                _ => {
                    receiver.insert(0, ch);
                }
            }
        }

        if receiver.is_empty() {
            None
        } else {
            Some(receiver)
        }
    }

    /// Check if a variable name looks like a password variable
    fn is_password_variable(name: &str) -> bool {
        let lower = name.to_lowercase();

        // Check if the name contains password-related patterns
        let has_password_pattern = Self::PASSWORD_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern));

        if !has_password_pattern {
            return false;
        }

        // Check if it's actually a container variable (false positive)
        // e.g., "credentialProperties", "passwordStore", "passwordMap"
        let is_container = Self::CONTAINER_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern));

        !is_container
    }

    /// Extract the condition part from an if statement
    fn extract_condition(if_text: &str) -> Option<&str> {
        // Find "if" followed by "("
        let if_pos = if_text.find("if")?;
        let after_if = &if_text[if_pos + 2..];
        let open_paren = after_if.find('(')?;
        let cond_start = if_pos + 2 + open_paren + 1;

        // Find matching close paren
        let mut paren_depth = 1;
        let mut cond_end = cond_start;

        for (i, ch) in if_text[cond_start..].char_indices() {
            match ch {
                '(' => paren_depth += 1,
                ')' => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        cond_end = cond_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if cond_end > cond_start {
            Some(&if_text[cond_start..cond_end])
        } else {
            None
        }
    }

    /// Extract the then-block from an if statement
    fn extract_then_block(if_text: &str) -> Option<&str> {
        // Find the condition's closing paren, then find the block after it
        let condition = Self::extract_condition(if_text)?;
        let cond_end = if_text.find(condition)? + condition.len();

        // Skip whitespace and find opening brace
        let after_cond = &if_text[cond_end..];
        let brace_pos = after_cond.find('{')?;
        let block_start = cond_end + brace_pos + 1;

        // Find matching close brace
        let mut brace_depth = 1;
        let mut block_end = block_start;

        for (i, ch) in if_text[block_start..].char_indices() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        block_end = block_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if block_end > block_start {
            Some(&if_text[block_start..block_end])
        } else {
            None
        }
    }

    /// Check if the then-block contains rejection patterns (making this safe)
    fn is_rejection_block(block: &str) -> bool {
        let lower = block.to_lowercase();
        Self::REJECTION_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Check if the then-block allows authentication (potentially vulnerable)
    fn is_acceptance_block(block: &str) -> bool {
        let lower = block.to_lowercase();
        Self::ACCEPTANCE_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Find empty check patterns and their receivers in a condition.
    /// Returns a list of (receiver, pattern) tuples.
    fn find_empty_checks(condition: &str) -> Vec<(String, &'static str)> {
        let mut results = Vec::new();

        // Check for .isEmpty() - no argument needed
        if let Some(receiver) = Self::extract_method_receiver(condition, "isEmpty") {
            results.push((receiver, ".isEmpty()"));
        }

        // Check for .equals("") - MUST verify the argument is empty string
        if let Some(equals_receiver) = Self::extract_equals_empty_receiver(condition) {
            results.push((equals_receiver, ".equals(\"\")"));
        }

        // Check for .length() == 0
        if condition.contains(".length() == 0") {
            if let Some(receiver) = Self::extract_method_receiver(condition, "length") {
                results.push((receiver, ".length() == 0"));
            }
        }

        // Also check for == "" pattern
        if condition.contains("== \"\"") {
            // Try to extract what's being compared
            if let Some(eq_pos) = condition.find("== \"\"") {
                let before_eq = condition[..eq_pos].trim();
                // Get the last word/expression
                if let Some(last_word) = before_eq.split_whitespace().last() {
                    results.push((last_word.to_string(), "== \"\""));
                }
            }
        }

        results
    }

    /// Extract the receiver of .equals("") call, but ONLY if the argument is actually ""
    /// Returns None if .equals() is called with a non-empty argument.
    fn extract_equals_empty_receiver(condition: &str) -> Option<String> {
        // Look for .equals("") pattern specifically
        let equals_empty_pattern = ".equals(\"\")";
        let pos = condition.find(equals_empty_pattern)?;

        // Work backwards from the .equals to find the receiver
        let before_equals = &condition[..pos];

        let mut receiver = String::new();
        let mut paren_depth = 0;

        for ch in before_equals.chars().rev() {
            match ch {
                ')' => {
                    paren_depth += 1;
                    receiver.insert(0, ch);
                }
                '(' => {
                    if paren_depth > 0 {
                        paren_depth -= 1;
                        receiver.insert(0, ch);
                    } else {
                        break;
                    }
                }
                ' ' | '\t' | '\n' | '!' | '&' | '|' | '=' if paren_depth == 0 => {
                    break;
                }
                _ => {
                    receiver.insert(0, ch);
                }
            }
        }

        if receiver.is_empty() {
            None
        } else {
            Some(receiver)
        }
    }
}

impl Check for EmptyPasswordCheck {
    fn name(&self) -> &'static str {
        "empty-password"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::IF_STMT]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Extract the condition from the if statement
        let condition = match Self::extract_condition(&node_text) {
            Some(c) => c,
            None => return vec![],
        };

        // Find all empty checks in the condition
        let empty_checks = Self::find_empty_checks(condition);

        if empty_checks.is_empty() {
            return vec![];
        }

        // Check if any of the empty checks are on password variables
        let password_empty_check = empty_checks
            .iter()
            .find(|(receiver, _)| Self::is_password_variable(receiver));

        let Some((receiver, _pattern)) = password_empty_check else {
            return vec![];
        };

        // Extract and analyze the then-block
        let then_block = match Self::extract_then_block(&node_text) {
            Some(b) => b,
            None => return vec![],
        };

        // If the block REJECTS authentication, this is safe code (not a vulnerability)
        if Self::is_rejection_block(then_block) {
            return vec![];
        }

        // Check if the block ACCEPTS authentication (vulnerability)
        if !Self::is_acceptance_block(then_block) {
            return vec![];
        }

        // We found a real vulnerability: empty password check leads to authentication acceptance
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Empty password check allows authentication bypass. Checking `{receiver}.isEmpty()` \
             and then granting access creates a critical security vulnerability where users can \
             authenticate with an empty password."
        );

        let mut finding = Finding::new(VulnType::EmptyPasswordCheck, message, location, code);
        finding.severity = Severity::Critical;
        finding.remediation = "Require non-empty passwords and validate properly. \
            Never allow authentication with empty passwords. \
            The check should reject (throw exception or return false) when password is empty."
            .to_string();

        vec![finding]
    }
}

/// Check for hardcoded credentials
pub struct HardcodedCredentialsCheck;

impl Check for HardcodedCredentialsCheck {
    fn name(&self) -> &'static str {
        "hardcoded-credentials"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::LOCAL_VAR_DECL, SyntaxKind::FIELD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();
        let lower_text = node_text.to_lowercase();

        // Check for security-related variable names
        let security_vars = [
            "password",
            "passwd",
            "pwd",
            "secret",
            "token",
            "key", // Added to catch accessKey, secretKey, etc.
            "apikey",
            "api_key",
            "private_key",
            "privatekey",
            "credential",
            "auth",
        ];

        let has_security_var = security_vars.iter().any(|&var| lower_text.contains(var));

        if !has_security_var {
            return vec![];
        }

        // Skip interface constants (typically semantic identifiers for APIs)
        if Self::is_interface_constant(node) {
            return vec![];
        }

        // Check if this is a placeholder first
        if Self::is_placeholder(&node_text) {
            return vec![];
        }

        // Check if the variable name indicates this is a descriptor, not an actual credential
        // e.g., PASSWORD_KEY, AUTH_HEADER, KEY_DISPLAY
        if Self::is_descriptor_variable(&node_text) {
            return vec![];
        }

        // Check if this looks like an error message, documentation, or config key
        if Self::is_message_or_doc(&node_text)
            || Self::is_message_variable(&node_text)
            || Self::is_config_key(&node_text)
            || Self::is_known_safe_value(&node_text)
        {
            return vec![];
        }

        // Extract variable name and value for semantic identifier check
        let var_name = Self::extract_variable_name(&node_text);
        let string_value = Self::extract_string_value(&node_text);

        // Skip semantic identifiers (audit events, action names, etc.)
        if let Some(value) = &string_value {
            if Self::is_semantic_identifier(&var_name, value) {
                return vec![];
            }
        }

        // Check for string concatenation patterns (often toString() or logging)
        if Self::is_string_concatenation(&node_text) {
            return vec![];
        }

        // Extract the string literal value for entropy analysis
        let string_value = Self::extract_string_value(&node_text);

        // Extract variable name for context-aware filtering
        let var_name = Self::extract_variable_name(&node_text);

        // FILTER OUT property keys, view names, self-referential constants, and internal keys before entropy analysis
        let is_secret = string_value.is_some_and(|value| {
            // Skip property keys/configuration paths
            if Self::is_property_key(&value) {
                false
            // Skip Spring MVC view names
            } else if Self::is_view_name_variable(&var_name, &value) {
                false
            // Skip self-referential constants (audit identifiers, action names, etc.)
            } else if Self::is_self_referential_constant(&var_name, &value) {
                false
            // Skip internal key constants
            } else if Self::is_internal_key_constant(&var_name, &value) {
                false
            } else {
                Self::is_likely_secret(&value)
            }
        });

        if is_secret {
            let location = context.node_location(node);
            let code = context.get_code_snippet(node);
            let message =
                "Hardcoded credential detected. Credentials should never be stored in source code as they can be easily exposed. Use environment variables or a secrets manager instead."
                    .to_string();

            let mut finding = Finding::new(VulnType::HardcodedCredentials, message, location, code);
            finding.severity = Severity::High;
            finding.remediation = "Use environment variables (System.getenv()) or a secrets manager (AWS Secrets Manager, HashiCorp Vault, etc.)".to_string();
            return vec![finding];
        }

        vec![]
    }
}

impl HardcodedCredentialsCheck {
    /// Calculate Shannon entropy of a string
    /// Returns a value typically between 0 (no entropy) and ~5 (high entropy for typical strings)
    /// Higher entropy suggests random/generated content rather than human-readable text
    fn calculate_entropy(s: &str) -> f64 {
        if s.is_empty() {
            return 0.0;
        }

        let mut char_counts: HashMap<char, usize> = HashMap::new();
        for c in s.chars() {
            *char_counts.entry(c).or_insert(0) += 1;
        }

        #[allow(clippy::cast_precision_loss)]
        let len = s.len() as f64;
        -char_counts
            .values()
            .map(|&count| {
                #[allow(clippy::cast_precision_loss)]
                let p = count as f64 / len;
                p * p.log2()
            })
            .sum::<f64>()
    }

    /// Check if a string value is in SCREAMING_SNAKE_CASE format
    /// This pattern indicates semantic constants (event names, error codes), not actual secrets
    fn is_screaming_snake_case(value: &str) -> bool {
        // Must contain at least one letter
        if !value.chars().any(char::is_alphabetic) {
            return false;
        }

        // All characters must be uppercase letters, digits, or underscores
        value
            .chars()
            .all(|c| c.is_uppercase() || c.is_numeric() || c == '_')
    }

    /// Check if a constant is defined in an interface
    /// Interface constants are typically semantic identifiers for APIs, not credentials
    fn is_interface_constant(node: &SyntaxNode) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::INTERFACE_DECL {
                return true;
            }
            current = parent.parent();
        }
        false
    }

    /// Check if the variable name and value suggest a semantic identifier (not a credential)
    /// Examples:
    ///   MULTIFACTOR_AUTHENTICATION_BYPASS = "MULTIFACTOR_AUTHENTICATION_BYPASS_EVALUATION" → true
    ///   SERVICE_TICKET = "SERVICE_TICKET" → true
    ///   API_KEY = "sk_live_abc123..." → false
    fn is_semantic_identifier(var_name: &str, value: &str) -> bool {
        // Both must be SCREAMING_SNAKE_CASE
        if !Self::is_screaming_snake_case(var_name) || !Self::is_screaming_snake_case(value) {
            return false;
        }

        // Normalize for comparison (remove common suffixes/prefixes)
        let normalize = |s: &str| -> String {
            s.trim_end_matches("_EVALUATION")
                .trim_end_matches("_EVENT")
                .trim_end_matches("_ACTION")
                .trim_end_matches("_TYPE")
                .trim_end_matches("_CODE")
                .trim_start_matches("EVENT_")
                .trim_start_matches("ACTION_")
                .to_string()
        };

        let normalized_name = normalize(var_name);
        let normalized_value = normalize(value);

        // Check for similarity (value contains name or vice versa)
        normalized_value.contains(&normalized_name) || normalized_name.contains(&normalized_value)
    }

    /// Determine if a string value is likely to be a secret based on entropy analysis
    /// and pattern matching
    ///
    /// Returns true if the string appears to be a secret/credential value
    fn is_likely_secret(value: &str) -> bool {
        // Empty strings are not secrets
        if value.is_empty() {
            return false;
        }

        let entropy = Self::calculate_entropy(value);

        // AWS access keys have a specific pattern: start with "AKIA"
        // Check this BEFORE SCREAMING_SNAKE_CASE to avoid false negatives
        if value.starts_with("AKIA") && value.len() >= 16 {
            return true;
        }

        // Stripe keys have specific patterns
        if (value.starts_with("sk_") || value.starts_with("pk_")) && value.len() > 20 {
            return true;
        }

        // SCREAMING_SNAKE_CASE values are typically semantic identifiers (event names, error codes)
        // These are NOT secrets, they're API constants like "AUTHENTICATION_EVENT" or "SERVICE_TICKET"
        // However, we need to be careful not to filter out actual secrets that happen to be uppercase
        // Only filter if it has multiple segments (at least 2 underscores) indicating a semantic identifier
        if Self::is_screaming_snake_case(value) && value.matches('_').count() >= 2 {
            return false;
        }

        // Low entropy suggests human-readable text or config keys, not secrets
        // Entropy < 3.0 = likely English text, config keys, or simple patterns
        // Examples: "clientSecret" (2.855), "apiKey" (2.585)
        if entropy < 3.0 {
            return false;
        }

        // High entropy + reasonable length = likely a secret
        // Entropy > 4.5 indicates very strong randomness (base64, hex, random strings)
        if value.len() >= 20 && entropy > 4.5 {
            return true;
        }

        // Very long strings with good entropy are likely secrets
        if value.len() >= 32 && entropy > 3.5 {
            return true;
        }

        // Medium-high entropy (3.5-4.5) + reasonable length = likely a secret
        // This catches passwords like "MySecretPassword123" (3.932 entropy, 19 chars)
        // and "MyDatabasePassword!" (3.577 entropy, 19 chars)
        if value.len() >= 15 && entropy > 3.5 {
            return true;
        }

        // Default: strings with very low entropy or very short are likely not secrets
        false
    }

    /// Extract the string literal value from a variable declaration
    /// Returns None if no string literal is found
    fn extract_string_value(text: &str) -> Option<String> {
        // Find the assignment operator
        let start = text.find("= \"").or_else(|| text.find("=\""))?;
        let after_eq = &text[start + 2..];
        let quote_start = after_eq.find('"')?;
        let content_start = start + 2 + quote_start + 1;
        let remaining = &text[content_start..];
        let end = remaining.find('"')?;
        Some(remaining[..end].to_string())
    }

    fn is_placeholder(text: &str) -> bool {
        let Some(value) = Self::extract_string_value(text) else {
            return false;
        };

        let lower = value.to_lowercase();
        lower.is_empty()
            || lower == "changeme"
            || lower == "password"
            || lower.starts_with("your_")
            || lower.starts_with('<')
            || lower == "todo"
            || lower == "fixme"
            || lower == "example"
            || lower == "test"
            || lower == "dummy"
            || lower.contains("placeholder")
            || lower.contains("changeme")
    }

    /// Check if the string value looks like an error message or documentation
    fn is_message_or_doc(text: &str) -> bool {
        // Extract the string literal content
        let Some(start) = text.find("= \"").or_else(|| text.find("=\"")) else {
            return false;
        };
        let after_eq = &text[start + 2..];
        let Some(quote_start) = after_eq.find('"') else {
            return false;
        };
        let content_start = start + 2 + quote_start + 1;
        let remaining = &text[content_start..];
        let Some(end) = remaining.find('"') else {
            return false;
        };
        let string_content = &remaining[..end];

        // Long strings with spaces are likely messages/documentation
        if string_content.len() > 30 && string_content.contains(' ') {
            return true;
        }

        // Strings that look like error messages
        let lower = string_content.to_lowercase();
        let message_indicators = [
            "must ",
            "should ",
            "cannot ",
            "can't ",
            "unable to",
            "failed to",
            "error",
            "invalid",
            "missing",
            "required",
            "not allowed",
            "not found",
            "please ",
            "specify ",
            "the following",
            "prefix with",
            "handler class",
            "for brokers",
            "implements",
            "interface",
            "javadoc",
        ];

        message_indicators.iter().any(|&ind| lower.contains(ind))
    }

    /// Check if the variable name pattern suggests this is a message/config key, not an actual credential
    fn is_message_variable(text: &str) -> bool {
        let lower = text.to_lowercase();
        // Variables ending in MSG, MESSAGE, DOC, DESCRIPTION, etc.
        lower.contains("_msg")
            || lower.contains("msg =")
            || lower.contains("_message")
            || lower.contains("message =")
            || lower.contains("_doc")
            || lower.contains("doc =")
            || lower.contains("_desc")
            || lower.contains("description")
            || lower.contains("_note")
            || lower.contains("note =")
            || lower.contains("_error")
            || lower.contains("error_")
            || lower.contains("_format")
            || lower.contains("_text")
            || lower.contains("_string")
            || lower.contains("_name")
    }

    /// Check if the variable name indicates it's a descriptor (key, header, label, etc.)
    /// rather than an actual credential value.
    ///
    /// Examples of descriptor variables (should NOT be flagged):
    /// - PASSWORD_KEY (holds "user.password" config key name)
    /// - AUTH_HEADER (holds "Authorization" header name)
    /// - KEY_DISPLAY (holds "Key converter class" display label)
    /// - CREDENTIAL_LIFETIME_MS_SASL_NEGOTIATED_PROPERTY_KEY
    ///
    /// Examples of actual credential variables (SHOULD be flagged):
    /// - API_KEY (holds actual API key value)
    /// - ENCRYPTION_KEY (holds actual encryption key)
    /// - SECRET_KEY (holds actual secret)
    fn is_descriptor_variable(text: &str) -> bool {
        // Extract the variable name from the declaration
        let var_name = Self::extract_variable_name(text);
        let upper = var_name.to_uppercase();

        // Non-KEY descriptor suffixes - these clearly describe metadata, not values
        let descriptor_suffixes = [
            "_HEADER",
            "_NAME",
            "_PROPERTY",
            "_CONFIG",
            "_DISPLAY",
            "_LABEL",
            "_DESC",
            "_DESCRIPTION",
            "_FIELD",
            "_COLUMN",
            "_ATTR",
            "_ATTRIBUTE",
            "_PARAM",
            "_PARAMETER",
            "_COUNTER",
            "_METRIC",
            "_PER_SEC",
        ];

        // Check if variable name ends with a descriptor suffix
        for suffix in &descriptor_suffixes {
            if upper.ends_with(suffix) {
                return true;
            }
        }

        // Special handling for _KEY suffix:
        // Only treat as descriptor if it's a MULTI-WORD compound name
        // e.g., "PASSWORD_CONFIG_KEY", "CREDENTIAL_LIFETIME_KEY"
        // But NOT simple names like "API_KEY", "SECRET_KEY", "ENCRYPTION_KEY"
        if upper.ends_with("_KEY") {
            // Require at least 3 parts: PREFIX + DESCRIPTOR + KEY
            // e.g., ["PASSWORD", "CONFIG", "KEY"] or ["GROUP", "SIZE", "COUNTER", "KEY"]
            // Excludes: ["API", "KEY"], ["SECRET", "KEY"], ["ENCRYPTION", "KEY"]
            if upper.split('_').count() >= 3 {
                return true;
            }
        }

        false
    }

    /// Extract the variable name from a declaration
    fn extract_variable_name(text: &str) -> String {
        // Look for patterns like: String varName = or final String VAR_NAME =
        // Try to find the variable name before the = sign
        if let Some(eq_pos) = text.find('=') {
            let before_eq = text[..eq_pos].trim();
            // The variable name is the last word before =
            if let Some(last_word) = before_eq.split_whitespace().last() {
                return last_word.to_string();
            }
        }
        String::new()
    }

    /// Check if this looks like a string concatenation (often toString() or logging)
    /// e.g., String delegationTokenImageString = "DelegationTokenImage(" + String.join(", ", tokenIds) + ")"
    fn is_string_concatenation(text: &str) -> bool {
        // Check for string concatenation patterns
        if text.contains("\" +") || text.contains("+ \"") {
            return true;
        }

        // Check for StringBuilder patterns
        if text.contains("StringBuilder") || text.contains(".append(") {
            return true;
        }

        // Check for String.format
        if text.contains("String.format") {
            return true;
        }

        false
    }

    /// Check if the string value looks like a config property key (not a secret value)
    fn is_config_key(text: &str) -> bool {
        // Extract the string literal content
        let Some(start) = text.find("= \"").or_else(|| text.find("=\"")) else {
            return false;
        };
        let after_eq = &text[start + 2..];
        let Some(quote_start) = after_eq.find('"') else {
            return false;
        };
        let content_start = start + 2 + quote_start + 1;
        let remaining = &text[content_start..];
        let Some(end) = remaining.find('"') else {
            return false;
        };
        let string_content = &remaining[..end];

        // Config property keys typically:
        // 1. Contain dots (e.g., "sasl.oauthbearer.client.id")
        // 2. Are lowercase with dots/underscores
        // 3. Don't contain spaces
        if string_content.contains('.') && !string_content.contains(' ') {
            // Check if it looks like a property key (lowercase, dots, no special chars except dot/underscore/hyphen)
            let is_property_key = string_content.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
            });
            if is_property_key {
                return true;
            }
        }

        // Also check for Java class names (fully qualified)
        // e.g., "com.example.security.oauth.DefaultJwtValidator"
        if string_content.contains('.') && !string_content.contains(' ') {
            let parts: Vec<&str> = string_content.split('.').collect();
            if parts.len() >= 3 {
                // Likely a class name if starts with org/com/net/io and has capital letters
                let first = parts[0];
                if (first == "org"
                    || first == "com"
                    || first == "net"
                    || first == "io"
                    || first == "java")
                    && string_content.chars().any(|c| c.is_ascii_uppercase())
                {
                    return true;
                }
            }
        }

        false
    }

    /// Detect if a string value is a property key/configuration path, not a credential
    fn is_property_key(value: &str) -> bool {
        // Pattern 1: Dotted property notation (2+ dots): "oauth2.client.secret"
        if value.matches('.').count() >= 2 {
            return true;
        }

        // Pattern 2: Well-known HTTP header names (lowercase with hyphens)
        // Only match standard HTTP headers, not arbitrary kebab-case strings
        if value.contains('-')
            && value
                .chars()
                .all(|c| c.is_lowercase() || c == '-' || c.is_ascii_digit())
        {
            // Must be a known HTTP header pattern to avoid false negatives on kebab-case secrets
            let known_header_patterns = [
                "access-control",
                "content-",
                "cache-control",
                "x-",
                "accept-",
                "allow-",
            ];

            if known_header_patterns
                .iter()
                .any(|&pattern| value.starts_with(pattern))
            {
                return true;
            }
        }

        // Pattern 3: URL paths: "/v1/auth/login"
        if value.starts_with('/') && value.len() > 1 {
            return true;
        }

        // Pattern 4: Spring property references: "${app.password}"
        if value.starts_with("${") && value.ends_with('}') {
            return true;
        }

        // Pattern 5: Environment variable references: "%PASSWORD%", "$PASSWORD"
        if (value.starts_with('%') && value.ends_with('%')) || value.starts_with('$') {
            return true;
        }

        // Pattern 6: Single dot notation (common property keys): "client.secret"
        if value.matches('.').count() == 1 && value.len() > 8 && !value.contains(' ') {
            return true;
        }

        false
    }

    /// Detect if a variable is a Spring MVC view name, not a credential
    fn is_view_name_variable(var_name: &str, value: &str) -> bool {
        let lower_name = var_name.to_lowercase();

        // Pattern 1: Variable name ends with View/Redirect/Template/Page
        let is_view_variable = lower_name.ends_with("view")
            || lower_name.ends_with("redirect")
            || lower_name.ends_with("template")
            || lower_name.ends_with("page")
            || lower_name.ends_with("url");

        if !is_view_variable {
            return false;
        }

        // Pattern 2: Value looks like a view path
        let value_lower = value.to_lowercase();
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        // value_lower is already lowercased
        let is_view_value = value.starts_with("redirect:")
            || value.starts_with("forward:")
            || value.contains('/') // Template path like "account/changePassword"
            || value_lower.ends_with(".html")
            || value_lower.ends_with(".jsp")
            || value_lower.ends_with(".ftl");

        is_view_variable && is_view_value
    }

    /// Detect if a constant is self-referential (value equals name)
    /// This is a common pattern for enum-like constants in audit/logging systems
    fn is_self_referential_constant(var_name: &str, value: &str) -> bool {
        // Normalize both name and value (remove underscores, convert to uppercase)
        let normalized_name = var_name.to_uppercase().replace('_', "");
        let normalized_value = value.to_uppercase().replace(['_', '"'], "");

        normalized_name == normalized_value
    }

    /// Detect if a constant is an internal key/identifier, not a credential
    fn is_internal_key_constant(var_name: &str, value: &str) -> bool {
        let lower_name = var_name.to_lowercase();

        // Pattern 1: Variable name pattern for keys (KEY_FOR_*, *_KEY, *_key)
        let has_key_pattern = lower_name.starts_with("key_")
            || lower_name.ends_with("_key")
            || lower_name.contains("_key_");

        if !has_key_pattern {
            return false;
        }

        // Pattern 2: Value starts with underscore (internal identifier convention)
        if value.starts_with('_') {
            return true;
        }

        // Pattern 3: Value contains "key" or "id" and looks like an identifier
        let lower_value = value.to_lowercase();
        if (lower_value.contains("_key")
            || lower_value.contains("_id")
            || lower_value.contains("key_"))
            && value.contains('_')
        {
            return true;
        }

        // Pattern 4: All-caps constant name with low-entropy value (identifiers, not secrets)
        if var_name.chars().all(|c| c.is_uppercase() || c == '_') && value.len() < 30 {
            // Check entropy - identifiers have low entropy
            let entropy = Self::calculate_entropy(value);
            if entropy < 3.5 {
                return true;
            }
        }

        false
    }

    /// Check if the string value is a known safe pattern (not a secret)
    fn is_known_safe_value(text: &str) -> bool {
        // Extract the string literal content
        let Some(start) = text.find("= \"").or_else(|| text.find("=\"")) else {
            return false;
        };
        let after_eq = &text[start + 2..];
        let Some(quote_start) = after_eq.find('"') else {
            return false;
        };
        let content_start = start + 2 + quote_start + 1;
        let remaining = &text[content_start..];
        let Some(end) = remaining.find('"') else {
            return false;
        };
        let string_content = &remaining[..end];
        let lower = string_content.to_lowercase();

        // Known safe patterns that are not secrets:

        // 0. Public URLs without embedded credentials
        if (string_content.starts_with("http://") || string_content.starts_with("https://"))
            && !string_content.contains('@')
        {
            return true;
        }

        // 1. Algorithm names
        let algorithms = [
            "rs256", "rs384", "rs512", "es256", "es384", "es512", "hs256", "hs384", "hs512",
            "sha256", "sha512", "md5", "aes", "rsa", "ecdsa", "hmac",
        ];
        if algorithms.iter().any(|&a| lower == a) {
            return true;
        }

        // 2. OAuth/SASL mechanism names
        let mechanisms = [
            "oauthbearer",
            "plain",
            "scram-sha-256",
            "scram-sha-512",
            "gssapi",
            "client_credentials",
            "authorization_code",
            "refresh_token",
            "password",
        ];
        if mechanisms.iter().any(|&m| lower == m) {
            return true;
        }

        // 3. HTTP header names (including X-* custom headers)
        let headers = [
            "authorization",
            "authentication",
            "x-api-key",
            "x-auth-token",
            "www-authenticate",
            "proxy-authorization",
        ];
        if headers.iter().any(|&h| lower == h) {
            return true;
        }

        // 3b. X-*-Authorization pattern (like "X-Connect-Authorization")
        if lower.starts_with("x-") && lower.ends_with("-authorization") {
            return true;
        }

        // 3c. Any X-* header that contains auth-related words
        if lower.starts_with("x-") && !string_content.contains(' ') {
            return true;
        }

        // 4. Grant type URNs
        if lower.starts_with("urn:ietf:params:oauth:grant-type:") {
            return true;
        }

        // 5. Short camelCase config key names (e.g., "clientSecret", "tokenAuth")
        // These are KEY NAMES, not values
        if string_content.len() <= 20
            && !string_content.contains(' ')
            && string_content
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase())
            && string_content.chars().any(|c| c.is_ascii_uppercase())
        {
            return true;
        }

        // 6. All-caps values with dots (property key names like "CREDENTIAL.LIFETIME.MS")
        if string_content.contains('.')
            && string_content
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '.' || c == '_')
        {
            return true;
        }

        // 7. All-caps short values (constants, not secrets)
        if string_content.len() <= 20
            && string_content
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_')
        {
            return true;
        }

        // 8. Display labels (contain spaces and common label words)
        if string_content.contains(' ') {
            let display_patterns = [
                "class",
                "type",
                "name",
                "display",
                "label",
                "converter",
                "key ",
            ];
            if display_patterns.iter().any(|&p| lower.contains(p)) {
                return true;
            }
        }

        // 9. Metric names (CamelCase with common metric suffixes)
        let metric_suffixes = ["PerSec", "Count", "Total", "Rate", "Latency", "Bytes"];
        if metric_suffixes.iter().any(|&s| string_content.ends_with(s)) {
            return true;
        }

        // 10. Kebab-case config/identifier names (like "expire-group-metadata")
        // But NOT if they contain security-related words that suggest actual secrets
        if string_content.contains('-')
            && !string_content.contains(' ')
            && string_content
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit())
        {
            // Exclude if it looks like an actual secret value
            let security_words = [
                "secret",
                "password",
                "token",
                "key",
                "credential",
                "auth",
                "api",
            ];
            let is_security_value = security_words.iter().any(|&w| lower.contains(w));
            if !is_security_value {
                return true;
            }
        }

        false
    }
}
