// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_semantics::db::{Database, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle};
use hyperion_semantics::{file_semantics::FileSemantics, ResolvedName, SemanticErrorKind};
use hyperion_workspace::{Language, WorkspaceIndex};

fn semantics_for(parent: &str, child: &str) -> Arc<FileSemantics> {
    let root = tempfile::tempdir().expect("workspace directory");
    let index = Arc::new(WorkspaceIndex::new(root.path().to_path_buf()));
    let parent_path = root.path().join("base/Parent.java");
    let child_path = root.path().join("app/Child.java");

    index
        .index_file_with_source(&parent_path, parent)
        .expect("index superclass");
    index
        .index_file_with_source(&child_path, child)
        .expect("index subclass");

    let file_id = index.get_file_id(&child_path).expect("subclass file ID");
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(child.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(index));
    db.set_workspace_version(0);

    db.snapshot().file_semantics(file_id)
}

fn undefined_names(parent: &str, child: &str) -> Vec<String> {
    semantics_for(parent, child)
        .diagnostics()
        .iter()
        .filter_map(|error| match &error.kind {
            SemanticErrorKind::UndefinedSymbol { symbol_name, .. } => Some(symbol_name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn inherited_field_as_qualified_receiver_binds_to_superclass() {
    let parent =
        "package base; public class Parent { protected Parent inherited; public void run() {} }";
    let child = "package app; class Child extends base.Parent { void test() { inherited.run(); } }";
    let semantics = semantics_for(parent, child);
    assert!(
        semantics.diagnostics().is_empty(),
        "{:?}",
        semantics.diagnostics()
    );
    assert!(semantics.resolved_names().values().any(|name| matches!(
        name,
        ResolvedName::Field { owner_fqn, name, .. }
            if owner_fqn == "base.Parent" && name == "inherited"
    )));
}

#[test]
fn inherited_method_is_not_a_value_receiver() {
    let names = undefined_names(
        "package base; public class Parent { protected void factory() {} }",
        "package app; class Child extends base.Parent { void test() { factory.run(); } }",
    );
    assert!(names.contains(&"factory".to_owned()), "{names:?}");
}

#[test]
fn accessible_inherited_field_and_method_do_not_report_undefined() {
    let names = undefined_names(
        "package base; public class Parent { protected int value; protected void run() {} }",
        "package app; class Child extends base.Parent { void test() { value++; run(); } }",
    );
    assert!(names.is_empty(), "unexpected undefined symbols: {names:?}");
}

#[test]
fn private_inherited_members_remain_undefined() {
    let names = undefined_names(
        "package base; public class Parent { private int secret; private void hide() {} }",
        "package app; class Child extends base.Parent { void test() { secret++; hide(); } }",
    );
    assert!(names.contains(&"secret".to_owned()), "{names:?}");
    assert!(names.contains(&"hide".to_owned()), "{names:?}");
}

#[test]
fn package_private_inherited_members_remain_undefined_across_packages() {
    let names = undefined_names(
        "package base; public class Parent { int local; void helper() {} }",
        "package app; class Child extends base.Parent { void test() { local++; helper(); } }",
    );
    assert!(names.contains(&"local".to_owned()), "{names:?}");
    assert!(names.contains(&"helper".to_owned()), "{names:?}");
}

#[test]
fn genuinely_missing_superclass_members_remain_undefined() {
    let names = undefined_names(
        "package base; public class Parent { protected void run() {} }",
        "package app; class Child extends base.Parent { void test() { missingValue++; missingCall(); } }",
    );
    assert!(names.contains(&"missingValue".to_owned()), "{names:?}");
    assert!(names.contains(&"missingCall".to_owned()), "{names:?}");
}
