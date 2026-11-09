// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_kotlin::{parse, SyntaxNode};
use hyperion_semantics::db::{
    Database, FileId, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::{KotlinTypeChecker, ResolvedName, SemanticErrorKind};
use hyperion_workspace::{Language, WorkspaceIndex};

fn kotlin_semantics(
    source: &str,
    extra_files: &[(&str, &str)],
) -> (
    Arc<hyperion_semantics::FileSemantics>,
    Arc<WorkspaceIndex>,
    tempfile::TempDir,
) {
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    for (path, contents) in extra_files {
        let path = temp.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture package");
        }
        index
            .index_file_with_source(&path, contents)
            .expect("index fixture");
    }

    let main_path = temp.path().join("Main.kt");
    index
        .index_file_with_source(&main_path, source)
        .expect("index Kotlin source");
    let file_id = index.get_file_id(&main_path).expect("main file id");

    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_string()));
    db.set_file_language(file_id, Language::Kotlin);
    db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(&index)));
    db.set_workspace_version(0);

    (db.snapshot().file_semantics(file_id), index, temp)
}

fn unresolved_names(source: &str) -> Vec<String> {
    let (semantics, _, _) = kotlin_semantics(source, &[]);
    semantics
        .diagnostics()
        .iter()
        .filter_map(|error| match &error.kind {
            SemanticErrorKind::UnresolvedReference { symbol_name } => Some(symbol_name.clone()),
            _ => None,
        })
        .collect()
}

fn type_check(source: &str) -> Vec<SemanticErrorKind> {
    type_check_with_files(source, &[])
}

fn type_check_with_files(source: &str, extra_files: &[(&str, &str)]) -> Vec<SemanticErrorKind> {
    let parsed = parse(source);
    assert!(parsed.errors.is_empty(), "test fixture must parse");
    let root = SyntaxNode::new_root(parsed.green_node);
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    for (path, contents) in extra_files {
        let path = temp.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        index
            .index_file_with_source(&path, contents)
            .expect("index fixture");
    }
    KotlinTypeChecker::new()
        .check_with_context(&root, FileId::new(0), index)
        .into_iter()
        .map(|error| error.kind)
        .collect()
}

#[test]
fn misspelled_local_is_unresolved() {
    let names = unresolved_names(
        r#"
fun test() {
    val actual = 1
    println(actaul)
}
"#,
    );
    assert_eq!(names, vec!["actaul"]);
}

#[test]
fn resolvable_local_is_recorded_and_not_diagnosed() {
    let source = r#"
fun test() {
    val actual = 1
    println(actual)
}
"#;
    let (semantics, _, _) = kotlin_semantics(source, &[]);
    assert!(semantics.diagnostics().is_empty());
    assert!(
        semantics.resolved_names().values().any(|resolved| {
            matches!(
                resolved,
                ResolvedName::LocalVariable { name, .. } if name == "actual"
            )
        }),
        "resolutions: {:?}",
        semantics.resolved_names()
    );
}

#[test]
fn indexed_stdlib_function_is_not_unresolved() {
    let source = r#"
import kotlin.collections.listOf

fun test() {
    val values = listOf(1, 2, 3)
    println(values)
}
"#;
    let (semantics, index, _) = kotlin_semantics(
        source,
        &[(
            "kotlin/collections/Collections.kt",
            "package kotlin.collections\nfun listOf(value: Int): List<Int> = TODO()",
        )],
    );
    assert!(
        !index.find_by_simple_name("listOf").is_empty(),
        "fixture symbol must actually be present in the workspace index"
    );
    assert!(semantics.diagnostics().is_empty());
}

#[test]
fn unrelated_workspace_simple_name_does_not_override_scoped_external_identity() {
    let source = "package use\nfun run() = Ghost()";
    let (semantics, _, _) =
        kotlin_semantics(source, &[("other/Ghost.kt", "package other\nclass Ghost")]);

    assert!(semantics.diagnostics().is_empty());
    // (The file's top-level `fun run` gives package `use` a JVM facade class, so
    // the package no longer counts as wholly unindexed and no speculative
    // `use.Ghost` identity is recorded; silence is what matters.)
    assert!(
        semantics.resolved_names().values().all(
            |resolved| !matches!(resolved, ResolvedName::Type { fqn, .. } | ResolvedName::External { fqn } if fqn == "other.Ghost")
        ),
        "an unrelated indexed simple name must never win: {semantics:#?}"
    );
}

#[test]
fn ambiguous_wildcard_type_suppresses_diagnostic_without_caching_an_identity() {
    let source = "package use\nimport alpha.*\nimport beta.*\nfun run() = User()";
    let (semantics, _, _) = kotlin_semantics(
        source,
        &[
            ("alpha/User.kt", "package alpha\nclass User"),
            ("beta/User.kt", "package beta\nclass User"),
        ],
    );

    assert!(
        semantics.diagnostics().is_empty(),
        "wildcard-backed external/ambiguous names remain diagnostic-safe"
    );
    assert!(
        semantics.resolved_names().values().all(|resolved| {
            !matches!(
                resolved,
                ResolvedName::Type { fqn, .. } | ResolvedName::External { fqn }
                    if fqn.ends_with("User")
            )
        }),
        "an ambiguous star import has no canonical identity: {semantics:#?}"
    );
}

#[test]
fn implicit_receiver_lambda_reference_is_not_speculatively_unresolved() {
    let names = unresolved_names(
        r#"
fun test(builder: StringBuilder) {
    with(builder) {
        append("value")
        length
    }
}
"#,
    );
    assert!(names.is_empty(), "implicit receiver references: {names:?}");
}

#[test]
fn unknown_receiver_member_is_not_speculatively_unresolved() {
    let names = unresolved_names(
        r#"
fun test() {
    missingReceiver.maybeProvidedByDsl()
}
"#,
    );
    assert!(names.is_empty(), "unknown receiver access: {names:?}");
}

#[test]
fn unindexed_import_reference_is_not_unresolved() {
    let names = unresolved_names(
        r#"
import external.library.generatedApi

fun test() {
    generatedApi()
}
"#,
    );
    assert!(names.is_empty(), "unindexed imported API: {names:?}");
}

#[test]
fn val_reassignment_is_an_error() {
    let errors = type_check(
        r#"
fun test() {
    val answer = 41
    answer = 42
}
"#,
    );
    assert!(errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { variable_name } if variable_name == "answer")));
}

#[test]
fn parameter_reassignment_is_an_error() {
    let errors = type_check(
        r#"
fun test(input: Int) {
    input = 42
}
"#,
    );
    assert!(errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { variable_name } if variable_name == "input")));
}

#[test]
fn var_reassignment_is_allowed() {
    let errors = type_check(
        r#"
fun test() {
    var answer = 41
    answer = 42
}
"#,
    );
    assert!(!errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { .. })));
}

#[test]
fn shadowing_inner_var_is_the_assignment_target() {
    let errors = type_check(
        r#"
fun test() {
    val value = 1
    {
        var value = 2
        value = 3
    }
}
"#,
    );
    assert!(!errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { .. })));
}

#[test]
fn val_initializer_is_not_a_reassignment() {
    let errors = type_check(
        r#"
fun test() {
    val answer = 42
}
"#,
    );
    assert!(!errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { .. })));
}

#[test]
fn first_assignment_to_deferred_val_is_not_speculatively_reassignment() {
    let errors = type_check(
        r#"
fun test() {
    val answer: Int
    answer = 42
}
"#,
    );
    assert!(!errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { .. })));
}

#[test]
fn member_assignment_with_unknown_receiver_is_not_a_val_error() {
    let errors = type_check(
        r#"
fun test(holder: Any) {
    holder.value = 42
}
"#,
    );
    assert!(!errors
        .iter()
        .any(|error| matches!(error, SemanticErrorKind::ValReassignment { .. })));
}

#[test]
fn nullable_access_diagnostics_follow_kotlin_flow_narrowing() {
    let cases = [
        (
            "unsafe nullable receiver",
            r#"fun test(str: String?) {
    str.length
}"#,
            1,
        ),
        (
            "safe call",
            r#"fun test(str: String?) {
    str?.length
}"#,
            0,
        ),
        (
            "non-null receiver",
            r#"fun test(str: String) {
    str.length
}"#,
            0,
        ),
        (
            "true null-check branch",
            r#"fun test(str: String?) {
    if (str != null) {
        str.length
    }
}"#,
            0,
        ),
        (
            "is-check branch",
            r#"fun test(value: Any?) {
    if (value is String) {
        value.length
    }
}"#,
            0,
        ),
        (
            // The variable is null-checked earlier in the function. That may
            // narrow it after the `if` (early return, `&&` chains, contracts),
            // which the flow tracker does not model, so stay silent.
            "outside narrowed scope",
            r#"fun test(str: String?) {
    if (str != null) {
        str.length
    }
    str.length
}"#,
            0,
        ),
        (
            "false null-check branch",
            r#"fun test(str: String?) {
    if (str == null) {
        println("missing")
    } else {
        str.length
    }
}"#,
            0,
        ),
        (
            "chained nullable receiver",
            r#"class User {
    val name: String? = null
}

fun test(user: User?) {
    user.name.length
}"#,
            1,
        ),
    ];

    for (name, source, expected_nullable_errors) in cases {
        let errors = type_check(source);
        let nullable_errors = errors
            .iter()
            .filter(|error| matches!(error, SemanticErrorKind::NullableTypeMismatch { .. }))
            .count();
        assert_eq!(
            nullable_errors, expected_nullable_errors,
            "case {name:?} produced {errors:#?}"
        );
        assert_eq!(
            errors.len(),
            expected_nullable_errors,
            "case {name:?} produced unrelated diagnostics: {errors:#?}"
        );
    }
}

#[test]
fn extension_scope_is_silent_for_names_the_workspace_never_declared() {
    let errors = type_check(
        r#"package com.example

fun test(str: String, items: List<Int>) {
    str.customExtension()
    items.flatMap { listOf(it) }
    str.toString()
}"#,
    );
    assert!(errors.is_empty(), "unexpected diagnostics: {errors:#?}");
}

#[test]
fn extension_scope_diagnostics_distinguish_imports_and_members() {
    let cases = [
        (
            "missing import",
            r#"package com.example

fun test(str: String) {
    str.customExtension()
}"#,
            1,
        ),
        (
            "explicit import",
            r#"package com.example
import com.utils.customExtension

fun test(str: String) {
    str.customExtension()
}"#,
            0,
        ),
        (
            "wildcard import",
            r#"package com.example
import com.utils.*

fun test(str: String) {
    str.customExtension()
}"#,
            0,
        ),
        (
            "same package declaration",
            r#"package com.example

fun String.customExtension() = "extended"

fun test(str: String) {
    str.customExtension()
}"#,
            0,
        ),
        (
            "standard library extension",
            r#"package com.example

fun test() {
    val str = "hello"
    str.uppercase()
}"#,
            0,
        ),
        (
            "regular member",
            r#"package com.example

class User {
    fun getName(): String = "John"
}

fun test(user: User) {
    user.getName()
}"#,
            0,
        ),
    ];

    // The workspace declares `String.customExtension` in another package, so
    // "not imported" is provable. Names the workspace has never seen (jar,
    // stdlib, DSL receivers) must never be reported.
    let workspace = [(
        "com/utils/Ext.kt",
        "package com.utils\n\nfun String.customExtension() = \"x\"\n",
    )];
    for (name, source, expected_scope_errors) in cases {
        let errors = type_check_with_files(source, &workspace);
        let scope_errors = errors
            .iter()
            .filter(|error| matches!(error, SemanticErrorKind::ExtensionFunctionNotInScope { .. }))
            .count();
        assert_eq!(
            scope_errors, expected_scope_errors,
            "case {name:?} produced {errors:#?}"
        );
        assert_eq!(
            errors.len(),
            expected_scope_errors,
            "case {name:?} produced unrelated diagnostics: {errors:#?}"
        );
    }
}

#[test]
fn extension_scope_does_not_type_a_member_chain_receiver_as_one_of_its_variables() {
    // `a.length` and `box.inner.c` are Ints, so neither call is on the String `a` or `c`
    let source = r#"package com.example

class Inner(val c: Int)
class Box(val inner: Inner)

fun test(a: String, c: String, box: Box) {
    a.length.customExtension()
    box.inner.c.customExtension()
}"#;
    let workspace = [(
        "com/utils/Ext.kt",
        "package com.utils\n\nfun String.customExtension() = \"x\"\n",
    )];
    let errors = type_check_with_files(source, &workspace);
    let typed_as_string: Vec<_> = errors
        .iter()
        .filter(|error| {
            matches!(
                error,
                SemanticErrorKind::ExtensionFunctionNotInScope { receiver_type, .. }
                    if receiver_type.ends_with("String")
            )
        })
        .collect();
    assert!(typed_as_string.is_empty(), "{typed_as_string:#?}");
}
