// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Clean, compiling Kotlin must produce no error diagnostics.
//!
//! A false error on valid code is worse than a missed error, so every fixture
//! here is code that `kotlinc` accepts.

use std::sync::Arc;

use hyperion_lsp::diagnostic_engine::{collect_diagnostics_for_file, DiagnosticOptions};
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{
    Database, FileId, Language, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_workspace::WorkspaceIndex;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

/// Diagnostics for `target` in a project made of `files` (relative path, text).
pub async fn project_diagnostics(files: &[(&str, &str)], target: &str) -> Vec<Diagnostic> {
    let temp = tempfile::tempdir().expect("temp workspace");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let mut target_source = None;
    for (rel, text) in files {
        let path = temp.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        index
            .index_file_with_source(&path, text)
            .expect("index fixture");
        if *rel == target {
            target_source = Some(*text);
        }
    }
    let source = target_source.expect("target among files");
    let path = temp.path().join(target);
    let file_id = index.get_file_id(&path).unwrap_or_else(|| FileId::new(0));

    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_string()));
    db.set_file_language(
        file_id,
        if is_kotlin_target(target) {
            Language::Kotlin
        } else {
            Language::Java
        },
    );
    db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(&index)));
    db.set_workspace_version(0);

    collect_diagnostics_for_file(
        file_id,
        if is_kotlin_target(target) {
            "kotlin"
        } else {
            "java"
        },
        source,
        &SemanticHost::with_database(db, Arc::clone(&index)),
        None,
        true,
        DiagnosticOptions::default(),
    )
    .await
}

/// Whether `target` is a Kotlin source, by its `.kt` extension.
fn is_kotlin_target(target: &str) -> bool {
    std::path::Path::new(target)
        .extension()
        .is_some_and(|extension| extension == "kt")
}

fn errors(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .map(|d| {
            format!(
                "{}:{} {}",
                d.range.start.line + 1,
                d.range.start.character,
                d.message
            )
        })
        .collect()
}

async fn assert_clean(source: &str) {
    let diagnostics = project_diagnostics(&[("Main.kt", source)], "Main.kt").await;
    let errors = errors(&diagnostics);
    assert!(
        errors.is_empty(),
        "false errors on clean Kotlin: {errors:#?}"
    );
}

#[tokio::test]
async fn infix_function_names_are_not_references() {
    assert_clean(
        r#"
fun bits(a: Int, b: Int, c: Long): Long {
    val x = a and b
    val y = a or b xor 1
    return c shl 2 shr 1 or (x.toLong() and y.toLong())
}
"#,
    )
    .await;
}

#[tokio::test]
async fn kotlin_stdlib_top_level_functions_resolve() {
    assert_clean(
        r#"
fun stdlib(s: String, l: List<Int>): Int {
    val bytes = byteArrayOf(1, 2, 3)
    val text = buildString { append(s); append(bytes.size) }
    val m = minOf(1, 2)
    checkNotNull(text)
    val lazyValue by lazy { "x" }
    val arr = l.toTypedArray()
    synchronized(l) { println(lazyValue) }
    return m + arr.size
}
"#,
    )
    .await;
}

#[tokio::test]
async fn compound_assignment_on_val_collection_is_not_a_reassignment() {
    assert_clean(
        r#"
class Invoice {
    private val lines = mutableListOf<String>()
    fun add(item: String): Invoice = apply { lines += item }
    fun remove(item: String) {
        lines -= item
    }
}
"#,
    )
    .await;
}

#[tokio::test]
async fn vararg_parameter_resolves_by_its_own_name() {
    assert_clean(
        r#"
class Q {
    fun g(vararg cs: String) = h(*cs)
    fun h(vararg cs: String) = cs.size
}
"#,
    )
    .await;
}

#[tokio::test]
async fn member_and_stdlib_calls_are_not_extension_scope_errors() {
    assert_clean(
        r#"
import java.math.BigDecimal

class Customer { fun getName(): String = "x" }
class Item(val quantity: Int, val description: String)

fun clean(items: List<Item>, names: Set<String>, s: String, c: Customer): String {
    items.forEach { it.quantity }
    val f = items.filter { it.quantity > 1 }.map { it.description }
    val sum = items.sumOf { it.quantity }
    val any = names.any { it.startsWith("a") } || names.isEmpty()
    val trimmed = s.trim().substring(1).padStart(3, '0')
    val r = runCatching { s.toInt() }.getOrDefault(0)
    val big = BigDecimal.ONE.add(BigDecimal.TEN)
    return "$f $sum $any $trimmed $r $big ${c.getName()} ${sum.toString()}"
}
"#,
    )
    .await;
}

fn has_extension_scope_error(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| d.message.contains("is not in scope"))
}

#[tokio::test]
async fn missing_extension_import_is_reported_only_when_the_workspace_declares_the_extension() {
    let ext = (
        "com/utils/Ext.kt",
        "package com.utils\n\nfun String.shout(): String = uppercase()\n",
    );
    let missing = (
        "com/app/Main.kt",
        "package com.app\n\nfun f(s: String) = s.shout()\n",
    );
    let diagnostics = project_diagnostics(&[ext, missing], "com/app/Main.kt").await;
    assert!(
        has_extension_scope_error(&diagnostics),
        "expected an import error: {diagnostics:?}"
    );

    let imported = (
        "com/app/Main.kt",
        "package com.app\n\nimport com.utils.shout\n\nfun f(s: String) = s.shout()\n",
    );
    let diagnostics = project_diagnostics(&[ext, imported], "com/app/Main.kt").await;
    assert!(!has_extension_scope_error(&diagnostics), "{diagnostics:?}");

    let unknown = (
        "com/app/Main.kt",
        "package com.app\n\nfun f(s: String) = s.notDeclaredAnywhere()\n",
    );
    let diagnostics = project_diagnostics(&[ext, unknown], "com/app/Main.kt").await;
    assert!(!has_extension_scope_error(&diagnostics), "{diagnostics:?}");
}

#[tokio::test]
async fn member_extension_in_companion_is_not_a_missing_import() {
    let body = (
        "com/x/Body.kt",
        "package com.x\n\nclass Body {\n    companion object {\n        fun String.toBody(): Body = Body()\n    }\n}\n",
    );
    let user = (
        "com/x/Use.kt",
        "package com.x\n\nclass Use {\n    fun f(s: String): Body = with(Body) { s.toBody() }\n}\n",
    );
    let diagnostics = project_diagnostics(&[body, user], "com/x/Use.kt").await;
    assert!(!has_extension_scope_error(&diagnostics), "{diagnostics:?}");
}

#[tokio::test]
async fn destructured_names_and_spread_arguments_resolve() {
    assert_clean(
        r#"
class Pair2(val first: Int, val second: Int) {
    operator fun component1() = first
    operator fun component2() = second
}

fun take(vararg xs: Int) = xs.size

fun f(pair: Pair2, items: List<Pair2>, rest: Holder): Int {
    val (a, b) = pair
    var total = take(a, b)
    for ((c, d) in items) {
        total += c + d
    }
    return total + take(*rest.values)
}

class Holder(val values: IntArray)
"#,
    )
    .await;
}
