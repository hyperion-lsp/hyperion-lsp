// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Missing access control / IDOR checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct MissingAccessControlCheck;

impl Check for MissingAccessControlCheck {
    fn name(&self) -> &'static str {
        "missing-access-control"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();

        let is_endpoint = text.contains("@GetMapping")
            || text.contains("@PostMapping")
            || text.contains("@PutMapping")
            || text.contains("@DeleteMapping")
            || text.contains("@PatchMapping")
            || text.contains("@RequestMapping");
        if !is_endpoint {
            return Vec::new();
        }

        let has_authz = text.contains("@PreAuthorize")
            || text.contains("@Secured")
            || text.contains("@RolesAllowed")
            || text.contains("hasRole(")
            || text.contains("hasAuthority(")
            || text.contains("authorizeHttpRequests");
        if has_authz {
            return Vec::new();
        }

        let has_id_param = text.contains("@PathVariable") || text.contains("@RequestParam");
        let id_like = text.contains("userId")
            || text.contains("accountId")
            || text.contains("profileId")
            || text.contains("ownerId")
            || text.contains("id)")
            || text.contains(" id ");
        let accesses_object = text.contains("findById(")
            || text.contains("getById(")
            || text.contains("new UserProfile(")
            || text.contains("getProfile(")
            || text.contains("loadProfile(");

        if !(has_id_param && id_like && accesses_object) {
            return Vec::new();
        }

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "Potential IDOR: endpoint uses a user-controlled identifier to access an object without an explicit authorization check.".to_string();
        let mut finding = Finding::new(VulnType::MissingAccessControl, message, location, code);
        finding.severity = Severity::High;
        finding.remediation =
            "Verify ownership or roles before returning objects addressed by user-controlled IDs."
                .to_string();
        vec![finding]
    }
}
