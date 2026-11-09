// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_semantics::db::{Database, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle};
use hyperion_semantics::SemanticErrorKind;
use hyperion_workspace::index::WorkspaceIndex;
use std::{fs, path::Path};
use tempfile::TempDir;

fn write_file(root: &Path, rel_path: &str, contents: &str) -> std::path::PathBuf {
    let path = root.join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&path, contents).expect("write file");
    path
}

#[test]
fn undefined_symbol_in_type_provides_case_mismatch_suggestion_from_imports() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let hostname_matcher_path = write_file(
        root,
        "com/example/HostnameMatcher.java",
        r#"
            package com.example;

            public class HostnameMatcher {
            }
        "#,
    );

    let test_path = write_file(
        root,
        "com/example/Test.java",
        r#"
            package com.example;

            import com.example.HostnameMatcher;

            public class Test {
                private HostNameMatcher matcher;
            }
        "#,
    );

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));
    index
        .index_file_with_source(
            &hostname_matcher_path,
            &fs::read_to_string(&hostname_matcher_path).unwrap(),
        )
        .unwrap();
    index
        .index_file_with_source(&test_path, &fs::read_to_string(&test_path).unwrap())
        .unwrap();

    let file_id = index.get_file_id(&test_path).expect("file id");
    let source = fs::read_to_string(&test_path).unwrap();

    let mut db = Database::default();
    db.set_file_text(file_id, std::sync::Arc::new(source.clone()));
    db.set_file_language(
        file_id,
        hyperion_workspace::Language::from_path(&test_path)
            .unwrap_or(hyperion_workspace::Language::Java),
    );
    db.set_workspace_index_handle(WorkspaceIndexHandle(std::sync::Arc::clone(&index)));
    db.set_workspace_version(0);

    let file_sem = db.snapshot().file_semantics(file_id);
    let errors = file_sem.diagnostics();

    let undefined = errors
        .iter()
        .find_map(|e| match &e.kind {
            SemanticErrorKind::UndefinedSymbol {
                symbol_name,
                suggested_symbol,
            } => Some((symbol_name.as_str(), suggested_symbol.as_deref())),
            _ => None,
        })
        .expect("expected UndefinedSymbol error");

    assert_eq!(undefined.0, "HostNameMatcher");
    assert_eq!(undefined.1, Some("HostnameMatcher"));
}
