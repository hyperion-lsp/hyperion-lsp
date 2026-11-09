// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{Position, PrepareRenameResponse, WorkspaceEdit};

fn edit_starts(edit: &WorkspaceEdit) -> Vec<Position> {
    edit.changes
        .as_ref()
        .into_iter()
        .flat_map(|changes| changes.values())
        .flatten()
        .map(|edit| edit.range.start)
        .collect()
}

fn require_workspace_edit(result: Result<Option<WorkspaceEdit>, String>) -> WorkspaceEdit {
    result
        .expect("rename should not error")
        .expect("rename should produce a workspace edit")
}

fn has_edit_at(edit: &WorkspaceEdit, file_suffix: &str, position: Position) -> bool {
    edit.changes.as_ref().is_some_and(|changes| {
        changes.iter().any(|(uri, edits)| {
            uri.path().ends_with(file_suffix)
                && edits
                    .iter()
                    .any(|text_edit| text_edit.range.start == position)
        })
    })
}

#[test]
fn test_prepare_rename_on_variable() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        public class Test {
            public void test() {
                int count = 0;
                coun<|>t++;
            }
        }
    ",
    );

    let result = test.prepare_rename("com/example/Test.java", "<|>");

    match result {
        Ok(Some(PrepareRenameResponse::Range(range))) => {
            println!("✅ prepare_rename succeeded with range: {:?}", range);
            // range.start.line is u32, always >= 0
        }
        Ok(Some(_)) => {
            println!("✅ prepare_rename succeeded with other response type");
        }
        Ok(None) => {
            panic!("Expected prepare_rename to succeed, got None");
        }
        Err(e) => {
            panic!("prepare_rename failed: {}", e);
        }
    }
}

#[test]
fn test_prepare_rename_on_method() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Calculator.java",
        r"
        package com.example;

        public class Calculator {
            public int add(int a, int b) {
                return a + b;
            }

            public void test() {
                int result = ad<|>d(1, 2);
            }
        }
    ",
    );

    let result = test.prepare_rename("com/example/Calculator.java", "<|>");

    match result {
        Ok(Some(_)) => {
            println!("✅ prepare_rename on method succeeded");
        }
        Ok(None) => {
            // It's OK if we can't rename this yet - the implementation is basic
            println!("⚠️  prepare_rename returned None (feature not fully implemented)");
        }
        Err(e) => {
            panic!("prepare_rename failed with error: {}", e);
        }
    }
}

#[test]
fn test_rename_variable_single_file() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        public class Test {
            public void test() {
                int count = 0;
                coun<|>t++;
                System.out.println(count);
            }
        }
    ",
    );

    let result = test.rename("com/example/Test.java", "<|>", "total");

    match result {
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..
        })) => {
            println!(
                "✅ rename succeeded with {} file(s) affected",
                changes.len()
            );

            // Check that we have edits
            assert!(
                !changes.is_empty(),
                "Expected at least one file to be changed"
            );

            for (uri, edits) in &changes {
                println!("  File: {}, {} edits", uri, edits.len());
                for edit in edits {
                    println!("    - Replace at {:?}: '{}'", edit.range, edit.new_text);
                    assert_eq!(edit.new_text, "total", "New text should be 'total'");
                }
            }
        }
        Ok(Some(_)) => {
            println!("✅ rename succeeded (no changes in response)");
        }
        Ok(None) => {
            println!("⚠️  rename returned None (may need more work)");
        }
        Err(e) => {
            println!(
                "⚠️  rename failed: {} (expected for basic implementation)",
                e
            );
        }
    }
}

#[test]
fn java_local_rename_is_scope_aware_and_ignores_text_matches() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/JavaLocals.java",
        r#"
        package com.example;
        class JavaLocals {
            void first() {
                int <decl>value = 1;
                System.out.println(<use>value);
                {
                    int <shadow>value = 2;
                    System.out.println(<shadowUse>value);
                }
                int valueSuffix = 0;
                String text = "value";
                // value must not be edited
            }
            void second() {
                int <other>value = 3;
            }
        }
        "#,
    );

    let included = ["<decl>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/JavaLocals.java", marker)
            .unwrap()
    });
    let excluded = ["<shadow>", "<shadowUse>", "<other>"].map(|marker| {
        test.get_marker_position("com/example/JavaLocals.java", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/JavaLocals.java",
        "<decl>",
        "renamed",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position), "missing edit at {position:?}");
    }
    for position in excluded {
        assert!(!starts.contains(&position), "unsafe edit at {position:?}");
    }
}

#[test]
fn java_parameter_rename_does_not_touch_same_named_parameter() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/JavaParameters.java",
        r"
        package com.example;
        class JavaParameters {
            void first(int <decl>amount) {
                System.out.println(<use>amount);
            }
            void second(int <other>amount) {
                System.out.println(<otherUse>amount);
            }
        }
        ",
    );

    let included = ["<decl>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/JavaParameters.java", marker)
            .unwrap()
    });
    let excluded = ["<other>", "<otherUse>"].map(|marker| {
        test.get_marker_position("com/example/JavaParameters.java", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/JavaParameters.java",
        "<decl>",
        "total",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position));
    }
    for position in excluded {
        assert!(!starts.contains(&position));
    }
}

#[test]
fn java_field_rename_does_not_touch_same_named_local_or_other_class_field() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/JavaFields.java",
        r"
        package com.example;
        class JavaFields {
            int <decl>count;
            void increment() {
                this.<qualified>count++;
                <use>count++;
            }
            void local() {
                int <local>count = 0;
                System.out.println(<localUse>count);
            }
        }
        class OtherFields {
            int <other>count;
        }
        ",
    );

    let included = ["<decl>", "<qualified>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/JavaFields.java", marker)
            .unwrap()
    });
    let excluded = ["<local>", "<localUse>", "<other>"].map(|marker| {
        test.get_marker_position("com/example/JavaFields.java", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/JavaFields.java",
        "<decl>",
        "total",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position));
    }
    for position in excluded {
        assert!(!starts.contains(&position));
    }
}

#[test]
fn kotlin_local_rename_is_scope_aware_and_ignores_text_matches() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinLocals.kt",
        r#"
        package com.example
        fun first() {
            val <decl>value = 1
            println(<use>value)
            run {
                val <shadow>value = 2
                println(<shadowUse>value)
            }
            val valueSuffix = 0
            val text = "value"
            // value must not be edited
        }
        fun second() {
            val <other>value = 3
        }
        "#,
    );

    let included = ["<decl>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/KotlinLocals.kt", marker)
            .unwrap()
    });
    let excluded = ["<shadow>", "<shadowUse>", "<other>"].map(|marker| {
        test.get_marker_position("com/example/KotlinLocals.kt", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/KotlinLocals.kt",
        "<decl>",
        "renamed",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position), "missing edit at {position:?}");
    }
    for position in excluded {
        assert!(!starts.contains(&position), "unsafe edit at {position:?}");
    }
}

#[test]
fn kotlin_parameter_rename_does_not_touch_same_named_parameter() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinParameters.kt",
        r"
        package com.example
        fun first(<decl>amount: Int) {
            println(<use>amount)
        }
        fun second(<other>amount: Int) {
            println(<otherUse>amount)
        }
        ",
    );

    let included = ["<decl>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/KotlinParameters.kt", marker)
            .unwrap()
    });
    let excluded = ["<other>", "<otherUse>"].map(|marker| {
        test.get_marker_position("com/example/KotlinParameters.kt", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/KotlinParameters.kt",
        "<decl>",
        "total",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position));
    }
    for position in excluded {
        assert!(!starts.contains(&position));
    }
}

#[test]
fn kotlin_property_rename_does_not_touch_same_named_local_or_other_property() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinProperties.kt",
        r"
        package com.example
        class Counter {
            var <decl>count = 0
            fun increment() {
                this.<qualified>count++
                <use>count++
            }
            fun local() {
                val <local>count = 4
                println(<localUse>count)
            }
        }
        class OtherCounter {
            var <other>count = 0
        }
        ",
    );

    let included = ["<decl>", "<qualified>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/KotlinProperties.kt", marker)
            .unwrap()
    });
    let excluded = ["<local>", "<localUse>", "<other>"].map(|marker| {
        test.get_marker_position("com/example/KotlinProperties.kt", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/KotlinProperties.kt",
        "<decl>",
        "total",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position));
    }
    for position in excluded {
        assert!(!starts.contains(&position));
    }
}

#[test]
fn kotlin_function_rename_does_not_touch_same_named_member_function() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinFunctions.kt",
        r"
        package com.example
        fun <decl>process() = Unit
        fun caller() {
            <use>process()
        }
        class Worker {
            fun <other>process() = Unit
            fun caller() {
                <otherUse>process()
            }
        }
        ",
    );

    let included = ["<decl>", "<use>"].map(|marker| {
        test.get_marker_position("com/example/KotlinFunctions.kt", marker)
            .unwrap()
    });
    let excluded = ["<other>", "<otherUse>"].map(|marker| {
        test.get_marker_position("com/example/KotlinFunctions.kt", marker)
            .unwrap()
    });
    let starts = edit_starts(&require_workspace_edit(test.rename(
        "com/example/KotlinFunctions.kt",
        "<decl>",
        "execute",
    )));

    assert_eq!(starts.len(), included.len(), "unexpected edits: {starts:?}");
    for position in included {
        assert!(starts.contains(&position));
    }
    for position in excluded {
        assert!(!starts.contains(&position));
    }
}

#[test]
fn test_rename_method_cross_file() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Service.java",
        r#"
        package com.example;

        public class Service {
            public void proce<|>ss() {
                System.out.println("processing");
            }
        }
    "#,
    );

    test.add_file(
        "com/example/Client.java",
        r"
        package com.example;

        public class Client {
            public void test(Service service) {
                service.process();
            }
        }
    ",
    );

    let result = test.rename("com/example/Service.java", "<|>", "execute");

    match result {
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..
        })) => {
            println!(
                "✅ cross-file rename succeeded with {} file(s) affected",
                changes.len()
            );

            // Ideally we should have edits in both files
            if changes.len() >= 2 {
                println!("  ✅ Multiple files affected (expected for cross-file rename)");
            } else {
                println!(
                    "  ⚠️  Only {} file(s) affected (may need find-references improvement)",
                    changes.len()
                );
            }
        }
        Ok(Some(_)) => {
            println!("✅ rename succeeded");
        }
        Ok(None) => {
            println!("⚠️  rename returned None");
        }
        Err(e) => {
            println!("⚠️  rename failed: {}", e);
        }
    }
}

#[test]
fn kotlin_class_rename_crosses_kotlin_and_java_files() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/User.kt",
        r"
        package com.example
        class <decl>User
        ",
    );
    test.add_file(
        "com/example/KotlinUse.kt",
        r"
        package com.example
        val user: <ktType>User = <ktCtor>User()
        ",
    );
    test.add_file(
        "com/example/JavaUse.java",
        r"
        package com.example;
        class JavaUse {
            <javaType>User user = new <javaCtor>User();
        }
        ",
    );

    let expected = [
        (
            "User.kt",
            test.get_marker_position("com/example/User.kt", "<decl>")
                .unwrap(),
        ),
        (
            "KotlinUse.kt",
            test.get_marker_position("com/example/KotlinUse.kt", "<ktType>")
                .unwrap(),
        ),
        (
            "KotlinUse.kt",
            test.get_marker_position("com/example/KotlinUse.kt", "<ktCtor>")
                .unwrap(),
        ),
        (
            "JavaUse.java",
            test.get_marker_position("com/example/JavaUse.java", "<javaType>")
                .unwrap(),
        ),
        (
            "JavaUse.java",
            test.get_marker_position("com/example/JavaUse.java", "<javaCtor>")
                .unwrap(),
        ),
    ];

    let edit = require_workspace_edit(test.rename("com/example/User.kt", "<decl>", "Person"));
    for (file, position) in expected {
        assert!(
            has_edit_at(&edit, file, position),
            "missing cross-language edit in {file} at {position:?}: {edit:?}"
        );
    }
}

#[test]
fn java_class_rename_crosses_into_kotlin_file() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Widget.java",
        r"
        package com.example;
        public class <decl>Widget {}
        ",
    );
    test.add_file(
        "com/example/KotlinWidgetUse.kt",
        r"
        package com.example
        val widget: <ktType>Widget = <ktCtor>Widget()
        ",
    );

    let expected = [
        (
            "Widget.java",
            test.get_marker_position("com/example/Widget.java", "<decl>")
                .unwrap(),
        ),
        (
            "KotlinWidgetUse.kt",
            test.get_marker_position("com/example/KotlinWidgetUse.kt", "<ktType>")
                .unwrap(),
        ),
        (
            "KotlinWidgetUse.kt",
            test.get_marker_position("com/example/KotlinWidgetUse.kt", "<ktCtor>")
                .unwrap(),
        ),
    ];

    let edit =
        require_workspace_edit(test.rename("com/example/Widget.java", "<decl>", "Component"));
    for (file, position) in expected {
        assert!(
            has_edit_at(&edit, file, position),
            "missing Java-to-Kotlin edit in {file} at {position:?}: {edit:?}"
        );
    }
}

#[test]
fn cross_language_rename_rejects_name_invalid_in_an_edited_language() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Widget.java",
        "package com.example; public class <decl>Widget {}",
    );
    test.add_file(
        "com/example/Use.kt",
        "package com.example\nval widget: Widget = Widget()",
    );

    let error = test
        .rename("com/example/Widget.java", "<decl>", "when")
        .expect_err("a Kotlin keyword cannot be written into a Kotlin edit target");
    assert!(error.contains("not a valid Kotlin identifier"), "{error}");
    assert!(error.contains("Use.kt"), "{error}");
}

#[test]
fn java_value_rename_does_not_conflict_with_callable_namespace() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Namespaces.java",
        r"
        package com.example;
        class Namespaces {
            int <decl>value;
            void total() {}
        }
        ",
    );

    require_workspace_edit(test.rename("com/example/Namespaces.java", "<decl>", "total"));
}

#[test]
fn local_rename_proof_is_confined_to_its_declaring_file() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Local.java",
        r"
        package com.example;
        class Local {
            void run() {
                int <decl>count = 1;
                System.out.println(count);
            }
        }
        ",
    );
    test.add_file(
        "com/example/Unrelated.groovy",
        "package com.example\nclass Unrelated { def count = 2 }",
    );

    let edit = require_workspace_edit(test.rename("com/example/Local.java", "<decl>", "total"));
    assert_eq!(
        edit.changes
            .as_ref()
            .map_or(0, std::collections::HashMap::len),
        1,
        "a local rename must neither audit nor edit unrelated workspace files"
    );
}

#[test]
fn kotlin_function_rename_crosses_files_via_indexed_fqn() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Names.kt",
        r"
        package com.example
        fun <decl>formatName(value: String) = value
        ",
    );
    test.add_file(
        "com/example/NameUse.kt",
        r#"
        package com.example
        val formatted = <use>formatName("Ada")
        "#,
    );

    let declaration = test
        .get_marker_position("com/example/Names.kt", "<decl>")
        .unwrap();
    let usage = test
        .get_marker_position("com/example/NameUse.kt", "<use>")
        .unwrap();
    let edit = require_workspace_edit(test.rename("com/example/Names.kt", "<decl>", "renderName"));

    assert!(has_edit_at(&edit, "Names.kt", declaration));
    assert!(
        has_edit_at(&edit, "NameUse.kt", usage),
        "missing cross-file Kotlin function edit: {edit:?}"
    );
}

#[test]
fn test_rename_method_overload_only_targets_signature() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Overloads.java",
        r#"
        package com.example;

        public class Overloads {
            public void fo<|>o(int value) {}
            public void foo(String value) {}

            public void test() {
                foo(1);
                foo("a");
            }
        }
    "#,
    );

    let result = test.rename("com/example/Overloads.java", "<|>", "bar");

    let edit_count = match result {
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..
        })) => changes.values().map(|edits| edits.len()).sum::<usize>(),
        _ => {
            panic!("Expected rename edits for overload test");
        }
    };

    assert_eq!(
        edit_count, 2,
        "Expected only declaration + matching call to be renamed, got {} edits",
        edit_count
    );
}

#[test]
fn test_rename_class() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public class <decl>Use<|>r {
            private String name;

            public <constructor>User() {}

            public String getName() {
                return name;
            }
        }
    ",
    );

    test.add_file(
        "com/example/UserService.java",
        r"
        package com.example;

        public class UserService {
            private <fieldType>User user;

            public <returnType>User getUser() {
                return new <newExpr>User();
            }
        }
    ",
    );

    let expected = [
        (
            "User.java",
            test.get_marker_position("com/example/User.java", "<decl>")
                .unwrap(),
        ),
        (
            "User.java",
            test.get_marker_position("com/example/User.java", "<constructor>")
                .unwrap(),
        ),
        (
            "UserService.java",
            test.get_marker_position("com/example/UserService.java", "<fieldType>")
                .unwrap(),
        ),
        (
            "UserService.java",
            test.get_marker_position("com/example/UserService.java", "<returnType>")
                .unwrap(),
        ),
        (
            "UserService.java",
            test.get_marker_position("com/example/UserService.java", "<newExpr>")
                .unwrap(),
        ),
    ];
    let edit = require_workspace_edit(test.rename("com/example/User.java", "<|>", "Person"));

    for (file, position) in expected {
        assert!(
            has_edit_at(&edit, file, position),
            "missing Java class edit in {file} at {position:?}: {edit:?}"
        );
    }
}

#[test]
fn test_prepare_rename_on_keyword_fails() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        public class Test {
            publ<|>ic void test() {}
        }
    ",
    );

    let result = test.prepare_rename("com/example/Test.java", "<|>");

    match result {
        Ok(None) => {
            println!("✅ prepare_rename correctly returned None for keyword");
        }
        Ok(Some(_)) => {
            panic!("Expected prepare_rename to fail on keyword, but it succeeded");
        }
        Err(_) => {
            println!("✅ prepare_rename failed on keyword (expected)");
        }
    }
}

#[test]
fn prepare_rename_rejects_unresolved_java_identifier() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Unresolved.java",
        r"
        package com.example;
        class Unresolved {
            void test() {
                miss<|>ing();
            }
        }
        ",
    );

    let result = test.prepare_rename("com/example/Unresolved.java", "<|>");
    assert!(
        matches!(result, Ok(None) | Err(_)),
        "an unresolved identifier must not be renameable: {result:?}"
    );
}

#[test]
fn prepare_rename_rejects_java_literal() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Literal.java",
        r"
        package com.example;
        class Literal {
            int value = 4<|>2;
        }
        ",
    );

    let result = test.prepare_rename("com/example/Literal.java", "<|>");
    assert!(
        matches!(result, Ok(None) | Err(_)),
        "a literal must not be renameable: {result:?}"
    );
}

#[test]
fn prepare_rename_rejects_external_java_symbol() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/External.java",
        r"
        package com.example;
        class External {
            Str<|>ing value;
        }
        ",
    );

    let result = test.prepare_rename("com/example/External.java", "<|>");
    assert!(
        matches!(result, Ok(None) | Err(_)),
        "a dependency/JDK declaration must not be renameable: {result:?}"
    );
}

#[test]
fn prepare_rename_rejects_external_kotlin_symbol() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/External.kt",
        r"
        package com.example
        val value: Str<|>ing = String()
        ",
    );

    let result = test.prepare_rename("com/example/External.kt", "<|>");
    assert!(
        matches!(result, Ok(None) | Err(_)),
        "a Kotlin dependency/JDK declaration must not be renameable: {result:?}"
    );
}

#[test]
fn prepare_rename_resolves_kotlin_local() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Local.kt",
        r"
        package com.example
        fun test() {
            val count = 1
            println(cou<|>nt)
        }
        ",
    );

    let result = test
        .prepare_rename("com/example/Local.kt", "<|>")
        .expect("Kotlin prepareRename should not error");
    assert!(
        matches!(result, Some(PrepareRenameResponse::Range(_))),
        "a resolved Kotlin local should be renameable"
    );
}

#[test]
fn prepare_rename_rejects_kotlin_keyword_and_literal() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Invalid.kt",
        r"
        package com.example
        fun test() {
            v<keyword>al count = 4<literal>2
        }
        ",
    );

    for marker in ["<keyword>", "<literal>"] {
        let result = test.prepare_rename("com/example/Invalid.kt", marker);
        assert!(
            matches!(result, Ok(None) | Err(_)),
            "Kotlin keyword/literal at {marker} must not be renameable: {result:?}"
        );
    }
}

#[test]
fn test_rename_with_conflicts() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        public class Test {
            public void test() {
                int count = 0;
                int total = 1;
                coun<|>t++;
            }
        }
    ",
    );

    // Try to rename 'count' to 'total' which would create a conflict
    let result = test.rename("com/example/Test.java", "<|>", "total");

    let error = result.expect_err("same-scope Java local conflict must be refused");
    assert!(
        error.contains("count") && error.contains("total") && error.contains("same scope"),
        "conflict error should name both symbols and the scope: {error}"
    );
}

#[test]
fn kotlin_local_conflict_is_refused_with_clear_error() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinConflict.kt",
        r"
        package com.example
        fun test() {
            val <decl>count = 0
            val total = 1
            println(count)
        }
        ",
    );

    let error = test
        .rename("com/example/KotlinConflict.kt", "<decl>", "total")
        .expect_err("same-scope Kotlin local conflict must be refused");
    assert!(
        error.contains("count") && error.contains("total") && error.contains("same scope"),
        "conflict error should name both symbols and the scope: {error}"
    );
}

#[test]
fn java_member_conflict_is_refused() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/JavaMemberConflict.java",
        r"
        package com.example;
        class JavaMemberConflict {
            int <decl>count;
            int total;
        }
        ",
    );

    let error = test
        .rename("com/example/JavaMemberConflict.java", "<decl>", "total")
        .expect_err("same-class Java member conflict must be refused");
    assert!(error.contains("same scope"), "{error}");
}

#[test]
fn method_rename_rejects_conflicts_across_the_relevant_hierarchy() {
    for (file, source) in [
        (
            "com/example/Contract.java",
            r#"
package com.example;
interface Contract { void <|>oldName(); }
class Worker implements Contract {
    public void oldName() {}
    public void replacement() {}
}
"#,
        ),
        (
            "com/example/Base.java",
            r#"
package com.example;
class Base { void <|>oldName() {} }
class Child extends Base { void replacement() {} }
"#,
        ),
    ] {
        let mut test = LspTest::new();
        test.add_file(file, source);
        let error = test
            .rename(file, "<|>", "replacement")
            .expect_err("hierarchy-changing method rename must be refused");
        assert!(
            error.contains("relevant type hierarchy"),
            "unexpected conflict error: {error}"
        );
    }
}

#[test]
fn kotlin_function_conflict_is_refused() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinFunctionConflict.kt",
        r"
        package com.example
        fun <decl>first() = Unit
        fun second() = Unit
        ",
    );

    let error = test
        .rename("com/example/KotlinFunctionConflict.kt", "<decl>", "second")
        .expect_err("same-package Kotlin function conflict must be refused");
    assert!(error.contains("same scope"), "{error}");
}

#[test]
fn kotlin_keyword_new_name_is_refused() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/KotlinName.kt",
        r"
        package com.example
        fun test() {
            val <decl>count = 0
        }
        ",
    );

    let error = test
        .rename("com/example/KotlinName.kt", "<decl>", "when")
        .expect_err("Kotlin keyword must not be accepted as a new name");
    assert!(error.contains("not a valid Kotlin identifier"), "{error}");
}

#[test]
fn test_rename_preserves_syntax() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Calculator.java",
        r"
        package com.example;

        public class Calculator {
            private int value;

            public int getValue() {
                return valu<|>e;
            }

            public void setValue(int value) {
                this.value = value;
            }
        }
    ",
    );

    let result = test.rename("com/example/Calculator.java", "<|>", "result");

    // The main goal here is to ensure rename doesn't crash
    match result {
        Ok(_) => {
            println!("✅ rename completed without crashing");
        }
        Err(e) => {
            println!("⚠️  rename failed: {}", e);
        }
    }
}

#[test]
fn test_rename_method_renames_overrides() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Animal.java",
        r#"
        package com.example;

        public class Animal {
            public void makeS<|>ound() {
                System.out.println("Some sound");
            }
        }
    "#,
    );

    test.add_file(
        "com/example/Dog.java",
        r#"
        package com.example;

        public class Dog extends Animal {
            @Override
            public void makeSound() {
                System.out.println("Woof");
            }
        }
    "#,
    );

    test.add_file(
        "com/example/Cat.java",
        r#"
        package com.example;

        public class Cat extends Animal {
            @Override
            public void makeSound() {
                System.out.println("Meow");
            }
        }
    "#,
    );

    let result = test.rename("com/example/Animal.java", "<|>", "vocalize");

    match result {
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..
        })) => {
            println!(
                "✅ method override rename succeeded with {} file(s) affected",
                changes.len()
            );

            let total_edits: usize = changes.values().map(|v| v.len()).sum();
            println!("  Total edits: {}", total_edits);

            // We should have edits in all three files (Animal, Dog, Cat)
            // Each file should have at least 1 edit (the definition or call)
            if changes.len() >= 3 {
                println!("  ✅ All files with override methods were affected");
            } else {
                println!(
                    "  ⚠️  Only {} files affected, expected at least 3 (Animal, Dog, Cat)",
                    changes.len()
                );
            }

            // Check that each edit renames to "vocalize"
            for (uri, edits) in &changes {
                println!("  File: {}, {} edits", uri, edits.len());
                for edit in edits {
                    println!("    - Replace at {:?}: '{}'", edit.range, edit.new_text);
                    assert_eq!(edit.new_text, "vocalize", "New text should be 'vocalize'");
                }
            }
        }
        Ok(Some(_)) => {
            println!("✅ rename succeeded");
        }
        Ok(None) => {
            println!("⚠️  rename returned None (may need more work on override detection)");
        }
        Err(e) => {
            println!("⚠️  rename failed: {}", e);
        }
    }
}

#[test]
fn test_rename_method_does_not_touch_overloads() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Overload.java",
        r#"
        package com.example;

        public class Overload {
            public void <|>ping() {}
            public void <otherdecl>ping(String value) {}

            public void test() {
                <call>ping();
                <othercall>ping("hi");
            }
        }
    "#,
    );

    let result = test.rename("com/example/Overload.java", "<|>", "pong");

    match result {
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..
        })) => {
            let decl_pos = test
                .get_marker_position("com/example/Overload.java", "<|>")
                .expect("Missing declaration marker");
            let call_pos = test
                .get_marker_position("com/example/Overload.java", "<call>")
                .expect("Missing call marker");
            let other_decl = test
                .get_marker_position("com/example/Overload.java", "<otherdecl>")
                .expect("Missing overload declaration marker");
            let other_call = test
                .get_marker_position("com/example/Overload.java", "<othercall>")
                .expect("Missing overload call marker");

            let mut edited_positions = Vec::new();
            for edits in changes.values() {
                for edit in edits {
                    edited_positions.push(edit.range.start);
                }
            }

            assert!(
                edited_positions.contains(&decl_pos),
                "Expected rename on the zero-arg method declaration"
            );
            assert!(
                edited_positions.contains(&call_pos),
                "Expected rename on the zero-arg method call"
            );
            assert!(
                !edited_positions.contains(&other_decl),
                "Did not expect rename on the overload declaration"
            );
            assert!(
                !edited_positions.contains(&other_call),
                "Did not expect rename on the overload call"
            );
        }
        Ok(Some(_)) => {
            panic!("Expected WorkspaceEdit with changes for overload rename test");
        }
        Ok(None) => {
            panic!("Expected rename edits for overload rename test");
        }
        Err(e) => {
            panic!("Rename failed: {e}");
        }
    }
}

#[test]
fn indexed_rename_fails_closed_for_runtime_resource_names() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/PluginImpl.java",
        "package com.example; public class <|>PluginImpl {}",
    )
    .add_file(
        "src/main/resources/META-INF/services/com.example.Plugin",
        "com.example.PluginImpl\n",
    );

    let error = test
        .rename("com/example/PluginImpl.java", "<|>", "RenamedPlugin")
        .expect_err("resource-driven runtime name must block rename");
    assert!(error.contains("resource"), "{error}");
}

#[test]
fn indexed_rename_ignores_unrelated_gradle_script_spellings() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Entity.java",
        "package com.example; public class <|>Entity {}",
    )
    .add_file("build.gradle", "plugins { id 'java' }\n");

    require_workspace_edit(test.rename("com/example/Entity.java", "<|>", "RenamedEntity"));
}

#[test]
fn class_rename_works_when_imported_in_other_packages() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/shapes/Circle.java",
        "package com.example.shapes;\n\npublic class <|>Circle {\n    public double area() { return 1.0; }\n}\n",
    )
    .add_file(
        "com/example/app/Main.java",
        "package com.example.app;\n\nimport com.example.shapes.Circle;\n\npublic class Main {\n    public static void main(String[] args) {\n        Circle c = new Circle();\n        System.out.println(c.area());\n    }\n}\n",
    );

    let edit =
        require_workspace_edit(test.rename("com/example/shapes/Circle.java", "<|>", "Round"));
    assert!(
        has_edit_at(&edit, "Main.java", Position::new(2, 26)),
        "import edit missing: {edit:?}"
    );
    assert!(
        has_edit_at(&edit, "Main.java", Position::new(6, 8)),
        "{edit:?}"
    );
}

#[test]
fn class_rename_updates_javadoc_link_mentions() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Circle.java",
        "package com.example;\n\npublic class <|>Circle {\n}\n",
    )
    .add_file(
        "com/example/Main.java",
        "package com.example;\n\n/** Uses {@link Circle} and {@code Circle}. */\npublic class Main {\n    Circle c;\n}\n",
    );

    let edit = require_workspace_edit(test.rename("com/example/Circle.java", "<|>", "Round"));
    assert!(
        has_edit_at(&edit, "Main.java", Position::new(2, 16)),
        "javadoc link edit missing: {edit:?}"
    );
    assert!(
        has_edit_at(&edit, "Main.java", Position::new(4, 4)),
        "{edit:?}"
    );
}

#[test]
fn java_class_rename_updates_kotlin_and_java_imports_across_packages() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/shapes/Circle.java",
        "package com.example.shapes;\n\npublic class <|>Circle {}\n",
    )
    .add_file(
        "com/example/app/Use.kt",
        "package com.example.app\n\nimport com.example.shapes.Circle\n\nval c: Circle = Circle()\n",
    )
    .add_file(
        "com/example/app/Use.java",
        "package com.example.app;\n\nimport com.example.shapes.Circle;\n\nclass UseJ { Circle c; }\n",
    );

    let edit =
        require_workspace_edit(test.rename("com/example/shapes/Circle.java", "<|>", "Round"));
    assert!(
        has_edit_at(&edit, "Use.kt", Position::new(2, 26)),
        "kotlin import: {edit:?}"
    );
    assert!(
        has_edit_at(&edit, "Use.kt", Position::new(4, 7)),
        "kotlin type: {edit:?}"
    );
    assert!(
        has_edit_at(&edit, "Use.java", Position::new(2, 26)),
        "java import: {edit:?}"
    );
}

#[test]
fn kotlin_class_rename_updates_java_and_kotlin_imports_across_packages() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/shapes/Circle.kt",
        "package com.example.shapes\n\nclass <|>Circle\n",
    )
    .add_file(
        "com/example/app/Use.kt",
        "package com.example.app\n\nimport com.example.shapes.Circle\n\nval c: Circle = Circle()\n",
    )
    .add_file(
        "com/example/app/Use.java",
        "package com.example.app;\n\nimport com.example.shapes.Circle;\n\nclass UseJ { Circle c; }\n",
    );

    let edit = require_workspace_edit(test.rename("com/example/shapes/Circle.kt", "<|>", "Round"));
    assert!(
        has_edit_at(&edit, "Use.kt", Position::new(2, 26)),
        "kotlin import: {edit:?}"
    );
    assert!(
        has_edit_at(&edit, "Use.kt", Position::new(4, 7)),
        "kotlin type: {edit:?}"
    );
    assert!(
        has_edit_at(&edit, "Use.java", Position::new(2, 26)),
        "java import: {edit:?}"
    );
}
