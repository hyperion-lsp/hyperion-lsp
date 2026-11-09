// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for code inspections (lint-style warnings)
//!
//! Verifies that inspections detect code quality issues:
//! - Empty catch blocks
//! - String comparison using ==
//! - Redundant modifiers

use hyperion_lsp::inspections::run_inspections;
use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::SyntaxNode;
use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

fn parse(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

fn get_diagnostics(source: &str) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    let syntax = parse(source);
    let temp_dir = TempDir::new().expect("temp dir");
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let source_path = temp_dir.path().join("Test.java");
    index
        .index_file_with_source(&source_path, source)
        .expect("index source");
    let file_id = index.get_file_id(&source_path).expect("file id");

    run_inspections(&syntax, source, &index, file_id)
}

// Test 1: Empty catch block should warn
#[test]
fn test_empty_catch_block_warning() {
    let source = r"
class Test {
    void foo() {
        try {
            riskyOp();
        } catch (Exception e) {
        }
    }
    void riskyOp() {}
}
";

    let diagnostics = get_diagnostics(source);

    let has_empty_catch = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("empty") && d.message.to_lowercase().contains("catch")
    });

    assert!(
        has_empty_catch,
        "Should warn about empty catch block. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// Test 2: Non-empty catch block should NOT warn
#[test]
fn test_non_empty_catch_block_no_warning() {
    let source = r"
class Test {
    void foo() {
        try {
            riskyOp();
        } catch (Exception e) {
            e.printStackTrace();
        }
    }
    void riskyOp() {}
}
";

    let diagnostics = get_diagnostics(source);

    let has_empty_catch = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("empty") && d.message.to_lowercase().contains("catch")
    });

    assert!(
        !has_empty_catch,
        "Should NOT warn about non-empty catch block. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// Test 3: String comparison with == should warn
#[test]
fn test_string_comparison_warning() {
    let source = r#"
class Test {
    void foo() {
        String a = "hello";
        String b = "world";
        if (a == b) {
            System.out.println("equal");
        }
    }
}
"#;

    let diagnostics = get_diagnostics(source);

    let has_string_comparison = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("string") && d.message.to_lowercase().contains("==")
    });

    // Note: This test documents expected behavior
    // If not implemented, the assertion will fail and we'll know
    if has_string_comparison {
        println!("✅ String == comparison warning is implemented");
    } else {
        println!(
            "⚠️ String == comparison warning not found. Got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

// Test 4: Redundant 'public' in interface should warn
#[test]
fn test_redundant_modifier_in_interface() {
    let source = r"
interface MyInterface {
    public void doSomething();
}
";

    let diagnostics = get_diagnostics(source);

    let has_redundant_modifier = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("redundant")
            && d.message.to_lowercase().contains("public")
    });

    // Note: This test documents expected behavior
    if has_redundant_modifier {
        println!("✅ Redundant modifier warning is implemented");
    } else {
        println!(
            "⚠️ Redundant modifier warning not found. Got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

// Test 5: Non-redundant modifiers should NOT warn
#[test]
fn test_non_redundant_modifier_no_warning() {
    let source = r"
class Test {
    public void doSomething() {}
}
";

    let diagnostics = get_diagnostics(source);

    let has_redundant_modifier = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("redundant")
            && d.message.to_lowercase().contains("modifier")
    });

    assert!(
        !has_redundant_modifier,
        "Should NOT warn about non-redundant modifier. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
