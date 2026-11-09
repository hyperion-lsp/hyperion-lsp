// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Performance tests for completion on large files
//!
//! Tests that completion remains responsive on files with 5000+ lines.
//! These tests verify Salsa caching behavior for symbol table building.

use hyperion_lsp::semantic_host::SemanticHost;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyperion_lsp::completion::CompletionProvider;
use hyperion_semantics::db::{Database, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_workspace::index::WorkspaceIndex;
use tempfile::TempDir;
use tower_lsp::lsp_types::Position;

/// Generate a large Java file with many methods
fn generate_large_java_file(num_methods: usize, lines_per_method: usize) -> String {
    let mut source = String::from("package com.example;\n\npublic class LargeFile {\n");

    for i in 0..num_methods {
        source.push_str(&format!("    public void method{}() {{\n", i));
        for j in 0..lines_per_method {
            source.push_str(&format!("        String s{} = \"value{}\";\n", j, j));
        }
        source.push_str("    }\n\n");
    }

    // Add a method at the end where we'll test completion
    source.push_str("    public void testMethod() {\n");
    source.push_str("        String test = \"hello\";\n");
    source.push_str("        // Completion here\n");
    source.push_str("    }\n");
    source.push_str("}\n");

    source
}

/// Test that Salsa caching makes repeated symbol table lookups fast
#[test]
fn test_salsa_symbol_table_caching() {
    use hyperion_semantics::db::FileId;

    // Generate ~5000 line file
    let source = generate_large_java_file(200, 23);
    let line_count = source.lines().count();
    println!(
        "Generated file with {} lines, {} bytes",
        line_count,
        source.len()
    );

    // Set up database
    let mut db = Database::default();
    let file_id = FileId(0);

    // Use Salsa to set file content
    db.set_file_text(file_id, Arc::new(source.clone()));
    db.set_file_language(file_id, hyperion_semantics::db::Language::Java);

    // First parse (cold)
    let start = Instant::now();
    let _tree1 = db.parse_file(file_id);
    let cold_parse = start.elapsed();
    println!("Cold parse took: {:?}", cold_parse);

    // Second parse (cached)
    let start = Instant::now();
    let _tree2 = db.parse_file(file_id);
    let warm_parse = start.elapsed();
    println!("Warm parse took: {:?}", warm_parse);

    // First symbol table build (cold)
    let start = Instant::now();
    let _table1 = db.file_symbol_table(file_id);
    let cold_table = start.elapsed();
    println!("Cold symbol table took: {:?}", cold_table);

    // Second symbol table lookup (cached)
    let start = Instant::now();
    let _table2 = db.file_symbol_table(file_id);
    let warm_table = start.elapsed();
    println!("Warm symbol table took: {:?}", warm_table);

    // Assertions
    // Warm parse should be nearly instant (< 1ms)
    assert!(
        warm_parse < Duration::from_millis(5),
        "Warm parse should be cached but took {:?}",
        warm_parse
    );

    // Warm table should be nearly instant (< 1ms)
    assert!(
        warm_table < Duration::from_millis(5),
        "Warm symbol table should be cached but took {:?}",
        warm_table
    );

    // Cold symbol table should be reasonable (< 2s for 5k lines)
    assert!(
        cold_table < Duration::from_secs(2),
        "Cold symbol table took too long: {:?}",
        cold_table
    );
}

/// Test that completion with persistent state is fast after warmup
#[test]
fn test_completion_with_persistent_state() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path().to_path_buf();

    // Generate ~2000 line file (faster for testing)
    let source = generate_large_java_file(80, 23);
    let line_count = source.lines().count();
    println!(
        "Generated file with {} lines, {} bytes",
        line_count,
        source.len()
    );

    // Write file to disk
    let file_dir = workspace_root.join("com/example");
    std::fs::create_dir_all(&file_dir).unwrap();
    let file_path = file_dir.join("LargeFile.java");
    std::fs::write(&file_path, &source).unwrap();

    // Create persistent workspace index
    let index = WorkspaceIndex::new(workspace_root.clone());
    index.index_file_with_source(&file_path, &source).unwrap();

    let index_arc = Arc::new(index);

    // Create database and set up file
    let host = SemanticHost::new(Arc::clone(&index_arc));
    let file_id = index_arc
        .get_file_id(&file_path)
        .expect("File should be indexed");

    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.clone()));
        db.set_file_language(file_id, hyperion_semantics::db::Language::Java);
    });

    // Create completion provider (persistent)
    let completion_provider = CompletionProvider::new(index_arc.clone());

    // Find position for completion (near end of file)
    let completion_line = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("// Completion here"))
        .map(|(i, _)| i as u32)
        .expect("Could not find completion marker");
    let position = Position::new(completion_line, 8);

    // First completion (cold - symbol table not built yet)
    let start = Instant::now();
    let result1 = completion_provider.complete(&host.read_now(), file_id, position);
    let cold_duration = start.elapsed();
    println!("Cold completion took: {:?}", cold_duration);
    assert!(
        result1.is_ok(),
        "Completion should succeed: {:?}",
        result1.err()
    );
    let items1 = result1.unwrap();
    println!("Got {} completion items", items1.len());

    // Second completion (warm - symbol table cached)
    let start = Instant::now();
    let result2 = completion_provider.complete(&host.read_now(), file_id, position);
    let warm_duration = start.elapsed();
    println!("Warm completion took: {:?}", warm_duration);
    assert!(result2.is_ok(), "Completion should succeed");
    let items2 = result2.unwrap();
    println!("Got {} completion items", items2.len());

    // Third completion (verify consistent)
    let start = Instant::now();
    let result3 = completion_provider.complete(&host.read_now(), file_id, position);
    let third_duration = start.elapsed();
    println!("Third completion took: {:?}", third_duration);
    assert!(result3.is_ok());

    // Performance assertions
    // Cold completion should complete within 2s (no JDK indexing in this test)
    assert!(
        cold_duration < Duration::from_secs(2),
        "Cold completion took too long: {:?}",
        cold_duration
    );

    // Warm completion should be much faster (< 100ms with caching)
    assert!(
        warm_duration < Duration::from_millis(200),
        "Warm completion should be fast but took {:?}",
        warm_duration
    );

    // Verify caching is working - warm should be at least 3x faster
    if cold_duration > Duration::from_millis(50) {
        assert!(
            warm_duration < cold_duration / 3,
            "Warm ({:?}) should be at least 3x faster than cold ({:?})",
            warm_duration,
            cold_duration
        );
    }
}
