// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JWT misuse checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct JwtHeaderMisuseCheck;
pub struct WeakJwtSecretCheck;

impl Check for JwtHeaderMisuseCheck {
    fn name(&self) -> &'static str {
        "jwt-header-misuse"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        let uses_jku = text.contains("getHeaderClaim(\"jku\")")
            || text.contains("getHeaderClaim(\"kid\")")
            || text.contains("getHeaderClaim(\"x5u\")");
        let uses_jwk_fetch = text.contains("JwkProviderBuilder")
            || text.contains("JwkProvider")
            || text.contains("new URL(")
            || text.contains("URL(");

        if !(uses_jku && uses_jwk_fetch) {
            return Vec::new();
        }

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "JWT verification depends on untrusted header values (jku/kid/x5u) to fetch keys. This can allow attacker-controlled key selection or remote JWK fetching.".to_string();
        let mut finding = Finding::new(VulnType::JwtMisuse, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Pin JWKS URLs and validate header values against an allowlist; do not fetch keys based on untrusted token headers.".to_string();
        vec![finding]
    }
}

impl Check for WeakJwtSecretCheck {
    fn name(&self) -> &'static str {
        "jwt-weak-secret"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        if !text.contains("JWT_SECRET") || !text.contains("signWith") {
            return Vec::new();
        }

        let has_small_secret_list = text.contains("String[] SECRETS")
            && text.contains("SECRETS[")
            && text.contains("TextCodec.BASE64");
        let uses_literal = has_short_literal(&text);

        if !(has_small_secret_list || uses_literal) {
            return Vec::new();
        }

        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "JWT signing secret appears to be hardcoded and low entropy. This enables offline guessing and token forgery.".to_string();
        let mut finding = Finding::new(VulnType::HardcodedSecret, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Use a high-entropy secret from environment variables or a secrets manager, and rotate regularly.".to_string();
        vec![finding]
    }
}

fn has_short_literal(text: &str) -> bool {
    let mut in_string = false;
    let mut current = String::new();
    let mut chars = text.chars();

    while let Some(ch) = chars.next() {
        if ch == '"' {
            if in_string {
                if !current.is_empty() && current.len() <= 12 {
                    return true;
                }
                current.clear();
                in_string = false;
            } else {
                in_string = true;
            }
            continue;
        }

        if in_string {
            if ch == '\\' {
                // Skip escaped character
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            } else {
                current.push(ch);
            }
        }
    }

    false
}
