// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_workspace::symbol_extractor::SymbolExtractor;

#[test]
fn test_generic_record_extraction() {
    let source = r#"
        package com.example;
        public record Pair<T, U>(T first, U second) {}
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!(
            "  - {} (kind: {:?}, range: {:?})",
            symbol.fqn, symbol.kind, symbol.range
        );
    }

    // Should have: Pair class + first() + second() + equals + hashCode + toString = 6
    assert_eq!(symbols.len(), 6, "Expected 6 symbols for generic record");

    // Check that both accessors are present
    let has_first = symbols.iter().any(|s| s.simple_name.as_ref() == "first");
    let has_second = symbols.iter().any(|s| s.simple_name.as_ref() == "second");

    assert!(has_first, "Missing 'first' accessor");
    assert!(has_second, "Missing 'second' accessor");
}

#[test]
fn test_non_generic_record_extraction() {
    let source = r#"
        package com.example;
        public record User(String name, String email) {}
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!(
            "  - {} (kind: {:?}, range: {:?})",
            symbol.fqn, symbol.kind, symbol.range
        );
    }

    assert_eq!(
        symbols.len(),
        6,
        "Expected 6 symbols for non-generic record"
    );

    let has_name = symbols.iter().any(|s| s.simple_name.as_ref() == "name");
    let has_email = symbols.iter().any(|s| s.simple_name.as_ref() == "email");

    assert!(has_name, "Missing 'name' accessor");
    assert!(has_email, "Missing 'email' accessor");
}
