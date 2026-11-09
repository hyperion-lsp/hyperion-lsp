// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests that fully-qualified class references in expression positions
//! (e.g., `com.foo.bar.Bar.doSomething()`) do NOT produce spurious
//! "cannot find symbol: com" diagnostics.

use hyperion_semantics::db::{Database, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle};
use hyperion_semantics::SemanticErrorKind;
use hyperion_workspace::index::WorkspaceIndex;
use std::{fs, path::Path};
use tempfile::TempDir;

fn write_and_index(
    root: &Path,
    index: &std::sync::Arc<WorkspaceIndex>,
    rel_path: &str,
    contents: &str,
) -> std::path::PathBuf {
    let path = root.join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&path, contents).expect("write file");
    index
        .index_file_with_source(&path, contents)
        .expect("index file");
    path
}

fn get_undefined_symbol_diagnostics(
    index: &std::sync::Arc<WorkspaceIndex>,
    file_path: &Path,
) -> Vec<String> {
    let file_id = index.get_file_id(file_path).expect("file id");
    let source = fs::read_to_string(file_path).unwrap();

    let mut db = Database::default();
    db.set_file_text(file_id, std::sync::Arc::new(source));
    db.set_file_language(
        file_id,
        hyperion_workspace::Language::from_path(file_path)
            .unwrap_or(hyperion_workspace::Language::Java),
    );
    db.set_workspace_index_handle(WorkspaceIndexHandle(std::sync::Arc::clone(index)));
    db.set_workspace_version(0);

    let file_sem = db.snapshot().file_semantics(file_id);
    file_sem
        .diagnostics()
        .iter()
        .filter_map(|e| match &e.kind {
            SemanticErrorKind::UndefinedSymbol { symbol_name, .. } => Some(symbol_name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn fully_qualified_method_call_no_spurious_diagnostic() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/util/Helper.java",
        r#"
            package com.example.util;

            public class Helper {
                public static void doWork() {}
            }
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            public class Main {
                public void test() {
                    com.example.util.Helper.doWork();
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        !errors.iter().any(|e| e == "com"),
        "Should not report 'com' as undefined symbol in fully-qualified reference. Got: {:?}",
        errors
    );
}

#[test]
fn fully_qualified_field_access_no_spurious_diagnostic() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/Constants.java",
        r#"
            package com.example;

            public class Constants {
                public static final String NAME = "test";
            }
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            public class Main {
                public void test() {
                    String s = com.example.Constants.NAME;
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        !errors.iter().any(|e| e == "com"),
        "Should not report 'com' as undefined in qualified field access. Got: {:?}",
        errors
    );
}

#[test]
fn inherited_field_access_resolved_in_subclass() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/Base.java",
        r#"
            package com.example;

            public class Base {
                protected String baseField = "base";
            }
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Child.java",
        r#"
            package com.example;

            public class Child extends Base {
                public String accessBase() {
                    return baseField;
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        !errors.iter().any(|e| e == "baseField"),
        "Should resolve inherited field from superclass. Got: {:?}",
        errors
    );
}

#[test]
fn inherited_field_access_resolves_from_grandparent() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/Grandparent.java",
        r#"
            package com.example;

            public class Grandparent {
                public String rootField = "root";
            }
        "#,
    );

    write_and_index(
        root,
        &index,
        "com/example/Parent.java",
        r#"
            package com.example;

            public class Parent extends Grandparent {
            }
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Child.java",
        r#"
            package com.example;

            public class Child extends Parent {
                public String accessRoot() {
                    return rootField;
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        !errors.iter().any(|e| e == "rootField"),
        "Should resolve inherited field from grandparent class. Got: {:?}",
        errors
    );
}

#[test]
fn inherited_field_access_three_level_chain_in_method_body() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/Base.java",
        r#"
            package com.example;

            public class Base {
                protected String baseField = "base";
            }
        "#,
    );

    write_and_index(
        root,
        &index,
        "com/example/Middle.java",
        r#"
            package com.example;

            public class Middle extends Base {
            }
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Leaf.java",
        r#"
            package com.example;

            public class Leaf extends Middle {
                public String accessDeepField() {
                    return baseField;
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        !errors.iter().any(|e| e == "baseField"),
        "Should resolve a field inherited from grandparent. Got: {:?}",
        errors
    );
}

#[test]
fn inherited_field_name_shadowed_by_parameter_and_local_variable() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/Base.java",
        r#"
            package com.example;

            public class Base {
                protected String token = "base";
            }
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Derived.java",
        r#"
            package com.example;

            public class Derived extends Base {
                public String readToken(String token) {
                    return token;
                }

                public String localTokenShadow() {
                    String token = "local";
                    return token;
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        !errors.iter().any(|e| e == "token"),
        "Should not report shadowed identifier names as undefined. Got: {:?}",
        errors
    );
}

#[test]
fn map_of_string_from_returns_hash_map_method_has_no_undefined_symbols() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            import java.util.Map;
            import java.util.HashMap;

            public class Main {
                public Map<String> returnsHashMap() {
                    return new HashMap<>();
                }

                public void test() {
                    Map<String> values = returnsHashMap();
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        errors.is_empty(),
        "Expected no undefined symbols for Map<String> returnsHashMap(). Got: {:?}",
        errors
    );
}

#[test]
fn map_string_variable_from_returns_hash_map_has_no_undefined_symbols() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            import java.util.Map;
            import java.util.HashMap;

            public class Main {
                public Map<String> returnsHashMap() {
                    return new HashMap<>();
                }

                public void test() {
                    Map<String> values = returnsHashMap();
                    values.put("key", "value");
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        errors.is_empty(),
        "Expected no undefined symbols for local generic Map assignment/call. Got: {:?}",
        errors
    );
}

#[test]
fn returns_subclass_assigned_to_superclass_has_no_undefined_symbols() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    write_and_index(
        root,
        &index,
        "com/example/Superclass.java",
        r#"
            package com.example;

            public class Superclass {}
        "#,
    );

    write_and_index(
        root,
        &index,
        "com/example/SubClass.java",
        r#"
            package com.example;

            public class SubClass extends Superclass {}
        "#,
    );

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            public class Main {
                public SubClass returnsSubClass() {
                    return new SubClass();
                }

                public void test() {
                    Superclass sup = returnsSubClass();
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        errors.is_empty(),
        "Expected no undefined symbols for superclass assignment from subclass return. Got: {:?}",
        errors
    );
}

#[test]
fn returns_implementation_assigned_to_interface_has_no_undefined_symbols() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            public interface Worker {}

            public class ConcreteWorker implements Worker {
                public ConcreteWorker returnsConcreteWorker() {
                    return new ConcreteWorker();
                }

                public void test() {
                    Worker worker = returnsConcreteWorker();
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        errors.is_empty(),
        "Expected no undefined symbols for interface assignment from implementation return. Got: {:?}",
        errors
    );
}

#[test]
fn interface_constant_in_superinterface_chain_has_no_undefined_symbols() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    let index = std::sync::Arc::new(WorkspaceIndex::new(root.to_path_buf()));

    let test_path = write_and_index(
        root,
        &index,
        "com/example/Main.java",
        r#"
            package com.example;

            public interface BaseContract {
                String BASE_LABEL = "base";
            }

            public interface DerivedContract extends BaseContract {
                String DERIVED_LABEL = "derived";
            }

            public class Impl implements DerivedContract {
                public String label() {
                    return BASE_LABEL;
                }
            }
        "#,
    );

    let errors = get_undefined_symbol_diagnostics(&index, &test_path);

    assert!(
        errors.is_empty(),
        "Expected no undefined symbols for superinterface constant access. Got: {:?}",
        errors
    );
}
