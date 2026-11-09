// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Insufficient logging detection for security-critical operations
//!
//! Detects authentication, authorization, and sensitive data operations without audit logging.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct InsufficientLoggingCheck;

fn get_method_with_modifiers(node: &SyntaxNode) -> String {
    let method_text = node.text().to_string();

    // Look for MODIFIER_LIST sibling that precedes this METHOD_DECL
    if let Some(parent) = node.parent() {
        let node_range = node.text_range();
        let mut prev_sibling = None;

        for child in parent.children() {
            if child.text_range() == node_range {
                // Found the method node, use the previous sibling if it's MODIFIER_LIST
                break;
            }
            if child.kind() == SyntaxKind::MODIFIER_LIST {
                prev_sibling = Some(child);
            }
        }

        if let Some(modifiers) = prev_sibling {
            return format!("{} {}", modifiers.text(), method_text);
        }
    }

    method_text
}

impl Check for InsufficientLoggingCheck {
    fn name(&self) -> &'static str {
        "insufficient-logging"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = get_method_with_modifiers(node);
        let mut findings = Vec::new();

        // Pattern 1: Authentication methods without logging
        let is_auth_method = text.contains("authenticate(")
            || text.contains("login(")
            || text.contains("signin(")
            || (text.contains("@PostMapping") && text.contains("/login"))
            || (text.contains("@PostMapping") && text.contains("/auth"))
            || text.contains("AuthenticationManager")
            || text.contains("UserDetails");

        if is_auth_method {
            let has_logging = text.contains("logger.")
                || text.contains("log.")
                || text.contains("LOG.")
                || text.contains("LOGGER.")
                || text.contains("auditLog");

            if !has_logging {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Insufficient logging: Authentication operation detected without audit logging. Failed and successful authentication attempts should be logged for security monitoring.".to_string();

                let mut finding =
                    Finding::new(VulnType::InsufficientLogging, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation = "Add audit logging for authentication events. Log username, timestamp, IP address, and success/failure status. Use a dedicated audit logger.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 2: Authorization decisions without logging
        let is_authz_method = text.contains("@PreAuthorize")
            || text.contains("hasRole(")
            || text.contains("hasAuthority(")
            || text.contains("checkPermission(")
            || text.contains("authorize(")
            || text.contains("isAuthorized(")
            || text.contains("canAccess(");

        if is_authz_method {
            let has_logging = text.contains("logger.")
                || text.contains("log.")
                || text.contains("LOG.")
                || text.contains("LOGGER.")
                || text.contains("auditLog");

            // Only flag if this is a sensitive operation (not just read operations)
            let is_sensitive = text.contains("delete")
                || text.contains("update")
                || text.contains("modify")
                || text.contains("create")
                || text.contains("POST")
                || text.contains("PUT")
                || text.contains("DELETE")
                || text.contains("PATCH");

            if !has_logging && is_sensitive {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Insufficient logging: Authorization check detected in sensitive operation without audit logging. Access control decisions should be logged for compliance and security monitoring.".to_string();

                let mut finding =
                    Finding::new(VulnType::InsufficientLogging, message, location, code);
                finding.severity = Severity::Low;
                finding.remediation = "Add audit logging for authorization decisions. Log user identity, requested resource, action, and decision (granted/denied).".to_string();
                findings.push(finding);
            }
        }

        // Pattern 3: Data deletion/modification without audit logging
        let is_delete_operation = (text.contains("@DeleteMapping")
            || text.contains("@PutMapping")
            || text.contains("@PatchMapping"))
            && (text.contains("delete")
                || text.contains("remove")
                || text.contains("update")
                || text.contains("modify"));

        if is_delete_operation {
            let has_logging = text.contains("logger.")
                || text.contains("log.")
                || text.contains("LOG.")
                || text.contains("LOGGER.")
                || text.contains("auditLog");

            let is_user_data = text.contains("User")
                || text.contains("Account")
                || text.contains("Profile")
                || text.contains("Customer")
                || text.contains("Order")
                || text.contains("Payment")
                || text.contains("Document");

            if !has_logging && is_user_data {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Insufficient logging: Data modification/deletion operation without audit logging. Changes to user data should be logged for compliance and forensics.".to_string();

                let mut finding =
                    Finding::new(VulnType::InsufficientLogging, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation = "Add audit logging before deleting or modifying data. Log user identity, affected resource ID, timestamp, and operation type.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 4: Exception handlers without logging (security events)
        let is_exception_handler = text.contains("catch")
            || text.contains("@ExceptionHandler")
            || text.contains("handleException");

        if is_exception_handler {
            let is_security_exception = text.contains("AuthenticationException")
                || text.contains("AccessDeniedException")
                || text.contains("SecurityException")
                || text.contains("UnauthorizedException")
                || text.contains("ForbiddenException");

            let has_logging = text.contains("logger.")
                || text.contains("log.")
                || text.contains("LOG.")
                || text.contains("LOGGER.");

            if is_security_exception && !has_logging {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Insufficient logging: Security exception handler without logging. Security-related exceptions should be logged for intrusion detection and incident response.".to_string();

                let mut finding =
                    Finding::new(VulnType::InsufficientLogging, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation = "Add logging for security exceptions. Include exception details, user context, and request information for security monitoring.".to_string();
                findings.push(finding);
            }
        }

        findings
    }
}
