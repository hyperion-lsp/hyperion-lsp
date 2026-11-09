// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for @Override annotation detection
//!
//! Validates that the LSP correctly identifies when @Override annotations
//! are present or missing on methods that override parent methods.

use hyperion_lsp::inspections::run_inspections;
use hyperion_parser::{lexer::Lexer, parser::Parser, syntax::SyntaxNode};
use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

/// Helper to check if a diagnostic about missing @Override exists
fn has_missing_override_diagnostic(source: &str, method_name: &str) -> bool {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    let temp_dir = TempDir::new().expect("temp dir");
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let source_path = temp_dir.path().join("Test.java");
    index
        .index_file_with_source(&source_path, source)
        .expect("index source");
    let file_id = index.get_file_id(&source_path).expect("file id");

    let diagnostics = run_inspections(&root, source, &index, file_id);

    diagnostics.iter().any(|d| {
        d.message.contains("@Override")
            && d.message.contains("lacks")
            && d.message.contains(method_name)
    })
}

#[test]
fn test_hashcode_with_override_annotation_same_line() {
    let source = r"
public class Test {
    @Override public int hashCode() {
        return 42;
    }
}
    ";

    assert!(
        !has_missing_override_diagnostic(source, "hashCode"),
        "hashCode has @Override on same line - should NOT be flagged"
    );
}

#[test]
fn test_hashcode_with_override_annotation_previous_line() {
    let source = r"
public class Test {
    @Override
    public int hashCode() {
        return 42;
    }
}
    ";

    assert!(
        !has_missing_override_diagnostic(source, "hashCode"),
        "hashCode has @Override on previous line - should NOT be flagged"
    );
}

#[test]
fn test_hashcode_without_override_annotation() {
    let source = r"
public class Test {
    public int hashCode() {
        return 42;
    }
}
    ";

    assert!(
        has_missing_override_diagnostic(source, "hashCode"),
        "hashCode lacks @Override - should be flagged"
    );
}

#[test]
fn test_equals_with_override_annotation() {
    let source = r"
public class Test {
    @Override
    public boolean equals(Object obj) {
        return false;
    }
}
    ";

    assert!(
        !has_missing_override_diagnostic(source, "equals"),
        "equals has @Override - should NOT be flagged"
    );
}

#[test]
fn test_equals_without_override_annotation() {
    let source = r"
public class Test {
    public boolean equals(Object obj) {
        return false;
    }
}
    ";

    assert!(
        has_missing_override_diagnostic(source, "equals"),
        "equals lacks @Override - should be flagged"
    );
}

#[test]
fn test_tostring_with_override_annotation() {
    let source = r#"
public class Test {
    @Override
    public String toString() {
        return "Test";
    }
}
    "#;

    assert!(
        !has_missing_override_diagnostic(source, "toString"),
        "toString has @Override - should NOT be flagged"
    );
}

#[test]
fn test_tostring_without_override_annotation() {
    let source = r#"
public class Test {
    public String toString() {
        return "Test";
    }
}
    "#;

    assert!(
        has_missing_override_diagnostic(source, "toString"),
        "toString lacks @Override - should be flagged"
    );
}

#[test]
fn test_clone_with_override_annotation() {
    let source = r"
public class Test {
    @Override
    public Object clone() {
        return null;
    }
}
    ";

    assert!(
        !has_missing_override_diagnostic(source, "clone"),
        "clone has @Override - should NOT be flagged"
    );
}

#[test]
fn test_multiple_annotations_including_override() {
    let source = r#"
public class Test {
    @SuppressWarnings("unchecked")
    @Override
    public int hashCode() {
        return 42;
    }
}
    "#;

    assert!(
        !has_missing_override_diagnostic(source, "hashCode"),
        "hashCode has @Override (with other annotations) - should NOT be flagged"
    );
}

#[test]
fn test_annotation_with_fully_qualified_name() {
    let source = r"
public class Test {
    @java.lang.Override
    public int hashCode() {
        return 42;
    }
}
    ";

    assert!(
        !has_missing_override_diagnostic(source, "hashCode"),
        "hashCode has @java.lang.Override - should NOT be flagged"
    );
}

#[test]
fn test_regular_method_not_flagged() {
    let source = r#"
public class Test {
    public void myMethod() {
        System.out.println("Not an override");
    }
}
    "#;

    assert!(
        !has_missing_override_diagnostic(source, "myMethod"),
        "myMethod is not an override method - should NOT be flagged"
    );
}
