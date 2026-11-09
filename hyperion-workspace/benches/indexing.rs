// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Benchmarks for workspace indexing performance
//!
//! These benchmarks verify:
//! - Re-indexing optimization (50K+ allocations eliminated)
//! - Symbol lookup performance
//! - Import resolution scaling
//!
//! Run with: cargo bench --bench indexing

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use tempfile::TempDir;

/// Generate test Java file with N classes and M methods per class
fn generate_class_file(class_name: &str, method_count: usize) -> String {
    let mut source = "package com.example;\n\n".to_string();
    source.push_str(&format!("public class {} {{\n", class_name));

    // Add fields
    for i in 0..5 {
        source.push_str(&format!("    private String field{};\n", i));
    }

    source.push('\n');

    // Add constructor
    source.push_str(&format!("    public {}() {{}}\n\n", class_name));

    // Add methods
    for i in 0..method_count {
        source.push_str(&format!(
            "    public void method{}() {{\n        // Method body\n    }}\n\n",
            i
        ));
    }

    source.push_str("}\n");
    source
}

/// Setup a test workspace with multiple files
fn create_workspace_files(file_count: usize, methods_per_file: usize) -> (TempDir, Vec<PathBuf>) {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();

    // Create package directory
    let package_dir = workspace_root.join("com").join("example");
    std::fs::create_dir_all(&package_dir).unwrap();

    // Generate files
    let mut files = Vec::with_capacity(file_count);
    for i in 0..file_count {
        let class_name = format!("TestClass{}", i);
        let source = generate_class_file(&class_name, methods_per_file);
        let file_path = package_dir.join(format!("{}.java", class_name));
        std::fs::write(&file_path, source).unwrap();
        files.push(file_path);
    }

    (temp_dir, files)
}

fn setup_indexed_workspace(
    file_count: usize,
    methods_per_file: usize,
) -> (TempDir, WorkspaceIndex) {
    let (temp_dir, files) = create_workspace_files(file_count, methods_per_file);
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let stats = index.index_files_parallel(&files);
    assert_eq!(
        stats.files_indexed, file_count,
        "benchmark fixture indexing failed"
    );
    assert!(
        index.find_symbol("com.example.TestClass0").is_some(),
        "benchmark fixture must contain indexed symbols"
    );
    (temp_dir, index)
}

/// Benchmark initial file indexing
fn bench_initial_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("initial_indexing");

    for &file_count in &[1, 5, 10, 20] {
        let (temp_dir, files) = create_workspace_files(file_count, 10);
        group.bench_with_input(
            BenchmarkId::from_parameter(file_count),
            &file_count,
            |b, &_count| {
                b.iter(|| {
                    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
                    black_box(index.index_files_parallel(black_box(&files)))
                });
            },
        );
    }

    group.finish();
}

/// Benchmark file re-indexing (measures optimization)
fn bench_file_reindexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_reindexing");

    for &method_count in &[10, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(method_count),
            &method_count,
            |b, &count| {
                let (temp_dir, index) = setup_indexed_workspace(1, count);
                let file_path = temp_dir.path().join("com/example/TestClass0.java");
                let file_id = index
                    .get_file_id(&file_path)
                    .expect("indexed benchmark file");

                // Initial indexing
                let source = generate_class_file("TestClass0", count);
                let _ = index.index_file_with_source_and_id(&file_path, &source, file_id);

                b.iter(|| {
                    // Re-index with same content (tests optimization)
                    let _ = black_box(
                        index.index_file_with_source_and_id(&file_path, &source, file_id),
                    );
                });
            },
        );
    }

    group.finish();
}

/// Benchmark file re-indexing with changed content
fn bench_file_reindexing_changed(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_reindexing_changed");

    for &method_count in &[10, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(method_count),
            &method_count,
            |b, &count| {
                let (temp_dir, index) = setup_indexed_workspace(1, count);
                let file_path = temp_dir.path().join("com/example/TestClass0.java");
                let file_id = index
                    .get_file_id(&file_path)
                    .expect("indexed benchmark file");

                // Initial indexing
                let source = generate_class_file("TestClass0", count);
                let _ = index.index_file_with_source_and_id(&file_path, &source, file_id);

                // Create modified version (add one method)
                let modified_source = generate_class_file("TestClass0", count + 1);

                let mut use_modified = false;
                b.iter_batched(
                    || {
                        use_modified = !use_modified;
                        if use_modified {
                            modified_source.clone()
                        } else {
                            source.clone()
                        }
                    },
                    |iteration_source| {
                        black_box(index.index_file_with_source_and_id(
                            &file_path,
                            &iteration_source,
                            file_id,
                        ))
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmark symbol lookup
fn bench_symbol_lookup(c: &mut Criterion) {
    let (_temp_dir, index) = setup_indexed_workspace(100, 10);
    assert!(!index
        .find_methods_by_name("com.example.TestClass42", "method5")
        .is_empty());

    let mut group = c.benchmark_group("symbol_lookup");

    // Benchmark exact symbol lookup
    group.bench_function("exact_match", |b| {
        b.iter(|| {
            let fqn = "com.example.TestClass42";
            black_box(index.find_symbol(fqn));
        });
    });

    // Benchmark class lookup
    group.bench_function("class_lookup", |b| {
        b.iter(|| {
            black_box(index.find_by_simple_name("TestClass42"));
        });
    });

    group.finish();
}

/// Benchmark method resolution
fn bench_method_resolution(c: &mut Criterion) {
    let (_temp_dir, index) = setup_indexed_workspace(50, 20);

    let mut group = c.benchmark_group("method_resolution");

    group.bench_function("find_methods_by_name", |b| {
        b.iter(|| {
            black_box(index.find_methods_by_name("com.example.TestClass25", "method10"));
        });
    });

    group.finish();
}

/// Benchmark package class name lookup (java.lang optimization)
fn bench_package_class_names(c: &mut Criterion) {
    let (_temp_dir, index) = setup_indexed_workspace(100, 10);
    assert_eq!(index.get_package_class_names("com.example").len(), 100);

    c.bench_function("get_package_class_names", |b| {
        b.iter(|| {
            black_box(index.get_package_class_names("com.example"));
        });
    });
}

criterion_group!(
    benches,
    bench_initial_indexing,
    bench_file_reindexing,
    bench_file_reindexing_changed,
    bench_symbol_lookup,
    bench_method_resolution,
    bench_package_class_names
);
criterion_main!(benches);
