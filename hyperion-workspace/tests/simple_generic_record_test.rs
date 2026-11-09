// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_workspace::symbol_extractor::SymbolExtractor;

#[test]
fn test_simple_generic_record() {
    let source = r#"
        package com.example;

        public record Pair<T, U>(T first, U second) {
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!("  - {} (kind: {:?})", symbol.fqn, symbol.kind);
    }

    assert_eq!(
        symbols.len(),
        6,
        "Should have 6 symbols for empty generic record"
    );
}

#[test]
fn test_generic_record_with_bound() {
    let source = r#"
        package com.example;

        public record Pair<T extends Comparable<T>>(T first, T second) {
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!("  - {} (kind: {:?})", symbol.fqn, symbol.kind);
    }

    assert_eq!(
        symbols.len(),
        6,
        "Should have 6 symbols for empty generic record with bounds"
    );
}

#[test]
fn test_generic_record_with_implements() {
    let source = r#"
        package com.example;

        public record Pair<T extends Comparable<T>>(T first, T second)
            implements Comparable<Pair<T>> {
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!("  - {} (kind: {:?})", symbol.fqn, symbol.kind);
    }

    assert_eq!(
        symbols.len(),
        6,
        "Should have 6 symbols for generic record with implements"
    );
}

#[test]
fn test_generic_record_with_method() {
    let source = r#"
        package com.example;

        public record Pair<T extends Comparable<T>>(T first, T second)
            implements Comparable<Pair<T>> {

            @Override
            public int compareTo(Pair<T> other) {
                return 0;
            }
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!("  - {} (kind: {:?})", symbol.fqn, symbol.kind);
    }

    assert_eq!(symbols.len(), 7, "Should have 7 symbols (6 + compareTo)");
    let has_compare_to = symbols
        .iter()
        .any(|s| s.simple_name.as_ref() == "compareTo");
    assert!(has_compare_to, "Should have compareTo method");
}
