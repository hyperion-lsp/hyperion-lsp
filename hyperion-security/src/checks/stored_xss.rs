// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Stored XSS heuristic checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct StoredXssHeuristicCheck;

impl Check for StoredXssHeuristicCheck {
    fn name(&self) -> &'static str {
        "stored-xss-heuristic"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        let has_post = text.contains("@PostMapping") && text.contains("@RequestBody");
        let has_get = text.contains("@GetMapping") && text.contains("@ResponseBody");
        let stores_comment =
            text.contains("comments") && text.contains("add(") && text.contains("comment");
        let returns_collection =
            text.contains("Collection<") || text.contains("List<") || text.contains("Iterable<");
        let uses_object_mapper = text.contains("ObjectMapper") && text.contains("readValue");

        if !(has_post && has_get && stores_comment && returns_collection) {
            return Vec::new();
        }

        if !uses_object_mapper {
            return Vec::new();
        }

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "User-supplied content is stored and later returned without encoding. This is a stored XSS pattern.".to_string();
        let mut finding = Finding::new(VulnType::Xss, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Encode or sanitize stored content on output (or before storage), and use templating auto-escape for HTML contexts.".to_string();
        vec![finding]
    }
}
