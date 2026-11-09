// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{
    Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::{ClassType, Type};
use hyperion_workspace::{Language, WorkspaceIndex};

#[test]
fn method_call_inference_uses_canonical_invocation_phase_selection() {
    let source = r#"
package com.example;

class Overloads {
    Integer choose(Integer number, String value) {
        return number;
    }

    String choose(long number, Object value) {
        return value.toString();
    }
}

class Usage {
    void test(Overloads overloads) {
        Object selected = overloads.choose(1, "value");
    }
}
"#;

    let temp = tempfile::tempdir().expect("temp workspace");
    let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let path = temp.path().join("com/example/Overloads.java");
    workspace_index
        .index_file_with_source(&path, source)
        .expect("source should index");

    let file_id = FileId::new(0);
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let parse = db.parse_file(file_id);
    let call_range = parse
        .syntax()
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_CALL_EXPR
                && node.text().to_string().trim() == "overloads.choose(1, \"value\")"
        })
        .expect("expected the choose call")
        .text_range();
    let inferred = db
        .snapshot()
        .file_semantics(file_id)
        .expression_type_at(call_range)
        .cloned()
        .unwrap_or(Type::Unknown);

    assert_eq!(inferred, Type::Class(ClassType::string()));
}

#[test]
fn method_call_inference_erases_raw_receiver_returns() {
    let source = r#"
package com.example;

class Box<T> {
    T get() {
        return null;
    }
}

class Usage {
    void test(Box raw, Box<String> typed) {
        Object erased = raw.get();
        String concrete = typed.get();
    }
}
"#;

    let temp = tempfile::tempdir().expect("temp workspace");
    let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let path = temp.path().join("com/example/Box.java");
    workspace_index
        .index_file_with_source(&path, source)
        .expect("source should index");

    let file_id = FileId::new(0);
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let parse = db.parse_file(file_id);
    let semantics = db.snapshot().file_semantics(file_id);
    let inferred_call = |text: &str| {
        let range = parse
            .syntax()
            .descendants()
            .find(|node| {
                node.kind() == SyntaxKind::METHOD_CALL_EXPR
                    && node.text().to_string().trim() == text
            })
            .unwrap_or_else(|| panic!("expected method call {text:?}"))
            .text_range();
        semantics
            .expression_type_at(range)
            .cloned()
            .unwrap_or(Type::Unknown)
    };

    assert_eq!(inferred_call("raw.get()"), Type::Class(ClassType::object()));
    assert_eq!(
        inferred_call("typed.get()"),
        Type::Class(ClassType::string())
    );
}
