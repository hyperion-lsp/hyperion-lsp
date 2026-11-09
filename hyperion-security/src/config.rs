// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scanner configuration

use crate::finding::{Severity, VulnType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Progress callback for reporting scan progress
///
/// Parameters:
/// - current: number of files scanned so far
/// - total: total number of files to scan
/// - current_file: path to the file currently being scanned
pub type ProgressCallback = Arc<dyn Fn(usize, usize, &str) + Send + Sync>;

/// Configuration for the security scanner
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ScanConfig {
    /// Minimum severity to report
    pub min_severity: Severity,

    /// Vulnerability types to scan for (empty = all)
    pub vuln_types: HashSet<VulnType>,

    /// File patterns to include
    pub include_patterns: Vec<String>,

    /// File patterns to exclude
    pub exclude_patterns: Vec<String>,

    /// Whether to follow symbolic links
    pub follow_symlinks: bool,

    /// Maximum file size to scan (bytes)
    pub max_file_size: usize,

    /// Output format
    pub output_format: OutputFormat,

    /// Whether to output to file
    pub output_file: Option<PathBuf>,

    /// Maximum depth for taint flow analysis (prevents infinite loops)
    pub max_taint_depth: usize,

    /// Timeout for scanning a single file
    pub file_timeout: Option<Duration>,

    /// Enable incremental caching
    pub incremental: bool,

    /// Path to cache file for incremental scanning
    pub cache_file: Option<PathBuf>,

    /// Progress callback (not serialized)
    pub progress_callback: Option<ProgressCallback>,

    /// Maximum number of parallel workers (0 = use all CPUs)
    pub max_workers: usize,

    /// Whether to exclude test directories and test files
    pub exclude_tests: bool,

    /// Enable interprocedural (cross-file) taint analysis.
    /// When enabled, taint is propagated across method calls using
    /// fixpoint analysis over the call graph.
    pub interprocedural: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Sarif,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            min_severity: Severity::Low,
            vuln_types: HashSet::new(), // All types
            include_patterns: vec!["**/*.java".to_string()],
            exclude_patterns: vec![
                "**/test/**".to_string(),
                "**/tests/**".to_string(),
                "**/target/**".to_string(),
                "**/build/**".to_string(),
                "**/node_modules/**".to_string(),
            ],
            follow_symlinks: false,
            max_file_size: 10 * 1024 * 1024, // 10 MB
            output_format: OutputFormat::Text,
            output_file: None,
            max_taint_depth: 50,
            file_timeout: Some(Duration::from_secs(10)),
            incremental: false,
            cache_file: None,
            progress_callback: None,
            max_workers: 0, // Use all CPUs
            exclude_tests: false,
            interprocedural: false,
        }
    }
}

// Implement Serialize/Deserialize manually to skip the callback
impl Serialize for ScanConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ScanConfig", 13)?;
        state.serialize_field("min_severity", &self.min_severity)?;
        state.serialize_field("vuln_types", &self.vuln_types)?;
        state.serialize_field("include_patterns", &self.include_patterns)?;
        state.serialize_field("exclude_patterns", &self.exclude_patterns)?;
        state.serialize_field("follow_symlinks", &self.follow_symlinks)?;
        state.serialize_field("max_file_size", &self.max_file_size)?;
        state.serialize_field("output_format", &self.output_format)?;
        state.serialize_field("output_file", &self.output_file)?;
        state.serialize_field("max_taint_depth", &self.max_taint_depth)?;
        state.serialize_field("file_timeout", &self.file_timeout)?;
        state.serialize_field("incremental", &self.incremental)?;
        state.serialize_field("cache_file", &self.cache_file)?;
        state.serialize_field("max_workers", &self.max_workers)?;
        state.serialize_field("exclude_tests", &self.exclude_tests)?;
        state.serialize_field("interprocedural", &self.interprocedural)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ScanConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[allow(clippy::struct_excessive_bools)]
        struct ScanConfigHelper {
            min_severity: Severity,
            vuln_types: HashSet<VulnType>,
            include_patterns: Vec<String>,
            exclude_patterns: Vec<String>,
            follow_symlinks: bool,
            max_file_size: usize,
            output_format: OutputFormat,
            output_file: Option<PathBuf>,
            max_taint_depth: usize,
            file_timeout: Option<Duration>,
            incremental: bool,
            cache_file: Option<PathBuf>,
            max_workers: usize,
            #[serde(default)]
            exclude_tests: bool,
            #[serde(default)]
            interprocedural: bool,
        }

        let helper = ScanConfigHelper::deserialize(deserializer)?;
        Ok(Self {
            min_severity: helper.min_severity,
            vuln_types: helper.vuln_types,
            include_patterns: helper.include_patterns,
            exclude_patterns: helper.exclude_patterns,
            follow_symlinks: helper.follow_symlinks,
            max_file_size: helper.max_file_size,
            output_format: helper.output_format,
            output_file: helper.output_file,
            max_taint_depth: helper.max_taint_depth,
            file_timeout: helper.file_timeout,
            incremental: helper.incremental,
            cache_file: helper.cache_file,
            progress_callback: None,
            max_workers: helper.max_workers,
            exclude_tests: helper.exclude_tests,
            interprocedural: helper.interprocedural,
        })
    }
}

impl std::fmt::Debug for ScanConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanConfig")
            .field("min_severity", &self.min_severity)
            .field("vuln_types", &self.vuln_types)
            .field("include_patterns", &self.include_patterns)
            .field("exclude_patterns", &self.exclude_patterns)
            .field("follow_symlinks", &self.follow_symlinks)
            .field("max_file_size", &self.max_file_size)
            .field("output_format", &self.output_format)
            .field("output_file", &self.output_file)
            .field("max_taint_depth", &self.max_taint_depth)
            .field("file_timeout", &self.file_timeout)
            .field("incremental", &self.incremental)
            .field("cache_file", &self.cache_file)
            .field("progress_callback", &self.progress_callback.is_some())
            .field("max_workers", &self.max_workers)
            .field("exclude_tests", &self.exclude_tests)
            .field("interprocedural", &self.interprocedural)
            .finish()
    }
}

impl ScanConfig {
    /// Create a new config that scans for all vulnerabilities
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// Add a vulnerability type to scan for
    #[must_use]
    pub fn with_vuln_type(mut self, vuln_type: VulnType) -> Self {
        self.vuln_types.insert(vuln_type);
        self
    }

    /// Set minimum severity
    #[must_use]
    pub const fn with_min_severity(mut self, severity: Severity) -> Self {
        self.min_severity = severity;
        self
    }

    /// Set output format
    #[must_use]
    pub const fn with_output_format(mut self, format: OutputFormat) -> Self {
        self.output_format = format;
        self
    }

    /// Enable incremental scanning with cache
    #[must_use]
    pub fn with_incremental(mut self, cache_file: PathBuf) -> Self {
        self.incremental = true;
        self.cache_file = Some(cache_file);
        self
    }

    /// Set maximum taint depth
    #[must_use]
    pub const fn with_max_taint_depth(mut self, depth: usize) -> Self {
        self.max_taint_depth = depth;
        self
    }

    /// Set file timeout
    #[must_use]
    pub const fn with_file_timeout(mut self, timeout: Duration) -> Self {
        self.file_timeout = Some(timeout);
        self
    }

    /// Set progress callback
    #[must_use]
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, usize, &str) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Set maximum number of parallel workers
    #[must_use]
    pub const fn with_max_workers(mut self, workers: usize) -> Self {
        self.max_workers = workers;
        self
    }

    /// Enable test directory exclusion
    #[must_use]
    pub const fn with_exclude_tests(mut self, exclude: bool) -> Self {
        self.exclude_tests = exclude;
        self
    }

    /// Add custom exclude patterns (appends to existing patterns)
    #[must_use]
    pub fn with_exclude_patterns(mut self, patterns: Vec<String>) -> Self {
        self.exclude_patterns.extend(patterns);
        self
    }

    /// Check if a vulnerability type should be scanned
    #[must_use]
    pub fn should_scan(&self, vuln_type: VulnType) -> bool {
        self.vuln_types.is_empty() || self.vuln_types.contains(&vuln_type)
    }

    /// Check if a finding meets the minimum severity
    #[must_use]
    pub fn meets_severity(&self, severity: Severity) -> bool {
        severity >= self.min_severity
    }

    /// Enable interprocedural (cross-file) taint analysis
    #[must_use]
    pub const fn with_interprocedural(mut self, enabled: bool) -> Self {
        self.interprocedural = enabled;
        self
    }
}
