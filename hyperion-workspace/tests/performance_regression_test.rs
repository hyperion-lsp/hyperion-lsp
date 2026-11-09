// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Performance regression tests for indexing operations
//!
//! These tests verify that performance improvements (like Arc<str> migration)
//! don't regress over time.

use hyperion_workspace::WorkspaceIndex;
use std::sync::Arc;
use tempfile::TempDir;

/// Generate a test Java file with many symbols to stress-test indexing
fn generate_large_class(class_name: &str, symbol_count: usize) -> String {
    let mut source = "package com.test;\n\n".to_string();
    source.push_str(&format!("public class {} {{\n", class_name));

    // Add fields
    for i in 0..symbol_count / 3 {
        source.push_str(&format!("    private String field{};\n", i));
    }

    source.push('\n');

    // Add methods
    for i in 0..symbol_count * 2 / 3 {
        source.push_str(&format!("    public void method{}() {{ }}\n", i));
    }

    source.push_str("}\n");
    source
}

#[test]
fn test_indexing_1000_symbols_completes_quickly() {
    // Create workspace with 10 files, 100 symbols each = 1000 total symbols
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();

    // Create package directory
    let package_dir = workspace_root.join("com").join("test");
    std::fs::create_dir_all(&package_dir).unwrap();

    // Generate 10 files with 100 symbols each
    for i in 0..10 {
        let class_name = format!("TestClass{}", i);
        let source = generate_large_class(&class_name, 100);
        let file_path = package_dir.join(format!("{}.java", class_name));
        std::fs::write(file_path, source).unwrap();
    }

    // Measure indexing time
    let index = WorkspaceIndex::new(workspace_root);
    let start = std::time::Instant::now();

    // Index all files by querying for them (triggers lazy indexing)
    for i in 0..10 {
        let fqn = format!("com.test.TestClass{}", i);
        let _ = index.find_symbol(&fqn);
    }

    let elapsed = start.elapsed();

    // Performance target: 1000 symbols should index in <100ms
    // With Arc<str> optimization, we expect this to be well under target
    println!("Indexed 1000 symbols in {:?}", elapsed);
    assert!(
        elapsed.as_millis() < 100,
        "Indexing 1000 symbols took {:?}, expected <100ms",
        elapsed
    );
}

#[test]
fn test_arc_str_clone_is_cheap() {
    // This test verifies that Arc<str> clones are O(1) cheap operations
    // by doing many clones and ensuring the time scales linearly with count,
    // not quadratically

    let fqn: Arc<str> = Arc::from("com.example.VeryLongFullyQualifiedNameForTestingPurposes");

    let start = std::time::Instant::now();
    let mut clones = Vec::new();

    // Do 10,000 clones
    for _ in 0..10_000 {
        clones.push(fqn.clone());
    }

    let elapsed_10k = start.elapsed();

    // Do another 10,000 clones (total 20,000)
    let start2 = std::time::Instant::now();
    for _ in 0..10_000 {
        clones.push(fqn.clone());
    }
    let elapsed_20k = start2.elapsed();

    println!(
        "10K clones: {:?}, next 10K clones: {:?}",
        elapsed_10k, elapsed_20k
    );

    // If clones are O(1), doubling the work should roughly double the time
    // Allow 3x variance for noise
    let ratio = elapsed_20k.as_nanos() as f64 / elapsed_10k.as_nanos() as f64;
    assert!(
        ratio < 3.0,
        "Clone time ratio {:.2} suggests non-O(1) behavior",
        ratio
    );

    // Both should be very fast (sub-millisecond)
    assert!(
        elapsed_10k.as_millis() < 10,
        "10K Arc clones took {:?}, expected <10ms",
        elapsed_10k
    );
}

#[test]
fn test_reindexing_same_content_is_fast() {
    // This test verifies that re-indexing the same content multiple times
    // is fast due to Arc<str> sharing and optimization

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let package_dir = workspace_root.join("com").join("test");
    std::fs::create_dir_all(&package_dir).unwrap();

    let source = generate_large_class("TestClass", 100);
    let file_path = package_dir.join("TestClass.java");
    std::fs::write(&file_path, &source).unwrap();

    let index = WorkspaceIndex::new(workspace_root);

    // First indexing (cold)
    let start = std::time::Instant::now();
    let file_id = hyperion_workspace::FileId(1);
    let _ = index.index_file_with_source_and_id(&file_path, &source, file_id);
    let first_index = start.elapsed();

    // Re-index same content 10 times (should be fast)
    let start = std::time::Instant::now();
    for _ in 0..10 {
        let _ = index.index_file_with_source_and_id(&file_path, &source, file_id);
    }
    let reindex_10x = start.elapsed();

    println!(
        "First index: {:?}, 10x reindex: {:?} (avg: {:?})",
        first_index,
        reindex_10x,
        reindex_10x / 10
    );

    // Average re-index should be similar to first index
    // (within 2x due to cache effects)
    let avg_reindex = reindex_10x / 10;
    let ratio = avg_reindex.as_nanos() as f64 / first_index.as_nanos() as f64;

    assert!(
        ratio < 2.0,
        "Re-indexing is {:.2}x slower than first index, expected <2x",
        ratio
    );
}

#[test]
fn test_symbol_lookup_is_fast() {
    // Verify that symbol lookups are sub-microsecond after indexing

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let package_dir = workspace_root.join("com").join("test");
    std::fs::create_dir_all(&package_dir).unwrap();

    // Create 100 files
    for i in 0..100 {
        let class_name = format!("TestClass{}", i);
        let source = generate_large_class(&class_name, 10);
        let file_path = package_dir.join(format!("{}.java", class_name));
        std::fs::write(file_path, source).unwrap();
    }

    let index = WorkspaceIndex::new(workspace_root);

    // Index all files
    for i in 0..100 {
        let fqn = format!("com.test.TestClass{}", i);
        index.find_symbol(&fqn);
    }

    // Now measure lookup time for 1000 lookups
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let fqn = "com.test.TestClass42";
        let _ = index.get_symbol(fqn);
    }
    let elapsed = start.elapsed();

    println!("1000 lookups in {:?} (avg: {:?})", elapsed, elapsed / 1000);

    // Average lookup should be <1µs (1000ns)
    let avg_nanos = elapsed.as_nanos() / 1000;
    assert!(
        avg_nanos < 1_000,
        "Average lookup time {}ns, expected <1000ns",
        avg_nanos
    );
}
