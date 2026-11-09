// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! XML parser configuration checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct UnsafeXmlParserConfigCheck;

impl Check for UnsafeXmlParserConfigCheck {
    fn name(&self) -> &'static str {
        "unsafe-xml-parser-config"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        let mut findings = Vec::new();

        // Pattern 1: XML parser creation without security configuration
        let creates_parser = text.contains("XMLInputFactory.newInstance")
            || text.contains("DocumentBuilderFactory.newInstance")
            || text.contains("SAXParserFactory.newInstance")
            || text.contains("TransformerFactory.newInstance")
            || text.contains("SchemaFactory.newInstance")
            || text.contains("new XMLReader")
            || text.contains("new SAXParser");

        if creates_parser {
            let has_hardening = text.contains("ACCESS_EXTERNAL_DTD")
                || text.contains("ACCESS_EXTERNAL_SCHEMA")
                || text.contains("FEATURE_SECURE_PROCESSING")
                || text.contains("disallow-doctype-decl")
                || text.contains("load-external-dtd")
                || text.contains("external-general-entities")
                || text.contains("external-parameter-entities")
                || text.contains("setFeature(\"http://");

            if !has_hardening {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "XML parser is created without disabling external entities or enabling secure processing. This can enable XXE attacks allowing attackers to read files, cause DoS, or perform SSRF.".to_string();
                let mut finding = Finding::new(VulnType::XxeInjection, message, location, code);
                finding.severity = Severity::High;
                finding.remediation = "Disable external entities by setting these features: setFeature(\"http://apache.org/xml/features/disallow-doctype-decl\", true), setFeature(\"http://xml.org/sax/features/external-general-entities\", false), and setAttribute(XMLConstants.ACCESS_EXTERNAL_DTD, \"\").".to_string();
                findings.push(finding);
            }
        }

        // Pattern 2: XPath evaluation with user input (XPath injection)
        let uses_xpath = text.contains("XPath")
            || text.contains("xpath.evaluate")
            || text.contains("xpath.compile");

        if uses_xpath {
            // Check if user input is used in XPath expression
            let has_user_input = text.contains("request.getParameter")
                || text.contains("@PathVariable")
                || text.contains("@RequestParam")
                || text.contains("@RequestBody")
                || text.contains("userInput")
                || text.contains("input")
                || text.contains("concat(")
                || text.contains("\"+\"")
                || text.contains("\" + ");

            let has_parameterization = text.contains("setXPathVariableResolver")
                || text.contains("setVariable")
                || text.contains("SimpleVariableResolver");

            if has_user_input && !has_parameterization {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "XPath injection: User input is used in XPath expression without parameterization. This allows attackers to manipulate query logic and access unauthorized data.".to_string();
                let mut finding = Finding::new(VulnType::XxeInjection, message, location, code);
                finding.severity = Severity::High;
                finding.remediation =
                    "Use XPathVariableResolver to parameterize XPath queries instead of string concatenation. Validate and sanitize all user input.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 3: XSLT transformation with user input
        let uses_xslt = text.contains("TransformerFactory")
            || text.contains("Transformer.transform")
            || text.contains("Templates")
            || text.contains("newTransformer");

        if uses_xslt {
            let has_user_input = text.contains("request.getParameter")
                || text.contains("@PathVariable")
                || text.contains("@RequestParam")
                || text.contains("@RequestBody")
                || text.contains("userInput")
                || text.contains("StreamSource");

            let has_hardening = text.contains("ACCESS_EXTERNAL_DTD")
                || text.contains("ACCESS_EXTERNAL_STYLESHEET")
                || text.contains("FEATURE_SECURE_PROCESSING");

            if has_user_input && !has_hardening {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "XSLT injection: User input used in XSLT transformation without security restrictions. This can lead to XXE, SSRF, or arbitrary code execution.".to_string();
                let mut finding = Finding::new(VulnType::XxeInjection, message, location, code);
                finding.severity = Severity::Critical;
                finding.remediation = "Restrict external access by setting ACCESS_EXTERNAL_DTD and ACCESS_EXTERNAL_STYLESHEET to empty string. Enable FEATURE_SECURE_PROCESSING. Never use untrusted input as XSLT stylesheets.".to_string();
                findings.push(finding);
            }
        }

        findings
    }
}
