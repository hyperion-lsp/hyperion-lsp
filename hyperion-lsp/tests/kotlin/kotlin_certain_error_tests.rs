// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin errors that are certain, and the near misses that must
//! stay silent.

use super::kotlin_clean_code_tests::project_diagnostics;
use tower_lsp::lsp_types::DiagnosticSeverity;

async fn errors(source: &str) -> Vec<String> {
    project_diagnostics(&[("Main.kt", source)], "Main.kt")
        .await
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .map(|d| format!("{}: {}", d.range.start.line + 1, d.message))
        .collect()
}

async fn assert_error(source: &str, fragment: &str) {
    let errors = errors(source).await;
    assert!(
        errors.iter().any(|error| error.contains(fragment)),
        "expected an error containing {fragment:?}, got {errors:#?}\n{source}"
    );
}

async fn assert_silent(source: &str) {
    let errors = errors(source).await;
    assert!(errors.is_empty(), "false errors: {errors:#?}\n{source}");
}

#[tokio::test]
async fn class_instance_assigned_to_an_unrelated_builtin_type() {
    assert_error(
        "class Circle(val r: Double)\n\nfun f() {\n    val c: String = Circle(4.0)\n}\n",
        "expected 'String', actual 'Circle'",
    )
    .await;
}

#[tokio::test]
async fn class_instance_assigned_to_an_unrelated_workspace_class() {
    assert_error(
        "class Circle(val r: Double)\nclass Square(val s: Double)\n\nfun f() {\n    val c: Square = Circle(4.0)\n}\n",
        "expected 'Square', actual 'Circle'",
    )
    .await;
}

#[tokio::test]
async fn literals_that_cannot_initialise_the_declared_type() {
    assert_error(
        "fun f() {\n    val s: String = 42\n}\n",
        "expected 'String', actual 'Int'",
    )
    .await;
    assert_error(
        "fun f() {\n    val n: Int = \"x\"\n}\n",
        "expected 'Int', actual 'String'",
    )
    .await;
    assert_error(
        "fun f() {\n    val b: Boolean = 1\n}\n",
        "expected 'Boolean', actual 'Int'",
    )
    .await;
    assert_error(
        "fun f() {\n    val d: Double = 1\n}\n",
        "expected 'Double', actual 'Int'",
    )
    .await;
    assert_error(
        "fun f() {\n    val s: String = null\n}\n",
        "Null cannot be a value of a non-null type 'String'",
    )
    .await;
}

#[tokio::test]
async fn valid_initialisers_stay_silent() {
    assert_silent(
        r#"
open class Base
class Derived : Base()
interface Shape
class Circle : Shape

fun f() {
    val a: String? = null
    val b: Long = 1
    val c: Long = 1L
    val d: Double = 1.0
    val e: Float = 1.5f
    val g: Any = 1
    val h: CharSequence = "x"
    val i: Base = Derived()
    val j: Shape = Circle()
    val k: Char = 'c'
    val l: Short = 1
    val m: Base? = null
}
"#,
    )
    .await;
}

#[tokio::test]
async fn call_with_the_wrong_number_of_positional_arguments() {
    assert_error(
        "fun greet(name: String) = name\n\nfun f() {\n    greet()\n}\n",
        "No value passed for parameter 'name'",
    )
    .await;
    assert_error(
        "fun greet(name: String) = name\n\nfun f() {\n    greet(\"a\", \"b\")\n}\n",
        "Too many arguments",
    )
    .await;
    assert_error(
        "class Circle(val r: Double)\n\nfun f() {\n    Circle()\n}\n",
        "No value passed for parameter 'r'",
    )
    .await;
    assert_error(
        "fun g(a: Int, b: Int = 1) = a + b\n\nfun f() {\n    g()\n}\n",
        "No value passed for parameter 'a'",
    )
    .await;
}

#[tokio::test]
async fn calls_that_only_look_wrong_stay_silent() {
    assert_silent(
        r#"
fun ok(a: Int, b: Int = 1) = a + b
fun many(vararg xs: Int) = xs.size
fun over(a: Int) = a
fun over(a: Int, b: Int) = a + b
fun named(a: Int, b: Int) = a + b
fun blocky(block: () -> Unit) = block()
fun mixed(a: Int, block: () -> Unit) = block()
class Circle(val r: Double, val name: String = "c")
class Multi(val a: Int) { constructor() : this(0) }

fun f(spread: IntArray) {
    ok(1)
    ok(1, 2)
    many()
    many(1, 2, 3)
    many(*spread)
    over(1)
    over(1, 2)
    named(b = 1, a = 2)
    blocky { }
    mixed(1) { }
    Circle(1.0)
    Circle(1.0, "x")
    Multi()
    Multi(1)
}
"#,
    )
    .await;
}

#[tokio::test]
async fn dsl_and_extension_contexts_do_not_guess_calls() {
    assert_silent(
        r#"
fun add(a: Int, b: Int) = a + b
fun String.wrapper() = add(1)
class Builder { fun add(a: Int) = a }
fun build(block: Builder.() -> Unit) = Builder().apply(block)

fun f() {
    build { add(1) }
}
"#,
    )
    .await;
}

#[tokio::test]
async fn member_that_exists_nowhere_is_unresolved() {
    assert_error(
        "package p\n\nclass Circle(val r: Double) {\n    fun area() = r\n}\n\nfun f() {\n    val c = Circle(1.0)\n    c.nonexistentMember()\n}\n",
        "unresolved reference: nonexistentMember",
    )
    .await;
}

#[tokio::test]
async fn members_extensions_and_stdlib_names_stay_silent() {
    assert_silent(
        r#"
package p

class Circle(val r: Double) {
    fun area() = r
    var label: String = ""
}
fun Circle.diameter() = r * 2

fun f() {
    val c = Circle(1.0)
    c.area()
    c.r
    c.label = "x"
    c.diameter()
    c.toString()
    c.hashCode()
    c.let { it.r }
    c.also { }
}
"#,
    )
    .await;
}

#[tokio::test]
async fn overloads_split_across_files_stay_silent() {
    let one = ("p/One.kt", "package p\n\nfun over(a: Int) = a\n");
    let two = (
        "p/Two.kt",
        "package p\n\nfun over(a: Int, b: Int) = a + b\n",
    );
    let use_it = (
        "p/Use.kt",
        "package p\n\nfun f() {\n    over(1, 2)\n    over(1)\n}\n",
    );
    let diagnostics = project_diagnostics(&[one, two, use_it], "p/Use.kt").await;
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}
