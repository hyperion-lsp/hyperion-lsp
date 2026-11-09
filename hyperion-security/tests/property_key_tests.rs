// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for property key detection in hardcoded URL check
//!
//! Verifies that configuration property keys are NOT flagged as hardcoded URLs.

use hyperion_parser::{Lexer, Parser, SyntaxNode};
use hyperion_security::checks::hardcoded_internal_url::HardcodedInternalUrlCheck;
use hyperion_security::checks::{Check, CheckContext};
use std::path::Path;

fn parse_java(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

fn scan_code(code: &str) -> Vec<hyperion_security::finding::Finding> {
    let root = parse_java(code);
    let check = HardcodedInternalUrlCheck;
    let context = CheckContext::new(Path::new("Test.java"), code);

    let mut findings = Vec::new();
    for node in root.descendants() {
        if check.target_kinds().contains(&node.kind()) {
            findings.extend(check.check(&node, &context));
        }
    }
    findings
}

#[test]
fn test_spring_value_annotation_property_key() {
    let code = r#"
        package com.example;

        import org.springframework.beans.factory.annotation.Value;

        public class ServiceConfig {
            @Value("${com.example.internal.service.url}")
            private String serviceUrl;

            @Value("${app.internal.api.endpoint}")
            private String apiEndpoint;
        }
    "#;

    let findings = scan_code(code);

    // Should NOT flag property keys in @Value annotations
    assert_eq!(
        findings.len(),
        0,
        "Property keys in @Value should not be flagged: {:?}",
        findings
    );
}

#[test]
fn test_spring_configuration_properties() {
    let code = r#"
        package com.example;

        import org.springframework.boot.context.properties.ConfigurationProperties;

        @ConfigurationProperties(prefix = "spring.datasource.url")
        public class DataSourceConfig {
            private String username;
            private String password;
        }
    "#;

    let findings = scan_code(code);

    // Should NOT flag property key in @ConfigurationProperties
    assert_eq!(
        findings.len(),
        0,
        "Property key in @ConfigurationProperties should not be flagged: {:?}",
        findings
    );
}

#[test]
fn test_property_key_constant() {
    let code = r#"
        package com.example;

        public class Constants {
            private static final String CONFIG_KEY = "app.internal.service.endpoint";
            private static final String DB_KEY = "spring.datasource.url";
            private static final String KAFKA_KEY = "kafka.producer.bootstrap.servers";
        }
    "#;

    let findings = scan_code(code);

    // Property keys with 4+ segments should not be flagged even without @Value context
    assert_eq!(
        findings.len(),
        0,
        "Property key constants should not be flagged: {:?}",
        findings
    );
}

#[test]
fn test_properties_get_method() {
    let code = r#"
        package com.example;

        import java.util.Properties;

        public class ConfigLoader {
            public void loadConfig(Properties properties) {
                String url = properties.get("com.example.internal.api.url");
            }
        }
    "#;

    let findings = scan_code(code);

    // Should NOT flag property keys in properties.get()
    assert_eq!(
        findings.len(),
        0,
        "Property keys in properties.get() should not be flagged: {:?}",
        findings
    );
}

#[test]
fn test_system_get_property() {
    let code = r#"
        package com.example;

        public class SystemConfig {
            public void loadConfig() {
                String url = System.getProperty("myapp.internal.service.url");
            }
        }
    "#;

    let findings = scan_code(code);

    // Should NOT flag property keys in System.getProperty()
    assert_eq!(
        findings.len(),
        0,
        "Property keys in getProperty() should not be flagged: {:?}",
        findings
    );
}

#[test]
fn test_actual_hardcoded_internal_url_still_detected() {
    let code = r#"
        package com.example;

        public class ServiceClient {
            private static final String API_URL = "http://api.internal/v1/users";
            private static final String DB_URL = "jdbc:mysql://db.local:3306/app";
        }
    "#;

    let findings = scan_code(code);

    // SHOULD flag actual hardcoded URLs with internal domains
    assert_eq!(
        findings.len(),
        2,
        "Actual hardcoded internal URLs should still be detected"
    );

    // Verify both findings are for internal URLs
    for finding in &findings {
        assert_eq!(
            finding.vuln_type,
            hyperion_security::finding::VulnType::HardcodedInternalUrl
        );
    }
}

#[test]
fn test_mixed_property_keys_and_urls() {
    let code = r#"
        package com.example;

        import org.springframework.beans.factory.annotation.Value;

        public class MixedConfig {
            // Property key - should NOT be flagged
            @Value("${com.example.internal.service}")
            private String configKey;

            // Actual URL - SHOULD be flagged
            private static final String HARDCODED_URL = "http://service.internal/api";

            // Property key constant - should NOT be flagged (4+ segments)
            private static final String PROPERTY_KEY = "app.internal.api.endpoint.url";
        }
    "#;

    let findings = scan_code(code);

    // Only the actual hardcoded URL should be flagged
    assert_eq!(
        findings.len(),
        1,
        "Only actual URLs should be flagged, not property keys"
    );

    assert!(findings[0].code_snippet.contains("http://service.internal"));
}

#[test]
fn test_short_property_key_without_context() {
    let code = r#"
        package com.example;

        public class Config {
            // 2-segment property key without @Value context
            // This is ambiguous - could be a property key or domain
            private static final String KEY = "service.internal";
        }
    "#;

    let findings = scan_code(code);

    // Without @Value context and with only 2 segments, this is treated as a potential domain
    // This is acceptable - better to flag ambiguous cases than miss real issues
    assert_eq!(
        findings.len(),
        1,
        "Short keys without context may be flagged (acceptable trade-off)"
    );
}

#[test]
fn test_long_property_key_without_context() {
    let code = r#"
        package com.example;

        public class Config {
            // 4+ segment property key - strong signal even without @Value
            private static final String KEY = "com.example.internal.service";
        }
    "#;

    let findings = scan_code(code);

    // 4+ segments is a strong signal for property key even without @Value
    assert_eq!(
        findings.len(),
        0,
        "Long property keys (4+ segments) should not be flagged even without context"
    );
}

#[test]
fn test_property_key_with_placeholder() {
    let code = r#"
        package com.example;

        public class Config {
            // Property placeholder syntax
            private String template = "${com.example.internal.service.url}";
        }
    "#;

    let findings = scan_code(code);

    // Should NOT flag strings containing ${...} placeholder syntax
    assert_eq!(
        findings.len(),
        0,
        "Property placeholders should not be flagged"
    );
}

#[test]
fn test_url_with_slash_always_flagged() {
    let code = r#"
        package com.example;

        public class Config {
            // Has slash - definitely a URL, not a property key
            private String url = "service.corp/api";
        }
    "#;

    let findings = scan_code(code);

    // URLs with slashes should always be flagged
    assert_eq!(findings.len(), 1, "URLs with slashes should be flagged");
}

#[test]
fn test_url_with_port_always_flagged() {
    let code = r#"
        package com.example;

        public class Config {
            // Has port - definitely a URL, not a property key
            private String url = "service.local:8080";
        }
    "#;

    let findings = scan_code(code);

    // URLs with ports should always be flagged
    assert_eq!(findings.len(), 1, "URLs with ports should be flagged");
}
