// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Benchmarks for core LSP operations
//!
//! This benchmark suite measures the performance of key LSP operations:
//! - Goto Definition
//! - Completion
//! - Hover
//! - Find References
//!
//! To run these benchmarks:
//! ```bash
//! cargo bench --bench lsp_operations
//! ```
//!
//! To compare with jdtls, see BENCHMARKS.md

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use hyperion_lsp::semantic_host::{ReadTxn, SemanticHost};
use hyperion_lsp::{
    completion::CompletionProvider, hover::HoverProvider, navigation::NavigationProvider,
};
use hyperion_semantics::{
    db::{Database, FileId, Language, ParseDatabase, SemanticDatabase},
    semantic_walker::compute_file_semantics,
    WorkspaceIndexHandle,
};
use hyperion_workspace::{QueryEngine, WorkspaceIndex};
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{Position, Url};

/// Test data for benchmarks
struct BenchmarkData {
    _temp_dir: TempDir,
    completion_provider: Arc<CompletionProvider>,
    txn: &'static ReadTxn,
    hover_provider: HoverProvider<'static>,
    navigation_provider: NavigationProvider<'static>,
    file_uri: Url,
    file_id: hyperion_semantics::db::FileId,
    source_text: String,
}

impl BenchmarkData {
    fn new() -> Self {
        // Create a simple test file
        let source_text = r"
package com.example;

import java.util.List;
import java.util.ArrayList;

public class TestClass {
    private String name;
    private int count;

    public TestClass(String name) {
        this.name = name;
        this.count = 0;
    }

    public String getName() {
        return name;
    }

    public void setName(String name) {
        this.name = name;
    }

    public int getCount() {
        return count;
    }

    public void increment() {
        count++;
    }

    public List<String> getNames() {
        List<String> names = new ArrayList<>();
        names.add(this.name);
        return names;
    }

    public void process() {
        String currentName = getName();
        setName(currentName.toUpperCase());
        increment();
        TestClass self = new TestClass(name);
        self.getName();
    }
}
"
        .to_string();

        let temp_dir = tempfile::tempdir().expect("benchmark workspace");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp_dir.path().to_path_buf()));

        let file_id = hyperion_semantics::db::FileId(1);

        // Create file path and URI
        let file_path = temp_dir.path().join("TestClass.java");
        let file_uri = Url::from_file_path(&file_path).unwrap();
        workspace_index
            .index_file_with_source_and_id(&file_path, &source_text, file_id)
            .expect("index LSP benchmark fixture");
        assert!(workspace_index
            .find_symbol("com.example.TestClass")
            .is_some());
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        host.with_db(|db| db.register_file(file_id, Arc::new(source_text.clone()), Language::Java));
        // The fixture lives for the whole benchmark process; leak its read
        // transaction so the providers can borrow it.
        let txn: &'static ReadTxn = Box::leak(Box::new(host.read_now()));
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));

        // Create providers directly
        let completion_provider = Arc::new(CompletionProvider::new(Arc::clone(&workspace_index)));
        let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
        let hover_provider = HoverProvider::new(
            txn,
            Arc::clone(&workspace_index),
            Arc::clone(&source_providers),
        );
        let navigation_provider = NavigationProvider::new(
            txn,
            Arc::clone(&workspace_index),
            Arc::clone(&query_engine),
            source_providers,
        );

        let data = Self {
            _temp_dir: temp_dir,
            completion_provider,
            txn,
            hover_provider,
            navigation_provider,
            file_uri,
            file_id,
            source_text,
        };

        for (name, position) in goto_cases() {
            assert!(
                data.navigation_provider
                    .goto_definition(data.file_id, position, &data.source_text, &data.file_uri,)
                    .unwrap_or_else(|error| panic!("{name} fixture failed: {error}"))
                    .is_some(),
                "{name} fixture did not resolve"
            );
        }
        for (name, position) in hover_cases() {
            assert!(
                data.hover_provider
                    .hover(data.file_id, position, &data.source_text)
                    .unwrap_or_else(|error| panic!("{name} fixture failed: {error}"))
                    .is_some(),
                "{name} fixture produced no hover"
            );
        }
        for (name, position) in completion_cases() {
            assert!(
                !data
                    .completion_provider
                    .complete(data.txn, data.file_id, position)
                    .unwrap_or_else(|error| panic!("{name} fixture failed: {error}"))
                    .is_empty(),
                "{name} fixture produced no completions"
            );
        }
        for (name, position) in reference_cases() {
            assert!(
                !data
                    .navigation_provider
                    .find_references(
                        data.file_id,
                        position,
                        &data.source_text,
                        &data.file_uri,
                        false,
                    )
                    .unwrap_or_else(|error| panic!("{name} fixture failed: {error}"))
                    .is_empty(),
                "{name} fixture produced no references"
            );
        }

        data
    }
}

/// A deliberately adversarial Java file for cold semantic-request benchmarks.
/// It combines many locals in one lexical scope with repeated qualified field
/// accesses, the two shapes most likely to expose accidental per-identifier
/// cloning or repeated hierarchy walks.
struct LargeSemanticData {
    _temp_dir: TempDir,
    workspace_index: Arc<WorkspaceIndex>,
    source_text: Arc<String>,
    file_id: FileId,
    hover_position: Position,
    root: hyperion_parser::SyntaxNode,
    symbol_table: Arc<hyperion_semantics::symbol::SymbolTable>,
}

impl LargeSemanticData {
    fn new(local_count: usize) -> Self {
        let mut source = String::from(
            "package bench;\npublic class LargeSemanticFile {\n    int shared;\n    void run() {\n",
        );
        for index in 0..local_count {
            source.push_str(&format!("        int local{index} = {index};\n"));
        }
        for index in 0..local_count {
            source.push_str(&format!("        this.shared += local{index};\n"));
        }
        source.push_str("    }\n}\n");
        let hover_line = u32::try_from(4 + local_count * 2 - 1).expect("fixture line fits u32");
        let hover_character =
            u32::try_from("        this.shared += ".len()).expect("fixture character fits u32");

        Self::from_source(
            source,
            Position::new(hover_line, hover_character),
            "LargeSemanticFile.java",
        )
    }

    fn with_inherited_field(access_count: usize, inheritance_depth: usize) -> Self {
        assert!(inheritance_depth > 0, "fixture needs an inheritance edge");
        let mut source = String::from("package bench;\nclass Base0 { int shared; }\n");
        for depth in 1..=inheritance_depth {
            source.push_str(&format!(
                "class Base{depth} extends Base{} {{}}\n",
                depth - 1
            ));
        }
        source.push_str(&format!(
            "public class LargeInheritedFile extends Base{inheritance_depth} {{\n    void run() {{\n"
        ));
        for _ in 0..access_count {
            source.push_str("        this.shared++;\n");
        }
        source.push_str("    }\n}\n");

        Self::from_source(source, Position::new(0, 0), "LargeInheritedFile.java")
    }

    fn from_source(source: String, hover_position: Position, file_name: &str) -> Self {
        let temp_dir = tempfile::tempdir().expect("large semantic benchmark workspace");
        let file_path = temp_dir.path().join("bench").join(file_name);
        let workspace_index = Arc::new(WorkspaceIndex::new(temp_dir.path().to_path_buf()));
        workspace_index
            .index_file_with_source(&file_path, &source)
            .expect("index large semantic benchmark fixture");
        let file_id = workspace_index
            .get_file_id(&file_path)
            .expect("large fixture file id");
        let source_text = Arc::new(source);

        let mut db = Database::new();
        db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(&workspace_index)));
        db.register_file(file_id, Arc::clone(&source_text), Language::Java);
        let root = db.parse_file(file_id).syntax();
        let symbol_table = db.file_symbol_table(file_id);

        Self {
            _temp_dir: temp_dir,
            workspace_index,
            source_text,
            file_id,
            hover_position,
            root,
            symbol_table,
        }
    }

    /// A fresh database and read transaction: the cold state the request pays for.
    fn cold_request(&self) -> ReadTxn {
        ReadTxn::scratch(|database| {
            database.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(
                &self.workspace_index,
            )));
            database.register_file(self.file_id, Arc::clone(&self.source_text), Language::Java);
        })
    }

    fn hover_provider<'txn>(&self, txn: &'txn ReadTxn) -> HoverProvider<'txn> {
        HoverProvider::new(
            txn,
            Arc::clone(&self.workspace_index),
            Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
        )
    }
}

/// A large reference query with many same-spelled local variables but only
/// two true field locations. This catches candidate scans that re-run full
/// navigation for every textual match.
struct LargeReferenceData {
    _temp_dir: TempDir,
    workspace_index: Arc<WorkspaceIndex>,
    source_text: Arc<String>,
    file_id: FileId,
    file_uri: Url,
}

impl LargeReferenceData {
    fn new(noise_count: usize) -> Self {
        let mut source = String::from(
            "package bench;\npublic class LargeReferenceFile {\n    private int value;\n    void touch() {\n",
        );
        for index in 0..noise_count {
            source.push_str(&format!("        {{ int value = {index}; value++; }}\n"));
        }
        source.push_str("        this.value++;\n    }\n}\n");

        let temp_dir = tempfile::tempdir().expect("large reference benchmark workspace");
        let file_path = temp_dir.path().join("bench/LargeReferenceFile.java");
        let file_uri = Url::from_file_path(&file_path).expect("large reference fixture URI");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp_dir.path().to_path_buf()));
        workspace_index
            .index_file_with_source(&file_path, &source)
            .expect("index large reference benchmark fixture");
        let file_id = workspace_index
            .get_file_id(&file_path)
            .expect("large reference fixture file id");

        Self {
            _temp_dir: temp_dir,
            workspace_index,
            source_text: Arc::new(source),
            file_id,
            file_uri,
        }
    }

    /// A fresh database and read transaction: the cold state the request pays for.
    fn cold_request(&self) -> ReadTxn {
        ReadTxn::scratch(|database| {
            database.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(
                &self.workspace_index,
            )));
            database.register_file(self.file_id, Arc::clone(&self.source_text), Language::Java);
        })
    }

    fn navigation_provider<'txn>(&self, txn: &'txn ReadTxn) -> NavigationProvider<'txn> {
        NavigationProvider::new(
            txn,
            Arc::clone(&self.workspace_index),
            Arc::new(QueryEngine::new(self.workspace_index.as_ref().clone())),
            Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
        )
    }
}

fn goto_cases() -> [(&'static str, Position); 3] {
    [
        ("simple_method_call", Position::new(38, 30)),
        ("field_access", Position::new(33, 24)),
        ("constructor_call", Position::new(41, 31)),
    ]
}

fn completion_cases() -> [(&'static str, Position); 3] {
    [
        ("member_access", Position::new(42, 13)),
        ("type_position", Position::new(32, 9)),
        ("general", Position::new(38, 8)),
    ]
}

fn hover_cases() -> [(&'static str, Position); 3] {
    [
        ("method_name", Position::new(15, 19)),
        ("field_name", Position::new(7, 20)),
        ("method_call", Position::new(38, 30)),
    ]
}

fn reference_cases() -> [(&'static str, Position); 2] {
    [
        ("method", Position::new(15, 19)),
        ("field", Position::new(7, 20)),
    ]
}

/// Benchmark goto-definition operation
fn bench_goto_definition(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("goto_definition");

    for (name, position) in goto_cases() {
        group.bench_with_input(BenchmarkId::from_parameter(name), &position, |b, pos| {
            b.iter(|| {
                let result = data.navigation_provider.goto_definition(
                    black_box(data.file_id),
                    black_box(*pos),
                    black_box(&data.source_text),
                    black_box(&data.file_uri),
                );
                black_box(result)
            });
        });
    }

    group.finish();
}

/// Benchmark completion operation
fn bench_completion(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("completion");

    for (name, position) in completion_cases() {
        group.bench_with_input(BenchmarkId::from_parameter(name), &position, |b, pos| {
            b.iter(|| {
                let result = data.completion_provider.complete(
                    data.txn,
                    black_box(data.file_id),
                    black_box(*pos),
                );
                black_box(result)
            });
        });
    }

    group.finish();
}

/// Benchmark hover operation
fn bench_hover(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("hover");

    for (name, position) in hover_cases() {
        group.bench_with_input(BenchmarkId::from_parameter(name), &position, |b, pos| {
            b.iter(|| {
                let result = data.hover_provider.hover(
                    black_box(data.file_id),
                    black_box(*pos),
                    black_box(&data.source_text),
                );
                black_box(result)
            });
        });
    }

    group.finish();
}

/// Benchmark find-references operation
fn bench_find_references(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("find_references");

    for (name, position) in reference_cases() {
        group.bench_with_input(BenchmarkId::from_parameter(name), &position, |b, pos| {
            b.iter(|| {
                let result = data.navigation_provider.find_references(
                    black_box(data.file_id),
                    black_box(*pos),
                    black_box(&data.source_text),
                    black_box(&data.file_uri),
                    black_box(false), // exclude declaration
                );
                black_box(result)
            });
        });
    }

    group.finish();
}

/// Benchmark a sequential mixed-request batch.
fn bench_mixed_request_batch(c: &mut Criterion) {
    let data = BenchmarkData::new();

    c.bench_function("mixed_request_batch", |b| {
        b.iter(|| {
            let pos1 = Position::new(38, 30); // goto-def
            let pos2 = Position::new(42, 13); // completion
            let pos3 = Position::new(15, 19); // hover

            let r1 = data.navigation_provider.goto_definition(
                data.file_id,
                pos1,
                &data.source_text,
                &data.file_uri,
            );
            let r2 = data
                .completion_provider
                .complete(data.txn, data.file_id, pos2);
            let r3 = data
                .hover_provider
                .hover(data.file_id, pos3, &data.source_text);

            black_box((r1, r2, r3))
        });
    });
}

/// Guard against request snapshot creation regressing to O(workspace files).
fn bench_semantic_snapshot(c: &mut Criterion) {
    const FILE_COUNT: u32 = 10_000;

    let mut db = Database::new();
    for file in 0..FILE_COUNT {
        db.register_file(
            FileId::new(file),
            Arc::new(format!("class BenchmarkFile{file} {{}}")),
            Language::Java,
        );
    }

    c.bench_function("semantic_snapshot/10k_registered_files", |b| {
        b.iter(|| black_box(db.snapshot()));
    });
}

/// Track cold whole-file semantic construction separately from request setup.
/// This caught a regression where every name cloned all variables visible at
/// that point, making large methods quadratic before hover could answer.
fn bench_large_file_semantics(c: &mut Criterion) {
    let data = LargeSemanticData::new(1_200);
    let mut group = c.benchmark_group("large_file_semantics");
    group.sample_size(20);
    group.bench_function("1200_locals_and_field_accesses", |b| {
        b.iter(|| {
            black_box(compute_file_semantics(
                black_box(&data.root),
                black_box(data.file_id),
                black_box(data.symbol_table.as_ref()),
                black_box(data.workspace_index.as_ref()),
            ))
        });
    });
    group.finish();
}

/// Repeated access to one deeply inherited field isolates hierarchy-election
/// work from the large-local-scope workload above.
fn bench_large_inherited_field_semantics(c: &mut Criterion) {
    let data = LargeSemanticData::with_inherited_field(1_200, 40);
    let verified = compute_file_semantics(
        &data.root,
        data.file_id,
        data.symbol_table.as_ref(),
        data.workspace_index.as_ref(),
    );
    assert_eq!(
        verified
            .resolved_names()
            .values()
            .filter(|resolved| matches!(
                resolved,
                hyperion_semantics::ResolvedName::Field { name, .. } if name == "shared"
            ))
            .count(),
        1_200,
        "large inherited field benchmark fixture"
    );
    let mut group = c.benchmark_group("large_inherited_field_semantics");
    group.sample_size(20);
    group.bench_function("40_levels_1200_accesses", |b| {
        b.iter(|| {
            black_box(compute_file_semantics(
                black_box(&data.root),
                black_box(data.file_id),
                black_box(data.symbol_table.as_ref()),
                black_box(data.workspace_index.as_ref()),
            ))
        });
    });
    group.finish();
}

/// End-to-end cold hover for the same large file. Salsa setup is outside the
/// timed region; parsing and semantic construction remain part of the request.
fn bench_large_file_cold_hover(c: &mut Criterion) {
    let data = LargeSemanticData::new(1_200);
    let mut group = c.benchmark_group("large_file_cold_hover");
    group.sample_size(20);
    group.bench_function("last_local", |b| {
        b.iter_batched(
            || data.cold_request(),
            |txn| {
                black_box(data.hover_provider(&txn).hover(
                    black_box(data.file_id),
                    black_box(data.hover_position),
                    black_box(data.source_text.as_str()),
                ))
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

/// End-to-end cold find-references request on a declaration whose spelling is
/// common in unrelated local scopes.
fn bench_large_file_cold_references(c: &mut Criterion) {
    let data = LargeReferenceData::new(1_500);
    let verified_txn = data.cold_request();
    let verified = data
        .navigation_provider(&verified_txn)
        .find_references(
            data.file_id,
            Position::new(2, 18),
            data.source_text.as_str(),
            &data.file_uri,
            true,
        )
        .expect("large reference benchmark request");
    assert_eq!(verified.len(), 2, "large reference benchmark fixture");
    let mut group = c.benchmark_group("large_file_cold_references");
    group.sample_size(20);
    group.bench_function("field_among_1500_same_named_locals", |b| {
        b.iter_batched(
            || data.cold_request(),
            |txn| {
                black_box(data.navigation_provider(&txn).find_references(
                    black_box(data.file_id),
                    black_box(Position::new(2, 18)),
                    black_box(data.source_text.as_str()),
                    black_box(&data.file_uri),
                    black_box(true),
                ))
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_goto_definition,
    bench_completion,
    bench_hover,
    bench_find_references,
    bench_mixed_request_batch,
    bench_semantic_snapshot,
    bench_large_file_semantics,
    bench_large_inherited_field_semantics,
    bench_large_file_cold_hover,
    bench_large_file_cold_references
);
criterion_main!(benches);
