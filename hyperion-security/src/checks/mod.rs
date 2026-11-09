// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Pattern-based security checks (non-taint)
//!
//! These checks analyze AST patterns without data flow tracking.
//! They detect issues like weak cryptography, insecure authentication,
//! dangerous APIs, and unsafe reflection usage.

use crate::finding::{Finding, Location};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::path::Path;
use std::time::Instant;

/// A pattern-based security check
pub trait Check: Send + Sync {
    /// Name of the check
    fn name(&self) -> &'static str;

    /// Check a syntax node and return findings
    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding>;

    /// Which node kinds this check applies to
    fn target_kinds(&self) -> &[SyntaxKind];
}

/// Context for pattern checks
pub struct CheckContext<'a> {
    pub file: &'a Path,
    pub source: &'a str,
    /// Once this instant has passed, `check_tree` starts no more checks.
    deadline: Option<Instant>,
}

impl<'a> CheckContext<'a> {
    pub const fn new(file: &'a Path, source: &'a str) -> Self {
        Self {
            file,
            source,
            deadline: None,
        }
    }

    /// Stop running checks once `deadline` has passed. The scan's partial result is then
    /// not usable, so the caller must treat the file as timed out.
    pub const fn with_deadline(mut self, deadline: Option<Instant>) -> Self {
        self.deadline = deadline;
        self
    }

    /// Whether the deadline set by [`Self::with_deadline`] has passed.
    pub fn deadline_passed(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Get the file path for this context
    pub const fn file_path(&self) -> &Path {
        self.file
    }

    /// Convert a syntax node to a Location
    pub fn node_location(&self, node: &SyntaxNode) -> Location {
        self.range_location(node.text_range())
    }

    /// Convert a syntax range to a Location, including individual declaration names.
    pub fn range_location(&self, range: rowan::TextRange) -> Location {
        let start_offset = usize::from(range.start());
        let end_offset = usize::from(range.end());

        let (start_line, start_col) = self.offset_to_line_col(start_offset);
        let (end_line, end_col) = self.offset_to_line_col(end_offset);

        Location {
            file: self.file.to_path_buf(),
            line: start_line,
            column: start_col,
            end_line,
            end_column: end_col,
        }
    }

    fn offset_to_line_col(&self, offset: usize) -> (u32, u32) {
        let mut line = 1u32;
        let mut col = 1u32;
        let mut skip_next_lf = false;

        for (idx, ch) in self.source.char_indices() {
            if idx >= offset {
                break;
            }
            if skip_next_lf {
                skip_next_lf = false;
                if ch == '\n' {
                    continue;
                }
            }
            if ch == '\r' {
                if self.source.as_bytes().get(idx + 1) == Some(&b'\n') {
                    skip_next_lf = true;
                }
                line += 1;
                col = 1;
                continue;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
                continue;
            }

            col += 1;
        }

        (line, col)
    }

    /// Get code snippet for a node
    pub fn get_code_snippet(&self, node: &SyntaxNode) -> String {
        node.text().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_line_col_crlf() {
        let source = "a\r\nb";
        let ctx = CheckContext::new(std::path::Path::new("/test.java"), source);
        let (line, col) = ctx.offset_to_line_col(3);
        assert_eq!((line, col), (2, 1));
    }
}

/// Registry of pattern checks
pub struct CheckRegistry {
    checks: Vec<Box<dyn Check>>,
}

impl CheckRegistry {
    pub fn new() -> Self {
        let mut registry = Self { checks: Vec::new() };

        // Register all checks
        registry.register_crypto_checks();
        registry.register_auth_checks();
        registry.register_dangerous_api_checks();
        registry.register_reflection_checks();
        registry.register_framework_checks();
        registry.register_additional_checks();
        registry.register_phase9_checks();
        registry.register_phase2_tier2_checks();
        registry.register_phase3_tier3_checks();
        registry.register_phase4_checks();

        registry
    }

    fn register_crypto_checks(&mut self) {
        use crate::checks::crypto::{
            EcbModeCheck, HardcodedIvCheck, InsecureRandomCheck, InsecureTlsCheck,
            SmallKeySizeCheck, WeakHashCheck,
        };
        self.checks.push(Box::new(EcbModeCheck));
        self.checks.push(Box::new(HardcodedIvCheck));
        self.checks.push(Box::new(WeakHashCheck));
        self.checks.push(Box::new(SmallKeySizeCheck));
        self.checks.push(Box::new(InsecureRandomCheck));
        self.checks.push(Box::new(InsecureTlsCheck));
    }

    fn register_auth_checks(&mut self) {
        use crate::checks::auth::{EmptyPasswordCheck, HardcodedCredentialsCheck};
        self.checks.push(Box::new(EmptyPasswordCheck));
        self.checks.push(Box::new(HardcodedCredentialsCheck));
    }

    fn register_dangerous_api_checks(&mut self) {
        use crate::checks::dangerous_api::{RuntimeHaltCheck, SystemExitCheck, ThreadStopCheck};
        self.checks.push(Box::new(ThreadStopCheck));
        self.checks.push(Box::new(SystemExitCheck));
        self.checks.push(Box::new(RuntimeHaltCheck));
    }

    fn register_reflection_checks(&mut self) {
        use crate::checks::reflection::UnsafeReflectionCheck;
        self.checks.push(Box::new(UnsafeReflectionCheck));
    }

    fn register_framework_checks(&mut self) {
        use crate::checks::spring_security::{
            CsrfDisabledCheck, PermitAllCheck, WeakPasswordEncoderCheck,
        };
        self.checks.push(Box::new(CsrfDisabledCheck));
        self.checks.push(Box::new(PermitAllCheck));
        self.checks.push(Box::new(WeakPasswordEncoderCheck));

        use crate::checks::jackson::UnsafeJacksonConfigCheck;
        self.checks.push(Box::new(UnsafeJacksonConfigCheck));

        use crate::checks::servlet::{InsecureCookieCheck, MissingSecurityHeadersCheck};
        self.checks.push(Box::new(InsecureCookieCheck));
        self.checks.push(Box::new(MissingSecurityHeadersCheck));

        use crate::checks::jpa::UnsafeCriteriaCheck;
        self.checks.push(Box::new(UnsafeCriteriaCheck));
    }

    fn register_additional_checks(&mut self) {
        use crate::checks::access_control::MissingAccessControlCheck;
        self.checks.push(Box::new(MissingAccessControlCheck));

        use crate::checks::jwt::{JwtHeaderMisuseCheck, WeakJwtSecretCheck};
        self.checks.push(Box::new(JwtHeaderMisuseCheck));
        self.checks.push(Box::new(WeakJwtSecretCheck));

        use crate::checks::deserialization::UnsafeObjectInputStreamCheck;
        self.checks.push(Box::new(UnsafeObjectInputStreamCheck));

        use crate::checks::stored_xss::StoredXssHeuristicCheck;
        self.checks.push(Box::new(StoredXssHeuristicCheck));

        use crate::checks::xxe::UnsafeXmlParserConfigCheck;
        self.checks.push(Box::new(UnsafeXmlParserConfigCheck));

        use crate::checks::zip_slip::ZipSlipCheck;
        self.checks.push(Box::new(ZipSlipCheck));
    }

    fn register_phase9_checks(&mut self) {
        use crate::checks::sensitive_data_logging::SensitiveDataLoggingCheck;
        self.checks.push(Box::new(SensitiveDataLoggingCheck));

        use crate::checks::mass_assignment::MassAssignmentCheck;
        self.checks.push(Box::new(MassAssignmentCheck));

        use crate::checks::stack_trace_exposure::StackTraceExposureCheck;
        self.checks.push(Box::new(StackTraceExposureCheck));

        use crate::checks::hardcoded_internal_url::HardcodedInternalUrlCheck;
        self.checks.push(Box::new(HardcodedInternalUrlCheck));
    }

    fn register_phase2_tier2_checks(&mut self) {
        use crate::checks::idor::IdorCheck;
        self.checks.push(Box::new(IdorCheck));

        use crate::checks::race_condition::RaceConditionCheck;
        self.checks.push(Box::new(RaceConditionCheck));

        use crate::checks::insufficient_logging::InsufficientLoggingCheck;
        self.checks.push(Box::new(InsufficientLoggingCheck));

        use crate::checks::ssti::SstiCheck;
        self.checks.push(Box::new(SstiCheck));
    }

    fn register_phase3_tier3_checks(&mut self) {
        use crate::checks::rate_limiting::MissingRateLimitingCheck;
        self.checks.push(Box::new(MissingRateLimitingCheck));

        use crate::checks::null_pointer_dos::NullPointerDosCheck;
        self.checks.push(Box::new(NullPointerDosCheck));
    }

    #[allow(clippy::unused_self)]
    const fn register_phase4_checks(&self) {
        // Note: VulnerableDependencyCheck is registered separately
        // because it operates on build files, not AST nodes
    }

    /// Run all checks on a syntax tree
    pub fn check_tree(&self, root: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Walk the tree once, collecting nodes by kind
        let mut nodes_by_kind: std::collections::HashMap<SyntaxKind, Vec<SyntaxNode>> =
            std::collections::HashMap::new();

        for node in root.descendants() {
            nodes_by_kind.entry(node.kind()).or_default().push(node);
        }

        // Run each check on relevant nodes. A check that reads text sees the same
        // construct in every enclosing statement, so the innermost node reports it and an
        // enclosing node's copy of the same message is dropped.
        for check in &self.checks {
            let mut nodes: Vec<&SyntaxNode> = check
                .target_kinds()
                .iter()
                .filter_map(|kind| nodes_by_kind.get(kind))
                .flatten()
                .collect();
            nodes.sort_by_key(|node| node.text_range().len());

            let mut reported: Vec<(rowan::TextRange, String)> = Vec::new();
            for node in nodes {
                if context.deadline_passed() {
                    return findings;
                }
                let range = node.text_range();
                let mut check_findings = check.check(node, context);
                for finding in &mut check_findings {
                    if finding.rule_id.is_none() {
                        finding.rule_id = Some(format!("check:{}", check.name()));
                    }
                }
                let inner_reported = |message: &str| {
                    reported.iter().any(|(inner, reported_message)| {
                        range.contains_range(*inner) && reported_message == message
                    })
                };
                let duplicate_flags: Vec<bool> = check_findings
                    .iter()
                    .map(|finding| inner_reported(&finding.message))
                    .collect();
                for (finding, duplicate) in check_findings.into_iter().zip(duplicate_flags) {
                    reported.push((range, finding.message.clone()));
                    if !duplicate {
                        findings.push(finding);
                    }
                }
            }
        }

        findings
    }
}

impl Default for CheckRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub mod access_control;
pub mod auth;
pub mod crypto;
pub mod dangerous_api;
pub mod deserialization;
pub mod hardcoded_internal_url;
pub mod idor;
pub mod insufficient_logging;
pub mod jackson;
pub mod jpa;
pub mod jwt;
pub mod mass_assignment;
pub mod null_pointer_dos;
pub mod race_condition;
pub mod rate_limiting;
pub mod reflection;
pub mod sensitive_data_logging;
pub mod servlet;
pub mod spring_security;
pub mod ssti;
pub mod stack_trace_exposure;
pub mod stored_xss;
pub mod vulnerable_dependencies;
pub mod xxe;
pub mod zip_slip;
