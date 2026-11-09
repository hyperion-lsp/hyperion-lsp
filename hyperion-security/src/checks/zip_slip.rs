// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Zip Slip detection

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct ZipSlipCheck;

impl Check for ZipSlipCheck {
    fn name(&self) -> &'static str {
        "zip-slip"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();

        let uses_zip = text.contains("ZipFile") || text.contains("ZipInputStream");
        let uses_entry_name = text.contains("getName()") || text.contains("ZipEntry");
        let writes_file = text.contains("Files.copy")
            || text.contains("FileOutputStream")
            || text.contains("Files.write")
            || text.contains("Files.newOutputStream")
            || text.contains("FileWriter");

        if !(uses_zip && uses_entry_name && writes_file) {
            return Vec::new();
        }

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        // This method-level pattern cannot establish whether a normalization or
        // containment check protects the actual write. Keep the finding for
        // review instead of treating any such call in the method as proof.
        let message = "A ZIP entry name may reach a filesystem write. Verify that the written path is normalized and confined to the extraction directory to prevent Zip Slip.".to_string();
        let mut finding = Finding::new(VulnType::PathTraversal, message, location, code);
        finding.severity = Severity::High;
        finding.remediation.push_str(
            " Ensure ZipEntry paths are normalized and confined to the target directory.",
        );
        vec![finding]
    }
}
