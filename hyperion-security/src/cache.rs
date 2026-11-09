// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Incremental caching for scan results
//!
//! Avoids rescanning unchanged files by caching findings based on file content hash.

use crate::finding::Finding;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_SCAN_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// A SHA-256 hash of file contents
type FileHash = [u8; 32];

/// Version of what the checks report. Bump it when a check changes its findings for the
/// same source. A cache written under another version is discarded on load, so its
/// findings are not replayed.
pub const SCAN_CACHE_VERSION: u32 = 1;

/// Cache entry for a scanned file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Hash of the file content when scanned
    pub content_hash: FileHash,

    /// When this file was last scanned
    pub scanned_at: SystemTime,

    /// Findings from the scan (empty if no issues)
    pub findings: Vec<Finding>,
}

/// Incremental scan cache
///
/// Stores scan results keyed by file path and content hash.
/// If a file hasn't changed (same hash), we can skip rescanning.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScanCache {
    /// Rule version the entries were written under (see [`SCAN_CACHE_VERSION`])
    #[serde(default)]
    version: u32,

    /// Map from absolute file path to cache entry
    entries: HashMap<PathBuf, CacheEntry>,

    /// Path to cache file on disk
    #[serde(skip)]
    cache_file: Option<PathBuf>,
}

impl Default for ScanCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            version: SCAN_CACHE_VERSION,
            entries: HashMap::new(),
            cache_file: None,
        }
    }

    /// Load cache from disk
    ///
    /// Returns a new cache if the file doesn't exist or can't be read.
    pub fn load(path: &Path) -> Self {
        // Validate path to prevent path traversal attacks
        if let Err(e) = validate_cache_path(path) {
            eprintln!("Warning: Invalid cache path: {e}");
            return Self::new();
        }

        match hyperion_workspace::read_utf8_file_bounded(path, MAX_SCAN_CACHE_BYTES) {
            Ok(contents) => match serde_json::from_str::<Self>(&contents) {
                Ok(mut cache) => {
                    if cache.version != SCAN_CACHE_VERSION {
                        // Entries from another rule version would replay stale findings
                        cache = Self::new();
                    }
                    cache.cache_file = Some(path.to_path_buf());
                    cache
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse cache file: {e}");
                    let mut cache = Self::new();
                    cache.cache_file = Some(path.to_path_buf());
                    cache
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let mut cache = Self::new();
                cache.cache_file = Some(path.to_path_buf());
                cache
            }
            Err(e) => {
                eprintln!("Warning: Failed to read cache file: {e}");
                let mut cache = Self::new();
                cache.cache_file = Some(path.to_path_buf());
                cache
            }
        }
    }

    /// Save cache to disk
    pub fn save(&self) -> io::Result<()> {
        let Some(ref path) = self.cache_file else {
            return Ok(()); // No cache file configured
        };

        // Validate path to prevent path traversal attacks
        validate_cache_path(path)?;

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(path, json)?;

        Ok(())
    }

    /// Check if a file is cached and unchanged
    ///
    /// Returns cached findings if file hash matches, None otherwise.
    pub fn get(&self, file_path: &Path, current_hash: &FileHash) -> Option<&[Finding]> {
        let entry = self.entries.get(file_path)?;

        if &entry.content_hash == current_hash {
            Some(&entry.findings)
        } else {
            None // Hash mismatch, file changed
        }
    }

    /// Store scan results for a file
    pub fn insert(&mut self, file_path: PathBuf, content_hash: FileHash, findings: Vec<Finding>) {
        let entry = CacheEntry {
            content_hash,
            scanned_at: SystemTime::now(),
            findings,
        };
        self.entries.insert(file_path, entry);
    }

    /// Remove stale entries for files that no longer exist
    pub fn prune_missing_files(&mut self) {
        self.entries.retain(|path, _| path.exists());
    }

    /// Clear all cache entries
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get number of cached files
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Compute the SHA-256 hash of a file up to an explicit size limit.
pub fn hash_file(path: &Path, maximum: usize) -> io::Result<FileHash> {
    use sha2::{Digest, Sha256};

    let mut file = fs::File::open(path)?;
    if file.metadata()?.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds {maximum} bytes"),
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    let mut total = 0usize;

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if total > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file exceeds {maximum} bytes"),
            ));
        }
        hasher.update(&buffer[..n]);
    }

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    Ok(hash)
}

/// Compute SHA-256 hash of string contents
pub fn hash_string(contents: &str) -> FileHash {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(contents.as_bytes());
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Validate cache file path to prevent path traversal attacks
///
/// Ensures the path:
/// - Is relative to current directory or absolute within a safe location
/// - Doesn't contain suspicious path components like "../../../etc/passwd"
/// - Canonicalizes to a path under the current working directory (for relative paths)
fn validate_cache_path(path: &Path) -> io::Result<()> {
    // Get the absolute/canonical path
    let canonical = if path.is_absolute() {
        // For absolute paths, ensure they don't point to sensitive system directories
        // Allow /tmp, /var/tmp, /var/folders (macOS temp), and user home directories
        let path_str = path.to_string_lossy();
        let is_temp = path_str.starts_with("/tmp/")
            || path_str.starts_with("/var/tmp/")
            || path_str.starts_with("/var/folders/"); // macOS temp
        let is_sensitive_system = path_str.starts_with("/etc/")
            || path_str.starts_with("/usr/")
            || path_str.starts_with("/sys/")
            || path_str.starts_with("/proc/")
            || path_str.starts_with("/boot/")
            || path_str.starts_with("/root/");

        if is_sensitive_system && !is_temp {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Cache path cannot be in sensitive system directories",
            ));
        }
        path.to_path_buf()
    } else {
        // For relative paths, resolve against current directory
        {
            let cwd = std::env::current_dir()?;
            let resolved = cwd.join(path);
            // Canonicalize if the parent exists, otherwise just use the resolved path
            match resolved.canonicalize() {
                Ok(canonical) => canonical,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // File doesn't exist yet, which is fine - check parent
                    if let Some(parent) = resolved.parent() {
                        if parent.exists() {
                            parent.join(resolved.file_name().ok_or_else(|| {
                                io::Error::new(io::ErrorKind::InvalidInput, "Invalid cache path")
                            })?)
                        } else {
                            resolved
                        }
                    } else {
                        resolved
                    }
                }
                Err(e) => return Err(e),
            }
        }
    };

    // Ensure the canonical path doesn't contain suspicious components
    for component in canonical.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Cache path cannot contain '..' after canonicalization",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Location;
    use crate::VulnType;

    fn dummy_finding() -> Finding {
        Finding::new(
            VulnType::SqlInjection,
            "Test finding".to_string(),
            Location {
                file: PathBuf::from("test.java"),
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 10,
            },
            "test code".to_string(),
        )
    }

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = ScanCache::new();
        let path = PathBuf::from("test.java");
        let hash = [1u8; 32];
        let findings = vec![dummy_finding()];

        cache.insert(path.clone(), hash, findings.clone());

        let cached = cache.get(&path, &hash);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }

    #[test]
    fn test_cache_hash_mismatch() {
        let mut cache = ScanCache::new();
        let path = PathBuf::from("test.java");
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        let findings = vec![dummy_finding()];

        cache.insert(path.clone(), hash1, findings);

        // Different hash should return None
        let cached = cache.get(&path, &hash2);
        assert!(cached.is_none());
    }

    #[test]
    fn test_hash_string_deterministic() {
        let content = "public class Test {}";
        let hash1 = hash_string(content);
        let hash2 = hash_string(content);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_string_different() {
        let content1 = "public class Test1 {}";
        let content2 = "public class Test2 {}";
        let hash1 = hash_string(content1);
        let hash2 = hash_string(content2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn hash_file_respects_size_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source.java");
        fs::write(&path, "large").unwrap();

        let error = hash_file(&path, 4).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
