// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Mass assignment vulnerability detection
//!
//! Detects when untrusted data is bound directly to model objects without
//! proper field filtering, allowing attackers to modify unintended fields.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for mass assignment vulnerabilities (CWE-915)
///
/// Detects:
/// - @ModelAttribute without @InitBinder protection
/// - BeanUtils.copyProperties() / populate() with untrusted data
/// - Spring Data REST auto-exposure without proper configuration
pub struct MassAssignmentCheck;

impl MassAssignmentCheck {
    /// Dangerous bean utility methods that copy properties
    const DANGEROUS_METHODS: &'static [&'static str] =
        &["copyProperties", "populate", "setProperties"];

    /// Check if a method has @ModelAttribute annotation without corresponding @InitBinder
    ///
    /// This is a simplified heuristic - we check if @ModelAttribute is present
    /// and if there's no obvious field filtering in the same class
    fn has_unprotected_model_attribute(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Check for @ModelAttribute annotation
        if !text.contains("@ModelAttribute") {
            return false;
        }

        // Walk up to the class level
        let class_node = Self::find_containing_class(node);

        if let Some(class) = class_node {
            let class_text = class.text().to_string();

            // Check if there's an @InitBinder in the same class
            // This is a heuristic - in real analysis we'd need to verify the binder
            // actually applies to this parameter
            if class_text.contains("@InitBinder") {
                return false;
            }

            // Also check for allowedFields configuration
            if class_text.contains("setAllowedFields") || class_text.contains("setDisallowedFields")
            {
                return false;
            }
        }

        true
    }

    /// Find the containing class declaration
    fn find_containing_class(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::CLASS_DECL {
                return Some(parent);
            }
            current = parent.parent();
        }
        None
    }

    /// Check if this is a repository with @RepositoryRestResource
    /// that might expose all fields by default
    fn has_unsafe_repository_rest_resource(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Check for @RepositoryRestResource without exported=false
        if text.contains("@RepositoryRestResource") {
            // If it explicitly sets exported=false, it's safe
            if text.contains("exported = false") || text.contains("exported=false") {
                return false;
            }

            // Otherwise, it might be exposing all fields
            return true;
        }

        false
    }
}

impl Check for MassAssignmentCheck {
    fn name(&self) -> &'static str {
        "mass-assignment"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::METHOD_DECL,      // For @ModelAttribute methods
            SyntaxKind::METHOD_CALL_EXPR, // For BeanUtils calls
            SyntaxKind::EXPR_STMT,        // For statement-level BeanUtils calls
            SyntaxKind::CLASS_DECL,       // For @RepositoryRestResource
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        match node.kind() {
            SyntaxKind::EXPR_STMT | SyntaxKind::METHOD_CALL_EXPR => {
                let text = node.text().to_string();

                // Check for BeanUtils dangerous methods
                for method in Self::DANGEROUS_METHODS {
                    if text.contains(&format!("BeanUtils.{method}("))
                        || text.contains(&format!("PropertyUtils.{method}("))
                    {
                        let location = context.node_location(node);
                        let code = context.get_code_snippet(node);
                        let message = format!(
                            "Mass assignment vulnerability: BeanUtils.{method}() copies all properties \
                            from source to target. If the source contains user input, an attacker can \
                            modify unintended fields."
                        );

                        let mut finding =
                            Finding::new(VulnType::MassAssignment, message, location, code);
                        finding.severity = Severity::High;
                        finding.remediation =
                            "Manually copy only the safe fields, or use a DTO with allowlisted fields only."
                                .to_string();
                        findings.push(finding);
                        break;
                    }
                }
            }

            SyntaxKind::METHOD_DECL if Self::has_unprotected_model_attribute(node) => {
                // Check for @ModelAttribute without @InitBinder
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Mass assignment vulnerability: @ModelAttribute without field filtering. \
                        An attacker can bind arbitrary request parameters to model fields, potentially \
                        modifying sensitive fields like 'isAdmin', 'role', or 'accountBalance'."
                    .to_string();

                let mut finding = Finding::new(VulnType::MassAssignment, message, location, code);
                finding.severity = Severity::High;
                finding.remediation =
                    "Add @InitBinder with setAllowedFields() to allowlist safe fields, \
                        or use a separate DTO class with only the fields that should be user-modifiable."
                        .to_string();
                findings.push(finding);
            }

            SyntaxKind::CLASS_DECL
                // Check for unsafe @RepositoryRestResource
                if Self::has_unsafe_repository_rest_resource(node) => {
                    let location = context.node_location(node);
                    let code = context.get_code_snippet(node);
                    let message = "Mass assignment risk: @RepositoryRestResource exposes repository methods via REST. \
                        By default, Spring Data REST allows clients to modify all entity fields, including \
                        sensitive ones like 'role' or 'isActive'."
                        .to_string();

                    let mut finding =
                        Finding::new(VulnType::MassAssignment, message, location, code);
                    finding.severity = Severity::High;
                    finding.remediation =
                        "Use @JsonIgnore on sensitive fields, configure excerptProjection, \
                        or set exported=false and implement custom controllers with field filtering."
                            .to_string();
                    findings.push(finding);
                }

            _ => {}
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_methods() {
        assert!(MassAssignmentCheck::DANGEROUS_METHODS.contains(&"copyProperties"));
        assert!(MassAssignmentCheck::DANGEROUS_METHODS.contains(&"populate"));
    }
}
