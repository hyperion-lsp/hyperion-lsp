// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_workspace::symbol_extractor::SymbolExtractor;

#[test]
fn test_record_with_override() {
    // Exact source from the failing LSP test
    let source = r#"
                package com.example;

                public record Pair<T extends Comparable<T>>(T first, T second)
                    implements Comparable<Pair<T>> {

                    @Override
                    public int compareTo(Pair<T> other) {
                        return this.first.compareTo(other.first);
                    }
                }
            "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!("  - {} (kind: {:?})", symbol.fqn, symbol.kind);
    }

    let has_compare_to = symbols
        .iter()
        .any(|s| s.simple_name.as_ref() == "compareTo");
    assert!(has_compare_to, "Missing 'compareTo' method in record");
}
