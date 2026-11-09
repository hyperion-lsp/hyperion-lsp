// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test redundant modifier detection
use hyperion_lsp::inspections::run_inspections;
use hyperion_parser::{lexer::Lexer, parser::Parser, syntax::SyntaxNode};
use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

fn has_redundant_modifier_diagnostic(source: &str) -> bool {
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

    diagnostics
        .iter()
        .any(|d| d.message.contains("redundant") && d.message.contains("public"))
}

#[test]
fn test_redundant_public_in_interface() {
    let source = r"
public interface Test {
    public void doSomething();
}
    ";

    assert!(
        has_redundant_modifier_diagnostic(source),
        "public modifier in interface method should be flagged as redundant"
    );
}

#[test]
fn test_no_redundant_public_in_class() {
    let source = r"
public class Test {
    public void doSomething() {
    }
}
    ";

    assert!(
        !has_redundant_modifier_diagnostic(source),
        "public modifier in class method should NOT be flagged"
    );
}
