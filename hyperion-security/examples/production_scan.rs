// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Example demonstrating production-ready scanning with Phase 4 features
//!
//! Run with: cargo run --example production_scan -- /path/to/java/project

use hyperion_security::{ScanConfig, SecurityScanner};
use std::env;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <directory-to-scan>", args[0]);
        std::process::exit(1);
    }

    let scan_path = PathBuf::from(&args[1]);
    if !scan_path.exists() {
        eprintln!("Error: Path does not exist: {}", scan_path.display());
        std::process::exit(1);
    }

    println!("=== Hyperion Security Scanner - Production Mode ===\n");

    // Configure scanner with all Phase 4 features
    let cache_file = scan_path.join(".hyperion-cache.json");
    let config = ScanConfig::default()
        // Enable incremental caching for fast rescans
        .with_incremental(cache_file.clone())
        // Limit taint analysis depth to prevent performance issues
        .with_max_taint_depth(50)
        // Set per-file timeout to avoid hangs
        .with_file_timeout(Duration::from_secs(10))
        // Use 8 parallel workers (adjust based on CPU)
        .with_max_workers(8)
        // Add progress callback for visibility
        .with_progress_callback(|current, total, path| {
            // Extract just the filename for cleaner output
            let path_buf = PathBuf::from(path);
            let filename = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            eprint!("\rScanning [{:>4}/{:<4}] {}...", current, total, filename);
        });

    println!("Configuration:");
    println!("  Cache file: {}", cache_file.display());
    println!("  Max taint depth: {}", config.max_taint_depth);
    println!("  File timeout: {:?}", config.file_timeout);
    println!("  Workers: {}", config.max_workers);
    println!("  Incremental: {}\n", config.incremental);

    // Create scanner
    let scanner = SecurityScanner::new(config);

    // Perform scan with timing
    let start = Instant::now();
    let (findings, stats) = scanner.scan_directory_with_stats(&scan_path);
    let elapsed = start.elapsed();

    // Clear progress line
    eprintln!("\r{}", " ".repeat(80));

    // Print statistics
    println!("\n=== Scan Statistics ===");
    println!("  Files scanned: {}", stats.files_scanned);
    println!("  Files cached: {}", stats.files_cached);
    println!("  Parse errors: {}", stats.parse_errors);
    println!("  Timeouts: {}", stats.timeouts);
    println!("  Total findings: {}", stats.total_findings);
    println!("  Elapsed time: {:.2}s", elapsed.as_secs_f64());

    if stats.files_scanned > 0 {
        let files_per_sec = stats.files_scanned as f64 / elapsed.as_secs_f64();
        println!("  Throughput: {:.1} files/sec", files_per_sec);
    }

    // Calculate cache effectiveness
    let total_files = stats.files_scanned + stats.files_cached;
    if total_files > 0 {
        let cache_hit_rate = (stats.files_cached as f64 / total_files as f64) * 100.0;
        println!("  Cache hit rate: {:.1}%", cache_hit_rate);
    }

    // Print findings summary
    println!("\n=== Findings Summary ===");
    if findings.is_empty() {
        println!("  No security issues found! ✓");
    } else {
        println!("  Found {} potential security issue(s):\n", findings.len());

        // Group by vulnerability type
        let mut by_type: std::collections::HashMap<_, Vec<_>> = std::collections::HashMap::new();
        for finding in &findings {
            by_type.entry(finding.vuln_type).or_default().push(finding);
        }

        for (vuln_type, findings) in &by_type {
            println!("  {} - {} occurrence(s)", vuln_type, findings.len());
            for finding in findings.iter().take(3) {
                println!(
                    "    - {}:{}",
                    finding.source_location.file.display(),
                    finding.source_location.line
                );
            }
            if findings.len() > 3 {
                println!("    ... and {} more", findings.len() - 3);
            }
        }
    }

    // Print performance tips
    if stats.files_cached > 0 {
        println!("\n💡 Tip: Incremental cache is active. Unchanged files were skipped.");
    } else if stats.files_scanned > 100 {
        println!("\n💡 Tip: Enable --incremental for faster rescans on large codebases.");
    }

    if stats.timeouts > 0 {
        println!(
            "⚠️  Warning: {} file(s) exceeded the timeout. Consider increasing --timeout.",
            stats.timeouts
        );
    }

    if stats.parse_errors > 0 {
        println!(
            "⚠️  Warning: {} file(s) had parse errors. Check logs for details.",
            stats.parse_errors
        );
    }

    // Exit with error code if findings exist
    if !findings.is_empty() {
        std::process::exit(1);
    }
}
