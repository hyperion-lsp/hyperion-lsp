// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Security scanner implementation

use crate::checks::{CheckContext, CheckRegistry};
use crate::config::ScanConfig;
use crate::finding::{Finding, Severity, VulnType};
use crate::logging_context::LoggingContext;
use crate::sinks::SinkRegistry;
use crate::sources::SourceRegistry;

use hyperion_parser::syntax::{AstNode, SourceFile};
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, instrument, warn};
use walkdir::WalkDir;

use super::utilities::{
    collect_dead_code_ranges, is_suppressed, lower_severity, ScanStats, SecurityScanner,
};
use super::visitor::SecurityVisitor;
impl SecurityScanner {
    #[must_use]
    pub fn new(config: ScanConfig) -> Self {
        let framework_registry = crate::frameworks::FrameworkRegistry::with_builtin();
        let (source_registry, sink_registry, sanitizer_registry) =
            framework_registry.into_registries();
        Self {
            config,
            source_registry,
            sink_registry,
            sanitizer_registry,
            logging_context: std::sync::RwLock::new(LoggingContext::unknown()),
        }
    }

    /// Create a scanner with framework-specific patterns
    ///
    /// This constructor uses the provided framework registry to build
    /// source/sink/sanitizer registries from enabled frameworks.
    ///
    /// # Arguments
    /// * `config` - Scan configuration
    /// * `framework_registry` - Registry with enabled frameworks
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::{SecurityScanner, ScanConfig};
    /// use hyperion_security::frameworks::FrameworkRegistry;
    ///
    /// let mut registry = FrameworkRegistry::with_builtin();
    /// registry.enable_frameworks(&["spring", "servlet"]);
    /// let (sources, sinks, sanitizers) = registry.into_registries();
    ///
    /// let scanner = SecurityScanner::with_frameworks(
    ///     ScanConfig::default(),
    ///     sources,
    ///     sinks,
    ///     sanitizers,
    /// );
    /// ```
    #[must_use]
    pub const fn with_frameworks(
        config: ScanConfig,
        source_registry: SourceRegistry,
        sink_registry: SinkRegistry,
        sanitizer_registry: crate::sanitizers::SanitizerRegistry,
    ) -> Self {
        Self {
            config,
            source_registry,
            sink_registry,
            sanitizer_registry,
            logging_context: std::sync::RwLock::new(LoggingContext::unknown()),
        }
    }

    /// Scan a directory for security issues
    #[instrument(skip(self), fields(dir = %dir.display()))]
    pub fn scan_directory(&self, dir: &Path) -> Vec<Finding> {
        let (findings, _stats) = self.scan_directory_with_stats(dir);
        findings
    }

    /// Scan a directory and return statistics. With `config.interprocedural`, the cross-file
    /// taint findings are added to the per-file ones, which still run.
    #[instrument(skip(self), fields(dir = %dir.display()))]
    pub fn scan_directory_with_stats(&self, dir: &Path) -> (Vec<Finding>, ScanStats) {
        let (mut findings, mut stats) = self.scan_directory_per_file_with_stats(dir);
        if self.config.interprocedural {
            let mut known: HashSet<_> = findings.iter().map(same_issue_key).collect();
            let cross_file: Vec<Finding> = self
                .scan_with_context(dir)
                .into_iter()
                .filter(|finding| known.insert(same_issue_key(finding)))
                .collect();
            Self::update_stats_with_findings(&mut stats, &cross_file);
            findings.extend(cross_file);
        }
        (findings, stats)
    }

    /// Scan a directory file by file, without cross-file taint propagation.
    fn scan_directory_per_file_with_stats(&self, dir: &Path) -> (Vec<Finding>, ScanStats) {
        self.update_logging_context(dir);
        let files: Vec<PathBuf> = WalkDir::new(dir)
            .follow_links(self.config.follow_symlinks)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                // Scan .java files and build.gradle files
                e.path().extension().map_or_else(
                    || {
                        // Also check for build.gradle (no extension)
                        e.path()
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.contains("build.gradle"))
                    },
                    |ext| ext == "java" || ext == "gradle",
                )
            })
            .filter(|e| {
                let path = e.path();
                if self.is_excluded(path) {
                    debug!("Skipping excluded path: {}", path.display());
                    false
                } else {
                    true
                }
            })
            .map(walkdir::DirEntry::into_path)
            .collect();

        info!(file_count = files.len(), "Scanning directory");
        self.scan_files_with_stats(&files)
    }

    fn update_logging_context(&self, root: &Path) {
        let context = LoggingContext::from_root(root);
        if let Ok(mut guard) = self.logging_context.write() {
            *guard = context;
        }
    }

    /// Scan source code
    #[instrument(skip(self, source), fields(file = %path.display()))]
    pub fn scan_source(&self, source: &str, path: &Path) -> Vec<Finding> {
        self.scan_source_until(source, path, None)
    }

    /// Scan source text, stopping the checks once `deadline` passes. `scan_file_resilient`
    /// passes the file budget here. Its caller then discards the partial result.
    pub(super) fn scan_source_until(
        &self,
        source: &str,
        path: &Path,
        deadline: Option<Instant>,
    ) -> Vec<Finding> {
        // Check if this is a build file (Gradle or Maven)
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.contains("build.gradle") || file_name == "pom.xml" {
            return self.scan_build_file(source, path);
        }

        let tokens = Lexer::new(source).collect();
        let (green_node, errors) = Parser::parse(tokens);
        let root = RawSyntaxNode::new_root(green_node);

        if !errors.is_empty() {
            warn!(
                file = %path.display(),
                error_count = errors.len(),
                "Parse errors encountered"
            );
        }

        let Some(source_file) = SourceFile::cast(root.clone()) else {
            warn!(file = %path.display(), "Failed to cast to SourceFile");
            return vec![];
        };

        let mut findings = Vec::new();
        let mut visitor = SecurityVisitor::new(self, source, path.to_path_buf());

        // Visit all type declarations
        for type_decl in source_file.types() {
            visitor.visit_type_decl(type_decl.syntax());
        }

        findings.extend(visitor.findings);

        // Run pattern-based checks (Phase 3)
        let check_registry = CheckRegistry::new();
        let check_context = CheckContext::new(path, source).with_deadline(deadline);
        findings.extend(check_registry.check_tree(&root, &check_context));

        let dead_code_ranges = collect_dead_code_ranges(&source_file, source);
        Self::adjust_findings_for_context(&mut findings, path, source, &dead_code_ranges);
        self.finalize_findings(&mut findings, source);

        if !findings.is_empty() {
            info!(
                file = %path.display(),
                finding_count = findings.len(),
                "Found security issues"
            );
        }

        findings
    }

    /// Scan using a pre-parsed syntax tree, avoiding redundant lexing/parsing.
    ///
    /// The diagnostic engine already parses every file via Salsa. This method
    /// accepts that parse result directly instead of re-parsing from source.
    #[instrument(skip(self, root, source), fields(file = %path.display()))]
    pub fn scan_syntax_node(
        &self,
        root: &RawSyntaxNode,
        source: &str,
        path: &Path,
    ) -> Vec<Finding> {
        // Check if this is a build file (Gradle or Maven)
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.contains("build.gradle") || file_name == "pom.xml" {
            return self.scan_build_file(source, path);
        }

        let Some(source_file) = SourceFile::cast(root.clone()) else {
            warn!(file = %path.display(), "Failed to cast to SourceFile");
            return vec![];
        };

        let mut findings = Vec::new();
        let mut visitor = SecurityVisitor::new(self, source, path.to_path_buf());

        for type_decl in source_file.types() {
            visitor.visit_type_decl(type_decl.syntax());
        }

        findings.extend(visitor.findings);

        let check_registry = CheckRegistry::new();
        // The LSP path has no timeout accounting, so its checks always run to completion
        let check_context = CheckContext::new(path, source);
        findings.extend(check_registry.check_tree(root, &check_context));

        let dead_code_ranges = collect_dead_code_ranges(&source_file, source);
        Self::adjust_findings_for_context(&mut findings, path, source, &dead_code_ranges);
        self.finalize_findings(&mut findings, source);

        if !findings.is_empty() {
            info!(
                file = %path.display(),
                finding_count = findings.len(),
                "Found security issues"
            );
        }

        findings
    }

    /// Apply final user-facing filtering and collapse repeated reports of the same sink.
    ///
    /// Finding IDs are intentionally not used here because they only encode the file and
    /// source line; distinct rules or sinks can legitimately share both.
    fn finalize_findings(&self, findings: &mut Vec<Finding>, source: &str) {
        findings.retain(|finding| !is_suppressed(finding, source));
        findings.retain(|finding| {
            self.config.should_scan(finding.vuln_type)
                && self.config.meets_severity(finding.severity)
        });

        let mut seen = HashSet::new();
        findings.retain(|finding| {
            let effective_location = finding
                .sink_location
                .as_ref()
                .unwrap_or(&finding.source_location);
            seen.insert((
                finding.vuln_type,
                finding.rule_id.clone(),
                finding.sink_kind.clone(),
                effective_location.clone(),
            ))
        });
    }

    fn adjust_findings_for_context(
        findings: &mut [Finding],
        path: &Path,
        source: &str,
        dead_code_ranges: &[(u32, u32)],
    ) {
        let path_str = path.to_string_lossy().to_lowercase();
        let source_lower = source.to_lowercase();

        let is_test_path = path_str.contains("/test/")
            || path_str.contains("/tests/")
            || path_str.contains("src/test/")
            || path_str.contains("src/tests/");

        let is_internal_path = path_str.contains("/internal/")
            || source_lower.contains("package ") && source_lower.contains(".internal");

        let is_deprecated = source_lower.contains("@deprecated")
            || source_lower.contains("deprecated ")
            || source_lower.contains("deprecated:");

        for finding in findings {
            if is_test_path || is_deprecated {
                finding.severity = Severity::Info;
                continue;
            }

            if is_internal_path {
                finding.severity = lower_severity(finding.severity);
            }

            if !dead_code_ranges.is_empty() {
                let line = finding.source_location.line;
                if dead_code_ranges
                    .iter()
                    .any(|(start, end)| line >= *start && line <= *end)
                {
                    finding.severity = Severity::Info;
                }
            }
        }
    }
}

/// What makes two findings the same issue, whichever pass reported it: the kind of
/// vulnerability and the exact place in the source. The per-file and the cross-file pass
/// word the message and set the rule id differently for the same issue.
fn same_issue_key(finding: &Finding) -> (VulnType, PathBuf, u32, u32) {
    let location = &finding.source_location;
    (
        finding.vuln_type,
        location.file.clone(),
        location.line,
        location.column,
    )
}
