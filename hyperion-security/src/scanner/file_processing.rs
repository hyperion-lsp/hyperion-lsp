// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! File processing methods for SecurityScanner
//!
//! Handles file I/O, caching, parallel scanning, and exclusion filtering.

use crate::cache::{hash_file, ScanCache};
use crate::checks::CheckContext;
use crate::finding::Finding;

use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, info, instrument, warn};

use super::utilities::{ScanStats, SecurityScanner};

impl SecurityScanner {
    pub fn scan_files_with_stats(&self, files: &[PathBuf]) -> (Vec<Finding>, ScanStats) {
        let stats = Arc::new(Mutex::new(ScanStats::default()));
        let cache = if self.config.incremental {
            self.config
                .cache_file
                .as_ref()
                .map(|cache_path| Arc::new(Mutex::new(ScanCache::load(cache_path))))
        } else {
            None
        };

        let counter = Arc::new(AtomicUsize::new(0));
        let total = files.len();

        // Configure rayon thread pool if max_workers is set
        let findings = if self.config.max_workers > 0 {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(self.config.max_workers)
                .build()
                .unwrap_or_else(|_| {
                    rayon::ThreadPoolBuilder::new()
                        .build()
                        .expect("default rayon pool must build")
                });

            pool.install(|| {
                self.scan_files_parallel(files, cache.as_ref(), &stats, &counter, total)
            })
        } else {
            self.scan_files_parallel(files, cache.as_ref(), &stats, &counter, total)
        };

        // Save cache if incremental
        if let Some(cache) = cache {
            if let Ok(mut cache_guard) = cache.lock() {
                cache_guard.prune_missing_files();
                if let Err(e) = cache_guard.save() {
                    eprintln!("Warning: Failed to save cache: {e}");
                }
            }
        }

        let final_stats = Arc::try_unwrap(stats).map_or_else(
            |arc| arc.lock().expect("stats mutex not poisoned").clone(),
            |mutex| mutex.into_inner().expect("stats mutex not poisoned"),
        );

        (findings, final_stats)
    }

    fn scan_files_parallel(
        &self,
        files: &[PathBuf],
        cache: Option<&Arc<Mutex<ScanCache>>>,
        stats: &Arc<Mutex<ScanStats>>,
        counter: &Arc<AtomicUsize>,
        total: usize,
    ) -> Vec<Finding> {
        files
            .par_iter()
            .flat_map(|path| {
                let current = counter.fetch_add(1, Ordering::Relaxed) + 1;

                // Report progress
                if let Some(ref callback) = self.config.progress_callback {
                    callback(current, total, &path.to_string_lossy());
                }

                self.scan_file_with_cache(path, cache, stats)
            })
            .collect()
    }

    fn scan_file_with_cache(
        &self,
        path: &Path,
        cache: Option<&Arc<Mutex<ScanCache>>>,
        stats: &Arc<Mutex<ScanStats>>,
    ) -> Vec<Finding> {
        // Hash at most once. On a cache miss the same digest is stored after scanning.
        let current_hash = cache.and_then(|_| hash_file(path, self.config.max_file_size).ok());
        if let (Some(cache_arc), Some(hash)) = (cache, current_hash.as_ref()) {
            let cached = cache_arc
                .lock()
                .ok()
                .and_then(|guard| guard.get(path, hash).map(<[Finding]>::to_vec));
            if let Some(cached_findings) = cached {
                let mut stats_guard = stats.lock().unwrap_or_else(|poisoned| {
                    warn!("Stats mutex poisoned, recovering");
                    poisoned.into_inner()
                });
                stats_guard.files_cached += 1;
                Self::update_stats_with_findings(&mut stats_guard, &cached_findings);
                drop(stats_guard);
                return cached_findings;
            }
        }

        // Scan the file
        let scan = self.scan_file_resilient(path, stats);

        // Update cache. A panicked scan has no result to keep, so the next run scans the file again.
        if let (Some(cache_arc), Some(hash), Some(scanned)) = (cache, current_hash, &scan) {
            if let Ok(mut cache_guard) = cache_arc.lock() {
                cache_guard.insert(path.to_path_buf(), hash, scanned.clone());
            }
        }
        let findings = scan.unwrap_or_default();

        // Recover from poisoned mutex - losing stats is worse than inconsistent stats
        let mut stats_guard = stats.lock().unwrap_or_else(|poisoned| {
            warn!("Stats mutex poisoned, recovering");
            poisoned.into_inner()
        });
        stats_guard.files_scanned += 1;
        Self::update_stats_with_findings(&mut stats_guard, &findings);
        drop(stats_guard);

        findings
    }

    pub(super) fn update_stats_with_findings(stats: &mut ScanStats, findings: &[Finding]) {
        stats.total_findings += findings.len();
        for finding in findings {
            let key = finding
                .rule_id
                .clone()
                .unwrap_or_else(|| finding.vuln_type.as_str().to_string());
            *stats.findings_by_rule.entry(key).or_insert(0) += 1;
        }
    }

    /// Scan one file. Returns `None` when the scan panicked or ran past its time budget,
    /// because neither leaves a complete result to report or cache.
    fn scan_file_resilient(
        &self,
        path: &Path,
        stats: &Arc<Mutex<ScanStats>>,
    ) -> Option<Vec<Finding>> {
        let start = Instant::now();

        // The budget runs from the same start as the timeout check below
        let deadline = self
            .config
            .file_timeout
            .and_then(|timeout| start.checked_add(timeout));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.scan_file_until(path, deadline)
        }));

        let findings = match result {
            Ok(findings) => findings,
            Err(_) => {
                {
                    // Recover from poisoned mutex - losing stats is worse than inconsistent stats
                    let mut stats_guard = stats.lock().unwrap_or_else(|poisoned| {
                        warn!("Stats mutex poisoned, recovering");
                        poisoned.into_inner()
                    });
                    stats_guard.parse_errors += 1;
                }
                warn!(file = %path.display(), "Panic while scanning file");
                return None;
            }
        };

        // A file over its budget reports nothing. Its checks may have stopped early, so the
        // partial findings are dropped and the file is not cached.
        if let Some(timeout) = self.config.file_timeout {
            if start.elapsed() >= timeout {
                {
                    // Recover from poisoned mutex - losing stats is worse than inconsistent stats
                    let mut stats_guard = stats.lock().unwrap_or_else(|poisoned| {
                        warn!("Stats mutex poisoned, recovering");
                        poisoned.into_inner()
                    });
                    stats_guard.timeouts += 1;
                }
                warn!(
                    file = %path.display(),
                    elapsed = ?start.elapsed(),
                    timeout = ?timeout,
                    "File scan timed out"
                );
                return None;
            }
        }

        Some(findings)
    }

    /// Check if a path should be excluded based on configured patterns
    ///
    /// # Performance
    ///
    /// This performs a linear scan through all exclusion patterns for each file,
    /// resulting in O(patterns × files) complexity. For typical usage (< 100 patterns),
    /// this is acceptable and avoids the complexity of building a RegexSet.
    ///
    /// If you have many exclusion patterns (> 100) and are scanning large codebases,
    /// consider using fewer, more general patterns (e.g., "**/test/**" instead of
    /// listing individual test directories).
    ///
    /// # Future Optimization
    ///
    /// If this becomes a bottleneck, we could:
    /// - Use `regex::RegexSet` to check all patterns in a single pass
    /// - Build a prefix tree (trie) for pattern matching
    /// - Cache exclusion results per directory
    pub(super) fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Check explicit exclude patterns first
        for pattern in &self.config.exclude_patterns {
            // Simple pattern matching (could use glob crate for more complex patterns)
            if pattern.contains("**") {
                let simple = pattern.replace("**", "");
                if path_str.contains(&simple) {
                    return true;
                }
            } else if path_str.contains(pattern) {
                return true;
            }
        }

        // Check test directory patterns if --exclude-tests is enabled
        if self.config.exclude_tests {
            // Common test directory patterns
            let test_dir_patterns = [
                "/test/",
                "/tests/",
                "/__tests__/",
                "/src/test/java/",
                "/src/test/kotlin/",
                "/src/test/groovy/",
                "/src/androidTest/",
                "/src/instrumentedTest/",
            ];

            for pattern in &test_dir_patterns {
                if path_str.contains(pattern) {
                    return true;
                }
            }

            // Test file naming patterns
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with("Test.java")
                    || file_name.ends_with("Tests.java")
                    || file_name.ends_with("TestCase.java")
                    || file_name.ends_with("IT.java")  // Integration tests
                    || file_name.ends_with("Spec.java")  // Spock/Kotlin specs
                    || file_name.ends_with("Test.kt")
                    || file_name.ends_with("Tests.kt")
                    || file_name.ends_with("TestCase.kt")
                    || file_name.ends_with("IT.kt")
                    || file_name.ends_with("Spec.kt")
                {
                    return true;
                }
            }
        }

        false
    }

    /// Scan a single file
    #[instrument(skip(self), fields(file = %path.display()))]
    pub fn scan_file(&self, path: &Path) -> Vec<Finding> {
        self.scan_file_until(path, None)
    }

    /// Scan a file, stopping its checks once `deadline` passes (see `scan_source_until`).
    fn scan_file_until(&self, path: &Path, deadline: Option<Instant>) -> Vec<Finding> {
        let source =
            match hyperion_workspace::read_utf8_file_bounded(path, self.config.max_file_size) {
                Ok(s) => s,
                Err(e) => {
                    warn!(file = %path.display(), error = %e, "Failed to read file");
                    return vec![];
                }
            };

        debug!(file = %path.display(), bytes = source.len(), "Parsing file");
        self.scan_source_until(&source, path, deadline)
    }

    /// Scan a build file for vulnerable dependencies
    pub(super) fn scan_build_file(&self, source: &str, path: &Path) -> Vec<Finding> {
        self.scan_build_file_with_gradle_resolution(source, path, None)
    }

    /// Scan a Gradle build file with optional resolved dependency provenance.
    ///
    /// A resolution report adds BOM-selected and transitive coordinates to the
    /// source declarations while preserving the scanner's configuration and
    /// deduplication behavior.
    pub fn scan_build_file_with_gradle_resolution(
        &self,
        source: &str,
        path: &Path,
        report: Option<&hyperion_build::gradle::GradleResolutionReport>,
    ) -> Vec<Finding> {
        use crate::checks::vulnerable_dependencies::VulnerableDependencyCheck;

        let check = VulnerableDependencyCheck;
        let check_context = CheckContext::new(path, source);
        let mut findings = report.map_or_else(
            || check.check_file(&check_context),
            |report| check.check_file_with_gradle_resolution(&check_context, report),
        );

        // Filter by config
        findings.retain(|f| {
            self.config.should_scan(f.vuln_type) && self.config.meets_severity(f.severity)
        });

        if !findings.is_empty() {
            info!(
                file = %path.display(),
                finding_count = findings.len(),
                "Found vulnerable dependencies"
            );
        }

        findings
    }
}
