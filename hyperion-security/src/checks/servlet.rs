// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Servlet security checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for insecure cookie settings
pub struct InsecureCookieCheck;

impl Check for InsecureCookieCheck {
    fn name(&self) -> &'static str {
        "insecure-cookie"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        // Check EXPR_STMT for setSecure(false)/setHttpOnly(false) patterns
        &[SyntaxKind::EXPR_STMT]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Check for setSecure(false) - explicitly disabling secure flag
        if node_text.contains("setSecure") && node_text.contains("false") {
            return vec![Self::create_secure_flag_finding(node, context)];
        }

        // Check for setHttpOnly(false) - explicitly disabling httpOnly flag
        if node_text.contains("setHttpOnly") && node_text.contains("false") {
            return vec![Self::create_httponly_flag_finding(node, context)];
        }

        vec![]
    }
}

impl InsecureCookieCheck {
    fn create_secure_flag_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "Cookie Secure flag explicitly set to false. Cookies will be transmitted over unencrypted HTTP connections, exposing them to network sniffing and man-in-the-middle attacks. Session cookies sent over HTTP can be stolen by attackers on the network.".to_string();

        let mut finding = Finding::new(VulnType::InsecureCookie, message, location, code);
        finding.severity = Severity::Medium;
        finding.remediation =
            "Set the Secure flag: cookie.setSecure(true) to ensure cookies are only sent over HTTPS"
                .to_string();
        finding
    }

    fn create_httponly_flag_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "Cookie HttpOnly flag explicitly set to false. Cookies will be accessible to JavaScript code via document.cookie. This enables XSS attacks to steal session cookies and compromise user accounts.".to_string();

        let mut finding = Finding::new(VulnType::InsecureCookie, message, location, code);
        finding.severity = Severity::Medium;
        finding.remediation = "Set the HttpOnly flag: cookie.setHttpOnly(true) to prevent JavaScript access to cookies".to_string();
        finding
    }
}

/// Check for missing security headers
pub struct MissingSecurityHeadersCheck;

impl Check for MissingSecurityHeadersCheck {
    fn name(&self) -> &'static str {
        "missing-security-headers"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL, SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();
        let source_text = context.source;

        if !Self::is_server_response_handler(source_text, &node_text) {
            return vec![];
        }

        // Look for methods that set headers but are missing security headers
        // This is a heuristic check - we look for setHeader/addHeader usage
        if !node_text.contains("setHeader") && !node_text.contains("addHeader") {
            return vec![];
        }

        // Check if response handling method
        if !node_text.contains("HttpServletResponse") && !node_text.contains("response") {
            return vec![];
        }

        let mut findings = Vec::new();

        // Check for missing X-Frame-Options
        if !node_text.contains("X-Frame-Options")
            && !node_text.contains("frameOptions")
            && node_text.contains("setHeader")
        {
            findings.push(Self::create_missing_header_finding(
                node,
                context,
                "X-Frame-Options",
                "Prevents clickjacking attacks by controlling iframe embedding",
            ));
        }

        findings
    }
}

impl MissingSecurityHeadersCheck {
    fn is_server_response_handler(source: &str, node_text: &str) -> bool {
        let strong_server_markers = [
            "@RestController",
            "@Controller",
            "@Path",
            "@WebServlet",
            "ServerResponse",
            "ContainerResponseFilter",
            "ResponseEntity",
            "@RequestMapping",
            "@GetMapping",
            "@PostMapping",
            "@PutMapping",
            "@DeleteMapping",
            "@PatchMapping",
        ];

        let has_strong_marker = strong_server_markers
            .iter()
            .any(|marker| source.contains(marker) || node_text.contains(marker));

        let has_response_filter = (source.contains("ContainerResponseFilter")
            || node_text.contains("ContainerResponseFilter"))
            && (source.contains("@Provider") || node_text.contains("@Provider"));

        let client_markers = [
            "HttpClient",
            "CloseableHttpClient",
            "HttpResponse",
            "HttpRequest",
            "OkHttpClient",
            "RestTemplate",
            "WebClient",
            "FeignClient",
            "Retrofit",
            "ClientBuilder",
        ];

        let has_server_marker = has_strong_marker || has_response_filter;

        if !has_server_marker {
            return false;
        }

        if client_markers
            .iter()
            .any(|marker| source.contains(marker) || node_text.contains(marker))
        {
            return has_server_marker;
        }

        has_server_marker
    }

    fn create_missing_header_finding(
        node: &SyntaxNode,
        context: &CheckContext,
        header: &str,
        description: &str,
    ) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = format!(
            "Response sets headers but is missing '{header}' security header. {description}"
        );

        let mut finding = Finding::new(VulnType::MissingSecurityHeader, message, location, code);
        finding.severity = Severity::Low;
        finding.remediation = format!(
            "Add the '{header}' header to the response using Spring Security's headers() configuration or response.setHeader()"
        );
        finding
    }
}
