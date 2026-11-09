// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Deadlock/liveness stress for the semantic gate.
//!
//! Interleaves every kind of writer (editor publication, external index
//! publication with an epoch bump, dependency hydration) with readers that run
//! Salsa queries, all against one host with index-write enforcement on. Any
//! wait-for cycle shows up as a scenario that misses its deadline; any index
//! mutation outside a write transaction panics.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hyperion_semantics::db::{FileId, Language, SemanticDatabase, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use lru::LruCache;
use parking_lot::Mutex;

use crate::semantic_host::SemanticHost;
use crate::source_publication::{hydrate_dependency_method_signatures, publish_semantic_revision};

const FILES: usize = 6;

fn iterations() -> usize {
    std::env::var("HYPERION_STRESS_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(25)
}

fn source(index: usize, revision: usize) -> String {
    let previous = index.saturating_sub(1);
    format!(
        "package stress;\npublic class C{index} {{\n    public int value() {{ return {index}; }}\n    public int chain(C{previous} other) {{ return other.value() + value(); }}\n    // revision {revision}\n}}\n"
    )
}

async fn scenario(iteration: usize) {
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let host = SemanticHost::new(Arc::clone(&index));
    host.enforce_exclusive_index_writes();
    let cache: Arc<Mutex<LruCache<(FileId, u32), hyperion_semantics::Type>>> = Arc::new(
        Mutex::new(LruCache::new(std::num::NonZeroUsize::new(8).unwrap())),
    );

    let mut files = Vec::new();
    {
        let mut txn = host.write().await;
        for i in 0..FILES {
            let path = temp.path().join(format!("C{i}.java"));
            let text = source(i, 0);
            std::fs::write(&path, &text).expect("write");
            txn.index()
                .index_file_with_source(&path, &text)
                .expect("index");
            let id = txn.index().get_file_id(&path).expect("id");
            txn.db().register_file(id, Arc::new(text), Language::Java);
            files.push((path, id));
        }
    }
    let files = Arc::new(files);
    let done_reads = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();

    // Editor publications (didChange-shaped): index + Salsa in one transaction.
    for editor in 0..3 {
        let host = Arc::clone(&host);
        let files = Arc::clone(&files);
        let cache = Arc::clone(&cache);
        tasks.push(tokio::spawn(async move {
            for revision in 1..=10usize {
                let (path, id) = &files[(editor + revision) % FILES];
                let i = (editor + revision) % FILES;
                let text = Arc::new(source(i, revision + editor * 100 + iteration * 1000));
                let (path, id, cache) = (path.clone(), *id, Arc::clone(&cache));
                let published = host
                    .write()
                    .await
                    .run_blocking(move |txn| {
                        let stats = txn
                            .index()
                            .index_file_with_source_and_id(&path, &text, id)
                            .expect("index edit");
                        publish_semantic_revision(
                            txn,
                            &cache,
                            id,
                            Arc::clone(&text),
                            Language::Java,
                            &stats.retired_file_ids,
                        )
                    })
                    .await
                    .expect("editor task");
                assert!(published, "an exclusive publication cannot go stale");
            }
        }));
    }

    // External index publication (JDK/Maven/Gradle-shaped): bounded steps, each a
    // write transaction, ending with the epoch bump.
    {
        let host = Arc::clone(&host);
        tasks.push(tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                for step in 0..10 {
                    host.publish_blocking(|txn| {
                        let symbols =
                            hyperion_workspace::symbol_extractor::SymbolExtractor::extract(
                                &format!("package ext; public class Type{step} {{}}"),
                            )
                            .expect("extract")
                            .symbols;
                        txn.index().add_external_symbols_batch(symbols);
                        txn.bump_workspace_epoch();
                    });
                }
            })
            .await
            .expect("external publisher");
        }));
    }

    // Dependency hydration (prepare outside the gate, install inside it).
    for hydrator in 0..2 {
        let host = Arc::clone(&host);
        let files = Arc::clone(&files);
        tasks.push(tokio::spawn(async move {
            for round in 0..6 {
                let (_, id) = &files[(hydrator + round) % FILES];
                hydrate_dependency_method_signatures(&host, *id).await;
            }
        }));
    }

    // Readers: Salsa queries under a read transaction, both inline and on a
    // blocking thread, released promptly.
    for reader in 0..4 {
        let host = Arc::clone(&host);
        let files = Arc::clone(&files);
        let done = Arc::clone(&done_reads);
        tasks.push(tokio::spawn(async move {
            for round in 0..12 {
                let (_, id) = files[(reader + round) % FILES].clone();
                let txn = host.read().await;
                let symbols = txn.file_symbols(id);
                assert!(!symbols.symbols.is_empty(), "the file stays registered");
                if round % 2 == 0 {
                    txn.run_blocking(move |txn| {
                        let _ = txn.file_semantics(id);
                    })
                    .await
                    .expect("reader task");
                } else {
                    let _ = txn.file_semantics(id);
                }
                done.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    let all = async {
        for task in tasks {
            task.await.expect("stress actor panicked");
        }
    };
    tokio::time::timeout(Duration::from_mins(1), all)
        .await
        .unwrap_or_else(|_| panic!("iteration {iteration}: the semantic gate wedged"));
    assert_eq!(done_reads.load(Ordering::SeqCst), 4 * 12);

    // The mirrors converge: every file's Salsa text is the index's revision.
    let txn = host.read().await;
    for (path, id) in files.iter() {
        let indexed = index.get_file_contents(*id).expect("indexed contents");
        assert_eq!(
            txn.file_text(*id).as_str(),
            indexed.as_str(),
            "Salsa and the index diverged for {}",
            path.display()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publishers_hydration_and_readers_never_wedge_the_gate() {
    for iteration in 0..iterations() {
        scenario(iteration).await;
    }
}
