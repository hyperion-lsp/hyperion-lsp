// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_semantics::db::{
    Database, FileId, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::semantic_error::SemanticErrorKind;
use hyperion_semantics::ResolvedName;
use hyperion_workspace::{Language, WorkspaceIndex};

#[test]
fn qualified_this_and_super_fields_resolve_to_their_exact_owners() {
    let source = r#"
package com.example;

class Parent {
    protected String inherited;
}

class Child extends Parent {
    int own;

    Object read() {
        this.own++;
        return super.inherited;
    }
}
"#;
    let temp = tempfile::tempdir().expect("semantic workspace");
    let path = temp.path().join("com/example/Child.java");
    let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    workspace_index
        .index_file_with_source(&path, source)
        .expect("qualified field fixture should index");
    let file_id = workspace_index
        .get_file_id(&path)
        .expect("qualified field fixture file id");
    assert_eq!(
        workspace_index.get_parent_class("com.example.Child"),
        Some("com.example.Parent".to_owned())
    );

    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let resolved_fields = semantics
        .resolved_names()
        .values()
        .filter_map(|resolved| match resolved {
            ResolvedName::Field {
                owner_fqn, name, ..
            } => Some((name.as_str(), owner_fqn.as_str())),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();

    assert!(
        resolved_fields.contains(&("own", "com.example.Child")),
        "this.own must bind to Child: {resolved_fields:?}"
    );
    assert!(
        resolved_fields.contains(&("inherited", "com.example.Parent")),
        "super.inherited must bind to Parent: {resolved_fields:?}; all: {:?}",
        semantics.resolved_names()
    );
}

#[test]
fn same_file_member_resolutions_carry_package_qualified_owner() {
    let source = r#"
package com.example;

class Owner {
    int field;

    void helper() {}

    void test() {
        field = 1;
        helper();
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let field_owner = semantics
        .resolved_names()
        .values()
        .find_map(|resolved| match resolved {
            ResolvedName::Field {
                owner_fqn, name, ..
            } if name == "field" => Some(owner_fqn.as_str()),
            _ => None,
        })
        .expect("field usage should resolve");
    let method_owner = semantics
        .resolved_names()
        .values()
        .find_map(|resolved| match resolved {
            ResolvedName::Method {
                owner_fqn, name, ..
            } if name == "helper" => Some(owner_fqn.as_str()),
            _ => None,
        })
        .expect("method call should resolve");

    assert_eq!(field_owner, "com.example.Owner");
    assert_eq!(method_owner, "com.example.Owner");
}

#[test]
fn nested_class_member_resolutions_carry_nested_owner() {
    let source = r#"
package com.example;

class Outer {
    class Inner {
        int value;

        int read() {
            return value;
        }
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let owner = semantics
        .resolved_names()
        .values()
        .find_map(|resolved| match resolved {
            ResolvedName::Field {
                owner_fqn, name, ..
            } if name == "value" => Some(owner_fqn.as_str()),
            _ => None,
        })
        .expect("nested field usage should resolve");

    assert_eq!(owner, "com.example.Outer.Inner");
}

#[test]
fn same_file_enum_constant_resolution_carries_enum_owner() {
    let source = r#"
package com.example;

enum State {
    ON,
    OFF;

    State initial() {
        return ON;
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let enum_owner = semantics
        .resolved_names()
        .values()
        .find_map(|resolved| match resolved {
            ResolvedName::EnumConstant { enum_fqn, name, .. } if name == "ON" => {
                Some(enum_fqn.as_str())
            }
            _ => None,
        })
        .expect("enum constant usage should resolve as an enum constant");

    assert_eq!(enum_owner, "com.example.State");
}

#[test]
fn record_component_usage_resolves_as_an_owned_field() {
    let source = r#"
package com.example;

record Point(int x) {
    int read() {
        return x;
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let owner = semantics
        .resolved_names()
        .values()
        .find_map(|resolved| match resolved {
            ResolvedName::Field {
                owner_fqn, name, ..
            } if name == "x" => Some(owner_fqn.as_str()),
            _ => None,
        })
        .expect("record component usage should resolve as a field");

    assert_eq!(owner, "com.example.Point");
}

#[test]
fn local_variable_does_not_resolve_outside_its_lexical_scope() {
    let source = r#"
class ScopeLeak {
    void first() {
        int hidden = 1;
    }

    void second() {
        hidden = 2;
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    assert!(
        semantics.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind,
            SemanticErrorKind::UndefinedSymbol { symbol_name, .. } if symbol_name == "hidden"
        )),
        "the local from first() must not be visible in second(); diagnostics: {:?}",
        semantics.diagnostics()
    );
}

#[test]
fn same_file_type_resolution_is_package_qualified_and_scope_aware() {
    let source = r#"
package com.example;

class First {
    Second sibling;
}

class Second {}

class Outer {
    class Inner {}
    Inner nested;
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let type_fqns: Vec<&str> = semantics
        .resolved_names()
        .values()
        .filter_map(|resolved| match resolved {
            ResolvedName::Type { fqn, .. } => Some(fqn.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        type_fqns.contains(&"com.example.Second"),
        "sibling type should be package-qualified; got {type_fqns:?}"
    );
    assert!(
        type_fqns.contains(&"com.example.Outer.Inner"),
        "nested type should carry its enclosing owner; got {type_fqns:?}"
    );
}

#[test]
fn local_variable_is_not_visible_before_its_declaration() {
    let source = r#"
class ForwardLocal {
    void test() {
        later = 1;
        int later = 2;
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    assert!(
        semantics.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind,
            SemanticErrorKind::UndefinedSymbol { symbol_name, .. } if symbol_name == "later"
        )),
        "a local variable's scope starts at its declarator; diagnostics: {:?}",
        semantics.diagnostics()
    );
}

#[test]
fn constructor_parameter_does_not_leak_into_sibling_method() {
    let source = r#"
class ConstructorScope {
    ConstructorScope(int constructorOnly) {
        constructorOnly = 1;
    }

    void test() {
        constructorOnly = 2;
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let undefined_count = semantics
        .diagnostics()
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.kind,
                SemanticErrorKind::UndefinedSymbol { symbol_name, .. }
                    if symbol_name == "constructorOnly"
            )
        })
        .count();
    assert_eq!(
        undefined_count,
        1,
        "the constructor body use is valid, while the sibling method use is not; diagnostics: {:?}",
        semantics.diagnostics()
    );
}

#[test]
fn not_yet_declared_local_does_not_hide_visible_field() {
    let source = r#"
class Shadowing {
    int value;

    void test() {
        value = 1;
        int value = 2;
        value = 3;
    }
}
"#;

    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test")));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let semantics = db.snapshot().file_semantics(file_id);
    let value_resolutions: Vec<&ResolvedName> = semantics
        .resolved_names()
        .values()
        .filter(|resolved| match resolved {
            ResolvedName::Field { name, .. } | ResolvedName::LocalVariable { name, .. } => {
                name == "value"
            }
            _ => false,
        })
        .collect();

    assert!(
        value_resolutions
            .iter()
            .any(|resolved| matches!(resolved, ResolvedName::Field { .. })),
        "the use before the local declarator should resolve to the field: {value_resolutions:?}"
    );
    assert!(
        value_resolutions
            .iter()
            .any(|resolved| matches!(resolved, ResolvedName::LocalVariable { .. })),
        "the use after the local declarator should resolve to the local: {value_resolutions:?}"
    );
}
