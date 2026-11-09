// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Nullability inference scaling and Salsa-cache benchmarks.
//!
//! Run with: `cargo bench -p hyperion-semantics --bench nullability`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use hyperion_semantics::db::{Database, FileId, SemanticDatabase, SourceDatabase};
use hyperion_workspace::Language;
use std::sync::Arc;

fn deep_return_chain(method_count: usize) -> String {
    let mut source = String::from("class Service { void run() {} }\nclass Deep {\n");
    for index in 0..method_count.saturating_sub(1) {
        source.push_str(&format!(
            "  Service value{index}() {{ return value{}(); }}\n",
            index + 1
        ));
    }
    source.push_str(&format!(
        "  Service value{}() {{ return new Service(); }}\n",
        method_count.saturating_sub(1)
    ));
    source.push_str("  void test() { Service value = value0(); value.run(); }\n}\n");
    source
}

fn wide_class(method_count: usize) -> String {
    let mut source = String::from("class Service { void run() {} }\nclass Wide {\n");
    for index in 0..method_count {
        source.push_str(&format!(
            "  Service value{index}() {{ Service value = new Service(); return value; }}\n"
        ));
    }
    source.push_str("  void test() { Service value = value0(); value.run(); }\n}\n");
    source
}

fn project_file(file_index: usize, method_count: usize) -> String {
    let mut source = format!(
        "class Service{file_index} {{ void run() {{}} }}\nclass ProjectFile{file_index} {{\n"
    );
    for method in 0..method_count {
        source.push_str(&format!(
            "  Service{file_index} value{method}() {{ return new Service{file_index}(); }}\n"
        ));
    }
    source.push_str(&format!(
        "  void test() {{ Service{file_index} value = value0(); value.run(); }}\n}}\n"
    ));
    source
}

fn cold_database(source: &str) -> (Database, FileId) {
    let mut database = Database::new();
    let file = FileId::new(1);
    database.set_file_text(file, Arc::new(source.to_string()));
    database.set_file_language(file, Language::Java);
    (database, file)
}

fn bench_cold_scaling(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("nullability_cold");
    group.sample_size(10);

    for method_count in [25, 100, 300] {
        let deep = deep_return_chain(method_count);
        group.bench_with_input(
            BenchmarkId::new("deep_return_chain", method_count),
            &deep,
            |bencher, source| {
                bencher.iter(|| {
                    let (database, file) = cold_database(source);
                    black_box(database.file_nullability(file));
                });
            },
        );

        let wide = wide_class(method_count);
        group.bench_with_input(
            BenchmarkId::new("wide_class", method_count),
            &wide,
            |bencher, source| {
                bencher.iter(|| {
                    let (database, file) = cold_database(source);
                    black_box(database.file_nullability(file));
                });
            },
        );
    }
    group.finish();
}

fn bench_cached_large_file(criterion: &mut Criterion) {
    let source = deep_return_chain(500);
    let (database, file) = cold_database(&source);
    black_box(database.file_nullability(file));

    criterion.bench_function("nullability_cached/deep_return_chain_500", |bencher| {
        bencher.iter(|| black_box(database.file_nullability(file)));
    });
}

fn bench_large_project(criterion: &mut Criterion) {
    const FILE_COUNT: usize = 2_000;
    const METHODS_PER_FILE: usize = 8;
    let sources: Vec<Arc<String>> = (0..FILE_COUNT)
        .map(|index| Arc::new(project_file(index, METHODS_PER_FILE)))
        .collect();
    let mut group = criterion.benchmark_group("nullability_project_2000_files");
    group.sample_size(10);

    group.bench_function("cold", |bencher| {
        bencher.iter(|| {
            let mut database = Database::new();
            for (index, source) in sources.iter().enumerate() {
                let file = FileId::new(index as u32);
                database.set_file_text(file, Arc::clone(source));
                database.set_file_language(file, Language::Java);
            }
            for index in 0..FILE_COUNT {
                black_box(database.file_nullability(FileId::new(index as u32)));
            }
        });
    });

    let mut database = Database::new();
    for (index, source) in sources.iter().enumerate() {
        let file = FileId::new(index as u32);
        database.set_file_text(file, Arc::clone(source));
        database.set_file_language(file, Language::Java);
        black_box(database.file_nullability(file));
    }
    group.bench_function("cached", |bencher| {
        bencher.iter(|| {
            for index in 0..FILE_COUNT {
                black_box(database.file_nullability(FileId::new(index as u32)));
            }
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_cold_scaling,
    bench_cached_large_file,
    bench_large_project
);
criterion_main!(benches);
