// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_workspace::symbol_extractor::SymbolExtractor;

#[test]
fn test_record_with_custom_method() {
    let source = r#"
        package com.example;

        public record User(String name, String email) {
            public String getDisplayName() {
                return name + " <" + email + ">";
            }
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let symbols = &result.symbols;

    println!("\n=== Extracted {} symbols ===", symbols.len());
    for symbol in symbols {
        println!("  - {} (kind: {:?})", symbol.fqn, symbol.kind);
    }

    // Should have: User + name + email + getDisplayName + equals + hashCode + toString = 7
    let has_custom_method = symbols
        .iter()
        .any(|s| s.simple_name.as_ref() == "getDisplayName");
    assert!(
        has_custom_method,
        "Missing custom method 'getDisplayName' in record"
    );
}
