// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for folding ranges
//!
//! Folding ranges allow editors to collapse code regions like classes,
//! methods, blocks, and comments.

use hyperion_lsp::folding::FoldingProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::FileId;
use std::sync::Arc;
use tower_lsp::lsp_types::FoldingRangeKind;

/// Helper to get folding ranges for Java source code
fn get_folding_ranges(source: &str) -> Vec<(u32, u32)> {
    let host = SemanticHost::for_tests();
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let provider = FoldingProvider::new();

    let result = provider.folding_ranges(&host.read_now(), file_id, source);
    match result {
        Ok(ranges) => ranges.iter().map(|r| (r.start_line, r.end_line)).collect(),
        Err(e) => {
            eprintln!("Folding ranges error: {e}");
            vec![]
        }
    }
}

/// Helper to get full folding range info
fn get_full_folding_ranges(
    source: &str,
) -> Vec<(u32, u32, Option<tower_lsp::lsp_types::FoldingRangeKind>)> {
    let host = SemanticHost::for_tests();
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let provider = FoldingProvider::new();

    let result = provider.folding_ranges(&host.read_now(), file_id, source);
    match result {
        Ok(ranges) => ranges
            .iter()
            .map(|r| (r.start_line, r.end_line, r.kind.clone()))
            .collect(),
        Err(e) => {
            eprintln!("Folding ranges error: {e}");
            vec![]
        }
    }
}

// ============================================================================
// Class Folding Tests
// ============================================================================

#[test]
fn test_class_folding() {
    let source = r"class Test {
    int field;
}";
    let ranges = get_folding_ranges(source);

    assert!(!ranges.is_empty(), "Should have at least one folding range");
    // Class body should fold from line 0 to line 2
    assert!(ranges.iter().any(|(start, end)| *start == 0 && *end == 2));
}

#[test]
fn test_interface_folding() {
    let source = r"interface Service {
    void process();
    String getName();
}";
    let ranges = get_folding_ranges(source);

    assert!(!ranges.is_empty(), "Should have at least one folding range");
    assert!(ranges.iter().any(|(start, end)| *start == 0 && *end == 3));
}

#[test]
fn test_enum_folding() {
    let source = r"enum Status {
    ACTIVE,
    INACTIVE
}";
    let ranges = get_folding_ranges(source);

    assert!(!ranges.is_empty(), "Should have at least one folding range");
    assert!(ranges.iter().any(|(start, end)| *start == 0 && *end == 3));
}

// ============================================================================
// Method Folding Tests
// ============================================================================

#[test]
fn test_method_folding() {
    let source = r"class Test {
    void method() {
        int x = 1;
        int y = 2;
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have at least 2 ranges: class body and method body
    assert!(
        ranges.len() >= 2,
        "Should have at least 2 folding ranges, got {}",
        ranges.len()
    );

    // Method body should fold (lines 1-4)
    assert!(
        ranges.iter().any(|(start, end)| *start == 1 && *end == 4),
        "Should have method body folding range"
    );
}

#[test]
fn test_constructor_folding() {
    let source = r"class Test {
    public Test() {
        init();
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have at least 2 ranges: class body and constructor body
    assert!(ranges.len() >= 2, "Should have at least 2 folding ranges");
}

#[test]
fn test_multiple_methods_folding() {
    let source = r"class Test {
    void first() {
        doFirst();
    }

    void second() {
        doSecond();
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have ranges for class + 2 methods
    assert!(
        ranges.len() >= 3,
        "Should have at least 3 folding ranges for class and 2 methods"
    );
}

// ============================================================================
// Block Folding Tests
// ============================================================================

#[test]
fn test_if_block_folding() {
    let source = r"class Test {
    void method() {
        if (condition) {
            doSomething();
        }
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have ranges for: class, method, if block
    assert!(ranges.len() >= 2, "Should have folding ranges for blocks");
}

#[test]
fn test_nested_blocks_folding() {
    let source = r"class Test {
    void method() {
        if (a) {
            if (b) {
                doSomething();
            }
        }
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have multiple nested folding ranges
    assert!(
        ranges.len() >= 3,
        "Should have folding ranges for nested blocks"
    );
}

// ============================================================================
// Comment Folding Tests
// ============================================================================

// Comment folding tests - now working by walking tokens in addition to nodes

#[test]
fn test_block_comment_folding() {
    let source = r"/*
 * This is a multi-line
 * block comment
 */
class Test {}";
    let ranges = get_full_folding_ranges(source);

    // Should have comment folding range
    assert!(
        ranges
            .iter()
            .any(|(_, _, kind)| *kind == Some(FoldingRangeKind::Comment)),
        "Should have comment folding range"
    );
}

#[test]
fn test_doc_comment_folding() {
    let source = r"/**
 * Documentation comment
 * for the class
 */
class Test {}";
    let ranges = get_full_folding_ranges(source);

    // Should have comment folding range
    assert!(
        ranges
            .iter()
            .any(|(_, _, kind)| *kind == Some(FoldingRangeKind::Comment)),
        "Should have doc comment folding range"
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_single_line_class_no_folding() {
    let source = "class Test {}";
    let ranges = get_folding_ranges(source);

    // Single line should not create folding range
    assert!(
        ranges.is_empty(),
        "Single line class should not have folding range"
    );
}

#[test]
fn test_empty_class_folding() {
    let source = r"class Test {
}";
    let ranges = get_folding_ranges(source);

    // Even empty class spanning multiple lines should be foldable
    assert!(
        !ranges.is_empty(),
        "Multi-line empty class should have folding range"
    );
}

#[test]
fn test_nested_class_folding() {
    let source = r"class Outer {
    class Inner {
        void method() {
            doSomething();
        }
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have ranges for: outer class, inner class, method
    assert!(
        ranges.len() >= 3,
        "Should have folding ranges for nested structures, got {}",
        ranges.len()
    );
}

// ============================================================================
// Kind Verification Tests
// ============================================================================

#[test]
fn test_region_kind_for_class() {
    let source = r"class Test {
    int field;
}";
    let ranges = get_full_folding_ranges(source);

    // Class should have Region kind
    assert!(
        ranges
            .iter()
            .any(|(_, _, kind)| *kind == Some(FoldingRangeKind::Region)),
        "Class should have Region folding kind"
    );
}

#[test]
fn test_complex_class_structure() {
    let source = r"/**
 * A test class
 */
class Test {
    private int field;

    public Test() {
        field = 0;
    }

    public void process() {
        if (field > 0) {
            doSomething();
        }
    }
}";
    let ranges = get_folding_ranges(source);

    // Should have multiple ranges: doc comment, class, constructor, method, if block
    assert!(
        ranges.len() >= 4,
        "Complex class should have multiple folding ranges, got {}",
        ranges.len()
    );
}

#[test]
fn annotation_array_braces_do_not_start_the_class_fold() {
    let source = "@SuppressWarnings({\n    \"unused\"})\nclass Example {\n    int value;\n}";
    let ranges = get_folding_ranges(source);
    assert!(ranges.contains(&(2, 4)), "class body: {ranges:?}");
    assert!(
        !ranges.contains(&(0, 4)),
        "annotation is outside the class body"
    );
}

#[test]
fn if_else_bodies_fold_independently_without_duplicate_ranges() {
    let source = "class Example {\n    void method() {\n        if (true) {\n            first();\n        } else {\n            second();\n        }\n    }\n}";
    let ranges = get_folding_ranges(source);
    assert!(ranges.contains(&(2, 4)));
    assert!(ranges.contains(&(4, 6)));
    assert!(
        !ranges.contains(&(2, 6)),
        "separate branch bodies: {ranges:?}"
    );
    let unique: std::collections::BTreeSet<_> = ranges.iter().collect();
    assert_eq!(
        unique.len(),
        ranges.len(),
        "duplicate folding regions: {ranges:?}"
    );
}

#[test]
fn folding_declines_text_that_does_not_match_the_parse_snapshot() {
    let host = SemanticHost::for_tests();
    let file_id = FileId::new(0);
    let source = "class Example {\n    int value;\n}";
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });
    let provider = FoldingProvider::new();
    assert_eq!(
        provider
            .folding_ranges(&host.read_now(), file_id, &format!("\n{source}"))
            .unwrap(),
        []
    );
}
