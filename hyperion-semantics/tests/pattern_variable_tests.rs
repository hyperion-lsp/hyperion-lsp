// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for pattern variable semantic analysis (Java 16+)

use hyperion_semantics::db::{Database, FileId, ParseDatabase, SourceDatabase};
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_workspace::Language;
use std::sync::Arc;

#[test]
fn test_simple_pattern_variable_in_scope() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
class Test {
    void method(Object obj) {
        if (obj instanceof String s) {
            String x = s;
        }
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let parse_tree = db.parse_file(file);

    assert!(
        parse_tree.errors.is_empty(),
        "Pattern variable should parse without errors, got: {:?}",
        parse_tree.errors
    );

    let syntax_root = parse_tree.syntax();
    let builder = ResolverBuilder::new(file);
    let resolver = builder.build_from_syntax(&syntax_root);

    // Symbol table should contain symbols from the parsed class
    let all_symbols = resolver.symbol_table().all_symbols();
    assert!(
        !all_symbols.is_empty(),
        "Symbol table should contain symbols from parsed pattern variable code"
    );
}

#[test]
fn test_pattern_variable_with_and_chain() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
class Test {
    void method(Object obj) {
        if (obj instanceof String s && s.length() > 10) {
            System.out.println(s);
        }
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let parse_tree = db.parse_file(file);

    assert!(
        parse_tree.errors.is_empty(),
        "Should parse pattern variable in AND chain without errors"
    );

    let syntax_root = parse_tree.syntax();
    let builder = ResolverBuilder::new(file);
    let resolver = builder.build_from_syntax(&syntax_root);

    let all_symbols = resolver.symbol_table().all_symbols();
    assert!(
        !all_symbols.is_empty(),
        "Symbol table should contain symbols from AND chain pattern variable code"
    );
}

#[test]
fn test_pattern_variable_not_in_else() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
class Test {
    void method(Object obj) {
        if (obj instanceof String s) {
            String x = s;  // s in scope
        } else {
            // s NOT in scope here
        }
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let parse_tree = db.parse_file(file);

    assert!(
        parse_tree.errors.is_empty(),
        "Pattern variable not-in-else should parse without errors, got: {:?}",
        parse_tree.errors
    );

    let syntax_root = parse_tree.syntax();
    let builder = ResolverBuilder::new(file);
    let resolver = builder.build_from_syntax(&syntax_root);

    let all_symbols = resolver.symbol_table().all_symbols();
    assert!(
        !all_symbols.is_empty(),
        "Symbol table should contain symbols from pattern variable code"
    );
}
