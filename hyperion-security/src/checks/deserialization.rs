// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Unsafe deserialization checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct UnsafeObjectInputStreamCheck;

impl Check for UnsafeObjectInputStreamCheck {
    fn name(&self) -> &'static str {
        "unsafe-objectinputstream"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        if !(text.contains("ObjectInputStream") && text.contains("readObject")) {
            return Vec::new();
        }

        let has_untrusted_source = text.contains("@RequestParam")
            || text.contains("@RequestBody")
            || text.contains("@PathVariable")
            || text.contains("getParameter(")
            || text.contains("getInputStream(")
            || text.contains("getReader(")
            || text.contains("getHeader(");

        if !has_untrusted_source {
            return Vec::new();
        }

        let has_filter = text.contains("ObjectInputFilter")
            || text.contains("setObjectInputFilter")
            || text.contains("resolveClass");
        if has_filter {
            return Vec::new();
        }

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "ObjectInputStream.readObject() deserializes potentially untrusted data without a filter or allowlist.".to_string();
        let mut finding = Finding::new(VulnType::UnsafeDeserialization, message, location, code);
        finding.severity = Severity::Critical;
        finding.remediation = "Avoid Java native serialization or configure ObjectInputFilter/allowlists to restrict classes.".to_string();
        vec![finding]
    }
}
