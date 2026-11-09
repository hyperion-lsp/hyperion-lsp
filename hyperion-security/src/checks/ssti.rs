// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Server-Side Template Injection (SSTI) detection
//!
//! Detects unsafe template construction with user input.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct SstiCheck;

impl SstiCheck {
    /// Check if text contains a variable declaration pattern like "String input"
    /// where the variable name is exactly the given name (not a prefix)
    fn has_variable_pattern(text: &str, var_type: &str, var_name: &str) -> bool {
        let pattern = format!("{var_type} {var_name}");

        // Find all occurrences of the pattern
        for (idx, _) in text.match_indices(&pattern) {
            // Check what follows the pattern
            let after_idx = idx + pattern.len();
            if after_idx >= text.len() {
                return true; // Pattern at end of text
            }

            // Get the character after the pattern
            if let Some(next_char) = text[after_idx..].chars().next() {
                // Valid word boundary characters for variable declarations
                if !next_char.is_alphanumeric() && next_char != '_' {
                    return true; // Found valid pattern like "String input," or "String input)"
                }
                // Otherwise it's something like "String inputData" - not a match
            }
        }
        false
    }
}

impl Check for SstiCheck {
    fn name(&self) -> &'static str {
        "ssti"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        let mut findings = Vec::new();

        // Check if user input is present
        // Use precise patterns to avoid FPs like "String templateName" matching "String template"
        let has_user_input = text.contains("request.getParameter")
            || text.contains("@PathVariable")
            || text.contains("@RequestParam")
            || text.contains("@RequestBody")
            || text.contains("userInput")
            || text.contains("userTemplate")
            || text.contains("userExpression")
            || Self::has_variable_pattern(&text, "String", "input")
            || Self::has_variable_pattern(&text, "String", "template")
            || Self::has_variable_pattern(&text, "String", "expression");

        if !has_user_input {
            return findings;
        }

        // Pattern 1: Thymeleaf template construction with user input
        let uses_thymeleaf = text.contains("th:text")
            || text.contains("th:utext")
            || text.contains("TemplateEngine")
            || text.contains("SpringTemplateEngine")
            || text.contains("ThymeleafViewResolver");

        if uses_thymeleaf {
            // Check for dynamic template construction
            let dynamic_template = text.contains("\"${\"")
                || text.contains("\" + ")
                || text.contains("concat(")
                || text.contains("String.format")
                || (text.contains("th:utext") && text.contains("${")); // th:utext is unescaped

            if dynamic_template {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Server-Side Template Injection (Thymeleaf): User input is used to construct template expressions. This allows attackers to execute arbitrary code on the server. Note: This is pattern-based detection. Verify the input source is actually user-controlled.".to_string();

                let mut finding =
                    Finding::new(VulnType::TemplateInjection, message, location, code);
                finding.severity = Severity::High;
                finding.confidence = 0.50;
                finding.remediation = "Never construct template expressions from user input. Use th:text (auto-escaped) instead of th:utext. Pass user data through model attributes, not template strings.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 2: FreeMarker template construction
        let uses_freemarker = text.contains("FreeMarker")
            || text.contains("Configuration")
            || text.contains(".ftl")
            || text.contains("Template.process")
            || text.contains("getTemplate(");

        if uses_freemarker {
            let dynamic_template = text.contains("\"${\"")
                || text.contains("\" + ")
                || text.contains("concat(")
                || text.contains("StringTemplateLoader")
                || (text.contains("getTemplate(")
                    && (text.contains("userTemplate") || text.contains("String template")));

            if dynamic_template {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Server-Side Template Injection (FreeMarker): User input is used to construct template expressions. Attackers can execute arbitrary code through FreeMarker built-ins. Note: This is pattern-based detection. Verify the input source is actually user-controlled.".to_string();

                let mut finding =
                    Finding::new(VulnType::TemplateInjection, message, location, code);
                finding.severity = Severity::High;
                finding.confidence = 0.50;
                finding.remediation = "Use pre-defined templates from trusted sources. Pass user data through data model, not template strings. Configure FreeMarker with sandbox restrictions.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 3: Velocity template construction
        let uses_velocity = text.contains("Velocity")
            || text.contains("VelocityEngine")
            || text.contains(".vm")
            || text.contains("VelocityContext");

        if uses_velocity {
            let dynamic_template = text.contains("\"$\"")
                || text.contains("\"#set\"")
                || text.contains("\" + ")
                || text.contains("evaluate(");

            if dynamic_template {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Server-Side Template Injection (Velocity): User input is used to construct template expressions. This can lead to remote code execution. Note: This is pattern-based detection. Verify the input source is actually user-controlled.".to_string();

                let mut finding =
                    Finding::new(VulnType::TemplateInjection, message, location, code);
                finding.severity = Severity::High;
                finding.confidence = 0.50;
                finding.remediation = "Use static templates only. Pass user data through VelocityContext, not template strings. Avoid Velocity.evaluate() with user input.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 4: JSP Expression Language injection
        let uses_jsp_el = text.contains("${")
            || text.contains("pageContext.getELContext")
            || text.contains("ExpressionFactory")
            || text.contains("ValueExpression");

        if uses_jsp_el {
            let dynamic_expression = (text.contains("\"${\"") && text.contains("\" + "))
                || text.contains("createValueExpression")
                || text.contains("evaluateExpression");

            if dynamic_expression {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Expression Language Injection: User input is used in JSP EL expression. This can lead to arbitrary code execution through EL built-in functions. Note: This is pattern-based detection. Verify the input source is actually user-controlled.".to_string();

                let mut finding = Finding::new(
                    VulnType::ExpressionLanguageInjection,
                    message,
                    location,
                    code,
                );
                finding.severity = Severity::High;
                finding.confidence = 0.50;
                finding.remediation = "Never construct EL expressions from user input. Use parameterized expressions or pass data through request attributes.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 5: Mustache/Handlebars template construction
        let uses_mustache = text.contains("Mustache")
            || text.contains("Handlebars")
            || text.contains("{{")
            || text.contains("MustacheFactory");

        if uses_mustache {
            let dynamic_template =
                text.contains("\"{{\"") || text.contains("\" + ") || text.contains("compile(");

            if dynamic_template {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Server-Side Template Injection (Mustache/Handlebars): User input is used to construct template. While less dangerous than other engines, this can still lead to information disclosure.".to_string();

                let mut finding =
                    Finding::new(VulnType::TemplateInjection, message, location, code);
                finding.severity = Severity::High;
                finding.remediation = "Use static templates. Pass user data through context objects, not template strings.".to_string();
                findings.push(finding);
            }
        }

        findings
    }
}
