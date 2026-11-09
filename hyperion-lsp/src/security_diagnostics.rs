// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Security Diagnostics Integration
//!
//! This module integrates the hyperion-security scanner into the LSP server,
//! providing real-time security vulnerability detection as the user types.

use dashmap::DashMap;
use hyperion_security::{
    finding_to_code_actions, finding_to_diagnostic, finding_to_hover, ScanCache, ScanConfig,
    SecurityScanner,
};
use hyperion_workspace::PathLookupKey;
use lsp_types::{CodeActionOrCommand, Diagnostic, Hover, Position, Url};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Security diagnostics provider
///
/// This struct manages the security scanner and provides methods to:
/// - Scan files for security vulnerabilities
/// - Convert findings to LSP diagnostics
/// - Provide code actions for fixes
/// - Show hover information with taint flow
pub struct SecurityDiagnosticsProvider {
    scanner: Arc<RwLock<SecurityScanner>>,
    cache: Arc<RwLock<ScanCache>>,
    // Track which files have security findings
    findings_by_file: Arc<DashMap<PathLookupKey, Vec<hyperion_security::Finding>>>,
    // Cache latest source text per file for accurate related info
    source_by_file: Arc<DashMap<PathLookupKey, String>>,
    // Remember the key observed when a display path was scanned. A deleted or
    // broken symlink cannot reproduce that key during invalidation.
    key_by_display_path: Arc<DashMap<PathBuf, HashSet<PathLookupKey>>>,
    project_classpaths: ProjectClasspathScope,
}

enum ProjectClasspathScope {
    Compatibility,
    Session {
        store: crate::project_classpath::ProjectClasspathStore,
        workspace_root: Arc<RwLock<Option<PathBuf>>>,
    },
}

impl SecurityDiagnosticsProvider {
    /// Creates a new security diagnostics provider
    #[must_use]
    pub fn new() -> Self {
        let config = ScanConfig::default();
        let scanner = SecurityScanner::new(config);
        let cache = ScanCache::new();

        Self {
            scanner: Arc::new(RwLock::new(scanner)),
            cache: Arc::new(RwLock::new(cache)),
            findings_by_file: Arc::new(DashMap::new()),
            source_by_file: Arc::new(DashMap::new()),
            key_by_display_path: Arc::new(DashMap::new()),
            project_classpaths: ProjectClasspathScope::Compatibility,
        }
    }

    /// Creates a new provider with custom configuration
    #[must_use]
    pub fn with_config(config: ScanConfig) -> Self {
        let scanner = SecurityScanner::new(config);
        let cache = ScanCache::new();

        Self {
            scanner: Arc::new(RwLock::new(scanner)),
            cache: Arc::new(RwLock::new(cache)),
            findings_by_file: Arc::new(DashMap::new()),
            source_by_file: Arc::new(DashMap::new()),
            key_by_display_path: Arc::new(DashMap::new()),
            project_classpaths: ProjectClasspathScope::Compatibility,
        }
    }

    /// Create a provider backed by the active server session's build state.
    #[must_use]
    pub(crate) fn with_project_classpaths(
        project_classpaths: crate::project_classpath::ProjectClasspathStore,
        workspace_root: Arc<RwLock<Option<PathBuf>>>,
    ) -> Self {
        let mut provider = Self::new();
        provider.project_classpaths = ProjectClasspathScope::Session {
            store: project_classpaths,
            workspace_root,
        };
        provider
    }

    /// Scans a file and returns security diagnostics
    ///
    /// This method:
    /// 1. Checks the cache for existing results
    /// 2. Runs the security scanner if needed
    /// 3. Converts findings to LSP diagnostics
    /// 4. Caches the results
    pub fn scan_file(&self, file_path: &Path, source_text: &str) -> Vec<Diagnostic> {
        self.scan_file_inner(file_path, source_text, None)
    }

    /// Scans a file using a pre-parsed syntax tree, avoiding redundant parsing.
    ///
    /// The diagnostic engine already parses every file via Salsa. This method
    /// passes that parse tree to the security scanner instead of re-parsing.
    pub fn scan_file_with_syntax(
        &self,
        file_path: &Path,
        source_text: &str,
        syntax: &hyperion_parser::SyntaxNode,
    ) -> Vec<Diagnostic> {
        self.scan_file_inner(file_path, source_text, Some(syntax))
    }

    fn scan_file_inner(
        &self,
        file_path: &Path,
        source_text: &str,
        syntax: Option<&hyperion_parser::SyntaxNode>,
    ) -> Vec<Diagnostic> {
        debug!(
            pre_parsed = syntax.is_some(),
            "Scanning file for security issues: {}",
            file_path.display()
        );
        let file_key = hyperion_workspace::path_lookup_key(file_path);
        self.key_by_display_path
            .entry(file_path.to_path_buf())
            .or_default()
            .insert(file_key.clone());
        self.source_by_file
            .insert(file_key.clone(), source_text.to_owned());

        let resolution = is_gradle_build_file(file_path)
            .then(|| match &self.project_classpaths {
                ProjectClasspathScope::Session {
                    store,
                    workspace_root,
                } => {
                    let workspace_root = workspace_root.read();
                    store.known_gradle_resolution_for_path(workspace_root.as_deref()?, file_path)
                }
                ProjectClasspathScope::Compatibility => {
                    crate::project_classpath::known_gradle_resolution_for_path(file_path)
                }
            })
            .flatten();
        let content_hash = security_cache_key(source_text, resolution.as_deref());

        // End the read guard before the miss path acquires the write lock.
        let cached_findings = {
            self.cache
                .read()
                .get(file_path, &content_hash)
                .map(<[_]>::to_vec)
        };
        let findings = cached_findings.unwrap_or_else(|| {
            let scanner = self.scanner.read();
            let findings = if resolution.is_some() {
                scanner.scan_build_file_with_gradle_resolution(
                    source_text,
                    file_path,
                    resolution.as_deref(),
                )
            } else if let Some(syntax) = syntax {
                scanner.scan_syntax_node(syntax, source_text, file_path)
            } else {
                scanner.scan_source(source_text, file_path)
            };
            info!(
                "Found {} security issues in {}",
                findings.len(),
                file_path.display()
            );
            self.cache
                .write()
                .insert(file_path.to_path_buf(), content_hash, findings.clone());
            findings
        });

        self.findings_by_file.insert(file_key, findings.clone());
        let source_lookup = |path: &Path| {
            let key = self.current_or_stored_key(path);
            self.source_by_file
                .get(&key)
                .map(|entry| entry.value().clone())
        };
        findings
            .iter()
            .map(|finding| finding_to_diagnostic(finding, source_text, Some(&source_lookup)))
            .collect()
    }

    /// Provides code actions for security findings at a specific location
    pub fn code_actions_at_position(
        &self,
        uri: &Url,
        position: Position,
        source_text: &str,
    ) -> Vec<CodeActionOrCommand> {
        let file_path = match uri.to_file_path() {
            Ok(path) => path,
            Err(()) => {
                warn!("Failed to convert URI to file path: {}", uri);
                return Vec::new();
            }
        };

        let file_key = self.current_or_stored_key(&file_path);
        // Get findings for this file
        let findings = match self.findings_by_file.get(&file_key) {
            Some(findings) => findings,
            None => return Vec::new(),
        };

        // Find findings that overlap with the position
        let mut actions = Vec::new();
        for finding in findings.iter() {
            if position_in_range(position, &finding.source_location, source_text) {
                let finding_actions = finding_to_code_actions(finding, uri, source_text);
                actions.extend(finding_actions);
            }
        }

        debug!(
            "Providing {} code actions for position {}:{}",
            actions.len(),
            position.line,
            position.character
        );
        actions
    }

    /// Provides hover information for security findings at a specific location
    pub fn hover_at_position(
        &self,
        uri: &Url,
        position: Position,
        source_text: &str,
    ) -> Option<Hover> {
        let file_path = match uri.to_file_path() {
            Ok(path) => path,
            Err(()) => {
                warn!("Failed to convert URI to file path: {}", uri);
                return None;
            }
        };

        let file_key = self.current_or_stored_key(&file_path);
        // Get findings for this file
        let findings = self.findings_by_file.get(&file_key)?;

        // Find the first finding that overlaps with the position
        for finding in findings.iter() {
            if position_in_range(position, &finding.source_location, source_text) {
                debug!(
                    "Providing security hover for {} at {}:{}",
                    finding.vuln_type, position.line, position.character
                );
                return Some(finding_to_hover(finding, source_text));
            }
        }

        None
    }

    /// Clears cached findings for a specific file
    pub fn invalidate_file(&self, file_path: &Path) {
        // Note: ScanCache doesn't have an invalidate_file method
        // Cache entries are invalidated by hash mismatch on next scan
        // Just clear the findings
        let file_key = self.current_or_stored_key(file_path);
        self.findings_by_file.remove(&file_key);
        self.source_by_file.remove(&file_key);
        let remove_display =
            self.key_by_display_path
                .get_mut(file_path)
                .is_some_and(|mut entry| {
                    entry.value_mut().remove(&file_key);
                    entry.value().is_empty()
                });
        if remove_display {
            self.key_by_display_path.remove(file_path);
        }
    }

    /// Clears all cached findings
    pub fn clear_cache(&self) {
        info!("Clearing security diagnostics cache");
        let mut cache = self.cache.write();
        cache.clear();
        self.findings_by_file.clear();
        self.source_by_file.clear();
        self.key_by_display_path.clear();
    }

    /// Returns statistics about the security scanner
    pub fn stats(&self) -> ScannerStats {
        let findings_count: usize = self.findings_by_file.iter().map(|e| e.value().len()).sum();
        let files_scanned = self.findings_by_file.len();

        ScannerStats {
            files_scanned,
            total_findings: findings_count,
            cache_size: self.cache.read().len(),
        }
    }

    fn current_or_stored_key(&self, file_path: &Path) -> PathLookupKey {
        // Observe the filesystem before touching DashMap guards. A live or
        // inaccessible path always follows its currently resolved key; exact
        // display fallback is reserved for a confirmed absent spelling.
        let current_key = hyperion_workspace::path_lookup_key(file_path);
        let path_is_absent = matches!(file_path.try_exists(), Ok(false));
        if self.findings_by_file.contains_key(&current_key)
            || self.source_by_file.contains_key(&current_key)
            || !path_is_absent
        {
            return current_key;
        }
        let stored_key = self.key_by_display_path.get(file_path).and_then(|entry| {
            let mut keys = entry.value().iter();
            let only = keys.next()?.clone();
            keys.next().is_none().then_some(only)
        });
        stored_key.unwrap_or(current_key)
    }
}

fn is_gradle_build_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("build.gradle" | "build.gradle.kts")
    )
}

fn security_cache_key(
    source_text: &str,
    resolution: Option<&hyperion_build::gradle::GradleResolutionReport>,
) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    fn hash_text(hasher: &mut Sha256, text: &str) {
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    }

    let mut hasher = Sha256::new();
    hash_text(&mut hasher, source_text);
    if let Some(resolution) = resolution {
        for edge in &resolution.dependency_edges {
            hasher.update([match edge.kind {
                hyperion_build::gradle::DependencyEdgeKind::Direct => 0,
                hyperion_build::gradle::DependencyEdgeKind::Transitive => 1,
            }]);
            hash_text(&mut hasher, &edge.from);
            hash_text(&mut hasher, &edge.to);
        }
    }
    hasher.finalize().into()
}

impl Default for SecurityDiagnosticsProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the security scanner
#[derive(Debug, Clone)]
pub struct ScannerStats {
    pub files_scanned: usize,
    pub total_findings: usize,
    pub cache_size: usize,
}

/// Checks if a position is within a location range
fn position_in_range(
    position: Position,
    location: &hyperion_security::finding::Location,
    source_text: &str,
) -> bool {
    let range = location_to_lsp_range(location, source_text);

    if position.line < range.start.line || position.line > range.end.line {
        return false;
    }

    if position.line == range.start.line && position.character < range.start.character {
        return false;
    }

    if position.line == range.end.line && position.character >= range.end.character {
        return false;
    }

    true
}

fn location_to_lsp_range(
    location: &hyperion_security::finding::Location,
    source_text: &str,
) -> lsp_types::Range {
    lsp_types::Range::new(
        location_to_lsp_position(location.line, location.column, source_text),
        location_to_lsp_position(location.end_line, location.end_column, source_text),
    )
}

fn location_to_lsp_position(line: u32, column: u32, source_text: &str) -> Position {
    if source_text.is_empty() {
        return Position::new(line.saturating_sub(1), column.saturating_sub(1));
    }

    let lines: Vec<&str> = source_text.lines().collect();
    let max_index = lines.len().saturating_sub(1);
    let line_index = (line.saturating_sub(1) as usize).min(max_index);
    let line_text = lines.get(line_index).copied().unwrap_or("");

    let target_chars = column.saturating_sub(1) as usize;
    let mut utf16_count = 0u32;

    for ch in line_text.chars().take(target_chars) {
        utf16_count += ch.len_utf16() as u32;
    }

    Position::new(line_index as u32, utf16_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_build::gradle::{DependencyEdge, GradleResolutionReport};
    use hyperion_security::finding::Location;
    use std::path::PathBuf;

    #[test]
    fn test_position_in_range() {
        let location = Location {
            file: PathBuf::from("/test.java"),
            line: 1,
            column: 3,
            end_line: 1,
            end_column: 5,
        };
        let source_text = "ab👋cd\n";

        // Position at start
        assert!(position_in_range(
            Position::new(0, 2),
            &location,
            source_text
        ));

        // Position in middle
        assert!(position_in_range(
            Position::new(0, 4),
            &location,
            source_text
        ));

        // Position at end (exclusive)
        assert!(!position_in_range(
            Position::new(0, 5),
            &location,
            source_text
        ));

        // Position before range
        assert!(!position_in_range(
            Position::new(0, 1),
            &location,
            source_text
        ));

        // Position after range
        assert!(!position_in_range(
            Position::new(0, 6),
            &location,
            source_text
        ));

        // Position on different line
        assert!(!position_in_range(
            Position::new(1, 0),
            &location,
            source_text
        ));
    }

    #[test]
    fn test_provider_creation() {
        let provider = SecurityDiagnosticsProvider::new();
        let stats = provider.stats();

        assert_eq!(stats.files_scanned, 0);
        assert_eq!(stats.total_findings, 0);
    }

    #[test]
    fn test_provider_with_config() {
        let config = ScanConfig::default();
        let provider = SecurityDiagnosticsProvider::with_config(config);
        let stats = provider.stats();

        assert_eq!(stats.files_scanned, 0);
        assert_eq!(stats.total_findings, 0);
    }

    #[cfg(unix)]
    #[test]
    fn findings_and_sources_share_identity_across_filesystem_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let real_file = real.join("Safe.java");
        let alias_file = alias.join("Safe.java");
        let source = "class Safe {}";
        std::fs::write(&real_file, source).unwrap();
        let provider = SecurityDiagnosticsProvider::new();

        provider.scan_file(&real_file, source);
        provider.scan_file(&alias_file, source);

        assert_eq!(provider.stats().files_scanned, 1);
        assert_eq!(provider.source_by_file.len(), 1);
        provider.invalidate_file(&alias_file);
        assert_eq!(provider.stats().files_scanned, 0);
        assert!(provider.source_by_file.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn invalidation_recovers_the_stored_identity_after_an_alias_breaks() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let real_file = real.join("Scanned.java");
        let alias_file = alias.join("Scanned.java");
        let source = "class Scanned {}";
        std::fs::write(&real_file, source).unwrap();
        let provider = SecurityDiagnosticsProvider::new();

        provider.scan_file(&alias_file, source);
        std::fs::remove_file(&alias).unwrap();
        assert_ne!(
            hyperion_workspace::path_lookup_key(&alias_file),
            hyperion_workspace::path_lookup_key(&real_file)
        );

        provider.invalidate_file(&alias_file);

        assert!(provider.findings_by_file.is_empty());
        assert!(provider.source_by_file.is_empty());
        assert!(!provider.key_by_display_path.contains_key(&alias_file));
    }

    #[cfg(unix)]
    #[test]
    fn invalidation_fails_closed_after_one_display_spelling_scanned_two_owners() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let new = temp.path().join("new");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        let old_source = "class OldOwner {}";
        let new_source = "class NewOwner {}";
        std::fs::write(old.join("Scanned.java"), old_source).unwrap();
        std::fs::write(new.join("Scanned.java"), new_source).unwrap();
        std::os::unix::fs::symlink(&old, &alias).unwrap();
        let alias_file = alias.join("Scanned.java");
        let provider = SecurityDiagnosticsProvider::new();

        provider.scan_file(&alias_file, old_source);
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&new, &alias).unwrap();
        provider.scan_file(&alias_file, new_source);
        std::fs::remove_file(&alias).unwrap();

        provider.invalidate_file(&alias_file);

        assert_eq!(provider.findings_by_file.len(), 2);
        assert_eq!(provider.source_by_file.len(), 2);
        assert_eq!(
            provider
                .key_by_display_path
                .get(&alias_file)
                .expect("ambiguous display history should be retained")
                .value()
                .len(),
            2
        );
    }

    #[test]
    fn dependency_graph_changes_invalidate_cached_build_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();
        let build_file = workspace.join("build.gradle");
        let source = "dependencies { implementation 'org.example:application:1.0' }";
        let provider = SecurityDiagnosticsProvider::new();

        crate::project_classpath::record_gradle_resolution(
            workspace,
            workspace,
            GradleResolutionReport {
                dependency_edges: vec![DependencyEdge::direct("org.example:application:1.0")],
                ..GradleResolutionReport::default()
            },
        );
        assert!(provider.scan_file(&build_file, source).is_empty());

        crate::project_classpath::record_gradle_resolution(
            workspace,
            workspace,
            GradleResolutionReport {
                dependency_edges: vec![
                    DependencyEdge::direct("org.example:application:1.0"),
                    DependencyEdge::transitive(
                        "org.example:application:1.0",
                        "org.example:logging:1.0",
                    ),
                    DependencyEdge::transitive(
                        "org.example:logging:1.0",
                        "org.apache.logging.log4j:log4j-core:2.14.1",
                    ),
                ],
                ..GradleResolutionReport::default()
            },
        );

        let diagnostics = provider.scan_file(&build_file, source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(
            "transitive dependency via org.example:application:1.0 -> \
             org.example:logging:1.0 -> org.apache.logging.log4j:log4j-core:2.14.1"
        ));

        crate::project_classpath::reset_project_classpaths(workspace);
    }

    #[test]
    fn session_provider_never_falls_back_to_another_store() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();
        let build_file = workspace.join("build.gradle");
        let source = "dependencies { implementation 'org.example:application:1.0' }";
        crate::project_classpath::record_gradle_resolution(
            workspace,
            workspace,
            GradleResolutionReport {
                dependency_edges: vec![
                    DependencyEdge::direct("org.example:application:1.0"),
                    DependencyEdge::transitive(
                        "org.example:application:1.0",
                        "org.apache.logging.log4j:log4j-core:2.14.1",
                    ),
                ],
                ..GradleResolutionReport::default()
            },
        );
        let provider = SecurityDiagnosticsProvider::with_project_classpaths(
            crate::project_classpath::ProjectClasspathStore::new(),
            Arc::new(RwLock::new(Some(workspace.to_path_buf()))),
        );

        assert!(provider.scan_file(&build_file, source).is_empty());

        crate::project_classpath::reset_project_classpaths(workspace);
    }
}
