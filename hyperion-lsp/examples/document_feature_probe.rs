// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded production-provider timing probe. Run with:
//! cargo run --release -p hyperion-lsp --example document_feature_probe
//! Reports median of five warm calls; parsing is included for Java outlines.

use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_lsp::{
    document_links::DocumentLinksProvider, folding::FoldingProvider, symbols::SymbolsProvider,
};
use hyperion_semantics::db::{FileId, ParseDatabase};
use hyperion_workspace::{Language, QueryEngine, WorkspaceIndex};
use std::{fmt::Write, hint::black_box, sync::Arc, time::Instant};

fn median_us(mut run: impl FnMut()) -> u128 {
    run();
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        run();
        samples.push(start.elapsed().as_micros());
    }
    samples.sort_unstable();
    samples[2]
}

fn main() {
    println!("members,bytes,outline_us,folding_us,links_us,workspace_symbols_us");
    for members in [128, 512, 2048] {
        let mut source = String::from("class Probe {\n");
        for i in 0..members {
            writeln!(source, "  // https://example.com/member/{i}\n  void member{i}() {{\n    int value = {i};\n  }}").unwrap();
        }
        source.push_str("}\n");
        let host = SemanticHost::for_tests();
        let file_id = FileId::new(1);
        host.with_db(|db| db.register_file(file_id, Arc::new(source.clone()), Language::Java));
        let txn = host.read_now();
        black_box(txn.parse_file(file_id));
        let outline = SymbolsProvider::new();
        let folding = FoldingProvider::new();
        let links = DocumentLinksProvider::new();
        let root = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(root.path().to_path_buf());
        index
            .index_file_with_source(&root.path().join("Probe.java"), &source)
            .unwrap();
        let query = QueryEngine::new(index);
        // Keep faster empty responses from looking like an optimization.
        let symbols = outline.java_document_symbols_from_source(&source).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].children.as_ref().unwrap().len(), members);
        assert_eq!(
            links.document_links(&txn, file_id, &source).unwrap().len(),
            members
        );
        assert_eq!(query.workspace_symbols("member").unwrap().len(), 100);
        let folds: std::collections::BTreeSet<_> = folding
            .folding_ranges(&txn, file_id, &source)
            .unwrap()
            .into_iter()
            .map(|range| (range.start_line, range.end_line))
            .collect();
        assert_eq!(folds.len(), members + 1);

        let outline_us = median_us(|| {
            black_box(outline.java_document_symbols_from_source(&source).unwrap());
        });
        let folding_us = median_us(|| {
            black_box(folding.folding_ranges(&txn, file_id, &source).unwrap());
        });
        let links_us = median_us(|| {
            black_box(links.document_links(&txn, file_id, &source).unwrap());
        });
        let workspace_us = median_us(|| {
            black_box(query.workspace_symbols("member").unwrap());
        });
        println!(
            "{members},{},{outline_us},{folding_us},{links_us},{workspace_us}",
            source.len()
        );
    }
}
