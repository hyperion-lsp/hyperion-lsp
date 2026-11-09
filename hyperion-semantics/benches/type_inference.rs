// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Benchmarks for var type inference performance
//!
//! These benchmarks verify the performance claims:
//! - O(n²) → O(n) improvement from caching
//! - Linear scaling with number of var declarations
//!
//! Run with: cargo bench --bench type_inference

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use hyperion_semantics::db::{Database, FileId, Language, SourceDatabase};
use hyperion_semantics::{ParseDatabase, SemanticDatabase};
use std::sync::Arc;

/// Generate test source with N var declarations in a chain
/// Each var depends on the previous one, testing cascading inference
fn generate_var_chain(var_count: usize) -> String {
    let mut source = String::from("package com.example;\n\n");
    source.push_str("import java.util.List;\n");
    source.push_str("import java.util.ArrayList;\n\n");
    source.push_str("public class Test {\n");
    source.push_str("    public void test() {\n");

    // First var: ArrayList<String>
    source.push_str("        var v0 = new ArrayList<String>();\n");

    // Chain of dependent vars: each gets the previous var
    for i in 1..var_count {
        source.push_str(&format!("        var v{} = v{};\n", i, i - 1));
    }

    source.push_str("    }\n");
    source.push_str("}\n");
    source
}

/// Generate test source with N independent var declarations
/// Tests parallel inference without dependencies
fn generate_independent_vars(var_count: usize) -> String {
    let mut source = String::from("package com.example;\n\n");
    source.push_str("import java.util.List;\n");
    source.push_str("import java.util.ArrayList;\n");
    source.push_str("import java.util.HashMap;\n\n");
    source.push_str("public class Test {\n");
    source.push_str("    public void test() {\n");

    for i in 0..var_count {
        let type_idx = i % 3;
        let init_expr = match type_idx {
            0 => "new ArrayList<String>()",
            1 => "new HashMap<String, Integer>()",
            _ => "\"test\"",
        };
        source.push_str(&format!("        var v{} = {};\n", i, init_expr));
    }

    source.push_str("    }\n");
    source.push_str("}\n");
    source
}

/// Generate test source with method calls
fn generate_method_chain_vars(var_count: usize) -> String {
    let mut source = String::from("package com.example;\n\n");
    source.push_str("import java.util.List;\n");
    source.push_str("import java.util.ArrayList;\n\n");
    source.push_str("public class Test {\n");
    source.push_str("    private ArrayList<String> getList() {\n");
    source.push_str("        return new ArrayList<>();\n");
    source.push_str("    }\n");
    source.push_str("    public void test() {\n");

    for i in 0..var_count {
        source.push_str(&format!("        var v{} = getList();\n", i));
    }

    source.push_str("    }\n");
    source.push_str("}\n");
    source
}

/// Benchmark var type inference scaling
fn bench_var_inference_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("var_inference_scaling");

    // Test with different numbers of var declarations
    for &var_count in &[10, 20, 50, 100, 200, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(var_count),
            &var_count,
            |b, &count| {
                let source = generate_var_chain(count);
                b.iter(|| {
                    let mut db = Database::new();
                    let file_id = FileId(1);
                    db.set_file_text(file_id, Arc::new(source.clone()));
                    db.set_file_language(file_id, Language::Java);

                    // Trigger parsing and symbol extraction
                    let _ = black_box(db.parse_file(file_id));
                    let _ = black_box(db.file_symbols(file_id));
                });
            },
        );
    }

    group.finish();
}

/// Benchmark independent var inference (no dependencies)
fn bench_independent_vars(c: &mut Criterion) {
    let mut group = c.benchmark_group("independent_var_inference");

    for &var_count in &[10, 50, 100, 200, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(var_count),
            &var_count,
            |b, &count| {
                let source = generate_independent_vars(count);
                b.iter(|| {
                    let mut db = Database::new();
                    let file_id = FileId(1);
                    db.set_file_text(file_id, Arc::new(source.clone()));
                    db.set_file_language(file_id, Language::Java);

                    let _ = black_box(db.parse_file(file_id));
                    let _ = black_box(db.file_symbols(file_id));
                });
            },
        );
    }

    group.finish();
}

/// Benchmark method call var inference
fn bench_method_chain_vars(c: &mut Criterion) {
    let mut group = c.benchmark_group("method_chain_var_inference");

    for &var_count in &[10, 50, 100, 200] {
        group.bench_with_input(
            BenchmarkId::from_parameter(var_count),
            &var_count,
            |b, &count| {
                let source = generate_method_chain_vars(count);
                b.iter(|| {
                    let mut db = Database::new();
                    let file_id = FileId(1);
                    db.set_file_text(file_id, Arc::new(source.clone()));
                    db.set_file_language(file_id, Language::Java);

                    let _ = black_box(db.parse_file(file_id));
                    let _ = black_box(db.file_symbols(file_id));
                });
            },
        );
    }

    group.finish();
}

/// Benchmark file re-parsing (tests Salsa caching)
fn bench_file_reparsing(c: &mut Criterion) {
    let source = generate_var_chain(100);
    let mut db = Database::new();
    let file_id = FileId(1);
    db.set_file_text(file_id, Arc::new(source.clone()));
    db.set_file_language(file_id, Language::Java);

    // Prime the cache
    let _ = db.parse_file(file_id);
    let _ = db.file_symbols(file_id);

    c.bench_function("reparse_cached_file", |b| {
        b.iter(|| {
            // Re-parse same content (should be cached)
            let _ = black_box(db.parse_file(file_id));
            let _ = black_box(db.file_symbols(file_id));
        });
    });
}

criterion_group!(
    benches,
    bench_var_inference_scaling,
    bench_independent_vars,
    bench_method_chain_vars,
    bench_file_reparsing
);
criterion_main!(benches);
