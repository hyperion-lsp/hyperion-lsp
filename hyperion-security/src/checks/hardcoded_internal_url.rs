// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hardcoded internal URL detection
//!
//! Detects hardcoded private IP addresses, localhost references, and internal
//! domain patterns in source code.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::net::Ipv4Addr;

/// Check for hardcoded internal URLs (CWE-1004)
///
/// Detects:
/// - Private IP addresses (10.x.x.x, 172.16-31.x.x, 192.168.x.x)
/// - Localhost references (127.0.0.1, localhost)
/// - Internal domain patterns (*.internal, *.local, *.corp)
pub struct HardcodedInternalUrlCheck;

impl HardcodedInternalUrlCheck {
    /// Internal domain suffixes
    const INTERNAL_DOMAIN_SUFFIXES: &'static [&'static str] =
        &[".internal", ".local", ".corp", ".lan"];

    /// Localhost patterns
    const LOCALHOST_PATTERNS: &'static [&'static str] = &["localhost", "127.0.0.1", "::1"];

    /// Property key context patterns (Spring and other frameworks)
    const PROPERTY_KEY_PATTERNS: &'static [&'static str] = &[
        "@Value(",
        "@ConfigurationProperties",
        "properties.get",
        "getProperty",
        "setProperty",
        "${", // Spring property placeholder
    ];

    /// Extract string literal value from a node
    fn extract_string_literal(node: &SyntaxNode) -> Option<String> {
        let text = node.text().to_string();

        // Find string literal between quotes
        let start = text.find('"')?;
        let after_start = &text[start + 1..];
        let end = after_start.find('"')?;
        Some(after_start[..end].to_string())
    }

    /// Check if an IP address is in a private range
    fn is_private_ip(ip: &str) -> bool {
        // Try to parse as IPv4
        if let Ok(addr) = ip.parse::<Ipv4Addr>() {
            // Check private ranges:
            // 10.0.0.0/8
            // 172.16.0.0/12
            // 192.168.0.0/16
            let octets = addr.octets();

            // 10.x.x.x
            if octets[0] == 10 {
                return true;
            }

            // 172.16.x.x - 172.31.x.x
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true;
            }

            // 192.168.x.x
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
        }

        false
    }

    /// Extract IP addresses from a string
    fn extract_ip_addresses(s: &str) -> Vec<String> {
        let mut ips = Vec::new();

        // Simple regex-like pattern: look for sequences like X.X.X.X
        let words: Vec<&str> = s.split(&[' ', '/', ':', '?', '&', '='][..]).collect();

        for word in words {
            // Check if this looks like an IP address
            let parts: Vec<&str> = word.split('.').collect();
            if parts.len() == 4 {
                // Check if all parts are numbers
                if parts.iter().all(|p| p.parse::<u8>().is_ok()) {
                    ips.push(word.to_string());
                }
            }
        }

        ips
    }

    /// Check if a string contains a localhost reference
    fn contains_localhost(s: &str) -> bool {
        let lower = s.to_lowercase();
        Self::LOCALHOST_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Check if a string contains an internal domain
    fn contains_internal_domain(s: &str) -> bool {
        let lower = s.to_lowercase();

        // Check for internal domain suffixes
        for suffix in Self::INTERNAL_DOMAIN_SUFFIXES {
            if lower.contains(suffix) {
                return true;
            }
        }

        false
    }

    /// Check if this looks like a test or example value
    fn is_test_or_example(s: &str) -> bool {
        let lower = s.to_lowercase();

        // Example IPs that are commonly used in documentation
        if s == "192.0.2.1" || s == "198.51.100.1" || s == "203.0.113.1" {
            return true;
        }

        // Check if the string contains test/example indicators
        lower.contains("example") || lower.contains("test") || lower.contains("sample")
    }

    /// Check if string is likely a configuration property key
    ///
    /// Property keys use dots as namespace separators, not domain delimiters.
    /// Examples: "com.example.internal.service", "spring.datasource.url"
    ///
    /// This differentiates between:
    /// - Property keys: "com.example.internal.service" (NOT a URL)
    /// - Actual URLs: "http://api.internal/v1/users" (IS a URL)
    fn is_property_key(s: &str, context_node: &SyntaxNode) -> bool {
        // Property keys don't have protocols
        let has_protocol = s.contains("://") || s.contains("jdbc:") || s.contains("mongodb:");
        if has_protocol {
            return false;
        }

        // Property keys don't have slashes (path separators)
        if s.contains('/') {
            return false;
        }

        // Property keys don't have ports (colons outside of protocol)
        if s.contains(':') && !s.contains("://") {
            return false;
        }

        // Property keys have multiple dot-separated segments
        let segments: Vec<&str> = s.split('.').collect();
        if segments.len() < 2 {
            return false; // Property keys typically have 2+ segments
        }

        // Check if all segments are valid identifiers
        let all_valid_identifiers = segments.iter().all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        });

        if !all_valid_identifiers {
            return false;
        }

        // Check surrounding context for property key patterns
        // Walk up to find annotation or method context
        let mut current = Some(context_node.clone());
        while let Some(node) = current {
            let node_text = node.text().to_string();

            // Check if we're in a property key context
            for pattern in Self::PROPERTY_KEY_PATTERNS {
                if node_text.contains(pattern) {
                    return true; // Strong signal - in property context
                }
            }

            // Don't walk too far up (max 5 levels)
            let depth = node
                .ancestors()
                .take_while(|n| n.text_range() != context_node.text_range())
                .count();
            if depth > 5 {
                break;
            }

            current = node.parent();
        }

        // Even without explicit context, 4+ segments is a strong signal
        // (e.g., "com.example.service.endpoint")
        segments.len() >= 4
    }

    /// Check if this looks like a URL or connection string
    fn looks_like_url_or_connection(s: &str) -> bool {
        let lower = s.to_lowercase();

        // Common URL/connection string protocols
        lower.contains("http://")
            || lower.contains("https://")
            || lower.contains("jdbc:")
            || lower.contains("mongodb:")
            || lower.contains("redis:")
            || lower.contains("amqp:")
            || lower.contains("kafka:")
            || lower.starts_with("//") // Protocol-relative URL
    }

    /// Check if the containing class is a configuration class
    ///
    /// Configuration classes typically have default values that are meant to be
    /// overridden via setters, @JsonProperty, or external configuration files.
    fn is_configuration_class(node: &SyntaxNode) -> bool {
        // Walk up to find the containing class
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::CLASS_DECL {
                let class_text = parent.text().to_string();

                // Check for configuration-related class names
                if let Some(class_name) = Self::extract_class_name(&class_text) {
                    if class_name.ends_with("Config")
                        || class_name.ends_with("Configuration")
                        || class_name.ends_with("Settings")
                        || class_name.ends_with("Properties")
                    {
                        return true;
                    }
                }

                // Check for Spring @Configuration or @ConfigurationProperties annotations
                if class_text.contains("@Configuration")
                    || class_text.contains("@ConfigurationProperties")
                {
                    return true;
                }

                // Check for Jackson @JsonProperty annotations (indicates JSON config)
                if class_text.contains("@JsonProperty") {
                    return true;
                }

                // Check if class extends a configuration base class
                if class_text.contains("extends Configuration")
                    || class_text.contains("extends BaseConfig")
                    || class_text.contains("extends AppConfig")
                {
                    return true;
                }

                // Found the class, no need to continue
                return false;
            }

            current = parent.parent();
        }

        false
    }

    /// Extract class name from class declaration text
    fn extract_class_name(class_text: &str) -> Option<String> {
        // Look for "class ClassName" pattern
        let words: Vec<&str> = class_text.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            if *word == "class" && i + 1 < words.len() {
                // Next word is the class name (might include generics or extends)
                let name_part = words[i + 1];
                // Strip generics and other noise
                let clean_name = name_part
                    .split('<')
                    .next()?
                    .split('{')
                    .next()?
                    .split('(')
                    .next()?
                    .trim();
                return Some(clean_name.to_string());
            }
        }
        None
    }

    /// Check if this node is a static final field
    ///
    /// Static final fields in configuration classes are often used for default values
    /// and represent code smells rather than security vulnerabilities.
    fn is_static_final_field(node: &SyntaxNode) -> bool {
        // Walk up to find field declaration
        let mut current = Some(node.clone());
        while let Some(n) = current {
            if n.kind() == SyntaxKind::FIELD_DECL {
                // In the Java parser, modifiers are in a MODIFIER_LIST sibling node,
                // not part of FIELD_DECL itself. Need to check the parent (CLASS_DECL)
                // for modifier information.
                if let Some(parent) = n.parent() {
                    let parent_text = parent.text().to_string();
                    // Check for both "static" and "final" modifiers in the class declaration
                    return parent_text.contains("static") && parent_text.contains("final");
                }
                return false;
            }
            current = n.parent();
        }
        false
    }
}

impl Check for HardcodedInternalUrlCheck {
    fn name(&self) -> &'static str {
        "hardcoded-internal-url"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::FIELD_DECL,
            SyntaxKind::STRING_LITERAL,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        // Extract string literal value
        let string_value = match Self::extract_string_literal(node) {
            Some(s) => s,
            None => return vec![],
        };

        // Skip empty strings
        if string_value.is_empty() {
            return vec![];
        }

        // Skip property keys (e.g., "com.example.internal.service" in @Value)
        if Self::is_property_key(&string_value, node) {
            return vec![];
        }

        // Skip test/example values
        if Self::is_test_or_example(&string_value) {
            return vec![];
        }

        let mut findings = Vec::new();

        // Check for private IP addresses
        let ips = Self::extract_ip_addresses(&string_value);
        for ip in &ips {
            if Self::is_private_ip(ip) {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);

                // Check if this is a static final field in a config class
                let is_static_final = Self::is_static_final_field(node);
                let is_config_class = Self::is_configuration_class(node);

                let message = if is_static_final && is_config_class {
                    format!(
                        "Hardcoded private IP address: {ip} in configuration constant. \
                        This appears to be a configuration default value. \
                        Consider moving to application.properties or environment variables \
                        for better environment-specific configuration."
                    )
                } else {
                    format!(
                        "Hardcoded private IP address: {ip}. Private IP addresses should not be \
                        hardcoded in source code as they can change between environments and expose \
                        internal network topology."
                    )
                };

                let mut finding =
                    Finding::new(VulnType::HardcodedInternalUrl, message, location, code);

                // Downgrade severity for static final fields in config classes
                finding.severity = if is_static_final && is_config_class {
                    Severity::Low
                } else {
                    Severity::Medium
                };

                finding.remediation =
                    "Use configuration files or environment variables for IP addresses and URLs. \
                    Example: String dbUrl = System.getenv(\"DB_URL\");"
                        .to_string();
                findings.push(finding);
                // Only report once per node
                return findings;
            }
        }

        // Check for localhost (only if it looks like a URL/connection string)
        if Self::looks_like_url_or_connection(&string_value)
            && Self::contains_localhost(&string_value)
        {
            let location = context.node_location(node);
            let code = context.get_code_snippet(node);

            // Check if this is a static final field in a config class
            let is_static_final = Self::is_static_final_field(node);
            let is_config_class = Self::is_configuration_class(node);

            let message = if is_static_final && is_config_class {
                "Hardcoded localhost reference in configuration constant. \
                This appears to be a configuration default value. \
                Consider moving to application.properties or environment variables \
                for better environment-specific configuration."
                    .to_string()
            } else {
                "Hardcoded localhost reference in production code. Localhost URLs can cause \
                issues when deployed to production environments where services run on different hosts."
                    .to_string()
            };

            let mut finding = Finding::new(VulnType::HardcodedInternalUrl, message, location, code);

            // Downgrade severity for static final fields in config classes
            finding.severity = if is_static_final && is_config_class {
                Severity::Low
            } else {
                Severity::Medium
            };

            finding.remediation =
                "Use configuration to specify service URLs. Different environments (dev/staging/prod) \
                should use different host configurations. For configuration classes, add setter methods \
                or use @JsonProperty to allow overriding defaults."
                    .to_string();
            findings.push(finding);
            return findings;
        }

        // Check for internal domain patterns
        if Self::contains_internal_domain(&string_value) {
            let location = context.node_location(node);
            let code = context.get_code_snippet(node);

            // Check if this is a static final field in a config class
            let is_static_final = Self::is_static_final_field(node);
            let is_config_class = Self::is_configuration_class(node);

            let message = if is_static_final && is_config_class {
                "Hardcoded internal domain name in configuration constant. \
                This appears to be a configuration default value. \
                Consider moving to application.properties or environment variables \
                for better environment-specific configuration."
                    .to_string()
            } else {
                "Hardcoded internal domain name. Internal domain names (.internal, .local, .corp) \
                should not be hardcoded as they expose internal infrastructure details."
                    .to_string()
            };

            let mut finding = Finding::new(VulnType::HardcodedInternalUrl, message, location, code);

            // Downgrade severity for static final fields in config classes
            finding.severity = if is_static_final && is_config_class {
                Severity::Low
            } else {
                Severity::Medium
            };

            finding.remediation = "Use configuration for internal service URLs. Consider using \
                service discovery (Consul, Eureka) instead of hardcoded hostnames."
                .to_string();
            findings.push(finding);
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_private_ip() {
        assert!(HardcodedInternalUrlCheck::is_private_ip("10.0.0.1"));
        assert!(HardcodedInternalUrlCheck::is_private_ip("10.255.255.255"));
        assert!(HardcodedInternalUrlCheck::is_private_ip("172.16.0.1"));
        assert!(HardcodedInternalUrlCheck::is_private_ip("172.31.255.255"));
        assert!(HardcodedInternalUrlCheck::is_private_ip("192.168.1.1"));
        assert!(HardcodedInternalUrlCheck::is_private_ip("192.168.255.255"));

        assert!(!HardcodedInternalUrlCheck::is_private_ip("8.8.8.8"));
        assert!(!HardcodedInternalUrlCheck::is_private_ip("1.1.1.1"));
        assert!(!HardcodedInternalUrlCheck::is_private_ip("172.15.0.1"));
        assert!(!HardcodedInternalUrlCheck::is_private_ip("172.32.0.1"));
    }

    #[test]
    fn test_extract_ip_addresses() {
        let ips = HardcodedInternalUrlCheck::extract_ip_addresses("jdbc:mysql://10.0.0.5:3306/db");
        assert_eq!(ips, vec!["10.0.0.5"]);

        let ips = HardcodedInternalUrlCheck::extract_ip_addresses("http://192.168.1.100/api");
        assert_eq!(ips, vec!["192.168.1.100"]);
    }

    #[test]
    fn test_contains_localhost() {
        assert!(HardcodedInternalUrlCheck::contains_localhost(
            "http://localhost:8080"
        ));
        assert!(HardcodedInternalUrlCheck::contains_localhost(
            "jdbc:mysql://127.0.0.1:3306/db"
        ));

        assert!(!HardcodedInternalUrlCheck::contains_localhost(
            "http://example.com"
        ));
    }

    #[test]
    fn test_contains_internal_domain() {
        assert!(HardcodedInternalUrlCheck::contains_internal_domain(
            "http://api.internal"
        ));
        assert!(HardcodedInternalUrlCheck::contains_internal_domain(
            "db.local"
        ));
        assert!(HardcodedInternalUrlCheck::contains_internal_domain(
            "service.corp"
        ));

        assert!(!HardcodedInternalUrlCheck::contains_internal_domain(
            "example.com"
        ));
    }

    #[test]
    fn test_is_test_or_example() {
        assert!(HardcodedInternalUrlCheck::is_test_or_example("192.0.2.1"));
        assert!(HardcodedInternalUrlCheck::is_test_or_example(
            "http://example.com"
        ));
        assert!(HardcodedInternalUrlCheck::is_test_or_example("test-url"));

        assert!(!HardcodedInternalUrlCheck::is_test_or_example("10.0.0.1"));
    }

    #[test]
    fn test_extract_class_name() {
        assert_eq!(
            HardcodedInternalUrlCheck::extract_class_name("public class DatabaseConfig {"),
            Some("DatabaseConfig".to_string())
        );

        assert_eq!(
            HardcodedInternalUrlCheck::extract_class_name(
                "class ServiceConfiguration extends BaseConfig {"
            ),
            Some("ServiceConfiguration".to_string())
        );

        assert_eq!(
            HardcodedInternalUrlCheck::extract_class_name("public class MySettings<T> {"),
            Some("MySettings".to_string())
        );

        assert_eq!(
            HardcodedInternalUrlCheck::extract_class_name("public interface MyInterface {"),
            None
        );
    }

    #[test]
    fn test_is_property_key_patterns() {
        use hyperion_parser::{Lexer, Parser};

        fn parse_java(source: &str) -> SyntaxNode {
            let tokens = Lexer::new(source).collect();
            let (green_node, _errors) = Parser::parse(tokens);
            SyntaxNode::new_root(green_node)
        }

        // Test property key patterns (should recognize as property keys)
        let test_cases = vec![
            // Spring property keys
            ("com.example.internal.service", true),
            ("spring.datasource.url", true),
            ("kafka.producer.bootstrap.servers", true),
            ("app.security.jwt.secret", true),
            ("myapp.internal.api.endpoint", true),
            // Actual URLs (should NOT match)
            ("http://api.internal/v1", false),
            ("https://service.local:8080", false),
            ("jdbc:mysql://db.local:3306/app", false),
            ("service.corp/api", false),
            ("192.168.1.100:8080", false),
            // Edge cases
            ("simple.key", true),          // 2 segments is OK
            ("http://example.com", false), // Has protocol
            ("api/v1/users", false),       // Has slashes
        ];

        for (value, expected) in test_cases {
            // Create a simple Java file with the string
            let code = format!(r#"class Test {{ String s = "{}"; }}"#, value);
            let root = parse_java(&code);

            // Find the string literal node
            let string_node = root
                .descendants()
                .find(|n: &SyntaxNode| n.kind() == SyntaxKind::STRING_LITERAL);

            if let Some(node) = string_node {
                let result = HardcodedInternalUrlCheck::is_property_key(value, &node);
                assert_eq!(
                    result, expected,
                    "is_property_key('{}') = {}, expected {}",
                    value, result, expected
                );
            }
        }
    }

    #[test]
    fn test_is_static_final_field() {
        use hyperion_parser::{Lexer, Parser};

        fn parse_java(source: &str) -> SyntaxNode {
            let tokens = Lexer::new(source).collect();
            let (green_node, _errors) = Parser::parse(tokens);
            SyntaxNode::new_root(green_node)
        }

        // Test static final field detection
        let static_final_code =
            r#"class Config { private static final String URL = "https://example.com"; }"#;
        let root = parse_java(static_final_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");
        assert!(
            HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should detect static final field"
        );

        // Test non-static field
        let non_static_code =
            r#"class Config { private final String url = "https://example.com"; }"#;
        let root = parse_java(non_static_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");
        assert!(
            !HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should not detect non-static field as static final"
        );

        // Test static but not final
        let static_only_code =
            r#"class Config { private static String url = "https://example.com"; }"#;
        let root = parse_java(static_only_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");
        assert!(
            !HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should not detect static-only field as static final"
        );

        // Test local variable (not a field)
        let local_var_code =
            r#"class Test { void method() { String url = "https://example.com"; } }"#;
        let root = parse_java(local_var_code);
        let var_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LOCAL_VAR_DECL)
            .expect("Should find local var decl");
        assert!(
            !HardcodedInternalUrlCheck::is_static_final_field(&var_node),
            "Should not detect local variable as static final field"
        );
    }

    #[test]
    fn test_static_final_url_in_config_class_severity() {
        use hyperion_parser::{Lexer, Parser};

        fn parse_java(source: &str) -> SyntaxNode {
            let tokens = Lexer::new(source).collect();
            let (green_node, _errors) = Parser::parse(tokens);
            SyntaxNode::new_root(green_node)
        }

        // Test: static final URL in config class should be LOW severity
        let config_code = r#"public class PaymentConfig { private static final String WEBHOOK_URL = "https://webhook.site"; }"#;
        let root = parse_java(config_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");

        assert!(
            HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should detect static final field"
        );
        assert!(
            HardcodedInternalUrlCheck::is_configuration_class(&field_node),
            "Should detect config class"
        );
    }

    #[test]
    fn test_non_static_url_in_service_class() {
        use hyperion_parser::{Lexer, Parser};

        fn parse_java(source: &str) -> SyntaxNode {
            let tokens = Lexer::new(source).collect();
            let (green_node, _errors) = Parser::parse(tokens);
            SyntaxNode::new_root(green_node)
        }

        // Test: non-static URL in service class should still be MEDIUM severity
        let service_code = r#"public class PaymentService { private String webhookUrl = "https://webhook.site"; }"#;
        let root = parse_java(service_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");

        assert!(
            !HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should not be static final"
        );
        assert!(
            !HardcodedInternalUrlCheck::is_configuration_class(&field_node),
            "Should not be config class"
        );
    }

    #[test]
    fn test_internal_domain_in_config_constant() {
        use hyperion_parser::{Lexer, Parser};

        fn parse_java(source: &str) -> SyntaxNode {
            let tokens = Lexer::new(source).collect();
            let (green_node, _errors) = Parser::parse(tokens);
            SyntaxNode::new_root(green_node)
        }

        // Test: internal domain in static final config constant
        let config_code = r#"public class DatabaseConfig { private static final String DB_HOST = "db.internal"; }"#;
        let root = parse_java(config_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");

        assert!(
            HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should detect static final field"
        );
        assert!(
            HardcodedInternalUrlCheck::is_configuration_class(&field_node),
            "Should detect config class"
        );
        assert!(
            HardcodedInternalUrlCheck::contains_internal_domain("db.internal"),
            "Should detect internal domain"
        );
    }

    #[test]
    fn test_private_ip_in_config_constant() {
        use hyperion_parser::{Lexer, Parser};

        fn parse_java(source: &str) -> SyntaxNode {
            let tokens = Lexer::new(source).collect();
            let (green_node, _errors) = Parser::parse(tokens);
            SyntaxNode::new_root(green_node)
        }

        // Test: private IP in static final config constant
        let config_code = r#"public class RedisConfiguration { private static final String REDIS_HOST = "10.0.0.5"; }"#;
        let root = parse_java(config_code);
        let field_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)
            .expect("Should find field decl");

        assert!(
            HardcodedInternalUrlCheck::is_static_final_field(&field_node),
            "Should detect static final field"
        );
        assert!(
            HardcodedInternalUrlCheck::is_configuration_class(&field_node),
            "Should detect configuration class"
        );
        assert!(
            HardcodedInternalUrlCheck::is_private_ip("10.0.0.5"),
            "Should detect private IP"
        );
    }
}
