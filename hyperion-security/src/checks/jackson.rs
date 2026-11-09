// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Jackson deserialization security checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Extract package name from a syntax node by walking up to find PACKAGE_DECL
fn get_package_name(node: &SyntaxNode) -> Option<String> {
    // Walk up to find the root
    let mut root = node.clone();
    while let Some(parent) = root.parent() {
        root = parent;
    }

    // Now we're at root, search for PACKAGE_DECL
    for descendant in root.descendants() {
        if descendant.kind() == SyntaxKind::PACKAGE_DECL {
            // Extract package name from the text
            let text = descendant.text().to_string();
            // Format: "package com.example.foo;"
            // Extract the package name
            let package_name = text
                .trim()
                .strip_prefix("package")
                .and_then(|s| s.trim().strip_suffix(';'))
                .map(|s| s.trim().to_string());
            return package_name;
        }
    }

    None
}

/// Detect if code is in a framework/plugin context where flexible Jackson config is intentional
fn is_framework_plugin_context(node: &SyntaxNode, context: &CheckContext) -> bool {
    let file_path = context.file_path().to_string_lossy();

    // Pattern 1: Package structure indicates framework/API/SPI code
    let package_name = get_package_name(node).unwrap_or_default();
    let package_lower = package_name.to_lowercase();

    // Check for framework package patterns
    // Match .api, .spi, .plugin, .extension in any position
    if package_lower.contains(".api")
        || package_lower.contains(".spi")
        || package_lower.contains(".plugin")
        || package_lower.contains(".extension")
        || package_lower.contains(".framework")
    {
        return true;
    }

    // Match core.config, core.authentication patterns
    if package_lower.contains("core.config") || package_lower.contains("core.authentication") {
        return true;
    }

    // Pattern 2: File path indicates framework code
    let path_lower = file_path.to_lowercase();
    if path_lower.contains("/api/")
        || path_lower.contains("/spi/")
        || path_lower.contains("/plugin/")
        || path_lower.contains("/core/")
        || path_lower.contains("/framework/")
    {
        return true;
    }

    // Pattern 3: Class is an interface or abstract class (plugin extension point)
    let class_text = node.text().to_string();
    if class_text.contains("public interface") || class_text.contains("public abstract class") {
        return true;
    }

    // Pattern 4: Test/example code
    if path_lower.contains("/test/") || path_lower.contains("/example/") {
        return true;
    }

    false
}

/// Check for unsafe Jackson ObjectMapper configuration
pub struct UnsafeJacksonConfigCheck;

impl Check for UnsafeJacksonConfigCheck {
    fn name(&self) -> &'static str {
        "unsafe-jackson-config"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::METHOD_DECL,
            SyntaxKind::CLASS_DECL, // For classes (may include annotation text)
            SyntaxKind::ANNOTATION, // For @JsonTypeInfo annotations
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Check for enableDefaultTyping() - most dangerous
        if node_text.contains("enableDefaultTyping") {
            return vec![Self::create_enable_default_typing_finding(node, context)];
        }

        // Check for activateDefaultTyping() - alternative method name
        if node_text.contains("activateDefaultTyping") {
            return vec![Self::create_enable_default_typing_finding(node, context)];
        }

        // Check for @JsonTypeInfo annotation with unsafe Id types
        if node_text.contains("@JsonTypeInfo") {
            // Check for Id.CLASS or Id.MINIMAL_CLASS
            if node_text.contains("Id.CLASS") || node_text.contains("Id.MINIMAL_CLASS") {
                return vec![Self::create_json_type_info_finding(node, context)];
            }

            // Also check string literal forms
            if node_text.contains("\"CLASS\"") || node_text.contains("\"MINIMAL_CLASS\"") {
                return vec![Self::create_json_type_info_finding(node, context)];
            }
        }

        // Check for setDefaultTyping() - older API
        if node_text.contains("setDefaultTyping") {
            return vec![Self::create_enable_default_typing_finding(node, context)];
        }

        vec![]
    }
}

impl UnsafeJacksonConfigCheck {
    fn create_enable_default_typing_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);

        // Check if this is framework/plugin code
        let in_framework_context = is_framework_plugin_context(node, context);

        if in_framework_context {
            // Lower severity - may be intentional for extensibility
            let message = "ObjectMapper.enableDefaultTyping() detected in framework/plugin code. \
                While this may be intentional for extensibility, this feature is disabled by default \
                in Jackson 2.10+ due to security risks. Verify that PolymorphicTypeValidator is configured.".to_string();

            let mut finding = Finding::new(
                VulnType::InsecureDeserializationConfig,
                message,
                location,
                code,
            );
            finding.severity = Severity::Medium; // Downgrade from Critical
            finding.remediation = "If this is intentional plugin architecture, configure PolymorphicTypeValidator to restrict allowed classes. Otherwise, use explicit @JsonTypeInfo with Id.NAME and allowlist.".to_string();
            finding
        } else {
            // Application code - keep CRITICAL severity
            let message = "ObjectMapper.enableDefaultTyping() enables polymorphic deserialization, which allows attackers to instantiate arbitrary classes. This can lead to Remote Code Execution (RCE) through gadget chains. This feature is disabled by default in Jackson 2.10+ due to security risks.".to_string();

            let mut finding = Finding::new(
                VulnType::InsecureDeserializationConfig,
                message,
                location,
                code,
            );
            finding.severity = Severity::Critical;
            finding.remediation = "Remove enableDefaultTyping(). If polymorphic types are needed, use explicit @JsonTypeInfo with MINIMAL_CLASS or NAME, and configure an allowlist using PolymorphicTypeValidator".to_string();
            finding
        }
    }

    fn create_json_type_info_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);

        // Check if this is framework/plugin code
        let in_framework_context = is_framework_plugin_context(node, context);

        if in_framework_context {
            // Lower severity - may be intentional for extensibility
            let message = "Flexible Jackson deserialization detected in framework/plugin code. \
                While this may be intentional for extensibility, verify that proper \
                PolymorphicTypeValidator is configured to prevent arbitrary class instantiation."
                .to_string();

            let mut finding = Finding::new(
                VulnType::InsecureDeserializationConfig,
                message,
                location,
                code,
            );
            finding.severity = Severity::Medium; // Downgrade from Critical
            finding.remediation = "If this is intentional plugin architecture, configure PolymorphicTypeValidator to restrict allowed classes. Otherwise, use @JsonTypeInfo with Id.NAME and explicit subtypes.".to_string();
            finding
        } else {
            // Application code - keep CRITICAL severity
            let message = "@JsonTypeInfo with Id.CLASS or Id.MINIMAL_CLASS enables unsafe polymorphic deserialization. Attackers can specify arbitrary classes in JSON payloads, potentially leading to Remote Code Execution.".to_string();

            let mut finding = Finding::new(
                VulnType::InsecureDeserializationConfig,
                message,
                location,
                code,
            );
            finding.severity = Severity::Critical;
            finding.remediation = "Use @JsonTypeInfo with Id.NAME and explicitly list allowed subtypes, or use a PolymorphicTypeValidator to restrict allowed classes".to_string();
            finding
        }
    }
}
