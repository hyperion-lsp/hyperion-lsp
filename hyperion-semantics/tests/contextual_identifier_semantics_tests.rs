// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_semantics::db::{Database, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle};
use hyperion_semantics::{ResolvedName, SemanticErrorKind};
use hyperion_workspace::{Language, WorkspaceIndex};
use rowan::{TextRange, TextSize};

#[test]
fn switch_enum_label_has_exact_identity_without_suppressing_case_body_errors() {
    let source = r#"
package p;

class Processor {
    enum Event { READY, DONE }

    void process(Event event) {
        switch (event) {
            case READY:
                missing();
                break;
            case UNKNOWN:
                break;
            default:
                break;
        }
    }
}
"#;
    let temp = tempfile::tempdir().expect("temporary workspace");
    let path = temp.path().join("p/Processor.java");
    let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    workspace_index
        .index_file_with_source(&path, source)
        .expect("switch fixture should index");
    let file_id = workspace_index.get_file_id(&path).expect("indexed file id");
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let label_start = source.find("case READY").expect("enum label") + "case ".len();
    let label_range = TextRange::new(
        TextSize::from(u32::try_from(label_start).expect("label offset fits u32")),
        TextSize::from(u32::try_from(label_start + "READY".len()).expect("label end fits u32")),
    );

    assert!(
        matches!(
            semantics.resolve_at(label_range),
            Some(ResolvedName::EnumConstant {
                enum_fqn,
                name,
                location: Some(_),
            }) if enum_fqn == "p.Processor.Event" && name == "READY"
        ),
        "resolved names: {:#?}",
        semantics.resolved_names()
    );
    assert!(semantics.diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        SemanticErrorKind::UndefinedSymbol { symbol_name, .. } if symbol_name == "missing"
    )));
    assert!(semantics.diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        SemanticErrorKind::UndefinedSymbol { symbol_name, .. } if symbol_name == "UNKNOWN"
    )));
}

#[test]
fn unpublished_import_revision_controls_switch_selector_identity() {
    let old_source = r#"
package app;
import p.State;
class Main { void run(State state) {} }
"#;
    let current_source = r#"
package app;
import q.State;
class Main {
    void run(State state) {
        switch (state) {
            case CURRENT:
                break;
        }
    }
}
"#;
    let temp = tempfile::tempdir().expect("temporary workspace");
    let p_path = temp.path().join("p/State.java");
    let q_path = temp.path().join("q/State.java");
    let main_path = temp.path().join("app/Main.java");
    let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    workspace_index
        .index_file_with_source(&p_path, "package p; public enum State { OLD }")
        .expect("old enum should index");
    workspace_index
        .index_file_with_source(&q_path, "package q; public enum State { CURRENT }")
        .expect("current enum should index");
    workspace_index
        .index_file_with_source(&main_path, old_source)
        .expect("stale main revision should index");
    let file_id = workspace_index
        .get_file_id(&main_path)
        .expect("indexed main file id");
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(current_source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let label_start = current_source.find("CURRENT").expect("current enum label");
    let label_range = TextRange::new(
        TextSize::from(u32::try_from(label_start).expect("label offset fits u32")),
        TextSize::from(u32::try_from(label_start + "CURRENT".len()).expect("label end fits u32")),
    );

    assert!(
        matches!(
            semantics.resolve_at(label_range),
            Some(ResolvedName::EnumConstant {
                enum_fqn,
                name,
                location: Some(_),
            }) if enum_fqn == "q.State" && name == "CURRENT"
        ),
        "the current syntax import must beat the stale indexed import: {:#?}",
        semantics.resolved_names()
    );
}

#[test]
fn class_literal_operands_bind_to_types_instead_of_values() {
    let temp = tempfile::tempdir().unwrap();
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(
            &temp.path().join("SomeEnum.java"),
            "package model; public enum SomeEnum { ONE, TWO }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Container.java"),
            "package model; public class Container { public enum Nested { ONE } }",
        )
        .unwrap();
    let source = r"
import model.SomeEnum;
import model.Container;
class Use {
    Object first() { return SomeEnum.class; }
    Object qualified() { return model.SomeEnum.class; }
    Object nested() { return Container.Nested.class; }
    Object array() { return SomeEnum[][].class; }
    Object primitive() { return int.class; }
    Object primitiveArray() { return int[].class; }
    Object voidType() { return void.class; }
    Object shadowed() { int SomeEnum = 1; return SomeEnum.class; }
}
";
    let path = temp.path().join("Use.java");
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.into()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(index));
    db.set_workspace_version(0);
    let semantics = db.snapshot().file_semantics(file_id);
    assert!(
        semantics.diagnostics().is_empty(),
        "{:?}",
        semantics.diagnostics()
    );
    for needle in ["SomeEnum.class", "SomeEnum[][].class"] {
        let start = source.find(needle).unwrap();
        let range = TextRange::new(
            TextSize::from(start as u32),
            TextSize::from((start + "SomeEnum".len()) as u32),
        );
        assert!(
            matches!(semantics.resolve_at(range), Some(ResolvedName::Type { fqn, .. }) if fqn == "model.SomeEnum")
        );
    }
}

#[test]
fn class_literals_do_not_hide_missing_types_or_accept_value_names() {
    let source = "class Use { Object onlyValue; Object first() { return Missing.class; } Object second() { return onlyValue.class; } }";
    let temp = tempfile::tempdir().unwrap();
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let path = temp.path().join("Use.java");
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.into()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(index));
    db.set_workspace_version(0);
    let semantics = db.snapshot().file_semantics(file_id);
    let missing: Vec<_> = semantics
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            SemanticErrorKind::UndefinedSymbol { symbol_name, .. } => Some(symbol_name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(missing, vec!["Missing", "onlyValue"]);
}
