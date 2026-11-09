// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Benchmarks for LSP cold start performance
//!
//! This benchmark suite measures the performance of LSP initialization:
//! - Workspace indexing (with and without cache)
//! - JDK stdlib indexing
//! - Full initialization (workspace + JDK)
//!
//! To run these benchmarks:
//! ```bash
//! cargo bench --bench cold_start
//! ```

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Get the project root directory
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Benchmark workspace indexing (measures file scanning, parsing, symbol extraction)
fn bench_workspace_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("workspace_indexing");

    // Increase measurement time for indexing operations
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    // Test with hyperion-lsp itself (small workspace ~50 files)
    let hyperion_lsp_root = project_root().join("hyperion-lsp");
    if hyperion_lsp_root.exists() {
        group.bench_function("hyperion_lsp", |b| {
            b.iter(|| {
                let index = WorkspaceIndex::new(hyperion_lsp_root.clone());
                black_box(index.build())
            });
        });
    }

    group.finish();
}

/// Benchmark JDK stdlib indexing
fn bench_jdk_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("jdk_indexing");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    group.bench_function("jdk_stdlib_cold", |b| {
        b.iter(|| {
            let workspace_root = project_root().join("hyperion-lsp");
            let workspace_index = Arc::new(WorkspaceIndex::new(workspace_root));
            black_box(hyperion_lsp::jdk_indexer::index_jdk_stdlib(workspace_index))
        });
    });

    group.finish();
}

/// Benchmark full initialization (workspace + JDK in parallel)
/// This simulates the actual LSP server initialization
fn bench_full_initialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_initialization");
    group.measurement_time(Duration::from_secs(40));
    group.sample_size(10);

    let hyperion_lsp_root = project_root().join("hyperion-lsp");
    if hyperion_lsp_root.exists() {
        group.bench_function("parallel_hyperion_lsp", |b| {
            b.iter(|| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let workspace_index = Arc::new(WorkspaceIndex::new(hyperion_lsp_root.clone()));

                    // Spawn both tasks in parallel (as the LSP server does on initialize)
                    let workspace_task = {
                        let index = Arc::clone(&workspace_index);
                        tokio::task::spawn_blocking(move || index.build())
                    };

                    let jdk_task = {
                        let index = Arc::clone(&workspace_index);
                        tokio::task::spawn_blocking(move || {
                            hyperion_lsp::jdk_indexer::index_jdk_stdlib(index)
                        })
                    };

                    // Wait for both to complete
                    let (workspace_result, jdk_result) = tokio::join!(workspace_task, jdk_task);
                    black_box((workspace_result, jdk_result))
                })
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_workspace_indexing,
    bench_jdk_indexing,
    bench_full_initialization
);
criterion_main!(benches);
