// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JPA/Hibernate security checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for HQL/JPQL injection through string concatenation
pub struct HqlInjectionCheck;

impl Check for HqlInjectionCheck {
    fn name(&self) -> &'static str {
        "hql-injection"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::METHOD_DECL,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Must contain query creation method
        let has_query_creation = node_text.contains("createQuery")
            || node_text.contains("createNativeQuery")
            || node_text.contains("createSQLQuery");

        if !has_query_creation {
            return vec![];
        }

        if !Self::looks_like_query(&node_text) {
            return vec![];
        }

        // Pattern 1: String literal + variable (SELECT ... WHERE x = " + var)
        if Self::has_variable_concat(&node_text) {
            return vec![Self::create_finding(node, context)];
        }

        // Pattern 2: String.format() with query and non-literal args
        if Self::has_variable_format(&node_text) {
            return vec![Self::create_finding(node, context)];
        }

        // Pattern 3: StringBuilder.append() building query with non-literals
        if Self::has_non_literal_append(&node_text) {
            return vec![Self::create_finding(node, context)];
        }

        vec![]
    }
}

impl HqlInjectionCheck {
    fn looks_like_query(node_text: &str) -> bool {
        node_text.contains("SELECT") || node_text.contains("FROM")
    }

    fn has_variable_concat(node_text: &str) -> bool {
        let chars: Vec<char> = node_text.chars().collect();
        for (idx, ch) in chars.iter().enumerate() {
            if *ch != '+' {
                continue;
            }
            let mut left = idx;
            while left > 0 && chars[left - 1].is_whitespace() {
                left -= 1;
            }
            if left == 0 {
                continue;
            }
            let left_char = chars[left - 1];

            let mut right = idx + 1;
            while right < chars.len() && chars[right].is_whitespace() {
                right += 1;
            }
            if right >= chars.len() {
                continue;
            }
            let right_char = chars[right];

            // Skip literal-only concatenation: "foo" + "bar"
            if left_char == '"' && right_char == '"' {
                continue;
            }

            return true;
        }
        false
    }

    fn has_variable_format(node_text: &str) -> bool {
        let format_idx = match node_text.find("String.format") {
            Some(idx) => idx,
            None => return false,
        };

        let after = &node_text[format_idx..];
        if !after.contains("%s") {
            return false;
        }

        let comma_idx = match after.find(',') {
            Some(idx) => idx,
            None => return false,
        };
        let mut idx = comma_idx + 1;
        while idx < after.len() && after.as_bytes()[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= after.len() {
            return false;
        }
        // If the first format arg is a literal, it's likely constant formatting.
        after.as_bytes()[idx] != b'"'
    }

    fn has_non_literal_append(node_text: &str) -> bool {
        let mut remainder = node_text;
        while let Some(idx) = remainder.find(".append(") {
            let after = &remainder[idx + ".append(".len()..];
            let mut i = 0;
            let bytes = after.as_bytes();
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] != b'"' {
                return true;
            }
            remainder = after;
        }
        false
    }

    fn create_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "HQL/JPQL query constructed using string concatenation. This creates SQL injection vulnerabilities even in ORM queries. Attackers can manipulate the query logic to access unauthorized data or bypass access controls.".to_string();

        let mut finding = Finding::new(VulnType::HqlInjection, message, location, code);
        finding.severity = Severity::Critical;
        finding.remediation = "Use parameterized queries: createQuery(\"SELECT u FROM User u WHERE u.name = :name\").setParameter(\"name\", userName)".to_string();
        finding
    }
}

/// Check for unsafe Criteria API usage
pub struct UnsafeCriteriaCheck;

impl Check for UnsafeCriteriaCheck {
    fn name(&self) -> &'static str {
        "unsafe-criteria"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Look for JPA Criteria API with string-based column names from user input
        // This is conservative - we look for Restrictions or CriteriaBuilder with string concat
        for line in node_text.lines() {
            if !Self::looks_like_criteria_line(line) {
                continue;
            }
            if HqlInjectionCheck::has_variable_concat(line)
                || HqlInjectionCheck::has_variable_format(line)
            {
                return vec![Self::create_finding(node, context)];
            }
        }

        vec![]
    }
}

impl UnsafeCriteriaCheck {
    fn looks_like_criteria_line(line: &str) -> bool {
        line.contains("Restrictions.")
            || line.contains("criteriaBuilder.")
            || line.contains("CriteriaBuilder")
            || line.contains("CriteriaQuery")
            || line.contains("Root<")
            || line.contains("root.get(")
            || line.contains("root.join(")
            || line.contains("query.where(")
            || line.contains("query.orderBy(")
            || line.contains("createAlias")
    }

    fn create_finding(node: &SyntaxNode, context: &CheckContext) -> Finding {
        let location = context.node_location(node);
        let code = context.get_code_snippet(node);
        let message = "Criteria API usage with string concatenation. While Criteria API is generally safe, constructing property names or aliases from user input can lead to injection vulnerabilities.".to_string();

        let mut finding = Finding::new(VulnType::HqlInjection, message, location, code);
        finding.severity = Severity::High;
        finding.remediation = "Use type-safe property references from metamodel or validate user input against an allowlist".to_string();
        finding
    }
}
