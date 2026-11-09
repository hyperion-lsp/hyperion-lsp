// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for incremental scanning and production hardening features

use hyperion_security::{ScanCache, ScanConfig, SecurityScanner};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Create a test Java file with a known vulnerability
const TIMEOUT_SOURCE: &str = r#"
public class Test {
    public void test(HttpServletRequest request) {
        String id = request.getParameter("id");
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#;

fn create_vuln_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn test_incremental_cache_hit() {
    let temp_dir = TempDir::new().unwrap();
    let cache_file = temp_dir.path().join("cache.json");

    let java_file = create_vuln_file(
        &temp_dir,
        "Test.java",
        r#"
public class Test {
    public void test(HttpServletRequest request) {
        String id = request.getParameter("id");
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#,
    );

    let config = ScanConfig::default().with_incremental(cache_file.clone());

    let scanner = SecurityScanner::new(config.clone());

    // First scan - should scan the file
    let (findings1, stats1) = scanner.scan_files_with_stats(std::slice::from_ref(&java_file));
    assert_eq!(stats1.files_scanned, 1);
    assert_eq!(stats1.files_cached, 0);
    assert!(!findings1.is_empty(), "Should find SQL injection");

    // Second scan - should use cache
    let scanner2 = SecurityScanner::new(config);
    let (findings2, stats2) = scanner2.scan_files_with_stats(&[java_file]);
    assert_eq!(stats2.files_scanned, 0, "Should not rescan");
    assert_eq!(stats2.files_cached, 1, "Should use cache");
    assert_eq!(
        findings1.len(),
        findings2.len(),
        "Cached results should match"
    );
}

#[test]
fn test_incremental_cache_invalidation() {
    let temp_dir = TempDir::new().unwrap();
    let cache_file = temp_dir.path().join("cache.json");

    let java_file = temp_dir.path().join("Test.java");
    fs::write(
        &java_file,
        r#"
public class Test {
    public void test(HttpServletRequest request) {
        String id = request.getParameter("id");
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#,
    )
    .unwrap();

    let config = ScanConfig::default().with_incremental(cache_file.clone());

    let scanner = SecurityScanner::new(config.clone());

    // First scan
    let (findings1, stats1) = scanner.scan_files_with_stats(std::slice::from_ref(&java_file));
    assert_eq!(stats1.files_scanned, 1);
    assert_eq!(stats1.files_cached, 0);
    assert!(!findings1.is_empty());

    // Modify file
    fs::write(
        &java_file,
        r#"
public class Test {
    public void test(HttpServletRequest request) {
        String id = request.getParameter("id");
        // Fixed: using PreparedStatement
        PreparedStatement stmt = conn.prepareStatement("SELECT * FROM users WHERE id = ?");
        stmt.setString(1, id);
        stmt.executeQuery();
    }
}
"#,
    )
    .unwrap();

    // Second scan - cache should be invalidated due to file change
    let scanner2 = SecurityScanner::new(config);
    let (findings2, stats2) = scanner2.scan_files_with_stats(&[java_file]);
    assert_eq!(stats2.files_scanned, 1, "Should rescan changed file");
    assert_eq!(stats2.files_cached, 0, "Cache should be invalid");
    assert!(findings2.is_empty(), "Fixed code should have no findings");
}

#[test]
fn test_scan_with_parse_errors() {
    let temp_dir = TempDir::new().unwrap();

    // Create a file with syntax errors
    let bad_file = create_vuln_file(
        &temp_dir,
        "Bad.java",
        r#"
public class Bad {
    public void test( {  // syntax error
        String x = "incomplete
    }
}
"#,
    );

    // Create a valid file
    let good_file = create_vuln_file(
        &temp_dir,
        "Good.java",
        r#"
public class Good {
    public void test() {
        String x = "complete";
    }
}
"#,
    );

    let config = ScanConfig::default();
    let scanner = SecurityScanner::new(config);

    // Should not crash, just skip bad files
    let (_findings, stats) = scanner.scan_files_with_stats(&[bad_file, good_file]);

    // Should scan both files even if one has errors
    assert!(
        stats.files_scanned >= 1,
        "Should scan at least the good file"
    );
    // Bad file might cause parse error but shouldn't crash
}

#[test]
fn test_progress_callback() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let temp_dir = TempDir::new().unwrap();

    let files: Vec<_> = (0..5)
        .map(|i| {
            create_vuln_file(
                &temp_dir,
                &format!("Test{}.java", i),
                "public class Test {}",
            )
        })
        .collect();

    let progress_calls = Arc::new(AtomicUsize::new(0));
    let progress_calls_clone = progress_calls.clone();

    let config = ScanConfig::default().with_progress_callback(move |_current, _total, _path| {
        progress_calls_clone.fetch_add(1, Ordering::Relaxed);
    });

    let scanner = SecurityScanner::new(config);
    scanner.scan_files_with_stats(&files);

    assert_eq!(
        progress_calls.load(Ordering::Relaxed),
        5,
        "Progress callback should be called for each file"
    );
}

#[test]
fn test_max_workers_configuration() {
    let temp_dir = TempDir::new().unwrap();

    let files: Vec<_> = (0..10)
        .map(|i| {
            create_vuln_file(
                &temp_dir,
                &format!("Test{}.java", i),
                "public class Test {}",
            )
        })
        .collect();

    // Test with limited workers
    let config = ScanConfig::default().with_max_workers(2);
    let scanner = SecurityScanner::new(config);

    let (findings, stats) = scanner.scan_files_with_stats(&files);

    // Should complete successfully with limited workers
    assert_eq!(stats.files_scanned, 10);
    assert_eq!(findings.len(), 0); // No vulns in empty classes
}

#[test]
fn test_cache_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let cache_file = temp_dir.path().join("cache.json");

    let java_file = create_vuln_file(
        &temp_dir,
        "Test.java",
        r#"
public class Test {
    public void test(HttpServletRequest request) {
        String id = request.getParameter("id");
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#,
    );

    // First scanner instance
    {
        let config = ScanConfig::default().with_incremental(cache_file.clone());
        let scanner = SecurityScanner::new(config);
        scanner.scan_files_with_stats(std::slice::from_ref(&java_file));
    }

    // Cache file should exist
    assert!(cache_file.exists(), "Cache file should be persisted");

    // Load cache in new instance
    let cache = ScanCache::load(&cache_file);
    assert!(!cache.is_empty(), "Cache should have entries");

    // New scanner instance should use persisted cache
    {
        let config = ScanConfig::default().with_incremental(cache_file);
        let scanner = SecurityScanner::new(config);
        let (_, stats) = scanner.scan_files_with_stats(&[java_file]);
        assert_eq!(stats.files_cached, 1, "Should use persisted cache");
    }
}

#[test]
fn test_timeout_handling() {
    use std::time::Duration;

    let temp_dir = TempDir::new().unwrap();
    let java_file = create_vuln_file(&temp_dir, "Test.java", TIMEOUT_SOURCE);

    // Control: without a budget the file is scanned and its finding is reported
    let (findings, stats) = SecurityScanner::new(ScanConfig::default())
        .scan_files_with_stats(std::slice::from_ref(&java_file));
    assert!(!findings.is_empty());
    assert_eq!(stats.timeouts, 0);

    // A zero budget is always exceeded: the file is counted as timed out and reports nothing
    let config = ScanConfig::default().with_file_timeout(Duration::ZERO);
    let (findings, stats) = SecurityScanner::new(config).scan_files_with_stats(&[java_file]);
    assert!(
        findings.is_empty(),
        "a file over its budget must not report partial findings"
    );
    assert_eq!(stats.timeouts, 1);
}

#[test]
fn test_timed_out_file_is_not_replayed_from_cache() {
    use std::time::Duration;

    let temp_dir = TempDir::new().unwrap();
    let cache_file = temp_dir.path().join("cache.json");
    let java_file = create_vuln_file(&temp_dir, "Test.java", TIMEOUT_SOURCE);

    {
        let config = ScanConfig::default()
            .with_incremental(cache_file.clone())
            .with_file_timeout(Duration::ZERO);
        SecurityScanner::new(config).scan_files_with_stats(std::slice::from_ref(&java_file));
    }

    let config = ScanConfig::default().with_incremental(cache_file);
    let (findings, stats) = SecurityScanner::new(config).scan_files_with_stats(&[java_file]);
    assert_eq!(stats.files_cached, 0, "a timed-out scan must not be cached");
    assert!(!findings.is_empty());
}

#[test]
fn test_scan_stats_accuracy() {
    let temp_dir = TempDir::new().unwrap();
    let cache_file = temp_dir.path().join("cache.json");

    // Create files with and without vulnerabilities
    let vuln_file = create_vuln_file(
        &temp_dir,
        "Vuln.java",
        r#"
public class Vuln {
    public void test(HttpServletRequest request) {
        String cmd = request.getParameter("cmd");
        Runtime.getRuntime().exec(cmd);
    }
}
"#,
    );

    let safe_file = create_vuln_file(
        &temp_dir,
        "Safe.java",
        "public class Safe { public void test() {} }",
    );

    let config = ScanConfig::default().with_incremental(cache_file);

    let scanner = SecurityScanner::new(config);
    let (findings, stats) = scanner.scan_files_with_stats(&[vuln_file, safe_file]);

    assert_eq!(stats.files_scanned, 2);
    assert_eq!(stats.files_cached, 0);
    assert!(stats.total_findings > 0, "Should find command injection");
    assert_eq!(stats.total_findings, findings.len());
}

#[test]
fn test_cache_from_another_rule_version_is_not_replayed() {
    let temp_dir = TempDir::new().unwrap();
    let cache_file = temp_dir.path().join("cache.json");
    let java_file = create_vuln_file(
        &temp_dir,
        "Test.java",
        r#"
public class Test {
    public void test(HttpServletRequest request) {
        String id = request.getParameter("id");
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#,
    );

    {
        let config = ScanConfig::default().with_incremental(cache_file.clone());
        SecurityScanner::new(config).scan_files_with_stats(std::slice::from_ref(&java_file));
    }

    // Make the saved cache look as if an older set of checks wrote it
    let contents = fs::read_to_string(&cache_file).unwrap();
    let mut saved: serde_json::Value = serde_json::from_str(&contents).unwrap();
    saved.as_object_mut().unwrap().remove("version");
    fs::write(&cache_file, saved.to_string()).unwrap();

    let config = ScanConfig::default().with_incremental(cache_file);
    let (_, stats) = SecurityScanner::new(config).scan_files_with_stats(&[java_file]);
    assert_eq!(
        stats.files_cached, 0,
        "entries written under another rule version must not be replayed"
    );
    assert_eq!(stats.files_scanned, 1);
}
