// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{
    Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::Type;
use hyperion_workspace::{Language, WorkspaceIndex};

fn inferred_name_type(source: &str, name: &str) -> Type {
    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from(
        "/tmp/hyperion-scope-aware-expression-inference",
    )));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let parse = db.parse_file(file_id);
    let range = parse
        .syntax()
        .descendants()
        .find(|node| node.kind() == SyntaxKind::NAME_EXPR && node.text().to_string().trim() == name)
        .unwrap_or_else(|| panic!("expected a NAME_EXPR for {name:?}"))
        .text_range();

    db.snapshot()
        .file_semantics(file_id)
        .expression_type_at(range)
        .cloned()
        .unwrap_or(Type::Unknown)
}

#[test]
fn expression_inference_does_not_leak_locals_across_methods() {
    let source = r#"
class ScopeLeak {
    void first() {
        String hidden = "secret";
    }

    void second() {
        Object observed = hidden;
    }
}
"#;

    assert_eq!(inferred_name_type(source, "hidden"), Type::Unknown);
}

#[test]
fn expression_inference_does_not_leak_locals_out_of_nested_blocks() {
    let source = r#"
class ScopeLeak {
    void test() {
        {
            String nested = "secret";
        }
        Object observed = nested;
    }
}
"#;

    assert_eq!(inferred_name_type(source, "nested"), Type::Unknown);
}

#[test]
fn expression_inference_respects_point_of_declaration() {
    let source = r#"
class ScopeLeak {
    void test() {
        Object observed = future;
        String future = "secret";
    }
}
"#;

    assert_eq!(inferred_name_type(source, "future"), Type::Unknown);
}
